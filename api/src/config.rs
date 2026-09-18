//! Server configuration, read entirely from environment variables (see
//! `.env.example` at the repo root for the full list and defaults).

use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub models_dir: PathBuf,
    pub model_key: String,
    /// Gates every `/v1/keys` route. `None` means key management is
    /// disabled entirely (see `ApiError::AdminKeyNotConfigured`) rather
    /// than falling back to some default, insecure value.
    pub admin_key: Option<String>,
    pub max_file_size_mb: u64,
    pub max_image_width: u32,
    pub max_image_height: u32,
    pub max_concurrent_inferences: usize,
    /// Max number of images accepted per `/v1/remove-background/batch`
    /// request - independent of `max_concurrent_inferences`, which bounds
    /// how many of them (across all in-flight requests) actually run at
    /// once.
    pub max_batch_size: usize,
    pub rate_limit_per_minute: u32,
    /// `*` or a comma-separated list of allowed origins.
    pub cors_origins: String,
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parsed<T: FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let admin_key = std::env::var("UNBAGRND_ADMIN_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty());

        Self {
            host: env_string("HOST", "0.0.0.0"),
            port: env_parsed("PORT", 8080),
            database_url: env_string("DATABASE_URL", "sqlite:///data/unbagrnd.db"),
            models_dir: PathBuf::from(env_string("UNBAGRND_MODELS_DIR", "/models")),
            model_key: env_string(
                "UNBAGRND_MODEL_KEY",
                unbagrnd_core::models::DEFAULT_MODEL_KEY,
            ),
            admin_key,
            max_file_size_mb: env_parsed("UNBAGRND_MAX_FILE_SIZE_MB", 20),
            max_image_width: env_parsed("UNBAGRND_MAX_IMAGE_WIDTH", 8192),
            max_image_height: env_parsed("UNBAGRND_MAX_IMAGE_HEIGHT", 8192),
            max_concurrent_inferences: env_parsed("UNBAGRND_MAX_CONCURRENT_INFERENCES", 2),
            max_batch_size: env_parsed("UNBAGRND_MAX_BATCH_SIZE", 10),
            rate_limit_per_minute: env_parsed("UNBAGRND_RATE_LIMIT_PER_MINUTE", 60),
            cors_origins: env_string("UNBAGRND_CORS_ORIGINS", "*"),
        }
    }

    pub fn max_body_bytes(&self) -> usize {
        (self.max_file_size_mb as usize).saturating_mul(1024 * 1024)
    }

    /// Body size cap for the batch route: enough room for `max_batch_size`
    /// images each up to `max_file_size_mb`, plus multipart framing
    /// overhead. Individual files are still checked against
    /// `max_body_bytes()` one at a time as they stream in.
    pub fn max_batch_body_bytes(&self) -> usize {
        self.max_body_bytes()
            .saturating_mul(self.max_batch_size)
            .saturating_add(1024 * 1024)
    }

    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
