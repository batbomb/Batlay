//! Live TikTok : lecture du flux du live par la page TikTok ouverte dans une
//! fenêtre invisible (WebView2, voir src-tauri), sans service tiers ; ou, au
//! choix, par le service de signature Euler Stream (`euler.rs`).

pub mod model;
pub mod browser;
pub mod euler;
pub mod proto;
pub mod service;
pub mod webcast;
