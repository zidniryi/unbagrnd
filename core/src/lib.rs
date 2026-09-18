//! Shared on-device background-removal core for unbagrnd.
//!
//! This crate has no dependency on any particular host: it's used as-is by
//! both the Tauri desktop app (`src-tauri`) and the self-hosted REST API
//! (`api`). Everything here runs entirely locally via the `ort` ONNX Runtime
//! bindings - no image ever leaves the machine this code runs on.

pub mod bg_remove;
pub mod models;
