//! Protocole DevTools de WebView2, propre à UNE fenêtre (celle de la page TikTok) :
//! c'est l'équivalent de `webContents.debugger` d'Electron. Batlay s'en sert pour
//! - écouter les trames du flux « webcast » que TikTok envoie à la page (domaine Network) ;
//! - ne pas télécharger vidéo, images et polices dans les pages « légères » (domaine Fetch) ;
//! - lire, depuis la page, ce que la page elle-même lit (Runtime.evaluate, sonde).
//!
//! Tout ici s'exécute sur le fil principal (fenêtres), dans `with_webview`. Les
//! objets COM ne quittent jamais ce fil ; seuls des octets et des chaînes en sortent.

use batlay_core::json::{self, Value};
use batlay_core::obj;
use batlay_core::tiktok::browser::PageEvent;
use batlay_core::util::encoding::base64_decode;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use tauri::webview::PlatformWebview;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2DevToolsProtocolEventReceiver, ICoreWebView2Profile2, ICoreWebView2_13, ICoreWebView2_8, COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
};
use webview2_com::{take_pwstr, CallDevToolsProtocolMethodCompletedHandler, ClearBrowsingDataCompletedHandler, DevToolsProtocolEventReceivedEventHandler, ProcessFailedEventHandler};
use windows_core::{Interface, HSTRING, PWSTR};

thread_local! {
    /// Abonnements gardés en vie tant que la fenêtre existe (fil principal seulement).
    static RECEIVERS: RefCell<HashMap<String, Vec<ICoreWebView2DevToolsProtocolEventReceiver>>> = RefCell::new(HashMap::new());
}

/// Fenêtre fermée : libère ses abonnements (appelé sur le fil principal).
pub fn forget(label: &str) {
    RECEIVERS.with(|r| {
        r.borrow_mut().remove(label);
    });
}

fn core_of(webview: &PlatformWebview) -> windows_core::Result<ICoreWebView2> {
    // SAFETY : contrôleur WebView2 valide, sur son fil.
    unsafe { webview.controller().CoreWebView2() }
}

type Done = Box<dyn FnOnce(Result<String, String>)>;

/// Appelle une méthode du protocole ; `done` reçoit la réponse JSON (une seule fois).
pub fn call(core: &ICoreWebView2, method: &str, params: &str, done: impl FnOnce(Result<String, String>) + 'static) {
    let slot: Rc<RefCell<Option<Done>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let in_handler = slot.clone();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
        if let Some(done) = in_handler.borrow_mut().take() {
            done(result.map(|_| json).map_err(|e| e.to_string()));
        }
        Ok(())
    }));
    // SAFETY : chaînes et gestionnaire valides pendant l'appel (WebView2 garde sa propre référence).
    let sent = unsafe { core.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &handler) };
    if let Err(e) = sent {
        if let Some(done) = slot.borrow_mut().take() {
            done(Err(e.to_string()));
        }
    }
}

/// S'abonne à un événement du protocole ; `on_event` reçoit ses paramètres (JSON).
fn subscribe(label: &str, core: &ICoreWebView2, event: &str, mut on_event: impl FnMut(&ICoreWebView2, String) + 'static) -> windows_core::Result<()> {
    // SAFETY : nom d'événement valide ; l'objet rendu est gardé dans RECEIVERS.
    let receiver = unsafe { core.GetDevToolsProtocolEventReceiver(&HSTRING::from(event))? };
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let mut params = PWSTR::null();
        // SAFETY : WebView2 alloue la chaîne ; take_pwstr la copie puis la libère.
        unsafe { args.ParameterObjectAsJson(&mut params)? };
        on_event(&sender, take_pwstr(params));
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY : gestionnaire valide ; jeton non utilisé (l'abonnement dure autant que la fenêtre).
    unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)? };
    RECEIVERS.with(|r| r.borrow_mut().entry(label.to_string()).or_default().push(receiver));
    Ok(())
}

/// Coupe le son de la page (la vidéo du live ne doit jamais s'entendre).
fn mute(core: &ICoreWebView2) {
    if let Ok(core8) = core.cast::<ICoreWebView2_8>() {
        // SAFETY : interface obtenue par cast, sur son fil.
        let _ = unsafe { core8.SetIsMuted(true) };
    }
}

/// Page plantée (navigateur ou rendu de la page) : signalée au cœur, qui referme et
/// réessaie. Une panne d'un cadre intérieur ou du GPU (relancé par WebView2) est ignorée.
fn on_process_failed(core: &ICoreWebView2, events: Sender<PageEvent>) -> windows_core::Result<()> {
    let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND(-1);
        if let Some(args) = args {
            // SAFETY : sortie écrite par WebView2.
            unsafe { args.ProcessFailedKind(&mut kind)? };
        }
        let reason = match kind {
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => "navigateur fermé",
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => "page plantée",
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => "page bloquée",
            _ => return Ok(()),
        };
        let _ = events.send(PageEvent::Gone(reason.into()));
        Ok(())
    }));
    let mut token = 0i64;
    // SAFETY : gestionnaire valide, sur son fil.
    unsafe { core.add_ProcessFailed(&handler, &mut token) }
}

/// Requêtes lourdes jamais téléchargées par les pages « légères » : images,
/// vidéo et polices, flux vidéo du live (flv, HLS, serveurs « pull- »).
const HEAVY_PATTERNS: &str = r#"{"patterns":[{"urlPattern":"*","resourceType":"Image"},{"urlPattern":"*","resourceType":"Media"},{"urlPattern":"*","resourceType":"Font"},{"urlPattern":"*.flv*"},{"urlPattern":"*.m3u8*"},{"urlPattern":"*://pull-*"}]}"#;

fn block_heavy(label: &str, core: &ICoreWebView2) -> windows_core::Result<()> {
    subscribe(label, core, "Fetch.requestPaused", |sender, params| {
        // Seules les requêtes visées par HEAVY_PATTERNS sont suspendues : toutes sont refusées.
        if let Ok(v) = json::parse(&params) {
            if let Some(id) = v.get("requestId").as_str() {
                let reply = obj! { "requestId" => id, "errorReason" => "BlockedByClient" };
                call(sender, "Fetch.failRequest", &reply.to_json(), |_| {});
            }
        }
    })?;
    call(core, "Fetch.enable", HEAVY_PATTERNS, |_| {});
    Ok(())
}

fn request_id(v: &Value) -> String {
    v.get("requestId").as_str().unwrap_or_default().to_string()
}

/// Prépare la fenêtre du live : son coupé, écoute du flux, puis ouverture de `url`
/// (seulement une fois l'écoute active, pour ne manquer aucune connexion).
pub fn attach_live(label: &str, webview: PlatformWebview, url: &str, lite: bool, events: Sender<PageEvent>) -> windows_core::Result<()> {
    let core = core_of(&webview)?;
    mute(&core);
    on_process_failed(&core, events.clone())?;
    let tx = events.clone();
    subscribe(label, &core, "Network.webSocketCreated", move |_, params| {
        if let Ok(v) = json::parse(&params) {
            let _ = tx.send(PageEvent::SocketCreated { id: request_id(&v), url: v.get("url").as_str().unwrap_or_default().to_string() });
        }
    })?;
    let tx = events.clone();
    subscribe(label, &core, "Network.webSocketFrameReceived", move |_, params| {
        let Ok(v) = json::parse(&params) else { return };
        let response = v.get("response");
        // Trames binaires seulement (opcode 2) : le protocole les donne en base64.
        if response.get("opcode").as_u64() != Some(2) {
            return;
        }
        if let Some(data) = response.get("payloadData").as_str().and_then(base64_decode) {
            let _ = tx.send(PageEvent::Frame { id: request_id(&v), data });
        }
    })?;
    let tx = events.clone();
    subscribe(label, &core, "Network.webSocketClosed", move |_, params| {
        if let Ok(v) = json::parse(&params) {
            let _ = tx.send(PageEvent::SocketClosed { id: request_id(&v) });
        }
    })?;
    if lite {
        block_heavy(label, &core)?;
    }
    navigate_after(&core, "Network.enable", url, events);
    Ok(())
}

/// Fenêtre de la sonde : page légère, son coupé ; ouvre `url`.
pub fn attach_probe(label: &str, webview: PlatformWebview, url: &str) -> windows_core::Result<()> {
    let core = core_of(&webview)?;
    mute(&core);
    block_heavy(label, &core)?;
    let (tx, _rx) = std::sync::mpsc::channel();
    // Fetch.enable a été envoyé avant : le protocole traite les commandes dans l'ordre.
    navigate_after(&core, "Runtime.enable", url, tx);
    Ok(())
}

/// Active `domain` puis, seulement après sa réponse, ouvre `url`.
fn navigate_after(core: &ICoreWebView2, domain: &str, url: &str, events: Sender<PageEvent>) {
    let target = core.clone();
    let url = url.to_string();
    call(core, domain, "{}", move |result| match result {
        Ok(_) => {
            // SAFETY : adresse https vérifiée par l'appelant.
            if let Err(e) = unsafe { target.Navigate(&HSTRING::from(url.as_str())) } {
                let _ = events.send(PageEvent::Gone(format!("ouverture de la page impossible ({e})")));
            }
        }
        Err(e) => {
            let _ = events.send(PageEvent::Gone(format!("lecture du flux impossible ({e})")));
        }
    });
}

/// Évalue `expression` dans la page (promesse attendue) ; `done` reçoit la valeur en JSON.
pub fn evaluate(webview: PlatformWebview, expression: &str, done: impl FnOnce(Result<Value, String>) + 'static) {
    let core = match core_of(&webview) {
        Ok(c) => c,
        Err(e) => return done(Err(e.to_string())),
    };
    let params = obj! { "expression" => expression, "awaitPromise" => true, "returnByValue" => true };
    call(&core, "Runtime.evaluate", &params.to_json(), move |result| {
        done(result.and_then(|text| {
            let v = json::parse(&text).map_err(|e| e.to_string())?;
            if !v.get("exceptionDetails").is_null() {
                return Err("erreur dans la page".into());
            }
            Ok(v.get("result").get("value").clone())
        }))
    });
}

type Cleared = Box<dyn FnOnce(Result<(), String>)>;

/// Efface tout le profil de cette fenêtre (cookies, stockage, cache) ; `done` est
/// appelé une fois l'effacement TERMINÉ (la fenêtre peut alors être fermée).
pub fn clear_profile(webview: PlatformWebview, done: impl FnOnce(Result<(), String>) + 'static) {
    // SAFETY : interfaces obtenues par cast, sur leur fil.
    let profile = core_of(&webview).and_then(|c| c.cast::<ICoreWebView2_13>()).and_then(|c| unsafe { c.Profile() }).and_then(|p| p.cast::<ICoreWebView2Profile2>());
    let profile = match profile {
        Ok(p) => p,
        Err(e) => return done(Err(e.to_string())),
    };
    let slot: Rc<RefCell<Option<Cleared>>> = Rc::new(RefCell::new(Some(Box::new(done))));
    let in_handler = slot.clone();
    let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
        if let Some(done) = in_handler.borrow_mut().take() {
            done(result.map_err(|e| e.to_string()));
        }
        Ok(())
    }));
    // SAFETY : gestionnaire valide (WebView2 garde sa propre référence).
    if let Err(e) = unsafe { profile.ClearBrowsingDataAll(&handler) } {
        if let Some(done) = slot.borrow_mut().take() {
            done(Err(e.to_string()));
        }
    }
}
