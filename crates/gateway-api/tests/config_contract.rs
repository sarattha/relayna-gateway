use gateway_api::config::Config;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const VARIABLES: &[&str] = &[
    "DATABASE_URL",
    "REDIS_URL",
    "LITELLM_BASE_URL",
    "LITELLM_SERVICE_KEY",
    "GATEWAY_ADMIN_TOKEN",
    "DIRECT_OPENAI_BASE_URL",
    "DIRECT_OPENAI_SERVICE_KEY",
    "RELAYNA_WORKER_TOKEN",
    "RELAYNA_STUDIO_BASE_URL",
    "RELAYNA_STUDIO_TOKEN",
    "GUARDRAIL_PII_MAPPING_TTL_SECONDS",
    "GUARDRAIL_MAPPING_ENCRYPTION_KEY",
    "ENTRA_APPLICATION_ID",
    "ENTRA_RELAYNA_KEY_HEADER",
    "ENTRA_AUTH_ENABLED",
    "ENTRA_TENANT_ID",
    "ENTRA_ISSUER",
    "ENTRA_OIDC_DISCOVERY_URL",
    "ENTRA_REQUIRED_SCOPE",
    "ENTRA_REQUIRED_ROLE",
    "ENTRA_ALLOWED_GROUPS",
    "ENTRA_ACCEPTED_ALGORITHMS",
    "ENTRA_JWKS_CACHE_TTL_SECONDS",
    "ENTRA_CLOCK_SKEW_SECONDS",
    "APIGEE_TRUSTED_HEADER_ENABLED",
    "APIGEE_TRUSTED_HEADER_SECRET",
    "PORTAL_OIDC_ENABLED",
    "PORTAL_OIDC_TENANT_ID",
    "PORTAL_OIDC_PRIVATE_KEY_PATH",
    "PORTAL_OIDC_CERTIFICATE_PATH",
    "PORTAL_ADMIN_EMAILS",
    "PORTAL_ADMIN_OBJECT_IDS",
    "PORTAL_OIDC_ISSUER",
    "PORTAL_OIDC_DISCOVERY_URL",
    "PORTAL_OIDC_REDIRECT_URI",
    "PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI",
    "PORTAL_SESSION_TTL_SECONDS",
    "PORTAL_LOGIN_TTL_SECONDS",
    "PORTAL_SESSION_COOKIE_SECURE",
    "OWNER_ENTRA_AUTH_ENABLED",
    "OWNER_ENTRA_TENANT_ID",
    "OWNER_ENTRA_ISSUER",
    "OWNER_ENTRA_OIDC_DISCOVERY_URL",
    "OWNER_ENTRA_ACCEPTED_ALGORITHMS",
    "OWNER_ENTRA_JWKS_CACHE_TTL_SECONDS",
    "OWNER_ENTRA_CLOCK_SKEW_SECONDS",
    "GATEWAY_BIND_ADDR",
    "GATEWAY_CONTROL_BIND_ADDR",
    "GATEWAY_MAX_BUFFERED_REQUESTS",
    "GATEWAY_MAX_INFLIGHT_BUFFER_BYTES",
    "LOG_LEVEL",
];

fn clear_environment() {
    for name in VARIABLES {
        std::env::remove_var(name);
    }
}

fn set_required_environment() {
    for (name, value) in [
        ("DATABASE_URL", "postgres://localhost/gateway"),
        ("REDIS_URL", "redis://localhost"),
        ("LITELLM_BASE_URL", "http://localhost:4000"),
        ("LITELLM_SERVICE_KEY", "service-key"),
        ("GATEWAY_BIND_ADDR", "127.0.0.1:8080"),
        ("GATEWAY_CONTROL_BIND_ADDR", "127.0.0.1:8081"),
        ("LOG_LEVEL", "gateway_api=debug"),
    ] {
        std::env::set_var(name, value);
    }
}

#[test]
fn complete_environment_builds_all_optional_auth_and_runtime_settings() {
    let _guard = ENV_LOCK.lock().expect("environment lock");
    clear_environment();
    set_required_environment();
    for (name, value) in [
        ("GATEWAY_ADMIN_TOKEN", "operator-token"),
        ("DIRECT_OPENAI_BASE_URL", "https://openai.example"),
        ("DIRECT_OPENAI_SERVICE_KEY", "openai-secret"),
        ("RELAYNA_WORKER_TOKEN", "worker-secret"),
        ("RELAYNA_STUDIO_BASE_URL", "https://studio.example"),
        ("RELAYNA_STUDIO_TOKEN", "studio-secret"),
        ("GUARDRAIL_PII_MAPPING_TTL_SECONDS", "7200"),
        ("GUARDRAIL_MAPPING_ENCRYPTION_KEY", "mapping-secret"),
        ("ENTRA_APPLICATION_ID", "relayna-application"),
        ("ENTRA_RELAYNA_KEY_HEADER", "x-relayna-key"),
        ("ENTRA_AUTH_ENABLED", "yes"),
        ("ENTRA_TENANT_ID", "tenant"),
        ("ENTRA_ISSUER", "https://issuer.example"),
        (
            "ENTRA_OIDC_DISCOVERY_URL",
            "https://issuer.example/.well-known/openid-configuration",
        ),
        ("ENTRA_REQUIRED_SCOPE", "gateway.call"),
        ("ENTRA_REQUIRED_ROLE", "gateway-user"),
        ("ENTRA_ALLOWED_GROUPS", "group-a, group-b,,"),
        ("ENTRA_ACCEPTED_ALGORITHMS", "RS256,RS384"),
        ("ENTRA_JWKS_CACHE_TTL_SECONDS", "600"),
        ("ENTRA_CLOCK_SKEW_SECONDS", "30"),
        ("APIGEE_TRUSTED_HEADER_ENABLED", "1"),
        ("APIGEE_TRUSTED_HEADER_SECRET", "apigee-secret"),
        ("PORTAL_OIDC_ENABLED", "true"),
        ("PORTAL_OIDC_TENANT_ID", "tenant"),
        (
            "PORTAL_OIDC_PRIVATE_KEY_PATH",
            "/run/secrets/portal-private.pem",
        ),
        (
            "PORTAL_OIDC_CERTIFICATE_PATH",
            "/run/secrets/portal-certificate.pem",
        ),
        (
            "PORTAL_ADMIN_EMAILS",
            " First.Admin@example.test,SECOND.ADMIN@example.test ",
        ),
        ("PORTAL_ADMIN_OBJECT_IDS", " ADMIN-OBJECT-1,ADMIN-OBJECT-2 "),
        ("PORTAL_OIDC_ISSUER", "https://issuer.example"),
        (
            "PORTAL_OIDC_DISCOVERY_URL",
            "https://issuer.example/.well-known/openid-configuration",
        ),
        (
            "PORTAL_OIDC_REDIRECT_URI",
            "https://gateway.example/admin-ui/auth/callback",
        ),
        (
            "PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI",
            "https://gateway.example/admin-ui",
        ),
        ("PORTAL_SESSION_TTL_SECONDS", "14400"),
        ("PORTAL_LOGIN_TTL_SECONDS", "300"),
        ("PORTAL_SESSION_COOKIE_SECURE", "yes"),
        ("OWNER_ENTRA_AUTH_ENABLED", "true"),
        ("OWNER_ENTRA_TENANT_ID", "tenant"),
        ("OWNER_ENTRA_ISSUER", "https://issuer.example"),
        (
            "OWNER_ENTRA_OIDC_DISCOVERY_URL",
            "https://issuer.example/.well-known/openid-configuration",
        ),
        ("OWNER_ENTRA_ACCEPTED_ALGORITHMS", "RS256"),
        ("OWNER_ENTRA_JWKS_CACHE_TTL_SECONDS", "120"),
        ("OWNER_ENTRA_CLOCK_SKEW_SECONDS", "15"),
        ("GATEWAY_MAX_BUFFERED_REQUESTS", "12"),
        ("GATEWAY_MAX_INFLIGHT_BUFFER_BYTES", "134217728"),
    ] {
        std::env::set_var(name, value);
    }

    let config = Config::from_env().expect("complete config");
    assert_eq!(config.guardrail_pii_mapping_ttl_seconds, 7200);
    assert_eq!(
        config
            .entra_auth
            .as_ref()
            .expect("Entra config")
            .allowed_groups
            .len(),
        2
    );
    assert!(config.apigee_trusted_header.is_some());
    let portal = config.portal_oidc.as_ref().expect("portal OIDC config");
    assert_eq!(portal.client_id, "relayna-application");
    assert_eq!(portal.session_ttl_seconds, 14_400);
    assert_eq!(portal.login_ttl_seconds, 300);
    assert!(portal.cookie_secure);
    assert_eq!(
        portal.admin_emails,
        vec![
            "first.admin@example.test".to_owned(),
            "second.admin@example.test".to_owned()
        ]
    );
    assert_eq!(
        portal.admin_object_ids,
        vec!["admin-object-1".to_owned(), "admin-object-2".to_owned()]
    );
    let owner = config
        .owner_entra_auth
        .as_ref()
        .expect("owner Entra config");
    assert_eq!(owner.audience, "relayna-application");
    assert_eq!(
        config
            .entra_auth
            .as_ref()
            .expect("request-plane Entra config")
            .audience,
        "relayna-application"
    );
    assert_eq!(owner.jwks_cache_ttl_seconds, 120);
    assert_eq!(config.gateway_max_buffered_requests, 12);
    assert_eq!(config.gateway_max_inflight_buffer_bytes, 134_217_728);
    let auth_env = config.gateway_auth_env();
    assert_eq!(auth_env.relayna_key_header, "x-relayna-key");
    assert!(auth_env.entra_auth.is_some());
    assert!(auth_env.apigee_trusted_header.is_some());
    clear_environment();
}

#[test]
fn every_enabled_entra_mode_requires_the_shared_application_id() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());

    for (enabled_name, required_values) in [
        (
            "ENTRA_AUTH_ENABLED",
            vec![
                ("ENTRA_TENANT_ID", "tenant"),
                ("ENTRA_ISSUER", "https://issuer.example"),
                (
                    "ENTRA_OIDC_DISCOVERY_URL",
                    "https://issuer.example/.well-known/openid-configuration",
                ),
            ],
        ),
        (
            "PORTAL_OIDC_ENABLED",
            vec![
                ("PORTAL_OIDC_TENANT_ID", "tenant"),
                ("PORTAL_OIDC_PRIVATE_KEY_PATH", "/run/private.pem"),
                ("PORTAL_OIDC_CERTIFICATE_PATH", "/run/certificate.pem"),
                ("PORTAL_OIDC_ISSUER", "https://issuer.example"),
                (
                    "PORTAL_OIDC_DISCOVERY_URL",
                    "https://issuer.example/.well-known/openid-configuration",
                ),
                (
                    "PORTAL_OIDC_REDIRECT_URI",
                    "https://gateway.example/admin-ui/auth/callback",
                ),
                (
                    "PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI",
                    "https://gateway.example/admin-ui",
                ),
            ],
        ),
        (
            "OWNER_ENTRA_AUTH_ENABLED",
            vec![
                ("OWNER_ENTRA_TENANT_ID", "tenant"),
                ("OWNER_ENTRA_ISSUER", "https://issuer.example"),
                (
                    "OWNER_ENTRA_OIDC_DISCOVERY_URL",
                    "https://issuer.example/.well-known/openid-configuration",
                ),
            ],
        ),
    ] {
        clear_environment();
        set_required_environment();
        std::env::set_var(enabled_name, "true");
        for (name, value) in required_values {
            std::env::set_var(name, value);
        }
        assert_eq!(
            Config::from_env().expect_err("missing shared application ID"),
            gateway_core::GatewayError::InvalidConfiguration,
            "{enabled_name} must require ENTRA_APPLICATION_ID"
        );
    }

    clear_environment();
}

#[test]
fn request_plane_entra_defaults_to_the_invoke_application_role() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    clear_environment();
    set_required_environment();
    for (name, value) in [
        ("ENTRA_APPLICATION_ID", "relayna-application"),
        ("ENTRA_AUTH_ENABLED", "true"),
        ("ENTRA_TENANT_ID", "tenant"),
        ("ENTRA_ISSUER", "https://issuer.example"),
        (
            "ENTRA_OIDC_DISCOVERY_URL",
            "https://issuer.example/.well-known/openid-configuration",
        ),
        ("APIGEE_TRUSTED_HEADER_ENABLED", "true"),
        ("APIGEE_TRUSTED_HEADER_SECRET", "secret"),
    ] {
        std::env::set_var(name, value);
    }

    let config = Config::from_env().expect("role-defaulted config");
    assert_eq!(
        config.entra_auth.expect("Entra config").required_role,
        Some(gateway_core::GATEWAY_INVOKE_ROLE.to_owned())
    );
    assert_eq!(
        config
            .apigee_trusted_header
            .expect("Apigee config")
            .required_role,
        Some(gateway_core::GATEWAY_INVOKE_ROLE.to_owned())
    );
    clear_environment();
}

#[test]
fn defaults_and_invalid_optional_values_are_handled_deterministically() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    clear_environment();
    set_required_environment();
    std::env::set_var("GUARDRAIL_PII_MAPPING_TTL_SECONDS", "invalid");
    std::env::set_var("ENTRA_AUTH_ENABLED", "false");
    std::env::set_var("APIGEE_TRUSTED_HEADER_ENABLED", "no");
    std::env::set_var("PORTAL_OIDC_ENABLED", "false");
    std::env::set_var("OWNER_ENTRA_AUTH_ENABLED", "false");
    let config = Config::from_env().expect("defaulted config");
    assert_eq!(config.guardrail_pii_mapping_ttl_seconds, 3600);
    assert!(config.entra_auth.is_none());
    assert!(config.apigee_trusted_header.is_none());
    assert!(config.portal_oidc.is_none());
    assert!(config.owner_entra_auth.is_none());
    assert_eq!(
        config.gateway_max_buffered_requests,
        gateway_proxy::DEFAULT_MAX_BUFFERED_REQUESTS
    );
    assert_eq!(
        config.gateway_max_inflight_buffer_bytes,
        gateway_proxy::DEFAULT_MAX_INFLIGHT_BUFFER_BYTES
    );
    assert_eq!(config.gateway_max_inflight_buffer_bytes, 536_870_912);

    std::env::set_var("GATEWAY_MAX_BUFFERED_REQUESTS", "0");
    assert!(Config::from_env().is_err());
    std::env::remove_var("GATEWAY_MAX_BUFFERED_REQUESTS");
    std::env::set_var("GATEWAY_MAX_INFLIGHT_BUFFER_BYTES", "invalid");
    assert!(Config::from_env().is_err());
    std::env::remove_var("GATEWAY_MAX_INFLIGHT_BUFFER_BYTES");
    std::env::set_var("ENTRA_AUTH_ENABLED", "sometimes");
    assert!(Config::from_env().is_err());
    std::env::set_var("ENTRA_AUTH_ENABLED", "false");
    std::env::set_var("GATEWAY_BIND_ADDR", "not-an-address");
    assert!(Config::from_env().is_err());
    clear_environment();
}
