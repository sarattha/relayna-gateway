use chrono::{DateTime, Duration, Utc};
use gateway_core::{
    AdminProjectStore, BudgetDecision, BudgetStore, EntraIdentityContext, EntraIdentitySource,
    GatewayError, GuardrailPolicy, KeyPolicy, ManagedIdentityCreateRequest,
    ManagedIdentityPatchRequest, ManagedIdentityProjectCreateRequest,
    ManagedIdentityProjectPatchRequest, MemberPatchRequest, MemberStatus, NewPortalSession,
    OidcLoginTransaction, PolicyLookup, PortalAccessStore, PortalAdminBootstrapPolicy,
    ProjectCreateRequest, ProjectMembershipUpsertRequest, Provider, RateLimitDecision,
    RateLimitStore, Route, ServiceMemberRole, ServiceMembershipUpsertRequest, UsageQuery,
    UsageQueryStore, OWNER_WORKLOAD_ROLE,
};
use gateway_store::{PostgresStore, RedisControlState};
use redis::AsyncCommands;
use uuid::Uuid;

struct IntegrationEnv {
    store: PostgresStore,
    redis: RedisControlState,
    redis_client: redis::Client,
}

fn portal_identity(
    tenant_id: &str,
    object_id: &str,
    email: &str,
    display_name: &str,
) -> EntraIdentityContext {
    EntraIdentityContext {
        tenant_id: tenant_id.into(),
        subject: Some(object_id.into()),
        object_id: Some(object_id.into()),
        app_id: None,
        authorized_party: None,
        email: Some(email.into()),
        display_name: Some(display_name.into()),
        nonce: None,
        scopes: Vec::new(),
        roles: Vec::new(),
        groups: Vec::new(),
        token_version: "2.0".into(),
        source: EntraIdentitySource::Jwt,
    }
}

async fn integration_env() -> Option<IntegrationEnv> {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping integration test: DATABASE_URL is not set");
            return None;
        }
    };
    let redis_url = match std::env::var("REDIS_URL") {
        Ok(value) => value,
        Err(_) => {
            eprintln!("skipping integration test: REDIS_URL is not set");
            return None;
        }
    };
    let store = PostgresStore::connect(&database_url)
        .await
        .expect("connect postgres");
    sqlx::query(
        r#"
        INSERT INTO policy_layers (id, layer_kind, policy, guardrail_policy)
        VALUES ($1, 'global', $2, $3)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(sqlx::types::Json(KeyPolicy::neutral_layer(1)))
    .bind(sqlx::types::Json(GuardrailPolicy::default()))
    .execute(store.pool())
    .await
    .expect("seed neutral global policy layer");
    let redis = RedisControlState::new(&redis_url).expect("create redis control state");
    let redis_client = redis::Client::open(redis_url).expect("create redis client");
    Some(IntegrationEnv {
        store,
        redis,
        redis_client,
    })
}

async fn insert_budgeted_key(
    store: &PostgresStore,
    daily_budget_usd: Option<f64>,
    monthly_budget_usd: Option<f64>,
) -> (Uuid, Uuid) {
    let project_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name) VALUES ($1, $2)")
        .bind(project_id)
        .bind(format!("integration-{project_id}"))
        .execute(store.pool())
        .await
        .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, owner_type, project_id, key_prefix, key_hash)
        VALUES ($1, 'project', $2, $3, 'hash')
        "#,
    )
    .bind(key_id)
    .bind(project_id)
    .bind(format!("rk_live_{}", key_id.simple()))
    .execute(store.pool())
    .await
    .expect("insert key");
    sqlx::query(
        r#"
        INSERT INTO key_policies (key_id, daily_budget_usd, monthly_budget_usd)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(key_id)
    .bind(daily_budget_usd)
    .bind(monthly_budget_usd)
    .execute(store.pool())
    .await
    .expect("insert policy");
    (project_id, key_id)
}

async fn insert_policy_key(store: &PostgresStore) -> (Uuid, Uuid) {
    let project_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name) VALUES ($1, $2)")
        .bind(project_id)
        .bind(format!("policy-{project_id}"))
        .execute(store.pool())
        .await
        .expect("insert project");
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, owner_type, project_id, key_prefix, key_hash)
        VALUES ($1, 'project', $2, $3, 'hash')
        "#,
    )
    .bind(key_id)
    .bind(project_id)
    .bind(format!("rk_live_{}", key_id.simple()))
    .execute(store.pool())
    .await
    .expect("insert key");
    sqlx::query(
        r#"
        INSERT INTO key_policies (
            key_id,
            allowed_routes,
            allowed_providers,
            allowed_services,
            allow_streaming,
            policy_version
        )
        VALUES ($1, ARRAY['/services/*']::text[], ARRAY['internal-service']::text[], ARRAY['ocr-service']::text[], true, 7)
        "#,
    )
    .bind(key_id)
    .execute(store.pool())
    .await
    .expect("insert policy");
    (project_id, key_id)
}

async fn insert_usage(
    store: &PostgresStore,
    key_id: Uuid,
    project_id: Uuid,
    request_id: &str,
    estimated_cost: Option<f64>,
    created_at: DateTime<Utc>,
) {
    sqlx::query(
        r#"
        INSERT INTO usage_events (
            request_id,
            key_id,
            project_id,
            route,
            provider,
            status,
            status_code,
            latency_ms,
            estimated_cost,
            created_at
        )
        VALUES ($1, $2, $3, '/v1/chat/completions', 'litellm', 'success', 200, 10, $4, $5)
        "#,
    )
    .bind(request_id)
    .bind(key_id)
    .bind(project_id)
    .bind(estimated_cost)
    .bind(created_at)
    .execute(store.pool())
    .await
    .expect("insert usage");
}

async fn seed_from_postgres(store: &PostgresStore, redis: &RedisControlState, now: DateTime<Utc>) {
    for seed in store.budget_counter_seeds(now).await.expect("load seeds") {
        redis
            .seed_budget_counters(
                seed.key_id,
                seed.daily_spend_usd,
                seed.monthly_spend_usd,
                now,
            )
            .await
            .expect("seed redis");
    }
}

#[tokio::test]
async fn usage_guardrail_counts_are_scoped_by_key_and_project() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let request_id = format!("shared-request-{}", Uuid::new_v4().simple());
    let (first_project, first_key) = insert_budgeted_key(&env.store, None, None).await;
    let (second_project, second_key) = insert_budgeted_key(&env.store, None, None).await;
    insert_usage(&env.store, first_key, first_project, &request_id, None, now).await;
    insert_usage(
        &env.store,
        second_key,
        second_project,
        &request_id,
        None,
        now,
    )
    .await;
    sqlx::query(
        r#"
        INSERT INTO guardrail_execution_events (
            request_id, key_id, project_id, guardrail_name, mode, action,
            failure_policy, latency_ms, created_at
        )
        VALUES ($1, $2, $3, 'project-scope-test', 'pre_call', 'block', 'fail_closed', 1, $4)
        "#,
    )
    .bind(&request_id)
    .bind(second_key)
    .bind(second_project)
    .bind(now)
    .execute(env.store.pool())
    .await
    .expect("insert second-project guardrail event");

    let first_query = UsageQuery {
        project_id: Some(first_project),
        ..UsageQuery::default()
    };
    assert_eq!(
        env.store
            .usage_summary(first_query.clone())
            .await
            .expect("first project summary")
            .guardrail_block_count,
        0
    );
    assert_eq!(
        env.store
            .usage_events(first_query.clone())
            .await
            .expect("first project events")
            .rows[0]
            .guardrail_action_count,
        0
    );
    assert_eq!(
        env.store
            .usage_export(first_query)
            .await
            .expect("first project export")
            .rows[0]
            .guardrail_action_count,
        0
    );

    let second_query = UsageQuery {
        project_id: Some(second_project),
        ..UsageQuery::default()
    };
    assert_eq!(
        env.store
            .usage_summary(second_query.clone())
            .await
            .expect("second project summary")
            .guardrail_block_count,
        1
    );
    assert_eq!(
        env.store
            .usage_events(second_query)
            .await
            .expect("second project events")
            .rows[0]
            .guardrail_action_count,
        1
    );
}

#[tokio::test]
async fn portal_access_state_is_durable_scoped_and_revocable() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let suffix = Uuid::new_v4().simple().to_string();
    let service_name = format!("owner-{suffix}");
    sqlx::query(
        r#"
        INSERT INTO service_registrations (name, route_pattern, enabled)
        VALUES ($1, $2, true)
        "#,
    )
    .bind(&service_name)
    .bind(format!("/services/{service_name}/*"))
    .execute(env.store.pool())
    .await
    .expect("insert owner service");

    let empty_bootstrap_policy = PortalAdminBootstrapPolicy::new(
        "tenant-integration",
        Vec::<String>::new(),
        Vec::<String>::new(),
    )
    .expect("empty bootstrap policy");
    let member_identity = portal_identity(
        "tenant-integration",
        &format!("object-{suffix}"),
        "owner@example.test",
        "Owner Test",
    );
    let member = env
        .store
        .upsert_oidc_member(&member_identity, &empty_bootstrap_policy, now)
        .await
        .expect("upsert pending member");
    assert_eq!(member.status, MemberStatus::Pending);
    assert_eq!(
        env.store.get_member(member.id).await.unwrap(),
        Some(member.clone())
    );
    assert!(env
        .store
        .list_members()
        .await
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == member.id));

    let bootstrap_object_id = format!("bootstrap-object-{suffix}");
    let bootstrap_identity = portal_identity(
        "tenant-integration",
        &bootstrap_object_id,
        "first.admin@example.test",
        "First Administrator",
    );
    let bootstrap_policy = PortalAdminBootstrapPolicy::new(
        "tenant-integration",
        ["first.admin@example.test".into()],
        [bootstrap_object_id],
    )
    .expect("bootstrap policy");
    let bootstrap_member = env
        .store
        .upsert_oidc_member(&bootstrap_identity, &bootstrap_policy, now)
        .await
        .expect("bootstrap first administrator");
    assert_eq!(bootstrap_member.status, MemberStatus::Active);
    assert!(bootstrap_member.is_admin());
    let blocked_bootstrap_member = env
        .store
        .patch_member(
            bootstrap_member.id,
            MemberPatchRequest {
                status: Some(MemberStatus::Blocked),
                admin: Some(false),
            },
        )
        .await
        .expect("block bootstrap administrator")
        .expect("bootstrap administrator exists");
    assert!(!blocked_bootstrap_member.is_admin());
    let mut signed_in_blocked_identity = bootstrap_identity;
    signed_in_blocked_identity.email = Some("FIRST.ADMIN@example.test".into());
    let signed_in_blocked_member = env
        .store
        .upsert_oidc_member(&signed_in_blocked_identity, &bootstrap_policy, now)
        .await
        .expect("sign in blocked bootstrap administrator");
    assert_eq!(signed_in_blocked_member.status, MemberStatus::Blocked);
    assert!(!signed_in_blocked_member.is_admin());

    let member = env
        .store
        .patch_member(
            member.id,
            MemberPatchRequest {
                status: Some(MemberStatus::Active),
                admin: Some(true),
            },
        )
        .await
        .expect("activate member")
        .expect("member exists");
    assert!(member.is_admin());
    assert!(env
        .store
        .patch_member(
            Uuid::new_v4(),
            MemberPatchRequest {
                status: None,
                admin: None,
            },
        )
        .await
        .unwrap()
        .is_none());

    let membership = env
        .store
        .upsert_service_membership(
            member.id,
            ServiceMembershipUpsertRequest {
                service_name: service_name.clone(),
                role: ServiceMemberRole::Owner,
            },
        )
        .await
        .expect("assign service");
    assert_eq!(membership.role, ServiceMemberRole::Owner);
    assert_eq!(
        env.store
            .member_service_role(member.id, &service_name)
            .await
            .unwrap(),
        Some(ServiceMemberRole::Owner)
    );
    assert_eq!(
        env.store.list_service_memberships(member.id).await.unwrap(),
        vec![membership]
    );

    let project = env
        .store
        .create_project(ProjectCreateRequest {
            name: format!("Owner project {suffix}"),
        })
        .await
        .expect("create owner project");
    let project_membership = env
        .store
        .upsert_project_membership(
            member.id,
            ProjectMembershipUpsertRequest {
                project_id: project.id,
                role: ServiceMemberRole::Viewer,
            },
        )
        .await
        .expect("assign project");
    assert_eq!(
        env.store
            .member_project_role(member.id, project.id)
            .await
            .unwrap(),
        Some(ServiceMemberRole::Viewer)
    );
    assert_eq!(
        env.store.list_project_memberships(member.id).await.unwrap(),
        vec![project_membership]
    );

    let project_binding = env
        .store
        .create_managed_identity_project(ManagedIdentityProjectCreateRequest {
            tenant_id: "tenant-integration".into(),
            client_id: format!("project-client-{suffix}"),
            object_id: Some(format!("project-workload-{suffix}")),
            display_name: "Project monitor".into(),
            project_id: project.id,
            required_role: OWNER_WORKLOAD_ROLE.into(),
            enabled: true,
        })
        .await
        .expect("create project workload binding");
    assert!(env
        .store
        .list_managed_identity_projects()
        .await
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == project_binding.id));
    assert!(env
        .store
        .workload_project_binding(
            &project_binding.tenant_id,
            &project_binding.client_id,
            project_binding.object_id.as_deref(),
            project.id,
            &[OWNER_WORKLOAD_ROLE.into()],
        )
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .workload_project_binding(
            &project_binding.tenant_id,
            &project_binding.client_id,
            project_binding.object_id.as_deref(),
            project.id,
            &["wrong.role".into()],
        )
        .await
        .unwrap()
        .is_none());
    let project_binding = env
        .store
        .patch_managed_identity_project(
            project_binding.id,
            ManagedIdentityProjectPatchRequest {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect("patch project binding")
        .expect("project binding exists");
    assert!(!project_binding.enabled);

    let binding = env
        .store
        .create_managed_identity(ManagedIdentityCreateRequest {
            tenant_id: "tenant-integration".into(),
            client_id: format!("client-{suffix}"),
            object_id: Some(format!("workload-{suffix}")),
            display_name: "Owner workload".into(),
            service_name: service_name.clone(),
            required_role: OWNER_WORKLOAD_ROLE.into(),
            enabled: true,
        })
        .await
        .expect("create workload binding");
    assert!(env
        .store
        .list_managed_identities()
        .await
        .unwrap()
        .iter()
        .any(|candidate| candidate.id == binding.id));
    assert!(env
        .store
        .workload_service_binding(
            &binding.tenant_id,
            &binding.client_id,
            binding.object_id.as_deref(),
            &service_name,
            &[OWNER_WORKLOAD_ROLE.into()],
        )
        .await
        .unwrap()
        .is_some());
    assert!(env
        .store
        .workload_service_binding(
            &binding.tenant_id,
            &binding.client_id,
            Some("different-object"),
            &service_name,
            &[OWNER_WORKLOAD_ROLE.into()],
        )
        .await
        .unwrap()
        .is_none());
    assert!(env
        .store
        .workload_service_binding(
            &binding.tenant_id,
            &binding.client_id,
            binding.object_id.as_deref(),
            &service_name,
            &["wrong.role".into()],
        )
        .await
        .unwrap()
        .is_none());

    let conflict_service_name = format!("owner-conflict-{suffix}");
    sqlx::query(
        "INSERT INTO service_registrations (name, route_pattern, enabled) VALUES ($1, $2, true)",
    )
    .bind(&conflict_service_name)
    .bind(format!("/services/{conflict_service_name}/*"))
    .execute(env.store.pool())
    .await
    .expect("insert conflicting owner service");
    let conflicting_binding = env
        .store
        .create_managed_identity(ManagedIdentityCreateRequest {
            tenant_id: binding.tenant_id.clone(),
            client_id: binding.client_id.clone(),
            object_id: binding.object_id.clone(),
            display_name: "Conflicting workload".into(),
            service_name: conflict_service_name.clone(),
            required_role: OWNER_WORKLOAD_ROLE.into(),
            enabled: true,
        })
        .await
        .expect("create conflicting workload binding");
    assert_eq!(
        env.store
            .patch_managed_identity(
                binding.id,
                ManagedIdentityPatchRequest {
                    service_name: Some(conflict_service_name),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err(),
        GatewayError::InvalidAccessPayload
    );
    assert!(env
        .store
        .delete_managed_identity(conflicting_binding.id)
        .await
        .expect("delete conflicting workload binding"));

    let binding = env
        .store
        .patch_managed_identity(
            binding.id,
            ManagedIdentityPatchRequest {
                display_name: Some("Renamed workload".into()),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .await
        .expect("patch workload binding")
        .expect("binding exists");
    assert_eq!(binding.display_name, "Renamed workload");
    assert!(!binding.enabled);

    let expired_transaction = OidcLoginTransaction {
        state_hash: format!("expired-state-{suffix}"),
        binding_hash: format!("expired-binding-{suffix}"),
        nonce: format!("expired-nonce-{suffix}"),
        pkce_verifier: format!("expired-verifier-{suffix}"),
        return_to: "/admin-ui".into(),
        expires_at: now - Duration::minutes(1),
    };
    env.store
        .create_oidc_login_transaction(expired_transaction.clone())
        .await
        .expect("create expired OIDC transaction");
    let transaction = OidcLoginTransaction {
        state_hash: format!("state-{suffix}"),
        binding_hash: format!("binding-{suffix}"),
        nonce: format!("nonce-{suffix}"),
        pkce_verifier: format!("verifier-{suffix}"),
        return_to: "/admin-ui/#/my-services".into(),
        expires_at: now + Duration::minutes(5),
    };
    env.store
        .create_oidc_login_transaction(transaction.clone())
        .await
        .expect("create OIDC transaction");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM oidc_login_transactions WHERE state_hash = $1",
        )
        .bind(&expired_transaction.state_hash)
        .fetch_one(env.store.pool())
        .await
        .expect("count expired OIDC transaction"),
        0
    );
    assert_eq!(
        env.store
            .consume_oidc_login_transaction(&transaction.state_hash, "wrong-binding", now)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        env.store
            .consume_oidc_login_transaction(
                &transaction.state_hash,
                &transaction.binding_hash,
                now,
            )
            .await
            .unwrap(),
        Some(transaction.clone())
    );
    assert!(env
        .store
        .consume_oidc_login_transaction(&transaction.state_hash, &transaction.binding_hash, now,)
        .await
        .unwrap()
        .is_none());

    let expired_session = NewPortalSession {
        session_hash: format!("expired-session-{suffix}"),
        member_id: member.id,
        csrf_hash: format!("expired-csrf-{suffix}"),
        expires_at: now - Duration::minutes(1),
    };
    env.store
        .create_portal_session(expired_session.clone())
        .await
        .expect("create expired portal session");
    let session = NewPortalSession {
        session_hash: format!("session-{suffix}"),
        member_id: member.id,
        csrf_hash: format!("csrf-{suffix}"),
        expires_at: now + Duration::hours(1),
    };
    env.store
        .create_portal_session(session.clone())
        .await
        .expect("create portal session");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM portal_sessions WHERE session_hash = $1"
        )
        .bind(&expired_session.session_hash)
        .fetch_one(env.store.pool())
        .await
        .expect("count expired portal session"),
        0
    );
    let stored = env
        .store
        .resolve_portal_session(&session.session_hash, now)
        .await
        .expect("resolve portal session")
        .expect("session exists");
    assert_eq!(stored.member.id, member.id);
    assert_eq!(stored.csrf_hash, session.csrf_hash);
    assert!(env
        .store
        .delete_portal_session(&session.session_hash)
        .await
        .unwrap());
    assert!(env
        .store
        .resolve_portal_session(&session.session_hash, now)
        .await
        .unwrap()
        .is_none());

    assert!(env.store.delete_managed_identity(binding.id).await.unwrap());
    assert!(env
        .store
        .delete_managed_identity_project(project_binding.id)
        .await
        .unwrap());
    assert!(env
        .store
        .delete_project_membership(member.id, project.id)
        .await
        .unwrap());
    assert!(env
        .store
        .delete_service_membership(member.id, &service_name)
        .await
        .unwrap());
    assert!(env
        .store
        .member_service_role(member.id, &service_name)
        .await
        .unwrap()
        .is_none());
    sqlx::query("DELETE FROM portal_members WHERE id = $1")
        .bind(member.id)
        .execute(env.store.pool())
        .await
        .expect("delete test member");
    sqlx::query("DELETE FROM service_registrations WHERE name = $1")
        .bind(service_name)
        .execute(env.store.pool())
        .await
        .expect("delete test service");
    assert!(env.store.delete_project(project.id).await.unwrap());
}

#[tokio::test]
async fn stored_policy_for_context_decodes_aliased_policy_columns() {
    let Some(env) = integration_env().await else {
        return;
    };
    let (project_id, key_id) = insert_policy_key(&env.store).await;

    let policy = env
        .store
        .policy_for_context(
            key_id,
            Some(project_id),
            None,
            Some(Route::ServiceWildcard),
            None,
        )
        .await
        .expect("policy");

    assert_eq!(policy.policy_version, 7);
    assert_eq!(policy.allowed_routes, [Route::ServiceWildcard]);
    assert_eq!(policy.allowed_providers, [Provider::InternalService]);
    assert_eq!(policy.allowed_services, ["ocr-service"]);
    assert!(policy.allow_streaming);
}

#[tokio::test]
async fn empty_redis_rehydrates_budget_spend_and_denies_over_budget_key() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, key_id) = insert_budgeted_key(&env.store, Some(1.0), Some(5.0)).await;
    insert_usage(
        &env.store,
        key_id,
        project_id,
        "rehydrate-over",
        Some(1.25),
        now,
    )
    .await;

    seed_from_postgres(&env.store, &env.redis, now).await;

    let decision = env
        .redis
        .check_budget(key_id, Some(1.0), Some(5.0), now)
        .await
        .expect("check budget");
    assert!(matches!(decision, BudgetDecision::Exceeded(_)));
}

#[tokio::test]
async fn rehydration_ignores_bad_costs_and_skips_unbudgeted_keys() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, budgeted_key_id) = insert_budgeted_key(&env.store, Some(10.0), None).await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "positive",
        Some(2.5),
        now,
    )
    .await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "zero",
        Some(0.0),
        now,
    )
    .await;
    insert_usage(
        &env.store,
        budgeted_key_id,
        project_id,
        "negative",
        Some(-8.0),
        now,
    )
    .await;
    insert_usage(&env.store, budgeted_key_id, project_id, "null", None, now).await;
    let (_, unbudgeted_key_id) = insert_budgeted_key(&env.store, None, None).await;

    seed_from_postgres(&env.store, &env.redis, now).await;

    let allowed = env
        .redis
        .check_budget(budgeted_key_id, Some(10.0), None, now)
        .await
        .expect("check budget");
    assert!(matches!(allowed, BudgetDecision::Allowed(_)));
    if let BudgetDecision::Allowed(state) = allowed {
        assert!((state.daily_spend_usd - 2.5).abs() < f64::EPSILON);
    }

    let seeds = env.store.budget_counter_seeds(now).await.expect("seeds");
    assert!(!seeds.iter().any(|seed| seed.key_id == unbudgeted_key_id));
}

#[tokio::test]
async fn rehydration_preserves_existing_budget_reservations() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let (project_id, key_id) = insert_budgeted_key(&env.store, Some(10.0), Some(20.0)).await;
    insert_usage(
        &env.store,
        key_id,
        project_id,
        "reservation-history",
        Some(1.0),
        now,
    )
    .await;
    env.redis
        .reserve_budget(key_id, "req-reservation", 0.5, now)
        .await
        .expect("reserve budget");

    seed_from_postgres(&env.store, &env.redis, now).await;
    env.redis
        .reconcile_budget_reservation(key_id, "req-reservation", 0.75, now)
        .await
        .expect("reconcile reservation");

    let decision = env
        .redis
        .check_budget(key_id, Some(10.0), Some(20.0), now)
        .await
        .expect("check budget");
    if let BudgetDecision::Allowed(state) = decision {
        assert!((state.daily_spend_usd - 1.25).abs() < 0.000_001);
    } else {
        panic!("expected budget to remain allowed");
    }
}

#[tokio::test]
async fn tpm_counter_is_shared_and_returns_retry_hint() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now() + Duration::seconds(1);
    let key_id = Uuid::new_v4();
    let mut connection = env
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("redis connection");
    let tpm_key = gateway_core::rate_limits::token_rate_limit_key(key_id, now);
    let _: usize = connection.del(&tpm_key).await.expect("delete tpm key");

    let first = env
        .redis
        .check_token_rate_limit(key_id, Some(10), 6, now)
        .await
        .expect("first tpm");
    assert!(matches!(first, RateLimitDecision::Allowed { count: 6 }));
    let second = env
        .redis
        .check_token_rate_limit(key_id, Some(10), 6, now)
        .await
        .expect("second tpm");
    assert!(matches!(
        second,
        RateLimitDecision::Exceeded {
            count: 12,
            retry_after_seconds: Some(_)
        }
    ));
}

#[tokio::test]
async fn redis_control_state_covers_request_limits_and_budget_lifecycle() {
    let Some(env) = integration_env().await else {
        return;
    };
    let now = Utc::now();
    let key_id = Uuid::new_v4();
    assert!(matches!(
        env.redis
            .check_request_rate_limit(key_id, None, now)
            .await
            .expect("unlimited RPM"),
        RateLimitDecision::Allowed { count: 0 }
    ));
    assert!(matches!(
        env.redis
            .check_request_rate_limit(key_id, Some(1), now)
            .await
            .expect("first RPM request"),
        RateLimitDecision::Allowed { count: 1 }
    ));
    assert!(matches!(
        env.redis
            .check_request_rate_limit(key_id, Some(1), now)
            .await
            .expect("second RPM request"),
        RateLimitDecision::Exceeded { .. }
    ));
    assert!(matches!(
        env.redis
            .check_token_rate_limit(key_id, None, 10, now)
            .await
            .expect("unlimited TPM"),
        RateLimitDecision::Allowed { count: 0 }
    ));
    assert!(matches!(
        env.redis
            .check_token_rate_limit(key_id, Some(10), -1, now)
            .await
            .expect("zero TPM estimate"),
        RateLimitDecision::Allowed { count: 0 }
    ));
    assert!(matches!(
        env.redis
            .check_budget(key_id, None, None, now)
            .await
            .expect("unlimited budget"),
        BudgetDecision::Allowed(_)
    ));
    env.redis
        .add_budget_spend(key_id, 0.0, now)
        .await
        .expect("ignore zero spend");
    env.redis
        .add_budget_spend(key_id, 0.25, now)
        .await
        .expect("add spend");
    env.redis
        .reserve_budget(key_id, "ignored", 0.0, now)
        .await
        .expect("ignore zero reservation");
    env.redis
        .reserve_budget(key_id, "release", 0.5, now)
        .await
        .expect("reserve budget");
    env.redis
        .release_budget_reservation(key_id, "release")
        .await
        .expect("release budget");
    let decision = env
        .redis
        .check_budget(key_id, Some(1.0), Some(1.0), now)
        .await
        .expect("check accumulated budget");
    assert!(matches!(decision, BudgetDecision::Allowed(_)));
}
