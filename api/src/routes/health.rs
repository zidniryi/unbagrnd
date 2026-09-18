use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    status: &'static str,
}

/// Liveness check. Never requires an API key.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses((status = 200, description = "The server is up", body = HealthResponse))
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
