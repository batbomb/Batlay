//! Twitch : connexion du compte (OAuth sans secret), chat et alertes par
//! EventSub, serveur local du chat (port 4000) pour OBS.

pub mod auth;
pub mod chat;
pub mod eventsub;
pub mod model;
pub mod oauth;
pub mod server;
pub mod service;

pub use service::{TwitchConfig, TwitchHost, TwitchService};
