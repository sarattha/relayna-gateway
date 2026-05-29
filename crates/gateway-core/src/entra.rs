use crate::{GatewayError, GatewayResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hmac::{Hmac, Mac};
use http::header::HeaderName;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::HashSet,
    sync::Mutex,
    time::{Duration, Instant},
};

type HmacSha256 = Hmac<Sha256>;
pub const ENTRA_DEFAULT_RELAYNA_KEY_HEADER: &str = "X-Relayna-Key";

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
        if self.relayna_key_header.trim().is_empty()
            || HeaderName::from_bytes(self.relayna_key_header.trim().as_bytes()).is_err()
        {
            return Err(GatewayError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntraIdentityContext {
    pub tenant_id: String,
    pub subject: Option<String>,
    pub object_id: Option<String>,
    pub app_id: Option<String>,
    pub authorized_party: Option<String>,
    pub scopes: Vec<String>,
    pub roles: Vec<String>,
    pub groups: Vec<String>,
    pub token_version: String,
    pub source: EntraIdentitySource,
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
            client: reqwest::Client::new(),
            cache: Mutex::new(None),
        })
    }

    pub async fn verify_authorization(
        &self,
        authorization: Option<&str>,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        let authorization = authorization.ok_or(GatewayError::MissingEntraAuthorization)?;
        let Some(token) = authorization.strip_prefix("Bearer ") else {
            return Err(GatewayError::MalformedEntraAuthorization);
        };
        self.verify_token(token.trim(), now).await
    }

    pub async fn verify_token(
        &self,
        token: &str,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        if token.is_empty() {
            return Err(GatewayError::MalformedEntraAuthorization);
        }

        let header = decode_header(token).map_err(|_| GatewayError::MalformedEntraAuthorization)?;
        let kid = header.kid.ok_or(GatewayError::InvalidEntraToken)?;
        let algorithm = header_algorithm_name(header.alg);
        if !self
            .config
            .accepted_algorithms
            .iter()
            .any(|accepted| accepted == algorithm)
        {
            return Err(GatewayError::InvalidEntraToken);
        }
        let algorithm =
            algorithm_to_jsonwebtoken(algorithm).ok_or(GatewayError::InvalidEntraToken)?;

        let mut jwk = self.cached_key(&kid).await?;
        if jwk.is_none() {
            self.refresh_keys().await?;
            jwk = self.cached_key(&kid).await?;
        }
        let jwk = jwk.ok_or(GatewayError::InvalidEntraToken)?;
        if jwk.kty != "RSA" {
            return Err(GatewayError::InvalidEntraToken);
        }
        if let Some(key_algorithm) = jwk.alg.as_deref() {
            if key_algorithm != header_algorithm_name(header.alg) {
                return Err(GatewayError::InvalidEntraToken);
            }
        }

        let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
            .map_err(|_| GatewayError::InvalidEntraToken)?;
        let mut validation = Validation::new(algorithm);
        validation.validate_aud = false;
        validation.validate_exp = false;
        validation.validate_nbf = false;
        validation.required_spec_claims.clear();

        let claims = decode::<EntraClaims>(token, &decoding_key, &validation)
            .map_err(|_| GatewayError::InvalidEntraToken)?
            .claims;
        self.validate_claims(claims, now)
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

    async fn refresh_keys(&self) -> GatewayResult<()> {
        let metadata = self
            .client
            .get(&self.config.oidc_discovery_url)
            .send()
            .await
            .map_err(|_| GatewayError::InvalidEntraToken)?
            .error_for_status()
            .map_err(|_| GatewayError::InvalidEntraToken)?
            .json::<OidcMetadata>()
            .await
            .map_err(|_| GatewayError::InvalidEntraToken)?;
        if metadata.issuer != self.config.issuer {
            return Err(GatewayError::InvalidEntraIssuer);
        }
        let jwks = self
            .client
            .get(metadata.jwks_uri)
            .send()
            .await
            .map_err(|_| GatewayError::InvalidEntraToken)?
            .error_for_status()
            .map_err(|_| GatewayError::InvalidEntraToken)?
            .json::<JwksDocument>()
            .await
            .map_err(|_| GatewayError::InvalidEntraToken)?;
        let expires_at = Instant::now() + Duration::from_secs(self.config.jwks_cache_ttl_seconds);
        *self
            .cache
            .lock()
            .map_err(|_| GatewayError::InvalidEntraToken)? = Some(CachedJwks {
            keys: jwks.keys,
            expires_at,
        });
        Ok(())
    }

    fn validate_claims(
        &self,
        claims: EntraClaims,
        now: DateTime<Utc>,
    ) -> GatewayResult<EntraIdentityContext> {
        if claims.iss != self.config.issuer {
            return Err(GatewayError::InvalidEntraIssuer);
        }
        if claims.tid != self.config.tenant_id {
            return Err(GatewayError::InvalidEntraIssuer);
        }
        if !audience_contains(&claims.aud, &self.config.audience) {
            return Err(GatewayError::InvalidEntraAudience);
        }

        let skew = ChronoDuration::seconds(self.config.clock_skew_seconds);
        if timestamp_to_datetime(claims.exp).is_none_or(|expires_at| expires_at + skew <= now) {
            return Err(GatewayError::ExpiredEntraToken);
        }
        if claims
            .nbf
            .and_then(timestamp_to_datetime)
            .is_some_and(|not_before| not_before - skew > now)
        {
            return Err(GatewayError::InvalidEntraToken);
        }
        if claims
            .iat
            .and_then(timestamp_to_datetime)
            .is_some_and(|issued_at| issued_at - skew > now)
        {
            return Err(GatewayError::InvalidEntraToken);
        }
        if claims.ver != "1.0" && claims.ver != "2.0" {
            return Err(GatewayError::InvalidEntraToken);
        }
        if claims.has_group_overage() {
            return Err(GatewayError::InsufficientEntraAuthorization);
        }

        let scopes = split_scopes(claims.scp.as_deref());
        let roles = claims.roles.unwrap_or_default();
        let groups = claims.groups.unwrap_or_default();
        self.validate_authorization(&scopes, &roles, &groups)?;

        Ok(EntraIdentityContext {
            tenant_id: claims.tid,
            subject: claims.sub,
            object_id: claims.oid,
            app_id: claims.appid,
            authorized_party: claims.azp,
            scopes,
            roles,
            groups,
            token_version: claims.ver,
            source: EntraIdentitySource::Jwt,
        })
    }

    fn validate_authorization(
        &self,
        scopes: &[String],
        roles: &[String],
        groups: &[String],
    ) -> GatewayResult<()> {
        if let Some(required_scope) = self.config.required_scope.as_deref() {
            if !scopes.iter().any(|scope| scope == required_scope) {
                return Err(GatewayError::InsufficientEntraAuthorization);
            }
        }
        if let Some(required_role) = self.config.required_role.as_deref() {
            if !roles.iter().any(|role| role == required_role) {
                return Err(GatewayError::InsufficientEntraAuthorization);
            }
        }
        if !self.config.allowed_groups.is_empty() {
            let groups: HashSet<&str> = groups.iter().map(String::as_str).collect();
            if !self
                .config
                .allowed_groups
                .iter()
                .any(|allowed| groups.contains(allowed.as_str()))
            {
                return Err(GatewayError::InsufficientEntraAuthorization);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn new_with_jwks_for_tests(config: EntraAuthConfig, keys: Vec<JsonWebKey>) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            cache: Mutex::new(Some(CachedJwks {
                keys,
                expires_at: Instant::now() + Duration::from_secs(3600),
            })),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApigeeTrustedHeaderConfig {
    pub secret: String,
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
    config.validate()?;
    let identity_header = identity_header.ok_or(GatewayError::UntrustedApigeeIdentity)?;
    let signature_header = signature_header.ok_or(GatewayError::UntrustedApigeeIdentity)?;
    let expected = hmac_sha256_base64url(config.secret.as_bytes(), identity_header.as_bytes())?;
    if !constant_time_eq(expected.as_bytes(), signature_header.as_bytes()) {
        return Err(GatewayError::UntrustedApigeeIdentity);
    }
    let identity_json = URL_SAFE_NO_PAD
        .decode(identity_header)
        .map_err(|_| GatewayError::UntrustedApigeeIdentity)?;
    let mut identity: EntraIdentityContext = serde_json::from_slice(&identity_json)
        .map_err(|_| GatewayError::UntrustedApigeeIdentity)?;
    identity.source = EntraIdentitySource::ApigeeTrustedHeader;
    Ok(identity)
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
        thread,
    };

    const PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCWBw7GMq3pLLEk
ogCzJrPLE98WVdQg2Pc2U73Ip/spFtZ42TzE3QiR+3F7jWzOOWHkwN7tmtyFTRZL
2xXVs1FyEAH67AbiuITnWCnL8CWIirR9WQqIZNGOetijwCRkJKVw3+7SKAekFasL
Emc6y+nFFUAJ9C155PjVdgGbus+GPWjJwuDa4qNIG+2khV8ddKRHTO6ouo+ubdjP
FQwlOJmTvMLDdeeww4WJC6b5xwDfteJaiMBUnfUSuQgrTriWxMdrP6+/BG8+aP0E
iFuPmP4aOOEIe3JTScxoVaDmWRxYu1DxfRv5rjhSAamyTjRHuIRpwJKY6g9kl0PM
5jLPiNWbAgMBAAECggEABvCqo/aCNFrR6FI/hV9mn8qlM5ZNgCYpQW/G7rpT6bGv
GDC3GUWUTj8lym5fYybPutJFPb6VMZbAT6AB4ZHa/v4d6VVJIux1kXIdn4aAu1FB
9Nkine6erGBNwegd3C1pzc/oxKX/ZygYHSSry9IqV+iaFBvMjWqkgX4PqFGQT12u
ZyhmC6PpACKD5KkBItswBHE2+6/dF3YGGhwLdswfMwOSprh9yOK+1GfuSPQAxz+m
H5Z0TLSaZUWfePkW9Bckpqy/IrfE/MkaAadpgOAUnYuD/VsvE+s6BGy3bZNsMnfV
ycfFJsZyh4CTPOVIJXlh4qhR+MHTdvibM2cwisk0CQKBgQDHg+J3uC3AJVkNQbkj
CCUnPjZQzyngKz18o6V/SdYcCmZa3x346xhgf3tZCi6JekangY1gcNswtHb+v1gP
OOBTM9rVx6ZEfVbJjxXJL1eo7fYOMDIiwxx7biPGoocRygs9tOczsLyotoHBfnu4
2NTGNpWzoLILthxcHT6YUuIjfQKBgQDAgH8A9YF22WTVemTWrh+JOfXuBZKIq3xz
duLn5hZojiRNvxevmxfYdr1A8RAktM1SuYVOVyUa9b7Oryj3krBKZ6IPS14UTVJd
yOzjBaC18mg+GNd5d0ThixdvR8YR1B/iAuSHU97YbceQXwM609ZtVEeOZ8ilET47
phuho2549wKBgBHpqV+OTa0+rSwfhnu6s3s+fqnqr5vMxgx2tNkky/BgjqgB+DLt
CJdBWtW3sE3viIBphUPvXf6VdUmR7FoRhTJzKWagMjFb/3rBOfeCJKCqHLlRYm70
jY4XaWbgGLhkJ3Y207s0Vvvexu+UZ49FCHVzWSVzfAjJJOqwRXpvlpYRAoGBAIAP
e23Z5Ox5prjVmWuvnk8d/dsAXgEJg9sURdLboCCiHsiRzWIHHQ/lvfUL6R9iIA1e
Wa5KDucmgG7i27AHJ7v3wTnHe6ip35eAaQaF4enofyyAPigSDgK1Ezv2VW/a2/Lw
1bKpOuBj95S1gGKMNmO443l61WHqXKo10L/xBzb1AoGBAIefdHC6nULpcLA3hZOm
+dSeN4f/d6rxyEQR6rD/itqjqGCZVw7giFRnHywk6JSKAuC/zeC/aDUaltyePbe7
ygV7sRNqXIBDTIQ2W00S1AadrU1J69b9jfPi+aXWZGDPK6C0UxWvcB1YAPK03sHP
7no3gRoqa7cX+V59YwEV8kCq
-----END PRIVATE KEY-----"#;

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

    fn jwk(kid: &str) -> JsonWebKey {
        JsonWebKey {
            kid: Some(kid.to_owned()),
            kty: "RSA".to_owned(),
            alg: Some("RS256".to_owned()),
            n: "lgcOxjKt6SyxJKIAsyazyxPfFlXUINj3NlO9yKf7KRbWeNk8xN0Ikftxe41szjlh5MDe7ZrchU0WS9sV1bNRchAB-uwG4riE51gpy_AliIq0fVkKiGTRjnrYo8AkZCSlcN_u0igHpBWrCxJnOsvpxRVACfQteeT41XYBm7rPhj1oycLg2uKjSBvtpIVfHXSkR0zuqLqPrm3YzxUMJTiZk7zCw3XnsMOFiQum-ccA37XiWojAVJ31ErkIK064lsTHaz-vvwRvPmj9BIhbj5j-GjjhCHtyU0nMaFWg5lkcWLtQ8X0b-a44UgGpsk40R7iEacCSmOoPZJdDzOYyz4jVmw".to_owned(),
            e: "AQAB".to_owned(),
        }
    }

    fn token(kid: &str, claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_owned());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(PRIVATE_KEY.as_bytes()).expect("private key"),
        )
        .expect("token")
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

    fn jwks_json(kid: &str) -> String {
        format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"{kid}","alg":"RS256","use":"sig","n":"{}","e":"AQAB"}}]}}"#,
            jwk(kid).n
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
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let token = token("test-kid", valid_claims());

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
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let mut claims = valid_claims();
        claims["aud"] = json!("https://graph.microsoft.com");
        let token = token("test-kid", claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraAudience);
    }

    #[tokio::test]
    async fn rejects_invalid_signature() {
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let mut token = token("test-kid", valid_claims());
        token.push('x');

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraToken);
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let mut claims = valid_claims();
        claims["exp"] = json!(Utc::now().timestamp() - 120);
        let token = token("test-kid", claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::ExpiredEntraToken);
    }

    #[tokio::test]
    async fn rejects_missing_required_scope() {
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let mut claims = valid_claims();
        claims["scp"] = json!("other.scope");
        let token = token("test-kid", claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }

    #[tokio::test]
    async fn rejects_group_overage() {
        let verifier = EntraJwtVerifier::new_with_jwks_for_tests(config(), vec![jwk("test-kid")]);
        let mut claims = valid_claims();
        claims["hasgroups"] = json!(true);
        let token = token("test-kid", claims);

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InsufficientEntraAuthorization);
    }

    #[tokio::test]
    async fn fetches_mock_oidc_metadata_and_jwks() {
        let discovery_url = start_mock_oidc(jwks_json("test-kid"));
        let mut config = config();
        config.oidc_discovery_url = discovery_url;
        let verifier = EntraJwtVerifier::new(config).expect("verifier");
        let token = token("test-kid", valid_claims());

        let identity = verifier
            .verify_token(&token, Utc::now())
            .await
            .expect("identity");

        assert_eq!(identity.tenant_id, "tenant-1");
        assert_eq!(identity.source, EntraIdentitySource::Jwt);
    }

    #[tokio::test]
    async fn rejects_unknown_kid_after_jwks_refresh() {
        let discovery_url = start_mock_oidc(jwks_json("different-kid"));
        let mut config = config();
        config.oidc_discovery_url = discovery_url;
        let verifier = EntraJwtVerifier::new(config).expect("verifier");
        let token = token("test-kid", valid_claims());

        let error = verifier.verify_token(&token, Utc::now()).await.unwrap_err();

        assert_eq!(error, GatewayError::InvalidEntraToken);
    }

    #[test]
    fn verifies_apigee_trusted_identity_signature() {
        let config = ApigeeTrustedHeaderConfig {
            secret: "trusted-secret".to_owned(),
        };
        let identity = EntraIdentityContext {
            tenant_id: "tenant-1".to_owned(),
            subject: Some("subject-1".to_owned()),
            object_id: None,
            app_id: None,
            authorized_party: None,
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
}
