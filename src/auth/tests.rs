use tempfile::tempdir;

use super::{AdminCredential, AdminStore, UserConfig, read_user_config, write_user_config};

#[tokio::test]
async fn admin_store_bootstrap_add_authenticate_and_remove() {
    let temp = tempdir().expect("tempdir");
    let store = AdminStore::new(temp.path());
    let first = AdminCredential {
        admin_uuid: uuid::Uuid::new_v4(),
        admin_token: "first-token".into(),
    };
    assert!(store.bootstrap(&first).await.expect("bootstrap first"));
    assert!(
        store
            .authenticate(first.admin_uuid, &first.admin_token)
            .await
            .expect("auth first")
    );

    let second = store
        .add_admin("ws://127.0.0.1:7000".into())
        .await
        .expect("add admin");
    assert!(
        store
            .authenticate(second.admin_uuid, &second.admin_token)
            .await
            .expect("auth second")
    );

    store
        .remove_admin(first.admin_uuid)
        .await
        .expect("remove first");
    assert!(
        !store
            .authenticate(first.admin_uuid, &first.admin_token)
            .await
            .expect("auth removed first")
    );
    assert!(store.remove_admin(second.admin_uuid).await.is_err());
}

#[tokio::test]
async fn user_config_roundtrips() {
    let temp = tempdir().expect("tempdir");
    let path = temp.path().join("config.json");
    let config = UserConfig::new("ws://127.0.0.1:7000".into());
    write_user_config(&path, &config)
        .await
        .expect("write config");
    let restored = read_user_config(&path).await.expect("read config");
    assert_eq!(restored.admin_uuid, config.admin_uuid);
    assert_eq!(restored.admin_token, config.admin_token);
    assert_eq!(restored.endpoint, config.endpoint);
}
