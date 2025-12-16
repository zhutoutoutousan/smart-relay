use axum::body::Body;
use axum::http::Request;
use smart_relay::{
    ai::AiClient,
    api::{router, AppState},
    config::{ConfigStore, SmartConfig},
};
use std::path::PathBuf;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_ok() {
    let state = AppState {
        cfg: ConfigStore::from_config(SmartConfig::default(), PathBuf::from("test.toml")),
        ai: AiClient::default(),
    };
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(res.status().is_success());
}

#[tokio::test]
async fn config_endpoint_returns_default() {
    let state = AppState {
        cfg: ConfigStore::from_config(SmartConfig::default(), PathBuf::from("test.toml")),
        ai: AiClient::default(),
    };
    let app = router(state);
    let res = app
        .oneshot(Request::builder().uri("/config").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(res.status().is_success());
}

