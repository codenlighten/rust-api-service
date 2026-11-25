//! Notes REST API: axum, sqlx/SQLite, argon2 password hashing, signed bearer tokens.
//!
//! The library exposes [`app::router`] so tests can drive the real router in-process.

pub mod app;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod extract;
pub mod openapi;
pub mod password;
pub mod routes;
pub mod token;
pub mod validate;
