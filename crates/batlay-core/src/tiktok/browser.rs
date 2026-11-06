//! Mode navigateur : la page du live TikTok est ouverte dans une fenêtre
//! INVISIBLE de Batlay (WebView2, voir src-tauri). Batlay n'y injecte rien : il
//! écoute, par le protocole DevTools propre à cette fenêtre, les trames du flux
//! « webcast » que TikTok envoie à la page, et les décode lui-même
//! (`webcast.rs`). Aucun service tiers ; la session TikTok de l'utilisateur
//! (s'il s'est connecté) est celle de la page.

use super::webcast::{decode_frame, is_webcast_socket_url, LiveEvent};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Ce que la fenêtre invisible signale (envoyé par l'application).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageEvent {
    SocketCreated { id: String, url: String },
    /// Trame binaire reçue par la page.
    Frame { id: String, data: Vec<u8> },
    SocketClosed { id: String },
    /// Fenêtre fermée, plantée, ou lecture du flux interrompue.
    Gone(String),
}

/// Fenêtre invisible ouverte (côté application).
pub trait LivePage: Send {
    fn close(&mut self);
    /// Affichée à l'utilisateur (« Afficher la page TikTok ») : on attend sans limite.
    fn is_visible(&self) -> bool;
}

/// Résultat de la sonde : ce que la page TikTok elle-même lit (mêmes cookies que le site).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeResult {
    /// Réponse de l'API « api-live » appelée DEPUIS la page.
    pub api: Option<(u16, String)>,
    /// Contenu du bloc SIGI_STATE.
    pub sigi: Option<String>,
    pub href: String,
    pub title: String,
}

/// Fenêtres TikTok fournies par l'application (WebView2) ; simulées en test.
pub trait PageHost: Send + Sync {
    /// Ouvre la page du live, invisible. `lite` : sans vidéo ni images (essayé en premier).
    fn open_live_page(&self, username: &str, lite: bool, events: Sender<PageEvent>) -> Result<Box<dyn LivePage>, String>;
    /// Ouvre la page, lit api-live et SIGI_STATE depuis la page, referme (20 s au plus).
    fn probe(&self, username: &str) -> Result<ProbeResult, String>;
    /// Session TikTok connectée dans Batlay (présence du cookie, jamais sa valeur).
    fn logged_in(&self) -> bool;
}

pub struct Timing {
    /// Délai pour voir apparaître le flux après l'ouverture de la page.
    pub stream_timeout: Duration,
    /// Après la fermeture du flux, délai laissé à la page pour le rouvrir.
    pub reopen_grace: Duration,
    /// Réveil régulier (arrêt demandé).
    pub tick: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { stream_timeout: Duration::from_secs(25), reopen_grace: Duration::from_secs(20), tick: Duration::from_millis(500) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Aucun flux reçu (pas en live, page bloquée…).
    NoStream(String),
    /// Le live était branché puis s'est arrêté.
    Lost(String),
    /// Fin du live annoncée par TikTok.
    Ended,
    /// Arrêt demandé par Batlay.
    Stopped,
}

/// Fait vivre la connexion (sur le fil appelant) : attend le flux, puis
/// décode chaque trame et appelle `on_event`. `on_connected` est appelé une fois.
pub fn run(
    mut page: Box<dyn LivePage>,
    rx: Receiver<PageEvent>,
    stop: Arc<AtomicBool>,
    timing: Timing,
    mut on_connected: impl FnMut(),
    mut on_event: impl FnMut(LiveEvent),
) -> Outcome {
    let outcome = drive(page.as_mut(), &rx, &stop, &timing, &mut on_connected, &mut on_event);
    page.close();
    outcome
}

fn drive(page: &mut dyn LivePage, rx: &Receiver<PageEvent>, stop: &AtomicBool, timing: &Timing, on_connected: &mut impl FnMut(), on_event: &mut impl FnMut(LiveEvent)) -> Outcome {
    let mut sockets: HashSet<String> = HashSet::new();
    let mut connected = false;
    let mut deadline = Some(Instant::now() + timing.stream_timeout);
    let mut grace_until: Option<Instant> = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Outcome::Stopped;
        }
        let now = Instant::now();
        if let Some(d) = deadline {
            if now >= d {
                if page.is_visible() {
                    // L'utilisateur regarde la page (vérification à faire) : on lui laisse le temps.
                    deadline = Some(now + timing.stream_timeout);
                } else {
                    return Outcome::NoStream("Page du live ouverte, mais aucun flux du live reçu.".into());
                }
            }
        }
        if let Some(g) = grace_until {
            if now >= g && sockets.is_empty() {
                return Outcome::Lost("flux du live fermé par la page".into());
            }
        }
        let event = match rx.recv_timeout(timing.tick) {
            Ok(e) => e,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return if connected { Outcome::Lost("fenêtre du live fermée".into()) } else { Outcome::NoStream("fenêtre du live fermée".into()) };
            }
        };
        match event {
            PageEvent::SocketCreated { id, url } => {
                if !is_webcast_socket_url(&url) {
                    continue;
                }
                sockets.insert(id);
                grace_until = None;
                if !connected {
                    connected = true;
                    deadline = None;
                    on_connected();
                }
            }
            PageEvent::Frame { id, data } => {
                if !sockets.contains(&id) {
                    continue;
                }
                match decode_frame(&data) {
                    Ok(events) => {
                        for e in events {
                            let end = e == LiveEvent::StreamEnd;
                            on_event(e);
                            if end {
                                return Outcome::Ended;
                            }
                        }
                    }
                    Err(e) => crate::log::warn(&format!("[TikTok] {e}")),
                }
            }
            PageEvent::SocketClosed { id } => {
                if sockets.remove(&id) && sockets.is_empty() {
                    // La page rouvre souvent son flux d'elle-même (changement de serveur).
                    grace_until = Some(Instant::now() + timing.reopen_grace);
                }
            }
            PageEvent::Gone(reason) => {
                if stop.load(Ordering::SeqCst) {
                    return Outcome::Stopped;
                }
                return if connected { Outcome::Lost(reason) } else { Outcome::NoStream(format!("Page du live fermée : {reason}")) };
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::encoding::base64_decode;
    use std::sync::mpsc::channel;
    use std::sync::Mutex;

    pub struct FakePage {
        pub closed: Arc<Mutex<u32>>,
        pub visible: Arc<AtomicBool>,
    }
    impl LivePage for FakePage {
        fn close(&mut self) {
            *self.closed.lock().unwrap() += 1;
        }
        fn is_visible(&self) -> bool {
            self.visible.load(Ordering::SeqCst)
        }
    }

    pub fn fast() -> Timing {
        Timing { stream_timeout: Duration::from_millis(150), reopen_grace: Duration::from_millis(150), tick: Duration::from_millis(10) }
    }

    pub const WEBCAST: &str = "wss://webcast16-ws-useast1a.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?room_id=1";

    fn page() -> (Box<dyn LivePage>, Arc<Mutex<u32>>, Arc<AtomicBool>) {
        let closed = Arc::new(Mutex::new(0));
        let visible = Arc::new(AtomicBool::new(false));
        (Box::new(FakePage { closed: closed.clone(), visible: visible.clone() }), closed, visible)
    }

    #[test]
    fn connects_decodes_and_ends() {
        let (p, closed, _) = page();
        let (tx, rx) = channel();
        tx.send(PageEvent::SocketCreated { id: "im".into(), url: "wss://im-ws.tiktok.com/ws/v2".into() }).unwrap();
        tx.send(PageEvent::SocketCreated { id: "w1".into(), url: WEBCAST.into() }).unwrap();
        let events = base64_decode(crate::tiktok::webcast::tests_fixtures::FRAME_EVENTS).unwrap();
        tx.send(PageEvent::Frame { id: "im".into(), data: events.clone() }).unwrap();
        tx.send(PageEvent::Frame { id: "w1".into(), data: events }).unwrap();
        tx.send(PageEvent::Frame { id: "w1".into(), data: base64_decode(crate::tiktok::webcast::tests_fixtures::FRAME_END).unwrap() }).unwrap();
        let mut connected = 0;
        let mut got = Vec::new();
        let out = run(p, rx, Arc::new(AtomicBool::new(false)), fast(), || connected += 1, |e| got.push(e));
        assert_eq!((out, connected, got.len()), (Outcome::Ended, 1, 9));
        assert_eq!(*closed.lock().unwrap(), 1, "page fermée");
    }

    #[test]
    fn no_stream_unless_visible() {
        let (p, _, _) = page();
        let (_tx, rx) = channel::<PageEvent>();
        assert!(matches!(run(p, rx, Arc::new(AtomicBool::new(false)), fast(), || {}, |_| {}), Outcome::NoStream(_)));
        let (p, _, visible) = page();
        visible.store(true, Ordering::SeqCst);
        let (tx, rx) = channel();
        let t = std::thread::spawn(move || run(p, rx, Arc::new(AtomicBool::new(false)), fast(), || {}, |_| {}));
        std::thread::sleep(Duration::from_millis(400));
        tx.send(PageEvent::SocketCreated { id: "w".into(), url: WEBCAST.into() }).unwrap();
        tx.send(PageEvent::Gone("fenêtre fermée".into())).unwrap();
        assert_eq!(t.join().unwrap(), Outcome::Lost("fenêtre fermée".into()), "visible : pas de délai dépassé");
    }

    #[test]
    fn reopened_stream_is_not_a_loss_but_a_closed_one_is() {
        let (p, _, _) = page();
        let (tx, rx) = channel();
        tx.send(PageEvent::SocketCreated { id: "w1".into(), url: WEBCAST.into() }).unwrap();
        tx.send(PageEvent::SocketClosed { id: "w1".into() }).unwrap();
        tx.send(PageEvent::SocketCreated { id: "w2".into(), url: WEBCAST.into() }).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::spawn(move || run(p, rx, s2, fast(), || {}, |_| {}));
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.is_finished(), "la page a rouvert son flux");
        tx.send(PageEvent::SocketClosed { id: "w2".into() }).unwrap();
        assert_eq!(t.join().unwrap(), Outcome::Lost("flux du live fermé par la page".into()));
        let (p, _, _) = page();
        let (tx, rx) = channel();
        tx.send(PageEvent::SocketCreated { id: "w1".into(), url: WEBCAST.into() }).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::spawn(move || run(p, rx, s2, fast(), || {}, |_| {}));
        stop.store(true, Ordering::SeqCst);
        assert_eq!(t.join().unwrap(), Outcome::Stopped);
        drop(tx);
    }
}
