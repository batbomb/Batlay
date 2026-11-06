//! # batlay-core
//!
//! Cœur de Batlay, écrit en Rust avec la seule bibliothèque standard.
//! Voir docs/DEVELOPPEMENT.md, section « Architecture Rust ».

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod alerts;
pub mod app;
pub mod brand_page;
pub mod config;
pub mod hosts_alias;
pub mod integrations;
pub mod json;
pub mod live;
pub mod log;
pub mod loopback;
pub mod net;
pub mod overlay_server;
pub mod runtime;
pub mod spotify;
pub mod system_media;
pub mod theme;
pub mod tiktok;
pub mod twitch;
pub mod updater;
pub mod util;
