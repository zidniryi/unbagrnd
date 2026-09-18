//! OpenAPI schema generation (`utoipa`), served as interactive docs at
//! `/docs`. Generated straight from the `#[utoipa::path]` annotations on
//! each handler, so it can't go stale the way a hand-maintained spec file
//! would.

use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use crate::error::{ErrorBody, ErrorDetail};
use crate::routes::health::{self, HealthResponse};
use crate::routes::keys::{
    self, CreateKeyRequest, CreateKeyResponse, ListKeysEntry, ListKeysResponse,
};
use crate::routes::remove_background::{
    self, BatchItemOutcome, BatchItemResult, RemoveBackgroundBatchRequest,
    RemoveBackgroundBatchResponse, RemoveBackgroundRequest,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "unbagrnd API",
        description = "Self-hosted, on-device background removal over HTTP. \
            No cloud, no account - every image is processed locally by this \
            server and never sent anywhere else.",
        license(name = "MIT")
    ),
    paths(
        health::health,
        keys::create_key,
        keys::list_keys,
        keys::revoke_key,
        remove_background::remove_background,
        remove_background::remove_background_batch,
    ),
    components(schemas(
        HealthResponse,
        CreateKeyRequest,
        CreateKeyResponse,
        ListKeysEntry,
        ListKeysResponse,
        RemoveBackgroundRequest,
        RemoveBackgroundBatchRequest,
        RemoveBackgroundBatchResponse,
        BatchItemResult,
        BatchItemOutcome,
        ErrorBody,
        ErrorDetail,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness check - never requires auth"),
        (name = "keys", description = "API key management - requires X-Admin-Key"),
        (name = "background-removal", description = "The actual product - requires X-API-Key"),
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::default);
        components.add_security_scheme(
            "api_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-API-Key"))),
        );
        components.add_security_scheme(
            "admin_key",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("X-Admin-Key"))),
        );
    }
}

pub fn swagger_ui() -> SwaggerUi {
    SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi())
}
