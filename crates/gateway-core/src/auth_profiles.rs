//! Route-local authentication policy. Only an authenticated key selects a profile.
use crate::{EndpointEntraPolicy, GatewayError, GatewayResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticationProfile {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(rename = "type")]
    pub authentication: ProfileAuthentication,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entra: Option<EndpointEntraPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileAuthentication {
    EntraAndRelaynaKey,
    RelaynaKeyOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileBinding {
    pub key_id: Uuid,
    pub profile_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticationProfiles {
    /// Clients submit the revision they read; the store increments it atomically.
    pub revision: u64,
    pub profiles: Vec<AuthenticationProfile>,
    pub bindings: Vec<ProfileBinding>,
}

impl AuthenticationProfile {
    pub fn entra(&self) -> Option<&EndpointEntraPolicy> {
        self.entra.as_ref()
    }
    pub fn authentication_type(&self) -> &'static str {
        if self.entra().is_some() {
            "entra_and_relayna_key"
        } else {
            "relayna_key_only"
        }
    }
}

impl AuthenticationProfiles {
    pub fn validate(&self, accessa: bool) -> GatewayResult<()> {
        if self.profiles.is_empty()
            || self.profiles.len() > 32
            || self.bindings.len() > 1024
            || self.revision >= i64::MAX as u64
        {
            return Err(GatewayError::InvalidServicePayload);
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for profile in &self.profiles {
            if profile.id.is_empty()
                || profile.id.len() > 64
                || !profile
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || profile.name.trim().is_empty()
                || profile.name.len() > 120
                || profile.name.chars().any(char::is_control)
                || !ids.insert(&profile.id)
                || !names.insert(profile.name.trim().to_lowercase())
                || (accessa && profile.entra().is_none())
                || (matches!(
                    profile.authentication,
                    ProfileAuthentication::EntraAndRelaynaKey
                ) != profile.entra.is_some())
            {
                return Err(GatewayError::InvalidServicePayload);
            }
            if let Some(entra) = profile.entra() {
                if [
                    &entra.required_scopes,
                    &entra.required_roles,
                    &entra.allowed_groups,
                ]
                .iter()
                .any(|claims| claims.len() > 64)
                {
                    return Err(GatewayError::InvalidServicePayload);
                }
                crate::EndpointAccess {
                    entra: Some(entra.clone()),
                    ..Default::default()
                }
                .validate("/profile")?;
            }
        }
        let mut keys = HashSet::new();
        for binding in &self.bindings {
            if !ids.contains(&binding.profile_id) || !keys.insert(binding.key_id) {
                return Err(GatewayError::InvalidServicePayload);
            }
        }
        Ok(())
    }

    pub fn select(&self, key_id: Uuid) -> GatewayResult<&AuthenticationProfile> {
        let mut bindings = self
            .bindings
            .iter()
            .filter(|binding| binding.key_id == key_id);
        let binding = bindings
            .next()
            .ok_or(GatewayError::AuthenticationProfileDenied)?;
        if bindings.next().is_some() {
            return Err(GatewayError::AuthenticationProfileDenied);
        }
        let mut profiles = self
            .profiles
            .iter()
            .filter(|profile| profile.id == binding.profile_id);
        let profile = profiles
            .next()
            .ok_or(GatewayError::AuthenticationProfileDenied)?;
        if !profile.enabled || profiles.next().is_some() {
            return Err(GatewayError::AuthenticationProfileDenied);
        }
        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> AuthenticationProfiles {
        serde_json::from_value(json!({"revision":0,"profiles":[
            {"id":"employees","name":"Employees","enabled":true,"type":"entra_and_relayna_key","entra":{"audience":"api://employees","required_scopes":["invoke"]}},
            {"id":"automation","name":"Automation","enabled":true,"type":"relayna_key_only"}
        ],"bindings":[{"key_id":Uuid::nil(),"profile_id":"employees"}]})).unwrap()
    }
    #[test]
    fn profiles_select_only_explicit_authenticated_bindings() {
        let mut set = fixture();
        set.validate(false).unwrap();
        assert!(set.validate(true).is_err());
        assert_eq!(
            set.select(Uuid::nil()).unwrap().authentication_type(),
            "entra_and_relayna_key"
        );
        assert!(set.select(Uuid::new_v4()).is_err());
        set.bindings[0].profile_id = "automation".into();
        assert!(set.select(Uuid::nil()).unwrap().entra().is_none());
        assert_eq!(
            set.select(Uuid::nil()).unwrap().authentication_type(),
            "relayna_key_only"
        );
        set.profiles[1].enabled = false;
        assert!(set.select(Uuid::nil()).is_err());
        set.profiles[1].enabled = true;
        set.bindings.push(set.bindings[0].clone());
        assert!(set.select(Uuid::nil()).is_err());
        assert!(set.validate(false).is_err());
        set.bindings.pop();
        set.profiles.push(set.profiles[1].clone());
        assert!(set.select(Uuid::nil()).is_err());
        assert!(set.validate(false).is_err());
        set.profiles.clear();
        assert!(set.select(Uuid::nil()).is_err());
        assert!(set.validate(false).is_err());
    }
    #[test]
    fn reject_invalid_bounded_configuration_and_keep_legacy_json() {
        for (field, value) in [
            ("id", json!("")),
            ("id", json!("a b")),
            ("id", json!("x".repeat(65))),
            ("name", json!(" ")),
            ("name", json!("x".repeat(121))),
            ("name", json!("a\nb")),
            ("name", json!(" automation ")),
        ] {
            let mut data = serde_json::to_value(fixture()).unwrap();
            data["profiles"][0][field] = value;
            assert!(serde_json::from_value::<AuthenticationProfiles>(data)
                .unwrap()
                .validate(false)
                .is_err());
        }
        let mut set = fixture();
        set.profiles = vec![set.profiles[0].clone(); 33];
        assert!(set.validate(false).is_err());
        let mut set = fixture();
        set.bindings = vec![set.bindings[0].clone(); 1025];
        assert!(set.validate(false).is_err());
        let mut set = fixture();
        set.revision = i64::MAX as u64;
        assert!(set.validate(false).is_err());
        let mut set = fixture();
        set.bindings[0].profile_id = "missing".into();
        assert!(set.validate(false).is_err());
        for field in ["required_scopes", "required_roles", "allowed_groups"] {
            let mut data = serde_json::to_value(fixture()).unwrap();
            data["profiles"][0]["entra"][field] = json!(vec!["scope"; 65]);
            assert!(serde_json::from_value::<AuthenticationProfiles>(data)
                .unwrap()
                .validate(false)
                .is_err());
        }
        let mut access = crate::EndpointAccess::default();
        assert!(access.identity_for_key(Uuid::nil()).unwrap().is_none());
        assert!(serde_json::to_value(&access)
            .unwrap()
            .get("authentication_profiles")
            .is_none());
        access.authentication_profiles = Some(fixture());
        access.validate("/v1/responses").unwrap();
        assert_eq!(
            access
                .identity_for_key(Uuid::nil())
                .unwrap()
                .unwrap()
                .audience,
            "api://employees"
        );
        assert!(access.identity_for_key(Uuid::new_v4()).is_err());
        access.skip_entra = true;
        assert!(access.validate("/v1/responses").is_err());
        for error in [
            GatewayError::AuthenticationProfileDenied,
            GatewayError::AuthenticationProfileConflict,
        ] {
            assert!(error.status_code().is_client_error());
            assert!(!error.code().is_empty());
            assert!(!error.public_message().is_empty());
        }
    }
}
