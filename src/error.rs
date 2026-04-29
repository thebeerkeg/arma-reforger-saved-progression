use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("player not found: {0}")]
    NotFound(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for BridgeError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            BridgeError::NotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            BridgeError::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            BridgeError::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            BridgeError::Database(_) | BridgeError::Internal(_) => {
                tracing::error!("internal error: {self:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

pub type BridgeResult<T> = Result<T, BridgeError>;
