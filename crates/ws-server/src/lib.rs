//! # ws-server
//!
//! The platform's network face: a hand-written HTTP/1.1 + WebSocket server on
//! `std::net`, exposing every WorldSim capability as a versioned REST API plus
//! live state streams (WebSocket and SSE).
//!
//! Why hand-written? Zero third-party dependencies keeps the supply chain
//! auditable and the build reproducible; the subset of HTTP we need is small
//! and covered by integration tests.
//!
//! Modules:
//! - [`http`]: request parsing / response writing, connection loop
//! - [`router`]: path-pattern routing
//! - [`api`]: the actual `/v1` endpoints, world registry, scheduler hub

pub mod api;
pub mod http;
pub mod router;
pub mod static_assets;

#[cfg(test)]
mod api_tests;
