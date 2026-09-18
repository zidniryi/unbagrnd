//! `POST /v1/remove-background` - the actual product. Everything else in
//! this crate exists to gate and support this one endpoint.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Multipart, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use image::{DynamicImage, ImageFormat};
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::ApiKeyAuth;
use crate::error::{ApiError, ErrorDetail};
use crate::state::AppState;

/// Purely documentary - the real request is a raw `multipart/form-data`
/// body with a single `image` field, extracted with [`Multipart`] below.
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct RemoveBackgroundRequest {
    /// The source photo. PNG, JPEG, WebP, BMP, TIFF and GIF are all
    /// accepted - the actual bytes are sniffed rather than trusting a
    /// filename or declared content type.
    #[schema(value_type = String, format = Binary)]
    image: Vec<u8>,
}

#[utoipa::path(
    post,
    path = "/v1/remove-background",
    tag = "background-removal",
    request_body(content = RemoveBackgroundRequest, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Transparent PNG with the background removed", content_type = "image/png"),
        (status = 400, description = "Missing/unreadable image", body = crate::error::ErrorBody),
        (status = 401, description = "Missing/invalid X-API-Key", body = crate::error::ErrorBody),
        (status = 413, description = "Image exceeds UNBAGRND_MAX_FILE_SIZE_MB", body = crate::error::ErrorBody),
        (status = 422, description = "Background removal failed", body = crate::error::ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = crate::error::ErrorBody),
    ),
    security(("api_key" = []))
)]
pub async fn remove_background(
    auth: ApiKeyAuth,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    tracing::info!(key_prefix = %auth.key_prefix, key_id = %auth.key_id, "remove-background request");

    let image_bytes = extract_image_field(&mut multipart, state.config.max_body_bytes()).await?;
    let original = decode_image(&state, &image_bytes)?;
    let png_bytes = run_inference(&state, original).await?;

    Ok((
        [(header::CONTENT_TYPE, "image/png")],
        Bytes::from(png_bytes),
    )
        .into_response())
}

/// Purely documentary - the real request is a raw `multipart/form-data`
/// body with one or more `images` fields, extracted with [`Multipart`]
/// below.
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct RemoveBackgroundBatchRequest {
    /// Up to `UNBAGRND_MAX_BATCH_SIZE` source photos, same accepted formats
    /// as the single-image endpoint.
    #[schema(value_type = Vec<String>, format = Binary)]
    images: Vec<Vec<u8>>,
}

#[derive(Serialize, ToSchema)]
pub struct RemoveBackgroundBatchResponse {
    /// One entry per uploaded image, in the same order they were uploaded.
    pub results: Vec<BatchItemResult>,
}

#[derive(Serialize, ToSchema)]
pub struct BatchItemResult {
    /// The multipart field's filename, if the client sent one.
    pub filename: Option<String>,
    #[serde(flatten)]
    pub outcome: BatchItemOutcome,
}

#[derive(Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum BatchItemOutcome {
    Ok {
        /// Standard base64-encoded transparent PNG.
        image_base64: String,
    },
    Error {
        error: ErrorDetail,
    },
}

#[utoipa::path(
    post,
    path = "/v1/remove-background/batch",
    tag = "background-removal",
    request_body(content = RemoveBackgroundBatchRequest, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Per-image results, in upload order - a single unreadable image doesn't fail the whole batch", body = RemoveBackgroundBatchResponse),
        (status = 400, description = "No \"images\" field, or more images than UNBAGRND_MAX_BATCH_SIZE", body = crate::error::ErrorBody),
        (status = 401, description = "Missing/invalid X-API-Key", body = crate::error::ErrorBody),
        (status = 413, description = "One of the images exceeds UNBAGRND_MAX_FILE_SIZE_MB", body = crate::error::ErrorBody),
        (status = 429, description = "Rate limit exceeded", body = crate::error::ErrorBody),
    ),
    security(("api_key" = []))
)]
pub async fn remove_background_batch(
    auth: ApiKeyAuth,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<RemoveBackgroundBatchResponse>, ApiError> {
    tracing::info!(key_prefix = %auth.key_prefix, key_id = %auth.key_id, "remove-background batch request");

    let items = extract_images_field(
        &mut multipart,
        state.config.max_body_bytes(),
        state.config.max_batch_size,
    )
    .await?;

    // Each item runs as its own task, all bounded by the same
    // `inference_semaphore` a single request uses - so a batch of N never
    // lets more than `UNBAGRND_MAX_CONCURRENT_INFERENCES` inferences run at
    // once, matching the limit's documented intent regardless of how many
    // requests (single or batch) are in flight.
    let mut handles = Vec::with_capacity(items.len());
    for (filename, bytes) in items {
        let state = Arc::clone(&state);
        handles.push(tokio::spawn(async move {
            let outcome = match decode_image(&state, &bytes) {
                Ok(original) => match run_inference(&state, original).await {
                    Ok(png_bytes) => BatchItemOutcome::Ok {
                        image_base64: base64::engine::general_purpose::STANDARD.encode(png_bytes),
                    },
                    Err(e) => BatchItemOutcome::Error {
                        error: e.to_detail(),
                    },
                },
                Err(e) => BatchItemOutcome::Error {
                    error: e.to_detail(),
                },
            };
            BatchItemResult { filename, outcome }
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(
            handle
                .await
                .map_err(|e| ApiError::Internal(format!("batch item task panicked: {e}")))?,
        );
    }

    Ok(Json(RemoveBackgroundBatchResponse { results }))
}

fn decode_image(state: &AppState, bytes: &[u8]) -> Result<DynamicImage, ApiError> {
    let original = image::load_from_memory(bytes).map_err(|_| ApiError::UnsupportedImage)?;

    if original.width() > state.config.max_image_width
        || original.height() > state.config.max_image_height
    {
        return Err(ApiError::InvalidImage(format!(
            "image dimensions {}x{} exceed the {}x{} limit",
            original.width(),
            original.height(),
            state.config.max_image_width,
            state.config.max_image_height
        )));
    }

    Ok(original)
}

/// Runs one image through the model, bounded by `state.inference_semaphore`.
async fn run_inference(state: &Arc<AppState>, original: DynamicImage) -> Result<Vec<u8>, ApiError> {
    // `acquire_owned` (rather than `acquire`) so the permit doesn't borrow
    // from `state`, which needs to move into `spawn_blocking` below.
    let permit = Arc::clone(&state.inference_semaphore)
        .acquire_owned()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let state = Arc::clone(state);
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ApiError> {
        let _permit = permit;
        let result = unbagrnd_core::bg_remove::remove_background(
            &state.inference,
            state.model_spec,
            &state.model_path,
            &original,
        )
        .map_err(ApiError::InferenceError)?;

        let mut png_bytes = Vec::new();
        result
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), ImageFormat::Png)
            .map_err(|e| ApiError::Internal(format!("could not encode output image: {e}")))?;
        Ok(png_bytes)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("inference task panicked: {e}")))?
}

/// Pulls the `image` field out of a multipart body, enforcing the
/// configured size limit as it streams (rather than after buffering the
/// whole thing), and rejecting a request with no `image` field at all.
async fn extract_image_field(
    multipart: &mut Multipart,
    max_bytes: usize,
) -> Result<Vec<u8>, ApiError> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::InvalidImage(e.to_string()))?
    {
        if field.name() != Some("image") {
            continue;
        }

        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::InvalidImage(e.to_string()))?;

        if bytes.len() > max_bytes {
            return Err(ApiError::ImageTooLarge {
                max_mb: (max_bytes / (1024 * 1024)) as u64,
            });
        }
        if bytes.is_empty() {
            return Err(ApiError::InvalidImage(
                "the \"image\" field was empty".to_string(),
            ));
        }

        return Ok(bytes.to_vec());
    }

    Err(ApiError::InvalidImage(
        "expected a multipart/form-data body with an \"image\" field".to_string(),
    ))
}

/// Pulls every `images` field out of a multipart body (one request can send
/// the field multiple times, once per file), enforcing the per-file size
/// limit as it streams and the batch's file-count limit as it goes - so an
/// oversized batch is rejected as soon as the limit is crossed, without
/// buffering files past it.
async fn extract_images_field(
    multipart: &mut Multipart,
    max_bytes_per_file: usize,
    max_batch_size: usize,
) -> Result<Vec<(Option<String>, Vec<u8>)>, ApiError> {
    let mut items = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::InvalidImage(e.to_string()))?
    {
        if field.name() != Some("images") {
            continue;
        }

        if items.len() >= max_batch_size {
            return Err(ApiError::BatchTooLarge {
                max: max_batch_size,
            });
        }

        let filename = field.file_name().map(str::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::InvalidImage(e.to_string()))?;

        if bytes.len() > max_bytes_per_file {
            return Err(ApiError::ImageTooLarge {
                max_mb: (max_bytes_per_file / (1024 * 1024)) as u64,
            });
        }
        if bytes.is_empty() {
            continue;
        }

        items.push((filename, bytes.to_vec()));
    }

    if items.is_empty() {
        return Err(ApiError::NoImages);
    }

    Ok(items)
}
