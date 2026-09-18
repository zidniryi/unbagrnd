//! A single error type for every route, with a consistent JSON shape and
//! HTTP status. Internal details (Rust error text, filesystem paths, model
//! internals) are logged server-side via `tracing` and never sent to the
//! caller - see [`ApiError::message`].

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug)]
pub enum ApiError {
    MissingApiKey,
    InvalidApiKey,
    AdminKeyRequired,
    AdminKeyNotConfigured,
    KeyNotFound,
    InvalidImage(String),
    ImageTooLarge { max_mb: u64 },
    UnsupportedImage,
    InferenceError(String),
    NoImages,
    BatchTooLarge { max: usize },
    RateLimited,
    Internal(String),
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    /// A stable, machine-readable error code, e.g. `INVALID_API_KEY`.
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            ApiError::MissingApiKey => "MISSING_API_KEY",
            ApiError::InvalidApiKey => "INVALID_API_KEY",
            ApiError::AdminKeyRequired => "ADMIN_KEY_REQUIRED",
            ApiError::AdminKeyNotConfigured => "ADMIN_KEY_NOT_CONFIGURED",
            ApiError::KeyNotFound => "KEY_NOT_FOUND",
            ApiError::InvalidImage(_) => "INVALID_IMAGE",
            ApiError::ImageTooLarge { .. } => "IMAGE_TOO_LARGE",
            ApiError::UnsupportedImage => "UNSUPPORTED_IMAGE",
            ApiError::InferenceError(_) => "INFERENCE_ERROR",
            ApiError::NoImages => "NO_IMAGES",
            ApiError::BatchTooLarge { .. } => "BATCH_TOO_LARGE",
            ApiError::RateLimited => "RATE_LIMITED",
            ApiError::Internal(_) => "INTERNAL_ERROR",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::MissingApiKey | ApiError::InvalidApiKey => StatusCode::UNAUTHORIZED,
            ApiError::AdminKeyRequired => StatusCode::UNAUTHORIZED,
            ApiError::AdminKeyNotConfigured => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::KeyNotFound => StatusCode::NOT_FOUND,
            ApiError::InvalidImage(_) | ApiError::UnsupportedImage => StatusCode::BAD_REQUEST,
            ApiError::ImageTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::InferenceError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::NoImages | ApiError::BatchTooLarge { .. } => StatusCode::BAD_REQUEST,
            ApiError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// The message sent to the API caller. Deliberately generic for
    /// anything that could leak internals - the real detail (for
    /// `Internal`/`InferenceError`) is logged separately in
    /// [`IntoResponse::into_response`].
    fn public_message(&self) -> String {
        match self {
            ApiError::MissingApiKey => "missing X-API-Key header".to_string(),
            ApiError::InvalidApiKey => "invalid or revoked API key".to_string(),
            ApiError::AdminKeyRequired => "missing or invalid X-Admin-Key header".to_string(),
            ApiError::AdminKeyNotConfigured => {
                "key management is disabled: UNBAGRND_ADMIN_KEY is not configured on this server"
                    .to_string()
            }
            ApiError::KeyNotFound => "no such API key".to_string(),
            ApiError::InvalidImage(msg) => msg.clone(),
            ApiError::ImageTooLarge { max_mb } => {
                format!("image exceeds the {max_mb} MB upload limit")
            }
            ApiError::UnsupportedImage => "unsupported or unrecognized image format".to_string(),
            ApiError::InferenceError(_) => "background removal failed".to_string(),
            ApiError::NoImages => {
                "expected a multipart/form-data body with at least one \"images\" field".to_string()
            }
            ApiError::BatchTooLarge { max } => {
                format!("batch exceeds the {max}-image limit per request")
            }
            ApiError::RateLimited => "rate limit exceeded, please slow down".to_string(),
            ApiError::Internal(_) => "internal server error".to_string(),
        }
    }
}

impl ApiError {
    /// The [`ErrorDetail`] this error would produce, logging the internal
    /// detail server-side first if there is one. Used both for the
    /// top-level error response ([`IntoResponse`] below) and for a single
    /// item's failure inside an otherwise-200 batch response.
    pub fn to_detail(&self) -> ErrorDetail {
        match self {
            ApiError::Internal(detail) => tracing::error!(%detail, "internal error"),
            ApiError::InferenceError(detail) => tracing::error!(%detail, "inference error"),
            _ => {}
        }

        ErrorDetail {
            code: self.code(),
            message: self.public_message(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = ErrorBody {
            error: self.to_detail(),
        };
        (status, Json(body)).into_response()
    }
}
