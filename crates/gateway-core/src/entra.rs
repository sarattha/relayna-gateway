use crate::{GatewayError, GatewayResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hmac::{Hmac, Mac};
use http::header::HeaderName;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

type HmacSha256 = Hmac<Sha256>;
pub const ENTRA_DEFAULT_RELAYNA_KEY_HEADER: &str = "X-Relayna-Key";
const ENTRA_OIDC_HTTP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntraAuthDebugContext<'a> {
    pub surface: &'a str,
    pub request_id: Option<&'a str>,
}

impl<'a> EntraAuthDebugContext<'a> {
    pub const fn new(surface: &'a str, request_id: Option<&'a str>) -> Self {
        Self {
            surface,
            request_id,
        }
    }
}

const DEFAULT_ENTRA_DEBUG_CONTEXT: EntraAuthDebugContext<'static> =
    EntraAuthDebugContext::new("entra", None);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntraAuthConfig {
    pub tenant_id: String,
    pub audience: String,
    pub issuer: String,
    pub oidc_discovery_url: String,
    pub required_scope: Option<String>,
    pub required_role: Option<String>,
    pub allowed_groups: Vec<String>,
    pub accepted_algorithms: Vec<String>,
    pub relayna_key_header: String,
    pub jwks_cache_ttl_seconds: u64,
    pub clock_skew_seconds: i64,
}

impl EntraAuthConfig {
    pub fn validate(&self) -> GatewayResult<()> {
        if self.tenant_id.trim().is_empty()
            || self.audience.trim().is_empty()
            || self.issuer.trim().is_empty()
            || self.oidc_discovery_url.trim().is_empty()
        {
            return Err(GatewayError::InvalidConfiguration);
        }
        if self.accepted_algorithms.is_empty()
            || self
                .accepted_algorithms
                .iter()
                .any(|algorithm| algorithm_to_jsonwebtoken(algorithm).is_none())
        {
            return Err(GatewayError::InvalidConfiguration);
        }
        validate_relayna_key_header_name(&self.relayna_key_header)?;
        Ok(())
    }
}

pub fn validate_relayna_key_header_name(header: &str) -> GatewayResult<()> {
    if header.trim().is_empty() || HeaderName::from_bytes(header.trim().as_bytes()).is_err() {
        return Err(GatewayError::InvalidConfiguration);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntraIdentityContext {
    pub tenant_id: String,
    pub subject: Option<String>,
    pub object_id: Option<String>,
    pub app_id: Option<String>,
    pub authorized_party: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub nonce: Option<String>,
    pub scopes: Vec<String>,
    pub roles: Vec<String>,
    pub groups: Vec<String>,
    pub token_version: String,
    pub source: EntraIdentitySource,
}

pub fn authorization_debug_identity(identity: &EntraIdentityContext) -> Value {
    json!({
        "tenant_id": identity.tenant_id,
        "subject": identity.subject,
        "object_id": identity.object_id,
        "app_id": identity.app_id,
        "authorized_party": identity.authorized_party,
        "email": identity.email,
        "display_name": identity.display_name,
        "nonce_present": identity.nonce.is_some(),
        "nonce_logged": false,
        "scopes": identity.scopes,
        "roles": identity.roles,
        "groups": identity.groups,
        "token_version": identity.token_version,
        "source": identity.source,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntraIdentitySource {
    Jwt,
    ApigeeTrustedHeader,
}

#[derive(Debug)]
pub struct EntraJwtVerifier {
    config: EntraAuthConfig,
    client: reqwest::Client,
    cache: Mutex<Option<CachedJwks>>,
}

#[derive(Debug, Clone)]
struct CachedJwks {
    keys: Vec<JsonWebKey>,
    expires_at: Instant,
}

impl EntraJwtVerifier {
    pub fn new(config: EntraAuthConfig) -> GatewayResult<Self> {
        config.validate()?;
        Ok(Self {
            config,
            client: entra_http_client()?,
            cache: Mutex::new(None),
        })
    }

    pub async fn verify_authorization(
        &self,
        authorization: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        self.verify_authorization_with_context(authorization, now, DEFAULT_ENTRA_DEBUG_CONTEXT)
            .await
    }

    pub async fn verify_authorization_with_context(
        &self,
        authorization: Option<&str>,
        now: DateTime<Utc>,
        context: EntraAuthDebugContext<'_>,
    ) -> GatewayResult<EntraIdentityContext> {
        let Some(authorization) = authorization else {
            return Err(self.reject(
                context,
                "authorization_header",
                "authorization_header_missing",
                GatewayError::MissingEntraAuthorization,
                None,
                "unverified",
                Value::Null,
            ));
        };
        let Some(token) = authorization.strip_prefix("Bearer ") else {
            return Err(self.reject(
                context,
                "authorization_header",
                "bearer_scheme_invalid",
                GatewayError::MalformedEntraAuthorization,
                None,
                "unverified",
                Value::Null,
            ));
        };
        self.verify_token_with_context(token.trim(), now, context)
            .await
    }

    pub async fn verify_token(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        self.verify_token_with_context(token, now, DEFAULT_ENTRA_DEBUG_CONTEXT)
            .await
    }

    pub async fn verify_token_with_context(
        &self,
        token: &str,
        now: DateTime<Utc>,
        context: EntraAuthDebugContext<'_>,
    ) -> GatewayResult<EntraIdentityContext> {
        if token.is_empty() {
            return Err(self.reject(
                context,
                "jwt_decode",
                "token_empty",
                GatewayError::MalformedEntraAuthorization,
                None,
                "unverified",
                Value::Null,
            ));
        }

        let header = decode_header(token).map_err(|_| {
            self.reject(
                context,
                "jwt_decode",
                "jwt_header_decode_failed",
                GatewayError::MalformedEntraAuthorization,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;
        let kid = header.kid.clone().ok_or_else(|| {
            self.reject(
                context,
                "jwt_header",
                "kid_missing",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;
        let algorithm = header_algorithm_name(header.alg);
        if !self
            .config
            .accepted_algorithms
            .iter()
            .any(|accepted| accepted == algorithm)
        {
            return Err(self.reject(
                context,
                "jwt_header",
                "algorithm_not_allowed",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                json!({"actual_algorithm": algorithm}),
            ));
        }
        let algorithm = algorithm_to_jsonwebtoken(algorithm).ok_or_else(|| {
            self.reject(
                context,
                "jwt_header",
                "algorithm_unsupported",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;

        let mut jwk = self.cached_key(&kid).await.map_err(|error| {
            self.reject(
                context,
                "jwks_cache",
                "jwks_cache_unavailable",
                error,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;
        if jwk.is_none() {
            self.refresh_keys(context, token).await?;
            jwk = self.cached_key(&kid).await.map_err(|error| {
                self.reject(
                    context,
                    "jwks_cache",
                    "jwks_cache_unavailable_after_refresh",
                    error,
                    Some(token),
                    "unverified",
                    Value::Null,
                )
            })?;
        }
        let jwk = jwk.ok_or_else(|| {
            self.reject(
                context,
                "jwks_key_selection",
                "kid_not_found_after_refresh",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                json!({"kid": kid}),
            )
        })?;
        if jwk.kty != "RSA" {
            return Err(self.reject(
                context,
                "jwks_key_selection",
                "key_type_not_rsa",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                json!({"key_type": jwk.kty}),
            ));
        }
        if let Some(key_algorithm) = jwk.alg.as_deref() {
            if key_algorithm != header_algorithm_name(header.alg) {
                return Err(self.reject(
                    context,
                    "jwks_key_selection",
                    "key_algorithm_mismatch",
                    GatewayError::InvalidEntraToken,
                    Some(token),
                    "unverified",
                    json!({
                        "token_algorithm": header_algorithm_name(header.alg),
                        "key_algorithm": key_algorithm,
                    }),
                ));
            }
        }

        let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e).map_err(|_| {
            self.reject(
                context,
                "jwks_key_selection",
                "rsa_key_components_invalid",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;
        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.required_spec_claims.clear();

        let claims = decode::<EntraClaims>(token, &decoding_key, &validation)
            .map_err(|error| {
                self.reject(
                    context,
                    "jwt_signature",
                    "signature_or_claim_schema_invalid",
                    GatewayError::InvalidEntraToken,
                    Some(token),
                    "unverified",
                    json!({"verification_error": jwt_error_reason(&error)}),
                )
            })?
            .claims;
        let identity = self.validate_claims(claims, now).map_err(|failure| {
            self.reject(
                context,
                "jwt_claims",
                failure.reason,
                failure.error,
                Some(token),
                "signature_verified",
                json!({"server_time": now.to_rfc3339()}),
            )
        })?;
        self.emit(
            context,
            "jwt_claims",
            "accepted",
            "token_verified",
            None,
            Some(token),
            "signature_verified",
            json!({"normalized_identity": authorization_debug_identity(&identity)}),
        );
        Ok(identity)
    }

    async fn cached_key(&self, kid: &str) -> GatewayResult<Option<JsonWebKey>> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| GatewayError::InvalidEntraToken)?;
        if let Some(cache) = cache.as_ref() {
            if Instant::now() < cache.expires_at {
                return Ok(cache
                    .keys
                    .iter()
                    .find(|key| key.kid.as_deref() == Some(kid))
                    .cloned());
            }
        }
        Ok(None)
    }

    async fn refresh_keys(
        &self,
        context: EntraAuthDebugContext<'_>,
        token: &str,
    ) -> GatewayResult<()> {
        let metadata_response = self
            .client
            .get(&self.config.oidc_discovery_url)
            .send()
            .await
            .map_err(|error| {
                self.reject(
                    context,
                    "oidc_discovery",
                    reqwest_error_reason(&error),
                    GatewayError::InvalidEntraToken,
                    Some(token),
                    "unverified",
                    Value::Null,
                )
            })?;
        if !metadata_response.status().is_success() {
            return Err(self.reject(
                context,
                "oidc_discovery",
                "discovery_http_status",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                json!({"upstream_status": metadata_response.status().as_u16()}),
            ));
        }
        let metadata = metadata_response
            .json::<OidcMetadata>()
            .await
            .map_err(|_| {
                self.reject(
                    context,
                    "oidc_discovery",
                    "discovery_json_invalid",
                    GatewayError::InvalidEntraToken,
                    Some(token),
                    "unverified",
                    Value::Null,
                )
            })?;
        if metadata.issuer != self.config.issuer {
            return Err(self.reject(
                context,
                "oidc_discovery",
                "discovery_issuer_mismatch",
                GatewayError::InvalidEntraIssuer,
                Some(token),
                "unverified",
                json!({"discovered_issuer": metadata.issuer}),
            ));
        }
        let jwks_response = self
            .client
            .get(metadata.jwks_uri)
            .send()
            .await
            .map_err(|error| {
                self.reject(
                    context,
                    "jwks_refresh",
                    reqwest_error_reason(&error),
                    GatewayError::InvalidEntraToken,
                    Some(token),
                    "unverified",
                    Value::Null,
                )
            })?;
        if !jwks_response.status().is_success() {
            return Err(self.reject(
                context,
                "jwks_refresh",
                "jwks_http_status",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                json!({"upstream_status": jwks_response.status().as_u16()}),
            ));
        }
        let jwks = jwks_response.json::<JwksDocument>().await.map_err(|_| {
            self.reject(
                context,
                "jwks_refresh",
                "jwks_json_invalid",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                Value::Null,
            )
        })?;
        let key_count = jwks.keys.len();
        let expires_at = Instant::now() + Duration::from_secs(self.config.jwks_cache_ttl_seconds);
        *self.cache.lock().map_err(|_| {
            self.reject(
                context,
                "jwks_cache",
                "jwks_cache_write_failed",
                GatewayError::InvalidEntraToken,
                Some(token),
                "unverified",
                Value::Null,
            )
        })? = Some(CachedJwks {
            keys: jwks.keys,
            expires_at,
        });
        self.emit(
            context,
            "jwks_refresh",
            "accepted",
            "jwks_cache_refreshed",
            None,
            Some(token),
            "unverified",
            json!({
                "key_count": key_count,
                "cache_ttl_seconds": self.config.jwks_cache_ttl_seconds,
            }),
        );
        Ok(())
    }

    fn validate_claims(
        &self,
        claims: EntraClaims,
        now: DateTime<Utc>,
    ) -> Result<EntraIdentityContext, ClaimValidationFailure> {
        if claims.iss != self.config.issuer {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraIssuer,
                "issuer_mismatch",
            ));
        }
        if claims.tid != self.config.tenant_id {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraIssuer,
                "tenant_mismatch",
            ));
        }
        if !audience_contains(&claims.aud, &self.config.audience) {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraAudience,
                "audience_mismatch",
            ));
        }

        let skew = ChronoDuration::seconds(self.config.clock_skew_seconds);
        if timestamp_to_datetime(claims.exp).is_none_or(|expires_at| expires_at + skew <= now) {
            return Err(ClaimValidationFailure::new(
                GatewayError::ExpiredEntraToken,
                "token_expired",
            ));
        }
        if claims
            .nbf
            .and_then(timestamp_to_datetime)
            .is_some_and(|not_before| not_before - skew > now)
        {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraToken,
                "token_not_yet_valid",
            ));
        }
        if claims
            .iat
            .and_then(timestamp_to_datetime)
            .is_some_and(|issued_at| issued_at - skew > now)
        {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraToken,
                "issued_at_in_future",
            ));
        }
        if claims.ver != "1.0" && claims.ver != "2.0" {
            return Err(ClaimValidationFailure::new(
                GatewayError::InvalidEntraToken,
                "token_version_unsupported",
            ));
        }
        if claims.has_group_overage() {
            return Err(ClaimValidationFailure::new(
                GatewayError::InsufficientEntraAuthorization,
                "group_overage_not_supported",
            ));
        }

        let scopes = split_scopes(claims.scp.as_deref());
        let roles = claims.roles.unwrap_or_default();
        let groups = claims.groups.unwrap_or_default();
        validate_entra_authorization_detailed(
            self.config.required_scope.as_deref(),
            self.config.required_role.as_deref(),
            &self.config.allowed_groups,
            &scopes,
            &roles,
            &groups,
        )
        .map_err(|reason| {
            ClaimValidationFailure::new(GatewayError::InsufficientEntraAuthorization, reason)
        })?;

        Ok(EntraIdentityContext {
            tenant_id: claims.tid,
            subject: claims.sub,
            object_id: claims.oid,
            app_id: claims.appid,
            authorized_party: claims.azp,
            email: claims.preferred_username.or(claims.email),
            display_name: claims.name,
            nonce: claims.nonce,
            scopes,
            roles,
            groups,
            token_version: claims.ver,
            source: EntraIdentitySource::Jwt,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "authorization rejection keeps the public error and structured debug evidence together"
    )]
    fn reject(
        &self,
        context: EntraAuthDebugContext<'_>,
        phase: &str,
        reason: &str,
        error: GatewayError,
        token: Option<&str>,
        token_trust: &str,
        extra: Value,
    ) -> GatewayError {
        self.emit(
            context,
            phase,
            "rejected",
            reason,
            Some(&error),
            token,
            token_trust,
            extra,
        );
        error
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        context: EntraAuthDebugContext<'_>,
        phase: &str,
        outcome: &str,
        reason: &str,
        error: Option<&GatewayError>,
        token: Option<&str>,
        token_trust: &str,
        extra: Value,
    ) {
        if !gateway_telemetry::authorization_debug_enabled() {
            return;
        }
        let mut details = Map::new();
        details.insert(
            "expected".to_owned(),
            json!({
                "tenant_id": self.config.tenant_id,
                "audience": self.config.audience,
                "issuer": self.config.issuer,
                "required_scope": self.config.required_scope,
                "required_role": self.config.required_role,
                "allowed_groups": self.config.allowed_groups,
                "accepted_algorithms": self.config.accepted_algorithms,
                "clock_skew_seconds": self.config.clock_skew_seconds,
            }),
        );
        if let Some(error) = error {
            details.insert("public_error_code".to_owned(), json!(error.code()));
        }
        if let Some(token) = token {
            details.insert("token_trust".to_owned(), json!(token_trust));
            details.insert("token".to_owned(), decoded_jwt_debug(token));
        }
        if let Value::Object(extra) = extra {
            details.extend(extra);
        }
        gateway_telemetry::authorization_debug(
            context.surface,
            phase,
            outcome,
            reason,
            context.request_id,
            Value::Object(details),
        );
    }

    #[cfg(test)]
    fn new_with_jwks_for_tests(config: EntraAuthConfig, keys: Vec<JsonWebKey>) -> Self {
        Self {
            config,
            client: entra_http_client().expect("valid Entra test HTTP client"),
            cache: Mutex::new(Some(CachedJwks {
                keys,
                expires_at: Instant::now() + Duration::from_secs(3600),
            })),
        }
    }
}

fn entra_http_client() -> GatewayResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(ENTRA_OIDC_HTTP_TIMEOUT)
        .build()
        .map_err(|_| GatewayError::InvalidConfiguration)
}

#[derive(Debug)]
struct ClaimValidationFailure {
    error: GatewayError,
    reason: &'static str,
}

impl ClaimValidationFailure {
    const fn new(error: GatewayError, reason: &'static str) -> Self {
        Self { error, reason }
    }
}

fn decoded_jwt_debug(token: &str) -> Value {
    let mut segments = token.split('.');
    let Some(header) = segments.next() else {
        return json!({"decode_error": "header_segment_missing"});
    };
    let Some(claims) = segments.next() else {
        return json!({"decode_error": "claims_segment_missing"});
    };
    if segments.next().is_none() || segments.next().is_some() {
        return json!({"decode_error": "compact_segment_count_invalid"});
    }
    let claims = decode_jwt_segment(claims)
        .map(redact_transaction_claims)
        .unwrap_or_else(|reason| json!({"decode_error": reason}));
    json!({
        "header": decode_jwt_segment(header).unwrap_or_else(|reason| json!({"decode_error": reason})),
        "claims": claims,
    })
}

fn redact_transaction_claims(mut claims: Value) -> Value {
    if let Value::Object(values) = &mut claims {
        for name in ["nonce", "at_hash", "c_hash", "s_hash"] {
            if values.contains_key(name) {
                values.insert(name.to_owned(), Value::String("[redacted]".to_owned()));
            }
        }
    }
    claims
}

fn decode_jwt_segment(segment: &str) -> Result<Value, &'static str> {
    let decoded = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| "base64url_invalid")?;
    serde_json::from_slice(&decoded).map_err(|_| "json_invalid")
}

fn reqwest_error_reason(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "network_timeout"
    } else if error.is_connect() {
        "network_connect_failed"
    } else if error.is_decode() {
        "response_decode_failed"
    } else {
        "network_request_failed"
    }
}

fn jwt_error_reason(error: &jsonwebtoken::errors::Error) -> &'static str {
    use jsonwebtoken::errors::ErrorKind;
    match error.kind() {
        ErrorKind::InvalidSignature => "invalid_signature",
        ErrorKind::InvalidAlgorithm => "invalid_algorithm",
        ErrorKind::MissingRequiredClaim(_) => "required_claim_missing",
        ErrorKind::Json(_) => "claim_json_invalid",
        ErrorKind::Base64(_) => "base64url_invalid",
        ErrorKind::Utf8(_) => "utf8_invalid",
        _ => "jwt_verification_failed",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApigeeTrustedHeaderConfig {
    pub secret: String,
    pub required_scope: Option<String>,
    pub required_role: Option<String>,
    pub allowed_groups: Vec<String>,
}

impl ApigeeTrustedHeaderConfig {
    pub fn validate(&self) -> GatewayResult<()> {
        if self.secret.trim().is_empty() {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(())
    }
}

pub fn verify_apigee_trusted_identity(
    identity_header: Option<&str>,
    signature_header: Option<&str>,
    config: &ApigeeTrustedHeaderConfig,
) -> GatewayResult<EntraIdentityContext> {
    verify_apigee_trusted_identity_with_context(
        identity_header,
        signature_header,
        config,
        DEFAULT_ENTRA_DEBUG_CONTEXT,
    )
}

pub fn verify_apigee_trusted_identity_with_context(
    identity_header: Option<&str>,
    signature_header: Option<&str>,
    config: &ApigeeTrustedHeaderConfig,
    context: EntraAuthDebugContext<'_>,
) -> GatewayResult<EntraIdentityContext> {
    config.validate()?;
    let identity_header = identity_header.ok_or_else(|| {
        apigee_debug(
            context,
            config,
            "trusted_header",
            "rejected",
            "identity_header_missing",
            None,
            Some(&GatewayError::UntrustedApigeeIdentity),
        );
        GatewayError::UntrustedApigeeIdentity
    })?;
    let signature_header = signature_header.ok_or_else(|| {
        apigee_debug(
            context,
            config,
            "trusted_header",
            "rejected",
            "signature_header_missing",
            None,
            Some(&GatewayError::UntrustedApigeeIdentity),
        );
        GatewayError::UntrustedApigeeIdentity
    })?;
    let expected = hmac_sha256_base64url(config.secret.as_bytes(), identity_header.as_bytes())?;
    if !constant_time_eq(expected.as_bytes(), signature_header.as_bytes()) {
        apigee_debug(
            context,
            config,
            "trusted_header_signature",
            "rejected",
            "signature_mismatch",
            None,
            Some(&GatewayError::UntrustedApigeeIdentity),
        );
        return Err(GatewayError::UntrustedApigeeIdentity);
    }
    let identity_json = URL_SAFE_NO_PAD.decode(identity_header).map_err(|_| {
        apigee_debug(
            context,
            config,
            "trusted_header_payload",
            "rejected",
            "identity_base64url_invalid",
            None,
            Some(&GatewayError::UntrustedApigeeIdentity),
        );
        GatewayError::UntrustedApigeeIdentity
    })?;
    let mut identity: EntraIdentityContext =
        serde_json::from_slice(&identity_json).map_err(|_| {
            apigee_debug(
                context,
                config,
                "trusted_header_payload",
                "rejected",
                "identity_json_invalid",
                None,
                Some(&GatewayError::UntrustedApigeeIdentity),
            );
            GatewayError::UntrustedApigeeIdentity
        })?;
    identity.source = EntraIdentitySource::ApigeeTrustedHeader;
    if let Err(reason) = validate_entra_authorization_detailed(
        config.required_scope.as_deref(),
        config.required_role.as_deref(),
        &config.allowed_groups,
        &identity.scopes,
        &identity.roles,
        &identity.groups,
    ) {
        apigee_debug(
            context,
            config,
            "trusted_header_authorization",
            "rejected",
            reason,
            Some(&identity),
            Some(&GatewayError::InsufficientEntraAuthorization),
        );
        return Err(GatewayError::InsufficientEntraAuthorization);
    }
    apigee_debug(
        context,
        config,
        "trusted_header_authorization",
        "accepted",
        "identity_verified",
        Some(&identity),
        None,
    );
    Ok(identity)
}

fn apigee_debug(
    context: EntraAuthDebugContext<'_>,
    config: &ApigeeTrustedHeaderConfig,
    phase: &str,
    outcome: &str,
    reason: &str,
    identity: Option<&EntraIdentityContext>,
    error: Option<&GatewayError>,
) {
    if !gateway_telemetry::authorization_debug_enabled() {
        return;
    }
    gateway_telemetry::authorization_debug(
        context.surface,
        phase,
        outcome,
        reason,
        context.request_id,
        json!({
            "public_error_code": error.map(GatewayError::code),
            "identity_trust": identity.map(|_| "hmac_verified").unwrap_or("unverified"),
            "identity": identity.map(authorization_debug_identity),
            "expected": {
                "required_scope": config.required_scope,
                "required_role": config.required_role,
                "allowed_groups": config.allowed_groups,
            },
            "identity_header_logged": false,
            "signature_header_logged": false,
        }),
    );
}

pub fn sign_apigee_trusted_identity(
    identity_header: &str,
    config: &ApigeeTrustedHeaderConfig,
) -> GatewayResult<String> {
    config.validate()?;
    hmac_sha256_base64url(config.secret.as_bytes(), identity_header.as_bytes())
}

fn hmac_sha256_base64url(secret: &[u8], data: &[u8]) -> GatewayResult<String> {
    let mut mac =
        HmacSha256::new_from_slice(secret).map_err(|_| GatewayError::InvalidConfiguration)?;
    mac.update(data);
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |acc, (left, right)| acc | (left ^ right))
        == 0
}

fn validate_entra_authorization_detailed(
    required_scope: Option<&str>,
    required_role: Option<&str>,
    allowed_groups: &[String],
    scopes: &[String],
    roles: &[String],
    groups: &[String],
) -> Result<(), &'static str> {
    if let Some(required_scope) = required_scope {
        if !scopes.iter().any(|scope| scope == required_scope) {
            return Err("required_scope_missing");
        }
    }
    if let Some(required_role) = required_role {
        if !roles.iter().any(|role| role == required_role) {
            return Err("required_role_missing");
        }
    }
    if !allowed_groups.is_empty()
        && !allowed_groups
            .iter()
            .any(|allowed| groups.iter().any(|group| group == allowed))
    {
        return Err("allowed_group_missing");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct OidcMetadata {
    issuer: String,
    jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct JwksDocument {
    keys: Vec<JsonWebKey>,
}

#[derive(Debug, Clone, Deserialize)]
struct JsonWebKey {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    n: String,
    e: String,
}

#[derive(Debug, Clone, Deserialize)]
struct EntraClaims {
    iss: String,
    aud: serde_json::Value,
    exp: i64,
    nbf: Option<i64>,
    iat: Option<i64>,
    tid: String,
    ver: String,
    sub: Option<String>,
    oid: Option<String>,
    appid: Option<String>,
    azp: Option<String>,
    preferred_username: Option<String>,
    email: Option<String>,
    name: Option<String>,
    nonce: Option<String>,
    scp: Option<String>,
    roles: Option<Vec<String>>,
    groups: Option<Vec<String>>,
    hasgroups: Option<bool>,
    #[serde(rename = "_claim_names")]
    claim_names: Option<serde_json::Value>,
}

impl EntraClaims {
    fn has_group_overage(&self) -> bool {
        self.hasgroups.unwrap_or(false)
            || self
                .claim_names
                .as_ref()
                .and_then(|value| value.get("groups"))
                .is_some()
    }
}

fn header_algorithm_name(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::RS256 => "RS256",
        Algorithm::RS384 => "RS384",
        Algorithm::RS512 => "RS512",
        _ => "unsupported",
    }
}

fn algorithm_to_jsonwebtoken(algorithm: &str) -> Option<Algorithm> {
    match algorithm {
        "RS256" => Some(Algorithm::RS256),
        "RS384" => Some(Algorithm::RS384),
        "RS512" => Some(Algorithm::RS512),
        _ => None,
    }
}

fn timestamp_to_datetime(timestamp: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(timestamp, 0)
}

fn audience_contains(audience: &serde_json::Value, expected: &str) -> bool {
    match audience {
        serde_json::Value::String(value) => value == expected,
        serde_json::Value::Array(values) => {
            values.iter().any(|value| value.as_str() == Some(expected))
        }
        _ => false,
    }
}

fn split_scopes(scopes: Option<&str>) -> Vec<String> {
    scopes
        .unwrap_or_default()
        .split_whitespace()
        .filter(|scope| !scope.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        process::{Command, Stdio},
        thread,
    };

    struct TestSigningKey {
        encoding_key: EncodingKey,
        jwk: JsonWebKey,
    }

    fn config() -> EntraAuthConfig {
        EntraAuthConfig {
            tenant_id: "tenant-1".to_owned(),
            audience: "api://relayna-gateway".to_owned(),
            issuer: "https://login.microsoftonline.com/tenant-1/v2.0".to_owned(),
            oidc_discovery_url: "http://127.0.0.1/.well-known/openid-configuration".to_owned(),
            required_scope: Some("gateway.invoke".to_owned()),
            required_role: None,
            allowed_groups: vec!["group-1".to_owned()],
            accepted_algorithms: vec!["RS256".to_owned()],
            relayna_key_header: ENTRA_DEFAULT_RELAYNA_KEY_HEADER.to_owned(),
            jwks_cache_ttl_seconds: 300,
            clock_skew_seconds: 60,
        }
    }

    #[test]
    fn rejects_invalid_relayna_key_header_name() {
        let mut config = config();
        config.relayna_key_header = "not a header".to_owned();

        assert_eq!(
            config.validate().unwrap_err(),
            GatewayError::InvalidConfiguration
        );
    }

    fn signing_key(kid: &str) -> TestSigningKey {
        let private_pem = generate_test_private_key();
        let (modulus, exponent) = public_components(&private_pem);
        let jwk = JsonWebKey {
            kid: Some(kid.to_owned()),
            kty: "RSA".to_owned(),
            alg: Some("RS256".to_owned()),
            n: URL_SAFE_NO_PAD.encode(modulus),
            e: URL_SAFE_NO_PAD.encode(exponent),
        };
        TestSigningKey {
            encoding_key: EncodingKey::from_rsa_pem(private_pem.as_bytes())
                .expect("test encoding key"),
            jwk,
        }
    }

    fn generate_test_private_key() -> String {
        let output = Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
            ])
            .output()
            .expect("run openssl genpkey for Entra JWT tests");
        assert!(
            output.status.success(),
            "openssl genpkey failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("openssl private key pem is utf8")
    }

    fn public_components(private_pem: &str) -> (Vec<u8>, Vec<u8>) {
        let mut child = Command::new("openssl")
            .args(["rsa", "-pubout", "-text", "-noout"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run openssl rsa for Entra JWT tests");
        child
            .stdin
            .as_mut()
            .expect("openssl stdin")
            .write_all(private_pem.as_bytes())
            .expect("write private pem to openssl");
        let output = child.wait_with_output().expect("openssl rsa output");
        assert!(
            output.status.success(),
            "openssl rsa failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        parse_openssl_public_key_text(&String::from_utf8(output.stdout).expect("openssl text"))
    }

    fn parse_openssl_public_key_text(text: &str) -> (Vec<u8>, Vec<u8>) {
        let mut modulus_hex = String::new();
        let mut in_modulus = false;
        let mut exponent_hex = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed == "modulus:" {
                in_modulus = true;
                continue;
            }
            if let Some(exponent) = trimmed.strip_prefix("publicExponent:") {
                in_modulus = false;
                let hex = exponent
                    .split_once("(0x")
                    .and_then(|(_, rest)| rest.strip_suffix(')'))
                    .expect("openssl public exponent hex");
                exponent_hex = Some(hex.to_owned());
                continue;
            }
            if in_modulus {
                modulus_hex.push_str(&trimmed.replace([':', ' '], ""));
            }
        }

        let mut modulus = hex_to_bytes(&modulus_hex);
        while modulus.first() == Some(&0) {
            modulus.remove(0);
        }
        let exponent = hex_to_bytes(&exponent_hex.expect("openssl public exponent"));
        (modulus, exponent)
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        let hex = if hex.len().is_multiple_of(2) {
            hex.to_owned()
        } else {
            format!("0{hex}")
        };
        (0..hex.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).expect("hex byte"))
            .collect()
    }

    fn token(key: &TestSigningKey, claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = key.jwk.kid.clone();
        encode(&header, &claims, &key.encoding_key).expect("token")
    }

    fn valid_claims() -> serde_json::Value {
        let now = Utc::now().timestamp();
        json!({
            "iss": "https://login.microsoftonline.com/tenant-1/v2.0",
            "aud": "api://relayna-gateway",
            "exp": now + 300,
            "nbf": now - 10,
            "iat": now - 10,
            "tid": "tenant-1",
            "ver": "2.0",
            "sub": "subject-1",
            "oid": "object-1",
            "azp": "client-1",
            "scp": "gateway.invoke",
            "groups": ["group-1"]
        })
    }

    #[test]
    fn decoded_debug_token_exposes_all_claims_without_compact_credential() {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"debug-kid"}"#);
        let claims = URL_SAFE_NO_PAD.encode(
            br#"{"tid":"tenant-1","roles":["gateway.invoke"],"nonce":"do-not-log","tenant_extension":{"mode":"custom"}}"#,
        );
        let compact = format!("{header}.{claims}.replayable-signature");

        let decoded = decoded_jwt_debug(&compact);

        assert_eq!(decoded["header"]["kid"], "debug-kid");
        assert_eq!(decoded["claims"]["roles"][0], "gateway.invoke");
        assert_eq!(decoded["claims"]["nonce"], "[redacted]");
        assert_eq!(decoded["claims"]["tenant_extension"]["mode"], "custom");
        let rendered = decoded.to_string();
        assert!(!rendered.contains(&compact));
        assert!(!rendered.contains("replayable-signature"));
        assert!(!rendered.contains("do-not-log"));
    }

    #[test]
    fn detailed_authorization_reasons_identify_missing_requirement() {
        assert_eq!(
            validate_entra_authorization_detailed(
                Some("gateway.invoke"),
                None,
                &[],
                &["other.scope".to_owned()],
                &[],
                &[],
            ),
            Err("required_scope_missing")
        );
        assert_eq!(
            validate_entra_authorization_detailed(
                None,
                Some("gateway.monitor.read"),
                &[],
                &[],
                &["other.role".to_owned()],
                &[],
            ),
            Err("required_role_missing")
        );
    }

    fn jwks_json(jwk: &JsonWebKey) -> String {
        let kid = jwk.kid.as_deref().expect("test kid");
        format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"{kid}","alg":"RS256","use":"sig","n":"{}","e":"{}"}}]}}"#,
            jwk.n, jwk.e
        )
    }

    fn start_mock_oidc(jwks: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock oidc");
        let addr = listener.local_addr().expect("mock addr");
        thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept mock request");
                let mut request = [0_u8; 2048];
                let bytes_read = stream.read(&mut request).expect("read mock request");
                let request = String::from_utf8_lossy(&request[..bytes_read]);
                let body = if request.starts_with("GET /.well-known/openid-configuration ") {
                    format!(
                        r#"{{"issuer":"https://login.microsoftonline.com/tenant-1/v2.0","jwks_uri":"http://{addr}/keys"}}"#
                    )
                } else {
                    jwks.clone()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock response");
            }
        });
        format!("http://{addr}/.well-known/openid-configuration")
    }

    #[tokio::test]
    async fn accepts_valid_entra_token() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let token = token(&key, valid_claims());

        let identity = verifier
            .verify_authorization(Some(&format!("Bearer {token}")), Utc::now())
            .await
            .expect("identity");

        assert_eq!(identity.tenant_id, "tenant-1");
        assert_eq!(identity.scopes, vec!["gateway.invoke"]);
        assert_eq!(identity.groups, vec!["group-1"]);
    }

    #[tokio::test]
    async fn rejects_wrong_audience() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let mut claims = valid_claims();
        claims["aud"] = json!("https://graph.microsoft.com");
        let token = token(&key, claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraAudience);
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let mut token = token(&key, valid_claims());
        token.push('x');

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraToken);
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let mut claims = valid_claims();
        claims["exp"] = json!(Utc::now().timestamp() - 120);
        let token = token(&key, claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::ExpiredEntraToken);
    }

    #[tokio::test]
    async fn rejects_missing_required_scope() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let mut claims = valid_claims();
        claims["scp"] = json!("other.scope");
        let token = token(&key, claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }

    #[tokio::test]
    async fn rejects_group_overage() {
        let key = signing_key("test-kid");
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![key.jwk.clone()]);
        let mut claims = valid_claims();
        claims["hasgroups"] = json!(true);
        let token = token(&key, claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }

    #[tokio::test]
    async fn fetches_mock_oidc_metadata_and_jwks() {
        let key = signing_key("test-kid");
        let discovery_url = start_mock_oidc(jwks_json(&key.jwk));
        let mut config = config();
        config.oidc_discovery_url = discovery_url;
        let verifier = EntraJwtVerifier::new(config).expect("verifier");
        let token = token(&key, valid_claims());

        let identity = verifier
            .verify_token(&token, Utc::now())
            .await
            .expect("identity");

        assert_eq!(identity.tenant_id, "tenant-1");
        assert_eq!(identity.source, EntraIdentitySource::Jwt);
    }

    #[tokio::test]
    async fn rejects_unknown_kid_after_jwks_refresh() {
        let key = signing_key("test-kid");
        let different_key = signing_key("different-kid");
        let discovery_url = start_mock_oidc(jwks_json(&different_key.jwk));
        let mut config = config();
        config.oidc_discovery_url = discovery_url;
        let verifier = EntraJwtVerifier::new(config).expect("verifier");
        let token = token(&key, valid_claims());

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraToken);
    }

    #[test]
    fn verifies_apigee_trusted_identity_signature() {
        let config = ApigeeTrustedHeaderConfig {
            secret: "trusted-secret".to_owned(),
            required_scope: Some("gateway.invoke".to_owned()),
            required_role: None,
            allowed_groups: Vec::new(),
        };
        let identity = EntraIdentityContext {
            tenant_id: "tenant-1".to_owned(),
            subject: Some("subject-1".to_owned()),
            object_id: None,
            app_id: None,
            authorized_party: None,
            email: None,
            display_name: None,
            nonce: None,
            scopes: vec!["gateway.invoke".to_owned()],
            roles: Vec::new(),
            groups: Vec::new(),
            token_version: "2.0".to_owned(),
            source: EntraIdentitySource::Jwt,
        };
        let identity_header =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&identity).expect("identity json"));
        let signature = sign_apigee_trusted_identity(&identity_header, &config).expect("signature");

        let verified =
            verify_apigee_trusted_identity(Some(&identity_header), Some(&signature), &config)
                .expect("trusted");

        assert_eq!(verified.source, EntraIdentitySource::ApigeeTrustedHeader);
    }

    #[test]
    fn rejects_apigee_trusted_identity_without_required_scope() {
        let config = ApigeeTrustedHeaderConfig {
            secret: "trusted-secret".to_owned(),
            required_scope: Some("gateway.invoke".to_owned()),
            required_role: None,
            allowed_groups: Vec::new(),
        };
        let identity = EntraIdentityContext {
            tenant_id: "tenant-1".to_owned(),
            subject: Some("subject-1".to_owned()),
            object_id: None,
            app_id: None,
            authorized_party: None,
            email: None,
            display_name: None,
            nonce: None,
            scopes: Vec::new(),
            roles: Vec::new(),
            groups: Vec::new(),
            token_version: "2.0".to_owned(),
            source: EntraIdentitySource::Jwt,
        };
        let identity_header =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&identity).expect("identity json"));
        let signature = sign_apigee_trusted_identity(&identity_header, &config).expect("signature");

        let error =
            verify_apigee_trusted_identity(Some(&identity_header), Some(&signature), &config)
                .unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }

    #[test]
    fn rejects_apigee_trusted_identity_without_allowed_group() {
        let config = ApigeeTrustedHeaderConfig {
            secret: "trusted-secret".to_owned(),
            required_scope: None,
            required_role: None,
            allowed_groups: vec!["allowed-group".to_owned()],
        };
        let identity = EntraIdentityContext {
            tenant_id: "tenant-1".to_owned(),
            subject: Some("subject-1".to_owned()),
            object_id: None,
            app_id: None,
            authorized_party: None,
            email: None,
            display_name: None,
            nonce: None,
            scopes: Vec::new(),
            roles: Vec::new(),
            groups: vec!["other-group".to_owned()],
            token_version: "2.0".to_owned(),
            source: EntraIdentitySource::Jwt,
        };
        let identity_header =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&identity).expect("identity json"));
        let signature = sign_apigee_trusted_identity(&identity_header, &config).expect("signature");

        let error =
            verify_apigee_trusted_identity(Some(&identity_header), Some(&signature), &config)
                .unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }
}
