use gateway_core::{
    AdminKeyCreate, AdminKeyPatch, AdminKeyStore, GatewayError, VirtualKeyMaterial,
};
use gateway_store::PostgresStore;
use sqlx::Row;
use uuid::Uuid;

#[tokio::test]
async fn key_names_persist_without_changing_credentials_or_legacy_keys() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping integration test: DATABASE_URL is not set");
        return;
    };
    let store = PostgresStore::connect(&database_url).await.unwrap();
    let material = VirtualKeyMaterial::generate().unwrap();
    let request: AdminKeyCreate = serde_json::from_value(serde_json::json!({
        "owner_type":"individual", "name":"  Production automation  "
    }))
    .unwrap();
    let created = store.create_admin_key(request, &material).await.unwrap();
    assert_eq!(created.name.as_deref(), Some("Production automation"));
    let legacy_id = Uuid::new_v4();
    let legacy_material = VirtualKeyMaterial::generate().unwrap();
    sqlx::query("INSERT INTO api_keys (id, owner_type, key_prefix, key_hash) VALUES ($1, 'individual', $2, $3)")
        .bind(legacy_id).bind(&legacy_material.key_prefix).bind(&legacy_material.key_hash)
        .execute(store.pool()).await.unwrap();
    assert_eq!(
        store.get_admin_key(legacy_id).await.unwrap().unwrap().name,
        None
    );

    for (patch, expected) in [
        (serde_json::json!({"name":"  ทีมงาน  "}), Some("ทีมงาน")),
        (serde_json::json!({"disabled":true}), Some("ทีมงาน")),
        (serde_json::json!({"name":null}), None),
        (
            serde_json::json!({"name":"Production automation"}),
            Some("Production automation"),
        ),
        (serde_json::json!({"name":"   "}), None),
    ] {
        let patch: AdminKeyPatch = serde_json::from_value(patch).unwrap();
        let changed = store
            .patch_admin_key(created.id, patch)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(changed.name.as_deref(), expected);
        assert_eq!(changed.id, created.id);
        assert_eq!(changed.key_prefix, created.key_prefix);
        assert_eq!(changed.policy, created.policy);
        let reopened = PostgresStore::connect(&database_url).await.unwrap();
        assert_eq!(
            reopened
                .get_admin_key(created.id)
                .await
                .unwrap()
                .unwrap()
                .name
                .as_deref(),
            expected
        );
        let keys = reopened.list_admin_keys().await.unwrap();
        assert_eq!(
            keys.iter()
                .find(|key| key.id == created.id)
                .unwrap()
                .name
                .as_deref(),
            expected
        );
    }
    for name in ["x".repeat(121), "bad\nname".into()] {
        let request =
            serde_json::from_value(serde_json::json!({"owner_type":"individual","name":name}))
                .unwrap();
        assert_eq!(
            store
                .create_admin_key(request, &material)
                .await
                .unwrap_err(),
            GatewayError::InvalidKeyPayload
        );
        let patch =
            serde_json::from_value(serde_json::json!({"name":name, "disabled":false})).unwrap();
        assert_eq!(
            store.patch_admin_key(created.id, patch).await.unwrap_err(),
            GatewayError::InvalidKeyPayload
        );
    }
    let row = sqlx::query("SELECT key_hash, disabled, name FROM api_keys WHERE id = $1")
        .bind(created.id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("key_hash"), material.key_hash);
    assert!(row.get::<bool, _>("disabled"));
    assert!(row.get::<Option<String>, _>("name").is_none());

    // The database constraint also prevents invalid length via non-API writers.
    assert!(sqlx::query("UPDATE api_keys SET name = $2 WHERE id = $1")
        .bind(created.id)
        .bind("x".repeat(121))
        .execute(store.pool())
        .await
        .is_err());
    sqlx::query("DELETE FROM api_keys WHERE id = ANY($1)")
        .bind(vec![created.id, legacy_id])
        .execute(store.pool())
        .await
        .unwrap();
}
