//! Read-only spend snapshots. Never merge LiteLLM's resettable counter into usage.
use super::*;
use gateway_core::LiteLlmCredentialMappingScope;
use uuid::Uuid;

#[derive(Serialize)]
struct SpendSnapshot {
    key_id: Uuid,
    status: &'static str,
    mapping_scope: Option<LiteLlmCredentialMappingScope>,
    spend_usd: Option<f64>,
    fetched_at: Option<chrono::DateTime<Utc>>,
}

pub(super) async fn key_spend(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(key_id): Path<Uuid>,
) -> Response {
    if let Err(response) = require_admin_scope(&state, &headers, SCOPE_USAGE_READ).await {
        return response;
    }
    let mut response = match snapshot(&state, key_id).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => error_response(&headers, error),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn snapshot(state: &AppState, key_id: Uuid) -> GatewayResult<Option<SpendSnapshot>> {
    let Some(key) = state.store.get_admin_key(key_id).await? else {
        return Ok(None);
    };
    let mapping = state
        .store
        .litellm_credential_mapping_for_context(key_id, key.project_id)
        .await?;
    let mut result = SpendSnapshot {
        key_id,
        status: "not_mapped",
        mapping_scope: mapping.as_ref().map(|m| m.scope),
        spend_usd: None,
        fetched_at: None,
    };
    if let Some(credential) = mapping {
        result.status = "unavailable";
        // Includes response-body consumption; redirects are disabled by the client.
        if let Ok(Ok(spend)) = tokio::time::timeout(
            Duration::from_secs(5),
            fetch_spend(state, &credential.credential),
        )
        .await
        {
            result.status = "available";
            result.spend_usd = Some(spend);
            result.fetched_at = Some(Utc::now());
        }
    }
    Ok(Some(result))
}

async fn fetch_spend(state: &AppState, mapped_key: &str) -> Result<f64, ()> {
    let upstream = resolve_litellm_ui_upstream(state).await.map_err(|_| ())?;
    let mut url = reqwest::Url::parse(&upstream.base_url).map_err(|_| ())?;
    url.set_path("/key/info");
    url.set_query(None);
    url.set_fragment(None);
    // LiteLLM accepts the SHA-256 identifier; keep raw keys out of access logs.
    url.query_pairs_mut().append_pair(
        "key",
        &format!("{:x}", Sha256::digest(mapped_key.as_bytes())),
    );
    let mut request = state.litellm_ui_client.get(url);
    request = match upstream.credential_header_mode {
        CredentialHeaderMode::AuthorizationBearer => request.bearer_auth(&upstream.credential),
        CredentialHeaderMode::CustomHeader => request.header(
            upstream.credential_header_name.as_deref().ok_or(())?,
            litellm_ui_custom_header_credential(&upstream),
        ),
    };
    // Do not return/log request errors, URLs, upstream bodies or key metadata.
    let mut response = request.send().await.map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len() + chunk.len() > 64 * 1024 {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_spend(&bytes)
}

fn parse_spend(bytes: &[u8]) -> Result<f64, ()> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    value
        .get("info")
        .and_then(|v| v.get("spend"))
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_finite_nonnegative_spend_and_discards_secrets() {
        for (body, expected) in [
            (
                r#"{"key":"secret","info":{"spend":0,"token":"secret"}}"#,
                0.0,
            ),
            (r#"{"info":{"spend":12.345}}"#, 12.345),
        ] {
            assert_eq!(parse_spend(body.as_bytes()), Ok(expected));
        }
        for body in [
            "invalid",
            "{}",
            r#"{"info":null}"#,
            r#"{"info":{"spend":null}}"#,
            r#"{"info":{"spend":"12"}}"#,
            r#"{"info":{"spend":-1}}"#,
            r#"{"info":{"spend":1e999}}"#,
        ] {
            assert_eq!(parse_spend(body.as_bytes()), Err(()));
        }
        let serialized = serde_json::to_string(&SpendSnapshot {
            key_id: Uuid::nil(),
            status: "unavailable",
            mapping_scope: None,
            spend_usd: None,
            fetched_at: None,
        })
        .unwrap();
        assert!(!serialized.contains("secret"));
        assert!(serialized.contains("\"spend_usd\":null"));
    }
}
