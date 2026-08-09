use crate::{EntraIdentityContext, GatewayError, GatewayResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const PORTAL_ROLE_ADMIN: &str = "admin";
pub const OWNER_WORKLOAD_ROLE: &str = "gateway.monitor.read";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalAdminBootstrapPolicy {
    tenant_id: String,
    emails: BTreeSet<String>,
    object_ids: BTreeSet<String>,
}

impl PortalAdminBootstrapPolicy {
    pub fn new(
        tenant_id: impl Into<String>,
        emails: impl IntoIterator<Item = String>,
        object_ids: impl IntoIterator<Item = String>,
    ) -> GatewayResult<Self> {
        let tenant_id = tenant_id.into().trim().to_lowercase();
        let emails = emails
            .into_iter()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<BTreeSet<_>>();
        let object_ids = object_ids
            .into_iter()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<BTreeSet<_>>();
        if tenant_id.is_empty() || emails.is_empty() != object_ids.is_empty() {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(Self {
            tenant_id,
            emails,
            object_ids,
        })
    }

    pub fn authorizes(&self, identity: &EntraIdentityContext) -> bool {
        identity
            .tenant_id
            .trim()
            .eq_ignore_ascii_case(&self.tenant_id)
            && identity
                .object_id
                .as_deref()
                .map(str::trim)
                .map(str::to_lowercase)
                .is_some_and(|object_id| self.object_ids.contains(&object_id))
            && identity
                .email
                .as_deref()
                .map(str::trim)
                .map(str::to_lowercase)
                .is_some_and(|email| self.emails.contains(&email))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalMemberLogin {
    pub tenant_id: String,
    pub object_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub bootstrap_admin: bool,
}

impl PortalMemberLogin {
    pub fn from_identity(
        identity: &EntraIdentityContext,
        bootstrap_policy: &PortalAdminBootstrapPolicy,
    ) -> GatewayResult<Self> {
        let tenant_id = identity.tenant_id.trim();
        let object_id = identity
            .object_id
            .as_deref()
            .or(identity.subject.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(GatewayError::InvalidEntraToken)?;
        if tenant_id.is_empty() {
            return Err(GatewayError::InvalidEntraToken);
        }
        Ok(Self {
            tenant_id: tenant_id.to_owned(),
            object_id: object_id.to_owned(),
            email: identity
                .email
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            display_name: identity
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            bootstrap_admin: bootstrap_policy.authorizes(identity),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Pending,
    Active,
    Blocked,
}

impl MemberStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> GatewayResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "blocked" => Ok(Self::Blocked),
            _ => Err(GatewayError::InvalidAccessPayload),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMemberRole {
    Owner,
    Viewer,
}

impl ServiceMemberRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> GatewayResult<Self> {
        match value {
            "owner" => Ok(Self::Owner),
            "viewer" => Ok(Self::Viewer),
            _ => Err(GatewayError::InvalidAccessPayload),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PortalMember {
    pub id: Uuid,
    pub tenant_id: String,
    pub object_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub status: MemberStatus,
    pub roles: Vec<String>,
    pub last_sign_in_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PortalMember {
    pub fn is_active(&self) -> bool {
        self.status == MemberStatus::Active
    }

    pub fn is_admin(&self) -> bool {
        self.is_active() && self.roles.iter().any(|role| role == PORTAL_ROLE_ADMIN)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MemberPatchRequest {
    #[serde(default)]
    pub status: Option<MemberStatus>,
    #[serde(default)]
    pub admin: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceMembership {
    pub member_id: Uuid,
    pub service_name: String,
    pub role: ServiceMemberRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServiceMembershipUpsertRequest {
    pub service_name: String,
    pub role: ServiceMemberRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedIdentityBinding {
    pub id: Uuid,
    pub tenant_id: String,
    pub client_id: String,
    pub object_id: Option<String>,
    pub display_name: String,
    pub service_name: String,
    pub required_role: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ManagedIdentityCreateRequest {
    pub tenant_id: String,
    pub client_id: String,
    #[serde(default)]
    pub object_id: Option<String>,
    pub display_name: String,
    pub service_name: String,
    #[serde(default = "default_owner_workload_role")]
    pub required_role: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
pub struct ManagedIdentityPatchRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub object_id: Option<Option<String>>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub required_role: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

fn default_owner_workload_role() -> String {
    OWNER_WORKLOAD_ROLE.to_owned()
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcLoginTransaction {
    pub state_hash: String,
    pub binding_hash: String,
    pub nonce: String,
    pub pkce_verifier: String,
    pub return_to: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPortalSession {
    pub session_hash: String,
    pub member_id: Uuid,
    pub csrf_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPortalSession {
    pub session_hash: String,
    pub member: PortalMember,
    pub csrf_hash: String,
    pub expires_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PortalSessionResponse {
    pub authenticated: bool,
    pub member: PortalMember,
    pub service_memberships: Vec<ServiceMembership>,
    pub csrf_token: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OwnerServiceSummary {
    pub service_name: String,
    pub role: ServiceMemberRole,
    pub enabled: bool,
    pub route_pattern: String,
}

#[async_trait]
pub trait PortalAccessStore: Send + Sync {
    async fn upsert_oidc_member(
        &self,
        identity: &EntraIdentityContext,
        bootstrap_policy: &PortalAdminBootstrapPolicy,
        now: DateTime<Utc>,
    ) -> GatewayResult<PortalMember>;

    async fn list_members(&self) -> GatewayResult<Vec<PortalMember>>;
    async fn get_member(&self, member_id: Uuid) -> GatewayResult<Option<PortalMember>>;
    async fn patch_member(
        &self,
        member_id: Uuid,
        patch: MemberPatchRequest,
    ) -> GatewayResult<Option<PortalMember>>;

    async fn list_service_memberships(
        &self,
        member_id: Uuid,
    ) -> GatewayResult<Vec<ServiceMembership>>;
    async fn upsert_service_membership(
        &self,
        member_id: Uuid,
        request: ServiceMembershipUpsertRequest,
    ) -> GatewayResult<ServiceMembership>;
    async fn delete_service_membership(
        &self,
        member_id: Uuid,
        service_name: &str,
    ) -> GatewayResult<bool>;

    async fn list_managed_identities(&self) -> GatewayResult<Vec<ManagedIdentityBinding>>;
    async fn create_managed_identity(
        &self,
        request: ManagedIdentityCreateRequest,
    ) -> GatewayResult<ManagedIdentityBinding>;
    async fn patch_managed_identity(
        &self,
        identity_id: Uuid,
        patch: ManagedIdentityPatchRequest,
    ) -> GatewayResult<Option<ManagedIdentityBinding>>;
    async fn delete_managed_identity(&self, identity_id: Uuid) -> GatewayResult<bool>;

    async fn create_oidc_login_transaction(
        &self,
        transaction: OidcLoginTransaction,
    ) -> GatewayResult<()>;
    async fn consume_oidc_login_transaction(
        &self,
        state_hash: &str,
        binding_hash: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<Option<OidcLoginTransaction>>;
    async fn create_portal_session(&self, session: NewPortalSession) -> GatewayResult<()>;
    async fn resolve_portal_session(
        &self,
        session_hash: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<Option<StoredPortalSession>>;
    async fn delete_portal_session(&self, session_hash: &str) -> GatewayResult<bool>;

    async fn member_service_role(
        &self,
        member_id: Uuid,
        service_name: &str,
    ) -> GatewayResult<Option<ServiceMemberRole>>;
    async fn workload_service_binding(
        &self,
        tenant_id: &str,
        client_id: &str,
        object_id: Option<&str>,
        service_name: &str,
        token_roles: &[String],
    ) -> GatewayResult<Option<ManagedIdentityBinding>>;
}

#[async_trait]
impl<T> PortalAccessStore for std::sync::Arc<T>
where
    T: PortalAccessStore + ?Sized,
{
    async fn upsert_oidc_member(
        &self,
        identity: &EntraIdentityContext,
        bootstrap_policy: &PortalAdminBootstrapPolicy,
        now: DateTime<Utc>,
    ) -> GatewayResult<PortalMember> {
        (**self)
            .upsert_oidc_member(identity, bootstrap_policy, now)
            .await
    }
    async fn list_members(&self) -> GatewayResult<Vec<PortalMember>> {
        (**self).list_members().await
    }
    async fn get_member(&self, member_id: Uuid) -> GatewayResult<Option<PortalMember>> {
        (**self).get_member(member_id).await
    }
    async fn patch_member(
        &self,
        member_id: Uuid,
        patch: MemberPatchRequest,
    ) -> GatewayResult<Option<PortalMember>> {
        (**self).patch_member(member_id, patch).await
    }
    async fn list_service_memberships(
        &self,
        member_id: Uuid,
    ) -> GatewayResult<Vec<ServiceMembership>> {
        (**self).list_service_memberships(member_id).await
    }
    async fn upsert_service_membership(
        &self,
        member_id: Uuid,
        request: ServiceMembershipUpsertRequest,
    ) -> GatewayResult<ServiceMembership> {
        (**self).upsert_service_membership(member_id, request).await
    }
    async fn delete_service_membership(
        &self,
        member_id: Uuid,
        service_name: &str,
    ) -> GatewayResult<bool> {
        (**self)
            .delete_service_membership(member_id, service_name)
            .await
    }
    async fn list_managed_identities(&self) -> GatewayResult<Vec<ManagedIdentityBinding>> {
        (**self).list_managed_identities().await
    }
    async fn create_managed_identity(
        &self,
        request: ManagedIdentityCreateRequest,
    ) -> GatewayResult<ManagedIdentityBinding> {
        (**self).create_managed_identity(request).await
    }
    async fn patch_managed_identity(
        &self,
        identity_id: Uuid,
        patch: ManagedIdentityPatchRequest,
    ) -> GatewayResult<Option<ManagedIdentityBinding>> {
        (**self).patch_managed_identity(identity_id, patch).await
    }
    async fn delete_managed_identity(&self, identity_id: Uuid) -> GatewayResult<bool> {
        (**self).delete_managed_identity(identity_id).await
    }
    async fn create_oidc_login_transaction(
        &self,
        transaction: OidcLoginTransaction,
    ) -> GatewayResult<()> {
        (**self).create_oidc_login_transaction(transaction).await
    }
    async fn consume_oidc_login_transaction(
        &self,
        state_hash: &str,
        binding_hash: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<Option<OidcLoginTransaction>> {
        (**self)
            .consume_oidc_login_transaction(state_hash, binding_hash, now)
            .await
    }
    async fn create_portal_session(&self, session: NewPortalSession) -> GatewayResult<()> {
        (**self).create_portal_session(session).await
    }
    async fn resolve_portal_session(
        &self,
        session_hash: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<Option<StoredPortalSession>> {
        (**self).resolve_portal_session(session_hash, now).await
    }
    async fn delete_portal_session(&self, session_hash: &str) -> GatewayResult<bool> {
        (**self).delete_portal_session(session_hash).await
    }
    async fn member_service_role(
        &self,
        member_id: Uuid,
        service_name: &str,
    ) -> GatewayResult<Option<ServiceMemberRole>> {
        (**self).member_service_role(member_id, service_name).await
    }
    async fn workload_service_binding(
        &self,
        tenant_id: &str,
        client_id: &str,
        object_id: Option<&str>,
        service_name: &str,
        token_roles: &[String],
    ) -> GatewayResult<Option<ManagedIdentityBinding>> {
        (**self)
            .workload_service_binding(tenant_id, client_id, object_id, service_name, token_roles)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_status_and_service_roles_are_strict() {
        assert_eq!(MemberStatus::parse("active").unwrap(), MemberStatus::Active);
        assert_eq!(MemberStatus::Active.as_str(), "active");
        assert_eq!(
            ServiceMemberRole::parse("owner").unwrap(),
            ServiceMemberRole::Owner
        );
        assert_eq!(ServiceMemberRole::Viewer.as_str(), "viewer");
        assert_eq!(
            MemberStatus::parse("enabled").unwrap_err(),
            GatewayError::InvalidAccessPayload
        );
        assert_eq!(
            ServiceMemberRole::parse("admin").unwrap_err(),
            GatewayError::InvalidAccessPayload
        );
    }

    #[test]
    fn only_active_admin_members_are_administrators() {
        let now = Utc::now();
        let mut member = PortalMember {
            id: Uuid::new_v4(),
            tenant_id: "tenant".into(),
            object_id: "object".into(),
            email: None,
            display_name: None,
            status: MemberStatus::Pending,
            roles: vec![PORTAL_ROLE_ADMIN.into()],
            last_sign_in_at: None,
            created_at: now,
            updated_at: now,
        };
        assert!(!member.is_admin());
        member.status = MemberStatus::Active;
        assert!(member.is_admin());
    }

    #[test]
    fn bootstrap_admin_requires_the_configured_tenant_object_and_email() {
        let policy = PortalAdminBootstrapPolicy::new(
            "tenant-1",
            ["FIRST.ADMIN@example.test".to_owned()],
            ["object-1".to_owned()],
        )
        .expect("bootstrap policy");
        let identity = EntraIdentityContext {
            tenant_id: "tenant-1".into(),
            subject: Some("subject-1".into()),
            object_id: Some("object-1".into()),
            app_id: None,
            authorized_party: None,
            email: Some("first.admin@example.test".into()),
            display_name: Some("First Administrator".into()),
            nonce: None,
            scopes: Vec::new(),
            roles: Vec::new(),
            groups: Vec::new(),
            token_version: "2.0".into(),
            source: crate::EntraIdentitySource::Jwt,
        };
        assert!(policy.authorizes(&identity));
        assert!(
            PortalMemberLogin::from_identity(&identity, &policy)
                .expect("member login")
                .bootstrap_admin
        );

        let mut reassigned_email = identity.clone();
        reassigned_email.object_id = Some("replacement-object".into());
        assert!(!policy.authorizes(&reassigned_email));
        let mut wrong_tenant = identity.clone();
        wrong_tenant.tenant_id = "tenant-2".into();
        assert!(!policy.authorizes(&wrong_tenant));
        let mut wrong_email = identity;
        wrong_email.email = Some("different@example.test".into());
        assert!(!policy.authorizes(&wrong_email));

        assert_eq!(
            PortalAdminBootstrapPolicy::new(
                "tenant-1",
                ["first.admin@example.test".into()],
                Vec::<String>::new(),
            )
            .unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }
}
