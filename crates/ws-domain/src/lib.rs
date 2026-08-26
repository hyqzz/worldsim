//! # ws-domain
//!
//! The vocabulary of WorldSim worlds: configuration ([`config`]), the material
//! library ([`materials`]), and the causal event language used to interact with
//! running worlds ([`events`]).
//!
//! This layer contains **no physics**. It defines *what can be said*; the
//! solver layer defines *what it means*.

pub mod config;
pub mod events;
pub mod materials;
pub mod wdl;
