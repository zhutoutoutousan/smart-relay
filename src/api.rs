use crate::{
    ai::AiClient,
    config::{ConfigStore, ProxyEndpoint, SmartConfig},
};
use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::net::TcpListener;
use tracing::info;

type ApiResult<T> = Result<T, (StatusCode, String)>;

#[derive(Clone)]
pub struct AppState {
    pub cfg: ConfigStore,
    pub ai: AiClient,
}

pub async fn serve(listen: String, cfg: ConfigStore, ai: AiClient) -> Result<()> {
    let state = AppState { cfg, ai };
    let app = router(state);

    let listener = TcpListener::bind(&listen).await?;
    info!("control plane listening on {}", listen);
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ai/health", get(ai_health))
        .route("/config", get(get_config))
        .route("/config/endpoints", post(add_endpoint))
        .route("/ai/propose", post(propose))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}

async fn ai_health(State(state): State<AppState>) -> ApiResult<&'static str> {
    state.ai.health().await.map_err(internal_error)?;
    Ok("ok")
}

async fn get_config(State(state): State<AppState>) -> ApiResult<Json<SmartConfig>> {
    let cfg = state.cfg.get().await;
    Ok(Json(cfg))
}

#[derive(Deserialize)]
struct AddEndpointRequest {
    name: String,
    host: String,
    port: u16,
}

async fn add_endpoint(
    State(state): State<AppState>,
    Json(req): Json<AddEndpointRequest>,
) -> ApiResult<impl IntoResponse> {
    let endpoint = ProxyEndpoint {
        id: uuid::Uuid::new_v4(),
        name: req.name,
        host: req.host,
        port: req.port,
        protocol: crate::config::ProxyProtocol::Vmess,
        tags: vec!["manual".into()],
    };
    state
        .cfg
        .add_endpoint(endpoint)
        .await
        .map_err(internal_error)?;
    state.cfg.save().await.map_err(internal_error)?;
    Ok("added")
}

#[derive(Deserialize)]
struct ProposeBody {
    goal: Option<String>,
}

async fn propose(
    State(state): State<AppState>,
    Json(body): Json<ProposeBody>,
) -> ApiResult<impl IntoResponse> {
    let snapshot = state.cfg.get().await;
    let goal = body
        .goal
        .unwrap_or_else(|| "low-latency balanced routing with censorship resistance".to_string());
    let plan = state
        .ai
        .propose_profile(&goal, Some(snapshot))
        .await
        .map_err(internal_error)?;
    Ok(plan)
}

fn internal_error(err: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

