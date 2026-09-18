//! unbagrnd-api as a library: everything except the `main()` entry point,
//! so integration tests (and anything else that wants to build the router
//! directly) don't have to shell out to the compiled binary.

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod openapi;
pub mod routes;
pub mod state;
