# Batlay — documentation de développement

> Présentation et installation : voir le [README](../README.md). Ce document détaille l'architecture, les choix techniques et les limites connues.

Application desktop Windows permettant de créer des overlays pour OBS (Browser Source) : musique en cours de lecture (Spotify, Lecture système Windows) et chat Twitch, personnalisables.

---

## ⚠️ État actuel du projet — à lire avant tout

**Batlay 0.5.0 est écrit en Rust (Tauri).** Electron a disparu : voir §2 et §2bis.

**Vérifié automatiquement (Linux, sans Windows, sans compte Spotify, sans OBS) :**
- Le cœur Rust (`crates/batlay-core`, sans aucune dépendance) se compile et passe ses tests (`npm run test:core`, plus de 120 tests).
- Ce cœur, lancé seul (`batlay-serve`), a servi les vraies pages d'overlay, d'alertes et du chat à un vrai **Chromium** : rendu, mise à jour en direct par WebSocket, révocation d'un lien, alerte de test, adresse à clé, refus d'un `Host` étranger.
- `npm test` (interface, plus de 550 tests) passe ici avec un adaptateur Vitest minimal (npm étant inaccessible), sauf `tests/useMusicStore.test.ts` (3 tests : `vi.resetModules` n'y est pas disponible), comme avant.
- Le **vrai tableau de bord** (bundle de `src/main.tsx`) a tourné dans Chromium contre le **vrai cœur** (`batlay-serve`), par un faux `window.__TAURI__` : chaque page se rend sans erreur, les réponses du cœur ont la forme attendue par l'interface (réglages, overlays, alertes, Live, TikTok, Twitch, mises à jour, image importée), le Mode démo arrive jusqu'à la page OBS.
- L'application Tauri (`src-tauri`) a été **vérifiée par le typage** contre les signatures de Tauri 2.12, wry 0.57 et webview2-com 0.39 recopiées de leurs sources, mais **jamais compilée pour de vrai** : crates.io est inaccessible ici. Sa vraie compilation se fait dans GitHub Actions (`.github/workflows/ci.yml` à chaque envoi, `release.yml` pour un tag).

**NON vérifié (à tester en priorité sur votre machine) :**
- **L'application elle-même** (fenêtres, WebView2, installateur NSIS, passage depuis la 0.4.x) : jamais lancée.
- **La page TikTok invisible** (protocole DevTools de WebView2) et **la Lecture système Windows** (PowerShell/WinRT) : jamais exécutées.
- **Spotify, Twitch et TikTok réels** : le code suit leurs API officielles (ou, pour TikTok, le site lui-même) mais n'a jamais parlé à un vrai compte. Le TLS passe par Windows (SChannel), absent ici.
- `npm run lint` ne fonctionne pas : le projet n'a aucun fichier `eslint.config.*` (ESLint ≥ 9 l'exige). Ce n'est pas nouveau.

---

## 1. Présentation

Batlay connecte Spotify, détecte le morceau en cours, et diffuse ces informations (titre, artiste, pochette, progression) vers une page d'overlay transparente que vous ajoutez dans OBS comme Browser Source. Un éditeur visuel permet de personnaliser entièrement l'apparence (position, taille, typographie, couleurs, animations) et de sauvegarder plusieurs overlays.

## 2. Architecture

```
Batlay/
├── crates/batlay-core/        # CŒUR, en Rust, SANS dépendance : tout le fonctionnement (§2bis)
│   └── src/
│       ├── app.rs             # Assemblage + `invoke` : la liste fermée des commandes du tableau de bord
│       ├── overlay_server.rs  # Serveur HTTP + WebSocket des overlays (port 3000, 127.0.0.1)
│       ├── alerts/  live.rs   # Alertes (hub, fichiers importés), fenêtre Live
│       ├── twitch/            # Module Twitch INDÉPENDANT (port fixe 4000) — voir §14
│       ├── tiktok/            # Live TikTok : page invisible, décodage du flux (protobuf), service
│       ├── spotify.rs         # OAuth PKCE (aucun secret), lecture du morceau en cours
│       ├── system_media.rs/.ps1  # Lecture système Windows (PowerShell/WinRT)
│       ├── updater.rs         # Mises à jour (latest.yml, SHA-512)
│       ├── config.rs  log.rs  # Réglages (%APPDATA%\Batlay), journal
│       ├── net/  util/        # HTTP/1.1, WebSocket, JSON, base64, SHA, gzip… (écrits ici, testés)
│       └── bin/batlay-serve.rs  # Outil : le cœur sans fenêtre (tests des pages OBS dans Chromium)
├── src-tauri/                 # APPLICATION Windows (Tauri 2) : relie le cœur à Windows
│   ├── src/main.rs            # Démarrage, instance unique, arrêt
│   ├── src/bridge.rs          # LA commande `batlay(cmd, args)` → `App::invoke`
│   ├── src/host.rs            # Fenêtres principale et Live, événements, thème, démarrage avec Windows
│   ├── src/tiktok_pages.rs    # Pages TikTok (profil WebView2 à part) : live invisible, sonde, connexion
│   ├── src/cdp.rs             # Protocole DevTools de WebView2 (écoute du flux TikTok)
│   ├── src/tls.rs  win32.rs   # TLS de Windows (SChannel) ; DPAPI, registre, ShellExecute
│   ├── capabilities/main.json # Permissions : la commande et les événements, fenêtres main/live seulement
│   ├── tauri.conf.json        # Fenêtres, CSP, installateur NSIS (hooks : retrait de la 0.4.x)
│   └── windows/hooks.nsh
├── src/                        # Tableau de bord React (inchangé dans son fonctionnement)
│   ├── platform/               # `window.batlay` (bridge.ts) au-dessus de Tauri (native.ts)
│   ├── services/artwork-lookup.ts  # Recherche de pochettes (réseau par le cœur, sources autorisées)
│   └── …                       # pages, stores, providers, éditeur, animations (voir les sections suivantes)
├── shared/                     # Types et fonctions pures communs aux pages (ex-electron/shared)
├── overlay/  alerts/  twitch-overlay/   # Pages chargées par OBS (servies par le cœur)
└── tests/                      # Vitest : interface + garde-fous du code Rust (tests/tauri-guards.test.ts)
```

### Flux de données

```
Spotify / Lecture système / Mode Demo
   ↓  (MusicProvider — interface commune ; `Track.artists[]` + `Track.artist`)
useMusicStore (Zustand, tableau de bord)  ──→  playbackClock (horloge de progression)
   ↓  window.batlay.overlay.broadcastState()  →  commande « overlay:broadcast-state »
crates/batlay-core/src/overlay_server.rs (cœur)
   │  (pochette base64 sortie du flux : /api/artwork/<id> ; message = { payload, sentAt })
   ↓  WebSocket
overlay/OverlayApp.tsx  ──→  horloge de progression (même code)  →  OBS Browser Source
```

Le tableau de bord ne détient **jamais** de jeton Spotify ou Twitch : l'authentification et les appels d'API sont dans le cœur, joint par `window.batlay.*` (`src/platform/bridge.ts`), qui n'a qu'une seule commande Tauri.

## 2bis. Architecture Rust (0.5.0)

**Pourquoi deux crates.** `crates/batlay-core` ne dépend de rien (bibliothèque standard seulement) : il se compile et se teste partout, hors ligne, en quelques secondes (`npm run test:core`). Tout ce qui touche à Windows (fenêtres, WebView2, TLS, DPAPI, registre) est dans `src-tauri`, derrière des interfaces (traits) que le cœur définit et que les tests simulent :

| Trait (cœur) | Rôle | Fourni par l'application | En test |
|---|---|---|---|
| `app::Host` | événements vers les fenêtres, ouvrir une adresse, fenêtre Live, thème, démarrage avec Windows, lancer l'installateur | `host.rs` | `TestHost` |
| `net::client::Connector` | connexions sortantes (TCP + TLS) | `tls.rs` (SChannel via native-tls) | `PlainConnector` + faux serveurs |
| `config::SecretBox` | chiffrement des jetons | `win32.rs` (DPAPI) | `NoSecretBox` |
| `tiktok::browser::PageHost` | page du live invisible, sonde, session TikTok | `tiktok_pages.rs` + `cdp.rs` | fausses pages |
| `system_media::PowerShell` | exécution du script WinRT | `SystemPowerShell` (cœur) | faux PowerShell |
| `runtime::{Clock, Timers, Spawner}` | heure, minuteurs, fils | `Rt::production()` | `ManualClock`, `ManualTimers`, `InlineSpawner` |

**Une seule commande.** Le tableau de bord appelle `invoke("batlay", { cmd, args })` ; `bridge.rs` vérifie la fenêtre (main ou live) et passe tout à `App::invoke`, qui tient la liste fermée des commandes (mêmes noms que les anciens canaux IPC) et revalide chaque argument. Aucune commande ne reçoit de port, de chemin, de jeton ni d'adresse arbitraire (`artwork:fetch` n'accepte que les six sources de pochettes, en https). `tests/tauri-guards.test.ts` vérifie que chaque commande du pont existe dans le cœur, et que chaque événement du cœur est écouté par le pont.

**Événements.** Mêmes noms qu'avant (`live:update`, `tiktok:state`, `twitch:state`, `update:status`, `theme:changed`, `integrations:alias-changed`, `overlay-server:error`, `tiktok:page-visible`), envoyés aux fenêtres main et live, déjà en JSON (`RawValue`, aucune conversion).

**Fils et sûreté.** Le cœur protège son état par des `Mutex` et ne garde jamais un verrou pendant un appel réseau. Règle de l'application : le fil principal (fenêtres) n'attend jamais le cœur ; tout appel au cœur venant d'un événement de fenêtre part sur un autre fil (le cœur, lui, peut attendre le fil principal : création d'une fenêtre, cookies). Chaque connexion des serveurs est isolée (`catch_unwind`) : une panique ne coupe jamais un serveur. L'arrêt (`RunEvent::Exit`) a 3 s au plus.

**Pages TikTok.** Profil WebView2 à part (`%LOCALAPPDATA%\com.adil.batlay\TikTokWebView`), fenêtres sans permission IPC (sites distants), navigation limitée à `https://(www.)tiktok.com`, son coupé, outils de développement désactivés. `cdp.rs` écoute `Network.webSocketCreated` / `webSocketFrameReceived` / `webSocketClosed` (l'équivalent de `webContents.debugger`) ; les pages « légères » refusent images, vidéo, polices et flux vidéo par le domaine `Fetch`. Les trames sont décodées par le cœur (`tiktok/webcast.rs`, protobuf écrit à la main d'après le schéma communautaire, testé avec des trames produites par le vrai encodeur protobuf de Google).

**WebView2 et minuteurs.** Toutes les fenêtres ont les mêmes options (`BROWSER_ARGS`, exigé par WebView2) : désactivation de `CalculateNativeWinOcclusion`, `IntensiveWakeUpThrottling` et des ralentissements de fond. Une fenêtre réduite garde son polling musical (2 s) et sa diffusion vers OBS, comme `backgroundThrottling: false` sous Electron.

**Données reprises de la 0.4.** Même dossier `%APPDATA%\Batlay` (`batlay-config.json`, `batlay-twitch.json`, `alert-media/`) : réglages, overlays, alertes, images et sons sont repris tels quels. Les jetons chiffrés par Electron (`safeStorage`) ne sont pas lisibles hors d'Electron : ils sont ignorés et retirés, d'où une reconnexion unique à Twitch et Spotify. Nouvelles clés : `spotify.refreshTokenDpapi`, `tokenDpapi` (DPAPI avec un complément de clé propre à Batlay). Le cache des pochettes (ancien `artwork-cache.json`) repart de zéro, dans le stockage du tableau de bord.

**Variables d'environnement (développement).** `BATLAY_DATA_DIR` (dossier de données), `BATLAY_FAKE_SMTC`, `BATLAY_FAKE_HOSTS` (tests du cœur).

## 3. Installation (sur une machine Windows avec accès réseau)

- Node.js 20+, Rust stable (`rustup`), et les outils de compilation C++ de Visual Studio (« Développement Desktop en C++ »), comme pour toute application Tauri. WebView2 est déjà présent sur Windows 10 (à jour) et 11.

```bash
npm install
```

## 4. Développement

```bash
npm run dev          # Tauri : construit les pages OBS (dist/), lance Vite et l'application
npm test             # tests de l'interface (Vitest)
npm run test:core    # tests du cœur Rust (fonctionne aussi hors Windows)
```

`npm run tauri` lance la CLI de Tauri 2 par `npx` (aucune dépendance npm de plus). Les pages OBS (`overlay.html`, `alerts.html`, `twitch-overlay.html`) sont servies par le cœur depuis `dist/` : `npm run dev` les construit une fois avant de lancer Vite (`dev:front`). Après une modification de ces pages, relancez `npm run dev`.

Sans Windows : `cargo run --manifest-path crates/batlay-core/Cargo.toml --bin batlay-serve -- dist` démarre les serveurs du cœur sans fenêtre (commandes JSON sur l'entrée standard), pour tester les pages OBS dans un navigateur.

## 5. Configuration Spotify

Spotify n'est **jamais** pré-configuré avec des identifiants embarqués. Pour l'activer :

1. Créez une application sur le [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Dans les paramètres de l'app, ajoutez exactement cette Redirect URI :
   ```
   http://127.0.0.1:8945/callback
   ```
3. Copiez le **Client ID** (le Client Secret n'est pas nécessaire — Batlay utilise le flow PKCE, la méthode que Spotify recommande explicitement pour les apps desktop, justement parce qu'un secret ne peut pas y être stocké en sécurité).
4. **Ajoutez votre propre compte Spotify à l'allowlist de l'app** : Dashboard > votre app > Settings > User Management > ajoutez l'email associé à votre compte Spotify. Tant que l'app reste en **Development Mode** (le cas par défaut, et suffisant pour un usage personnel), Spotify refuse l'autorisation à tout compte non explicitement ajouté ici — c'est l'étape la plus souvent oubliée.
5. Depuis les changements Spotify de février 2026, un **compte Spotify Premium** est requis pour utiliser une app en Development Mode (y compris le vôtre en tant que testeur).
6. Dans Batlay : **Connexions > Spotify**, collez le Client ID, puis cliquez sur **Connecter Spotify**.

Un navigateur s'ouvre pour l'autorisation ; Batlay capte le retour via un petit serveur HTTP local éphémère (port `8945`) et échange le code contre un token. Le refresh token est chiffré par Windows (DPAPI, clé `spotify.refreshTokenDpapi`) avant d'être stocké — jamais en clair, jamais dans les logs. Code : `crates/batlay-core/src/spotify.rs`.

Les scopes utilisés (`user-read-currently-playing`, `user-read-playback-state`) sont des scopes de **lecture seule** sur le morceau en cours — ils sont restés accessibles en Development Mode après les restrictions Spotify de fin 2024/2026 (contrairement aux endpoints de recommandations, follow, ou modification de playlists, qui nécessitent désormais l'Extended Quota Mode, hors de portée pour un usage personnel).

## 6. Mode Demo

Aucune configuration requise. **Connexions > Mode démo > Lancer la démo** fait défiler 6 morceaux d'exemple (dont des collaborations à 2 et 3 artistes et des noms accentués : *Lean On* — Major Lazer, DJ Snake, MØ ; *Balance ton quoi* — Angèle) avec une progression calculée sur l'horloge, exploitable pour tester overlays, animations et OBS sans compte Spotify.

## 6bis. Lecture système (Windows) — pour les comptes non-Premium

Depuis mars 2026, Spotify exige qu'un compte **Premium** possède l'app Development Mode utilisée pour l'OAuth (voir section 5, point 5) — et comme chaque utilisateur de Batlay crée son propre Client ID, chaque utilisateur est son propre "propriétaire d'app" et doit donc être Premium. C'est une restriction imposée par Spotify côté serveur, impossible à contourner en modifiant Batlay.

Pour les comptes gratuits, **Connexions > Lecture système (Windows) > Connecter** lit directement les métadonnées "En cours de lecture" exposées par Windows via les *Global System Media Transport Controls* (la même API que le Volet de contrôle multimédia / les touches multimédias du clavier) :

- Aucun compte Spotify, Client ID ou OAuth requis.
- Fonctionne avec Spotify Free, un navigateur (YouTube Music...), VLC, ou toute autre app qui expose son "Now Playing" à Windows.
- Implémenté dans `crates/batlay-core/src/system_media.rs` + `system_media.ps1` (PowerShell/WinRT lancé par le cœur, sans fenêtre, script passé par `-EncodedCommand`, délai borné) et `src/services/system-media-provider.ts` (provider `MusicProvider` du tableau de bord, polling 2s comme `SpotifyProvider`).
- **Choix du lecteur** : si plusieurs apps jouent en même temps (ex. Spotify + un onglet YouTube), **Connexions > Lecture système > Lecteur à suivre** liste les sessions actives (`GetSessions()` WinRT) et permet d'en épingler une explicitement — sinon Batlay suit par défaut celle qui a le focus multimédia côté Windows. Le choix est persisté (`settings.preferredSystemMediaAppId`).
- **Pochette** : récupérée **en arrière-plan** (`Thumbnail` WinRT converti en data URI base64, puis recherche en ligne si absente : iTunes, Deezer, ListenBrainz, MusicBrainz et Audius interrogés **en parallèle**, sans clé d'API — la première réponse fiable l'emporte et annule les autres ; voir l'en-tête de `src/services/artwork-lookup.ts` ; le réseau passe par la commande `artwork:fetch` du cœur, limitée à ces sources) : le titre, l'artiste et la progression s'affichent sans l'attendre, et l'état est ré-émis dès que l'image arrive. Mise en cache tant que le morceau ne change pas ; les tentatives sont espacées et bornées. Côté OBS, l'image n'est plus renvoyée dans chaque message WebSocket mais servie une fois via `/api/artwork/<id>` (cache navigateur).
- **Spotify Desktop** : SMTC peut rester sur le morceau précédent pendant une pub et n'afficher qu'une partie des artistes. Pour Spotify uniquement, Batlay lit aussi le **titre de la fenêtre** Spotify (`Artiste - Titre`, `Advertisement` pendant une pub) et le recoupe avec SMTC (`src/utils/spotify-metadata.ts`) : pendant une pub, l'overlay affiche « Publicité / Spotify » (barre vide, aucune recherche de pochette) au lieu du morceau précédent ; l'artiste est complété quand la fenêtre en donne davantage **et** est cohérente avec SMTC. Au moindre désaccord (changement de piste en cours), SMTC reste seul juge. Si Spotify est réduit dans la zone de notification, sa fenêtre est masquée : le recoupement est alors inactif et seul SMTC est utilisé. Pour diagnostiquer, activer le journal debug (section 6quater) : le cas `fenetre-spotify` affiche le titre de fenêtre à côté des valeurs SMTC.
- **Plusieurs artistes** : Windows ne transmet que la chaîne que l'application source lui donne. Batlay l'affiche **telle quelle** (aucun découpage). Si un lecteur n'y met que l'artiste principal, Batlay ne peut pas retrouver les autres — voir la section 6quater et les limitations.
- Limitations connues : **Windows 10 1809+ / Windows 11 uniquement** ; non testé sur une vraie machine Windows dans cet environnement (pas d'accès à PowerShell/WinRT ici) — à valider en priorité avant `npm run dist`.

## 6ter. Progression en temps réel (Dashboard + overlay OBS)

Les providers ne fournissent une position que toutes les ~1 à 3 s (polling). Entre deux mesures, **une seule horloge partagée** (`src/utils/progress-clock.ts`) fait avancer la barre ; le Dashboard, l'éditeur et la page OBS (`overlay/OverlayApp.tsx`) utilisent exactement ce même code.

Pourquoi la barre saccadait, et ce qui a changé :

| Cause | Correctif |
|---|---|
| Affichage rafraîchi toutes les 250 ms (4 images/s) + `transition` CSS qui rattrapait en retard | Mise à jour **image par image** (`requestAnimationFrame`, `src/utils/frame-ticker.ts`) directement dans le DOM (`translate3d`, `src/components/LiveProgress.tsx`) : 60 images/s sans re-rendre React |
| `updatedAt` posé **après** les appels (latence perdue) | `updatedAt` = instant de la **mesure** : milieu de l'aller-retour HTTP pour Spotify, horodatage pris dans PowerShell pour la Lecture système |
| Position SMTC brute : c'est la position à `LastUpdatedTime`, pas « en direct » | Extrapolation `position + (mesure − LastUpdatedTime)` si lecture en cours (`resolvePosition`) |
| 2ᵉ process PowerShell **bloquant** pour la pochette | Pochette asynchrone, ré-émission à l'arrivée |
| Recalage brutal sur chaque mesure (sauts, retours en arrière) | Écart < 120 ms : ignoré ; 120 ms–4 s : absorbé en accélérant/ralentissant la barre (vitesse toujours dans [0,5×, 1,5×] → **jamais de recul**) ; > 4 s ou autre morceau : vrai saut (seek) |
| Pochette base64 (100–500 Ko) renvoyée à chaque message WebSocket | Servie une fois par HTTP ; le message ne pèse plus que quelques centaines d'octets |
| Message WebSocket sans notion d'âge, dépendant de l'horloge d'OBS | `sentAt` (horloge de Batlay) + `updatedAt` → âge exact de la mesure, indépendant de l'horloge de la page OBS |
| Pause détectée avec 1–2 s de retard → la barre reculait | La barre se fige où elle est (tolérance 2,5 s) |
| Fin de morceau : polling à cadence fixe | Spotify : relevé programmé pile après la fin prévue ; rafale rapide après tout changement ; recul exponentiel en cas d'erreur/429 |
| Déconnexion : OBS gardait l'ancien morceau (barre qui continuait) | L'état « rien en lecture » est diffusé à la déconnexion |

Mesuré dans Chromium contre le vrai serveur d'overlay avec un provider simulé bruité (latence 60–760 ms, pause, changement de morceau) : 0 recul, erreur d'affichage entre −65 ms et +184 ms, vitesse lissée ≤ 1,15×.

## 6quater. Artistes multiples et encodage UTF-8

**Artistes.** `Track.artists: string[]` (liste structurée) et `Track.artist` (chaîne « Artiste 1, Artiste 2, Artiste 3 ») circulent ensemble sur tout le pipeline. La liste existe parce que la chaîne est ambiguë : « Tyler, The Creator » contient une virgule. Le composant *Artiste(s)* de l'overlay accepte un séparateur au choix (`, ` · `•` `&` `/` …) et **défile** (marquee) au lieu de couper avec « … » quand le texte dépasse le bloc — ce qui donnait l'impression qu'un seul artiste s'affichait. Le parsing Spotify est dans `src/services/spotify-track.ts` (tolère morceau, fichier local sans id, épisode). Un test (`tests/no-first-artist-only.test.ts`) échoue si `artists[0]` & co. réapparaissent.

**Caractères corrompus (« � »).** Cause racine : `powershell.exe` écrit sa sortie dans la page de code **OEM** de Windows (850 en France), pas en UTF-8 ; Node la décodait en UTF-8, et l'octet de « é » en CP850 devient « � » — information perdue, aucun remplacement a posteriori ne peut la retrouver. Correctif (`crates/batlay-core/src/system_media.rs`) : PowerShell encode son JSON en UTF-8 puis en **Base64** (ASCII pur), le cœur décode les octets lui-même (`decode_output`, avec repli UTF-8 strict → Windows-1252), testé en Rust. Le code source, lui, était sain : aucun `�`, `Ã©` ni BOM dans le projet (vérifié). En filet de sécurité, `src/utils/text-encoding.ts` répare le cas où c'est la **source** qui fournit un texte déjà mal décodé (tags MP3 UTF-8 lus en Windows-1252 : « CafÃ© » → « Café »), uniquement si le résultat est de l'UTF-8 strictement valide, et retire les caractères de contrôle. Un `�` déjà présent n'est volontairement jamais masqué. `.editorconfig` impose UTF-8/LF.

**Robustesse du décodage et nettoyage (`cleanMetadataText`).** `decode_output` tolère un BOM devant le Base64, un CRLF final et un Base64 replié sur plusieurs lignes, et ne prend pas un simple mot (« null ») pour notre sortie. `SystemPowerShell` (trait `PowerShell`, simulé en test) reste le chemin unique d'exécution. `cleanMetadataText` (pure) retire caractères de contrôle, NUL, BOM et invisibles (U+200B, U+2060, U+FEFF), normalise en NFC et ramène toute suite d'espaces à un seul ; elle ne découpe ni ne réécrit jamais un nom (« Tyler, The Creator », « Simon & Garfunkel », « AC/DC », « Earth, Wind & Fire », « Mötley Crüe », « !!! » sont testés). ZWJ/ZWNJ (U+200D/U+200C) ne sont retirés qu'en bordure : à l'intérieur d'un texte ils ont un sens (emoji composés, persan).

**Journal de diagnostic des métadonnées.** Désactivé par défaut. Dans la console de Batlay (`F12`), `localStorage.setItem("batlay:debug-metadata", "1")` puis relancer : pour les morceaux qui illustrent un des trois cas (*accent*, *multi-artistes*, *non-latin*), une seule entrée par morceau donne le texte brut, le texte nettoyé et leurs points de code Unicode (`Ang[U+00E8]le`), plus `AlbumArtist` tel que le lecteur le déclare — lu **uniquement** pour ce diagnostic, jamais utilisé à l'affichage. Un `U+FFFD` est signalé : l'information est déjà perdue en amont. Retrait : `localStorage.removeItem("batlay:debug-metadata")`.

## 6quinquies. Éditeur d'overlay

- **Créer** : « Nouvel overlay » propose un des 6 presets (avec miniatures) ou **Canvas vide** (taille au choix, préréglages OBS). Le nom est facultatif. Les presets existants sont inchangés.
- **Composants** (palette *Ajouter un composant*, qui reste ouverte pour en enchaîner plusieurs) : pochette, titre, artiste(s), album, barre de progression, temps écoulé, **temps restant**, durée, source, **texte libre** (variables `{title} {artist} {album} {source}`), **forme / fond** (carte, séparateur…).
- **Fond, bordure, arrondi, ombre** — pour chaque composant **et** pour l'overlay entier : fond uni ou dégradé (linéaire/radial, angle) avec **transparence**, bordure (épaisseur, couleur, continu/tirets/pointillés), rayon des coins, ombre portée (x, y, flou, étalement, couleur, opacité), ombre de texte, opacité globale.
- **Calques** : glisser-déposer pour réordonner, monter/descendre, dupliquer, supprimer, renommer (double-clic), masquer, verrouiller.
- **Position / taille** : glisser à la souris, 8 poignées de redimensionnement, magnétisme avec guides (bords et centres du canvas et des autres composants), champs X/Y/L/H, alignement sur le canvas, canvas redimensionnable (préréglages OBS).
- **Aperçu temps réel** : rendu identique à la page OBS (même code), rogné comme OBS ; morceau *Exemple* (barre animée) ou *En direct* (musique détectée) ; fond de scène simulé (damier, sombre, clair, vert).
- **Annuler / rétablir** (100 niveaux ; un geste de souris = une étape). Un brouillon non enregistré est conservé si vous quittez l'éditeur puis y revenez.
- **Raccourcis** (liste complète : bouton clavier au-dessus de l'aperçu) : `Ctrl+S` enregistrer · `Ctrl+Z` / `Ctrl+Y` · `Ctrl+D` dupliquer · `Suppr` · flèches = 1 px (`Maj` = 10 px) · `Ctrl+↑` / `Ctrl+↓` (ou `Ctrl+[` / `Ctrl+]`, qui demandent AltGr en AZERTY) calque en dessous / au-dessus · `Ctrl+0` zoom ajusté, `Ctrl++` / `Ctrl+-` et `Ctrl+molette` zoom · pendant un glissement : `Maj` = axe droit ou proportions, `Alt` = sans magnétisme ou depuis le centre · `Échap` désélectionne.
- Rétrocompatibilité : tous les nouveaux champs sont optionnels ; les overlays déjà enregistrés et les presets (`shadow: true`, `borderWidth`…) sont toujours lus. Une bordure est maintenant comprise **dans** la taille du bloc (`border-box`), ce qui peut réduire de quelques pixels la pochette des presets Glass/Neon.

## 6sexies. Démarrage et fenêtre principale

- **Start minimized** = la fenêtre est **affichée puis réduite** dans la barre des tâches, jamais invisible (`host.rs`, `create_main_window`). Elle est créée cachée, affichée à la fin du premier chargement (`on_page_load`), avec un filet de sécurité de 5 s. Le tableau de bord n'est jamais ralenti, fenêtre réduite (options WebView2, §2bis) : polling musical et WebSocket vers OBS continuent. Pas d'icône de zone de notification.
- **Instance unique** (`tauri-plugin-single-instance`, enregistré en premier) : relancer Batlay ramène la fenêtre existante (désiconifiée, affichée, au premier plan) au lieu de lancer un second processus qui se battrait pour les ports 3000 et 4000.
- **Fermer la fenêtre principale ferme Batlay**, fenêtre Live et pages TikTok invisibles comprises.
- **Launch on startup** : valeur « Batlay » de `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, **uniquement si l'app est installée** (build de production). Elle est réalignée sur le réglage à chaque démarrage ; les entrées de la 0.4 (`com.adil.batlay`, `electron.app.Batlay`), qui visent un programme désinstallé, sont retirées.

## 6septies. Thème sombre / clair / système

- Réglage `theme` : `"dark" | "light" | "system"` (Settings > Appearance). Un ancien `"dark"` reste valide ; toute valeur inconnue retombe sur `"dark"`. `system` suit Windows, y compris quand il bascule pendant que Batlay tourne.
- **Source de vérité** : `shared/theme.ts` (tokens hexadécimaux dark/light, résolution, fond de fenêtre). Le tableau de bord pose les variables CSS RGB sur `<html>` (`src/theme/apply-theme.ts`) ; Tailwind les lit via `rgb(var(--x) / <alpha-value>)` (`tailwind.config.js`), donc `bg-base-800/60` continue de fonctionner. Les valeurs sont celles de l'identité 0.5 (figées par un test, voir §6octies-bis).
- Nouveaux jetons sémantiques : `fg`, `muted`, `faint`, `line` (bordures des cartes), `accent`/`ok`/`danger`/`warn` (texte de statut lisible sur les surfaces) ; `bg-stage` est le fond **fixe** des miniatures d'overlay. Contraste WCAG AA (≥ 4,5:1) vérifié par test pour le texte principal, `muted` et les statuts, dans les deux thèmes.
- **Jamais thématisés** : la page overlay d'OBS, `OverlayPreview`, et les fonds d'aperçu de l'éditeur (Damier / Sombre / Clair / Scène) — un test le garantit.
- Zéro flash : l'application résout le thème (registre de Windows pour « system »), donne sa couleur au fond de la fenêtre (`crates/batlay-core/src/theme.rs`, mêmes valeurs que `WINDOW_BACKGROUND`), et le pose dans la page avant son chargement (`window.__BATLAY_INITIAL__`) : le tableau de bord l'applique **avant** `createRoot`, sans aller-retour. La bascule de Windows arrive par `theme:changed`.

## 6octies-bis. Identité visuelle 0.5 (« Nova »)

Ce qui distingue la 0.5 de la 0.4 au premier coup d'œil :
- **Logo et icône** : chauve-souris blanche sur une tuile dégradée indigo → cyan, l'égaliseur découpé dans le corps (`assets/icon.svg`, source de toutes les icônes ; `BrandMark` / `BatGlyph` dans `src/components/BrandMark.tsx` reprennent les mêmes tracés ; la tuile in-app suit la couleur d'accent).
- **Couleurs** : encre plus profonde (`base-950` #07070C), textes secondaires et indices plus lisibles (`faint` ≥ 3:1, contre ~1,6:1 en 0.4), accent par défaut **#6650F5** (0.4 : #7C4DFF, toujours proposé dans Paramètres) et second ton **« nova »** cyan, dérivé de l'accent (`novaOf` dans `src/theme/accent.ts` : teinte −55°), utilisé par les dégradés (logo, boutons, repères, barres de progression).
- **Typographie** : titres et chiffres en **Sora** (`@fontsource-variable/sora`), texte en Inter. Space Grotesk reste chargée pour les aperçus d'overlays.
- **Barre de titre Batlay** (`src/layouts/TitleBar.tsx`) : les fenêtres du tableau de bord et Live n'ont plus le cadre Windows (`decorations(false)`, ombre gardée : contour, coins arrondis de Windows 11, redimensionnement par les bords). Marque + pastille de version, recherche (Ctrl+K) au centre, « Mettre à jour » quand une version est prête, boutons réduire / agrandir / fermer (`src/platform/window-controls.ts`, API de fenêtre de Tauri). Tout ce qui n'est pas un bouton porte `data-tauri-drag-region` (déplacer ; double-clic : agrandir). Permissions ajoutées, et seulement celles-là : `core:window:allow-start-dragging`, `allow-internal-toggle-maximize`, `allow-minimize`, `allow-toggle-maximize`, `allow-is-maximized`, `allow-close` (`tests/tauri-guards.test.ts`). La fenêtre de connexion TikTok (site distant) garde le cadre Windows.
- **Coquille** : barre latérale épurée (logo et recherche montés dans la barre de titre, bloc « État » regroupant les voyants, bouton « Réduire »), en-têtes de page en Sora, cartes avec filet lumineux, pastilles d'état avec leur point, notifications à liseré, fond « nova » (lueur sous la barre de titre + trame de points).
- **Vue d'ensemble** : bandeau d'accueil (salutation, état de préparation Musique / Chat / TikTok, jauge « Prêt pour le live », actions), tuiles de statistiques (`StatTile`, partagé avec la page Live).
- **Briques communes** (`src/components/ui.tsx`, `globals.css`) : `PageHeader` (icône + section), `EmptyState`, choix segmentés `.seg` / `.seg-active`, `.surface-inset`, `.brand-tile`, `.chip-version`, `.nav-item-active`.
- **Accueil 0.5** (`src/components/WhatsNew.tsx`) : affiché une seule fois (mémorisé dans le stockage de la WebView), avec le rappel de la reconnexion Twitch / Spotify.
- **Paramètres** : « À propos » et « Mises à jour » réunis dans une carte Batlay (logo, version donnée par l'application, état, progression).
- **Mise à jour** : « Installer et redémarrer » affiche l'écran Batlay « Installation de la mise à jour » (`UpdateInstallOverlay`), puis lance l'installateur **en silence** (`/S /R`) : aucune fenêtre d'installateur, Batlay se rouvre tout seul.
- **Installateur / désinstallateur** (NSIS) : panneau latéral et bannière refaits (`assets/installer/*.bmp`, rendus depuis le logo), icône du désinstallateur, textes d'accueil, de fin et de désinstallation réécrits, confirmation avant d'abandonner (`src-tauri/windows/hooks.nsh`, inclus par le modèle de Tauri avant ses pages). Les fonds restent ceux de Windows : sur fond sombre, le texte des cases à cocher resterait noir avec les styles visuels.
- **Pages du navigateur** (retour de connexion Spotify et Twitch) : même identité (`crates/batlay-core/src/brand_page.rs`), tout en ligne.

## 6octies. Navigation et interface

- **Langue** : toute l'interface est en français (le réglage `language` existe dans la config mais n'est pas encore exploité : pas d'internationalisation).
- **Barre latérale** : trois blocs séparés par un filet, sans titres — *Vue d'ensemble, Live, Overlays, Alertes, Intégrations* · *Lecture en cours, Chat Twitch, Connexions* · *Paramètres* (`src/layouts/nav.ts`, testé dans `tests/nav.test.ts`). Dans l'éditeur elle se réduit à des icônes (la fenêtre fait 1024 px minimum) et peut être rouverte ; « Overlays » reste allumé pendant l'édition. Le pied de barre donne accès à **Copier l'URL OBS** et au bloc **État** (serveur, source, chat, TikTok : cliquables, ils mènent à la page qui permet de corriger). La recherche (Ctrl+K) est dans la barre de titre.
- **Overlay principal** : l'overlay marqué « Principal » (ancien « Active ») est celui dont l'URL OBS est proposée dans la barre latérale et le tableau de bord. S'il n'existe plus, c'est le premier de la liste (`pickMainOverlay`). Cela n'a **aucun effet sur le serveur** : chaque overlay reste servi sur sa propre URL.
- **Tableau de bord** : « Mise en route » en trois étapes (source, overlay, URL OBS), lecture en cours, aperçu de l'overlay principal avec la vraie musique.
- **Éditeur** : nom modifiable dans l'en-tête ; propriétés d'un composant en trois onglets (*Général*, *Texte/Pochette/Barre*, *Apparence*) ; barre d'outils de vue (magnétisme, fond, exemple/direct, zoom) ; « Abandonner » demande confirmation car l'historique d'annulation est effacé.
- **Paramètres** : le port n'est enregistré qu'à la validation (Entrée / sortie du champ) et doit être compris entre 1024 et 65535 (`validatePort`).
- **Liens OBS régénérables** : l'URL d'un overlay contient son identifiant (`/overlay/<id>`), donc *régénérer* un lien = donner un nouvel `id` à l'overlay (`regenerateObsIds`, `src/utils/obs-link.ts`, `updatedAt` inchangé). Disponible pour un overlay (menu « … » de *Mes overlays* : le nouveau lien est copié) ou pour tous (Paramètres > Serveur d'overlay), toujours après confirmation. L'ancien lien est **révoqué côté serveur** (`OverlayServer.setOverlayConfigs`) : ses sources OBS sont déconnectées, et le WebSocket refuse désormais tout id inconnu — sans quoi une source dont le lien a changé se reconnecterait toutes les 2 s et recevrait encore l'état de lecture. Ce refus s'applique aussi à un overlay supprimé. L'overlay principal suit son nouvel id, et un brouillon non enregistré de l'éditeur est rattaché au nouvel id (`src/utils/unsaved-drafts.ts`).
- Fonctions pures de l'interface (zoom, port, overlay principal, dates relatives) : `src/utils/ui-helpers.ts`, testées dans `tests/ui-helpers.test.ts`.

## 6nonies. Moteur d'animations et de transitions

Un moteur **commun** anime l'overlay musique et le chat. Les futures alertes et les futurs widgets pourront l'utiliser tels quels. Il ne dépend d'aucune bibliothèque : il s'appuie sur la **Web Animations API** (`element.animate`), native dans Chromium, donc dans OBS, Streamlabs et le tableau de bord (WebView2). Aucune boucle JavaScript : le navigateur interpole lui-même, sur le compositeur pour `opacity` et `transform`.

### Architecture (`src/animation/`)

| Fichier | Rôle | Pur (sans DOM) |
|---|---|---|
| `config.ts` | Identifiants d'effets, bornes, valeurs par défaut, **validation** (`normalize*`), combinaisons prêtes à l'emploi, `resolveComponentMotion` | oui |
| `effects.ts` | Bibliothèque d'effets : images clés Web Animations, propriétés animées déclarées | oui |
| `swap.ts` | Transition entre deux états d'un élément (stabilisation, un seul calque sortant, mise à jour sur place) | oui |
| `presence.ts` | Liste animée (chat, futures files d'alertes) : sortants gardés à leur place, retraits instantanés, plafond | oui |
| `engine.ts` | Lecture (`playEnter`, `playExit`, `playLoop`), un canal par élément (`main`, `loop`), annulation, mouvement réduit | non (WAAPI) |
| `react.tsx` | `AnimationProvider`, `AnimatedSwap`, `useListItemMotion` : adaptateurs sans aucune règle propre | non |

La logique, les données et le rendu sont séparés :
- les **données** (`OverlayConfig.animations`, `OverlayComponentConfig.animation`, les paramètres d'URL du chat) sont validées par `config.ts` ;
- la **décision** (quoi animer, quand) est prise par `swap.ts` et `presence.ts` ;
- le **rendu** (`OverlayPreview.tsx`, `TwitchChatOverlay.tsx`) ne fait que brancher les deux.

### Effets disponibles
- **Entrée** : fondu, glissement depuis la gauche, la droite, le haut ou le bas, zoom avant, apparition avec rebond, révélation du texte.
- **Sortie** : fondu, glissement vers la gauche, la droite, le haut ou le bas, zoom arrière, réduction progressive, disparition floutée.
- **Décoratifs** (désactivés par défaut) : pulsation, lueur, brillance, flottement, rotation (pochette ronde).
- **Défilement des titres trop longs** : déjà présent (débordement « Défilement », `MarqueeText`). La barre de progression reste animée image par image (`LiveProgress`).

Les identifiants historiques (`fade`, `slideLeft`, `slideRight`, `slideUp`, `slideDown`, `scale`) sont conservés : les overlays enregistrés restent valides. Un glissement porte le nom du **sens du mouvement** : `slideUp` monte, donc arrive par le bas. Côté sortie, `scale` correspond au zoom arrière.

### Ajouter un effet
1. Ajoutez son identifiant dans `ENTER_EFFECTS`, `EXIT_EFFECTS` ou `LOOP_EFFECTS` (`config.ts`).
2. Ajoutez sa définition dans `ENTER_EFFECT_DEFS`, `EXIT_EFFECT_DEFS` ou `LOOP_EFFECT_DEFS` (`effects.ts`) :
   - libellé ;
   - `properties` (propriétés animées) ;
   - `keyframes(ctx)` à partir de `ctx.intensity` (0 → 1) ;
   - pour un effet décoratif : `target` (`self` ou `decoration`), et `decorationStyle` / `clip` si besoin.
3. C'est tout. L'éditeur, le chat et la validation le proposent automatiquement.

`tests/animation.test.ts` vérifie ensuite que la définition existe, qu'aucune propriété de mise en page (`width`, `top`, `margin`…) n'est animée, et qu'une entrée finit visible et une sortie cachée.

### Utiliser le moteur dans un overlay
- **Un élément qui change de contenu** :
  ```tsx
  <AnimatedSwap swapKey={contenu} value={données} render={(v) => …} layerStyle={boîte} enter={…} exit={…} loop={…} />
  ```
  Une nouvelle `swapKey` déclenche une transition. Même clé avec des données plus récentes : mise à jour sur place, sans animation.
- **Une liste** : `reconcilePresence(...)` puis `useListItemMotion(ref, { leaving, enter, exit, onExited })` sur chaque ligne ; `removeLeaving` à la fin de la sortie.
- **Activation** : les animations ne jouent que sous un `<AnimationProvider>` (page OBS, aperçu de l'éditeur). Ailleurs (miniatures, listes, rendu serveur), le rendu est **strictement identique** à l'ancien (test HTML dans `tests/overlay-motion.test.ts`).

### Configuration
- **Overlay** (`OverlayConfig.animations`) :
  - champs historiques : `enter`, `exit`, `durationMs`, `easing` ;
  - champs **facultatifs** : `enabled`, `delayMs`, `intensity`, `staggerMs` (décalage entre composants, dans l'ordre d'empilement), `respectReducedMotion`.
  
  Absents, ils prennent leur valeur par défaut. Les presets d'overlay ne sont pas modifiés.
- **Composant** (`OverlayComponentConfig.animation`, facultatif) :
  - `mode` : `inherit` (par défaut), `custom` ou `none` ;
  - réglages propres ;
  - `loop` : effet décoratif, déclenché `change` (quelques cycles après chaque morceau, par défaut) ou `always` (en continu pendant la lecture, **en pause** à l'arrêt).
- **Validation** : effet inconnu, durée hors bornes (50 ms à 3 s), délai négatif, `easing` refusé par le navigateur (seuls les mots-clés et un `cubic-bezier` valide passent) → valeur par défaut. Elle s'applique à l'import (`overlay-import.ts`) et au rendu.
- **Éditeur** :
  - sans sélection, section **Animations et transitions** : combinaisons Subtil, Fluide, Dynamique, Gaming, Sans animation, entrée, sortie, durée ; délai, décalage, amplitude, accélération et mouvement réduit dans « Réglages avancés » ;
  - pour chaque composant, onglet **Animation** ;
  - chaque modification rejoue la transition dans l'aperçu (alternance entre deux morceaux d'exemple), et **Tester la transition** la rejoue à la demande.
- **Diffusion** : les réglages font partie de la configuration de l'overlay, déjà poussée en direct par WebSocket à l'enregistrement. Aucun système de synchronisation supplémentaire : la source n'est pas rechargée.
- **Chat** : paramètres d'URL compacts, seulement s'ils diffèrent du défaut (aucune nouvelle URL pour un chat déjà configuré) :
  - `anim` : arrivée, `slideUp` par défaut, soit l'ancienne animation CSS de 0,4 em en 180 ms ;
  - `out` : départ, `fade` par défaut ;
  - `ad` : durée en ms, 50 à 2000, 180 par défaut ;
  - `motion` : `auto` (respecte le système), `on` ou `off`.
  
  Comme les autres réglages du chat, ils sont dans l'URL : il faut recopier l'URL dans la source après un changement.

### Comportement : overlay musique
- Chaque composant a sa **propre clé** (`src/utils/overlay-motion.ts`) : quand seule la pochette arrive, seule la pochette s'anime. Le fond, les formes et la barre de progression ne changent pas d'un morceau à l'autre ; ils entrent et sortent avec la musique.
- **Stabilisation de 140 ms** avant toute transition. Une rafale de métadonnées partielles ne donne qu'une transition, vers l'état final ; un aller-retour A → B → A n'anime rien. Au plus **un** calque sortant à la fois.
- **Mises à jour tardives** : un état mesuré avant celui déjà affiché est ignoré (`isStalePlayback`), sauf saut d'horloge de plus de 10 s.
- Lecteur indisponible : l'état « rien en lecture » fait sortir les composants proprement. Une reconnexion reprend l'état courant sans rechargement.

### Comportement : chat
- **Arrivée** animée pour chaque nouveau message. L'historique rejoué à la connexion n'est pas animé, pour éviter une cascade au chargement.
- **Départ** animé pour la disparition automatique (`fade`) et le dépassement du nombre maximal. Un message sortant **garde sa place** dans le flux : aucun chevauchement, ordre conservé. Au plus 8 sortants simultanés : sous un flot, les plus anciens partent sans attendre.
- **Modération** (message supprimé, utilisateur purgé, chat vidé) : retrait **immédiat**, jamais animé.
- La connexion Twitch, les permissions et le contenu ne sont pas touchés. Badges, emotes et couleurs sont rendus exactement comme avant.

### Nettoyage et performances
- Une animation par élément et par canal. En lancer une autre l'annule, et `finished` se résout toujours (`finished`, `cancelled` ou `skipped`), sans promesse rejetée.
- Démontage : `cancelAll(el)`, minuteurs de stabilisation effacés, écouteurs `matchMedia` retirés.
- Aucune boucle permanente par défaut. Un effet continu tourne sur le compositeur (WAAPI) et se met en pause quand la musique s'arrête.
- Seuls `opacity` et `transform` sont animés, sauf deux exceptions déclarées : le **flou de sortie** (`filter`, plafonné à 8 px) et la **révélation du texte** (`clip-path`). Toutes deux sont repeintes, sans recalcul de mise en page. La lueur et la brillance animent un calque décoratif (opacité ou translation) : le composant n'est jamais repeint.
- `prefers-reduced-motion` est respecté par défaut (musique : réglage par overlay ; chat : `motion=auto`). Il n'y a ni clignotement ni opacité coupée net.

### Tests
- `tests/animation.test.ts` : validation, défauts, compatibilité des anciens identifiants, résolution, presets ; bibliothèque d'effets (propriétés autorisées) ; moteur avec de faux éléments (remplacement, annulation, canaux, pause, absence d'API) ; transitions (changements successifs, rafales, aller-retour, sortie interrompue) ; listes (arrivées rapides, retrait animé, modération, plafond, retour pendant la sortie).
- `tests/overlay-motion.test.ts` : clés de contenu (accents, plusieurs artistes), mises à jour tardives, rendu identique des six presets, import d'anciens exports et de valeurs invalides, paramètres d'URL du chat (anciennes URL inchangées, aller-retour, valeurs invalides, désactivation).
- **Vérifié dans Chromium** (bancs d'essai hors dépôt, pages bundlées contre les vrais serveurs) : 29 vérifications. Elles couvrent la transition réelle, la rafale, l'aller-retour, l'état tardif, le réglage changé en direct sans rechargement, la désactivation, le mouvement réduit, la pause de la rotation, le chat en rafale (ordre, plafond, absence de chevauchement), la modération et la disparition automatique.

### Limites connues
- **Non vérifié dans OBS réel ni sous Windows**. Les mesures de fluidité ont été prises en Chromium headless (rendu logiciel, 60 images/s pendant une transition) : elles ne prédisent pas le comportement d'OBS avec un jeu en cours.
- La stabilisation retarde un vrai changement de morceau d'environ 140 ms.
- Chat : les réglages d'animation sont dans l'URL, donc il faut recopier l'URL. Les messages arrivés à la connexion ne sont pas animés.
- L'entrée part toujours de l'état caché : une transition interrompue en plein milieu repart du début, sans continuité de l'animation précédente.
- La révélation du texte n'est proposée que pour les composants texte. La rotation n'a de sens que pour une pochette ronde.

## 6decies. Alertes (followers, abonnés) et fond « pochette »

### Alertes : architecture

```
Twitch EventSub (twitch/chat.rs, abonnements facultatifs) ─┐
Live TikTok (tiktok/service.rs, page invisible) ───────────┤
Outil TikTok → GET/POST /api/alerts/hook/<clé> ────────────┼─▶ AlertHub (crates/batlay-core/src/alerts/hub.rs)
Bouton « Tester » (commande alerts:test) ───────────────────┘     validation · filtre · débit
                                                                    │ OverlayServer::broadcast_alert
                                                                    ▼  WS /ws-alerts
                                             page /alerts (alerts/AlertsApp.tsx) : file → AlertPlayer → AlertCard
```

Les fichiers :
- `crates/batlay-core/src/alerts/model.rs` : types, `normalize_incoming` (tout événement entrant), `normalize_config`, `should_show`, `describe` ; `shared/alerts.ts` garde les types et le rendu du texte (`renderAlertText`, texte, jamais HTML) pour les pages.
- `src/alerts/` : file (`queue.ts`, pure), cycle d'une alerte (`AlertPlayer.tsx` : entrée, son, durée, sortie, avec le moteur d'animations commun), rendu partagé OBS / aperçu (`AlertCard.tsx`), sons (`sound.ts` : carillon synthétisé en Web Audio, ou fichier).
- `crates/batlay-core/src/alerts/media.rs` : images et sons importés, type reconnu par le contenu (octets), nom = hash, servis par `/api/alert-media/<id>`.

Le réglage s'enregistre dans `batlay-config.json` (`alerts`, `alertsHookKey`). Il est poussé en direct aux pages ouvertes, sans rechargement.

### Twitch
- **Abonnements EventSub** : `channel.follow` v2 (condition `moderator_user_id` = la chaîne), `channel.subscribe` (ignoré si `is_gift`), `channel.subscription.message`, `channel.subscription.gift` (anonyme géré). Ils ne sont demandés que si le chat affiché est celui du compte connecté.
- **Permissions** : `moderator:read:followers` et `channel:read:subscriptions` sont **facultatives**. `/validate` n'exige toujours que `user:read:chat` : une ancienne session reste valide. `TwitchState.alertScopes` indique ce qui est accordé.
- **Reconnexion** : « Autoriser les alertes » garde la session et le chat pendant la nouvelle autorisation. En cas de refus ou de délai dépassé, l'ancienne session reste en place. En cas de succès, l'ancien jeton est révoqué.

### TikTok
TikTok ne publie aucune API pour les follows, abonnements et cadeaux d'un live. Trois méthodes, au choix de l'utilisateur (`TikTokLiveSettings.mode` : `browser` par défaut, `euler`, `local` ; `library` de la 0.4.x = `euler`). Une seule est active à la fois ; `tiktok:update-settings {mode}` rebranche aussitôt.

**1. Navigateur intégré (`browser`, page Alertes → TikTok → pseudo → Se connecter).** Aucun service tiers :
- `tiktok_pages.rs` ouvre la page `tiktok.com/@pseudo/live` dans une fenêtre WebView2 **invisible** (ni affichée, ni dans la barre des tâches), profil TikTok à part, son coupé, navigation limitée à tiktok.com, aucune fenêtre secondaire. Aucun script n'y est injecté.
- `cdp.rs` écoute, par le protocole DevTools propre à cette fenêtre, les trames binaires des sockets « webcast » (`is_webcast_socket_url`) ; le cœur les décode (`tiktok/webcast.rs` : `PushFrame` → gzip → `FetchResult` → messages `Chat`, `Social`, `Gift`, `Like`, `RoomUserSeq`, `Barrage`, `Control`).
- `tiktok/browser.rs` (`run`) : branché dès que la page ouvre le flux ; sinon « aucun flux » après 25 s (réarmé tant que la page est affichée), nouvel essai en alternant page légère (sans vidéo ni images) et page complète. Flux fermé : 20 s de grâce pour que la page le rouvre, puis rebranchement.
- `tiktok/service.rs` : attente du live (hors live : 90 s ; fin de live : 60 s ; autres erreurs : 30 s → 5 min), « Déconnecter », reprise au démarrage si actif. Avant d'ouvrir la page, recherche du live (oEmbed officiel pour le compte, puis la page elle-même par une sonde invisible qui lit `api-live` et `SIGI_STATE` depuis la page, puis requêtes directes). Statut 2 avec un identifiant vide (compte privé) donne l'état `hidden` : avec la session TikTok connectée, Batlay se branche quand même.
- Événements : `follow` → `follow` (un même spectateur une fois par live), Super Fan → `subscribe`, `gift` → **`tiktokGift`** (une série = une alerte, à la fin ; valeur minimale en diamants, quantité comprise ; variable `{gift}`). Commentaires → fenêtre Live et overlays du chat (`src=`). Audience : `viewerCount`, `totalLikeCount` → `{viewers}` `{likes}` (1 envoi/s au plus).
- « Se connecter à TikTok » ouvre la page officielle `tiktok.com/login` (profil TikTok) ; Batlay lit seulement la **présence** du cookie `sessionid` (toutes les 1,5 s tant que la fenêtre est ouverte), jamais sa valeur. « Se déconnecter » efface tout le profil TikTok.
- « Afficher la page TikTok » (`tiktok:show-page`, événement `tiktok:page-visible`) montre la page invisible (vérification demandée par TikTok) ; la fermer la remet en arrière-plan sans couper le live.
- Diagnostic (`tiktok:diagnose`) : compte, session, recherche du live, dernier état, sans connexion au live ; rapport texte prêt à copier.
- **Non vérifié** sur un vrai live : le format exact des URL de flux et des messages (schéma communautaire), le comportement d'une page cachée sur la durée.

**2. Euler Stream (`euler`).** Même déroulé que la bibliothèque communautaire tiktok-live-connector 2.5, réécrit dans `tiktok/euler.rs` :
- recherche du live d'abord par simples requêtes (page du live, `api-live`), la fenêtre invisible en dernier ;
- `GET https://api.eulerstream.com/webcast/rooms/{room}/connect?client_enter=true&user_agent=…` (clé facultative dans l'en-tête `x-api-key`, jamais dans l'adresse ; aucune redirection suivie) → `ProtoMessageFetchResult` (protobuf) + cookie `x-set-tt-cookie` (`nom=valeur` seul, caractères vérifiés) ;
- WebSocket vers `wsUrl` (refusée si ce n'est pas un `wss://` webcast de tiktok.com) avec `cursor`, `internal_ext`, `wsParams` et les paramètres de la page web ; trame `im_enter_room`, `hb` toutes les 10 s, `ack` (`logId` + `internalExt`) pour chaque trame qui le demande ; décodage par `webcast.rs` ;
- **jamais** la session TikTok (ni `x-cookie-header`, ni `x-oauth-token`) : un live caché aux visiteurs donne l'état `hidden` avec un message qui renvoie au navigateur intégré ;
- erreurs : 429 → attente 2 min ; 401/403 (clé) et 402 (offre payante) → erreur sans nouvel essai ; autres → 30 s → 5 min ;
- clé (`tiktok:set-euler-key`, `null` pour l'effacer) chiffrée par DPAPI dans `tiktokEuler.apiKeyDpapi`, refusée si le chiffrement est indisponible ; l'état publie seulement `eulerKey: bool`.
- **Non vérifié** contre le vrai service ni sur un vrai live.

**3. Adresse locale (`local`) pour un outil tiers** (TikFinity, Streamer.bot…) : Batlay ne se connecte pas au live, la carte TikTok affiche l'adresse :
- `/api/alerts/hook/<32 hex>` : `GET ?type=follow&user=NOM` ou `POST` JSON `{"type","user","amount","months","message","gift"}` (4 Ko au plus) ;
- types acceptés : `follow`, `subscribe` (alias `sub`), `resub`, `gift`, `tiktokGift` (alias `cadeau`) ;
- la clé est comparée en temps constant ; une mauvaise clé renvoie 404 ;
- débit limité à 20 alertes par tranche de 10 s (les deux chemins partagent cette limite) ;
- le contrôle `Host` du serveur s'applique ; l'`Origin` n'est pas exigée, car c'est une URL à clé appelée par des outils.

### Fond « pochette »
- `FillConfig.mode = "artwork"`, avec `artwork: { blur, zoom, dim }` facultatif (`resolveArtworkFill`). La couleur sert de fond de secours.
- Rendu : une `<img>` (l'URL ne passe jamais dans du CSS), `object-fit: cover`, `transform: scale`, `filter: blur() brightness()`. Elle est placée en `z-index: -1` dans un bloc `isolation: isolate`, et rognée par un conteneur `overflow: hidden` qui suit l'arrondi.
- Changement de pochette : la clé de transition du bloc inclut la pochette, d'où un fondu par le moteur d'animations.
- Coût : le flou est calculé une fois par image affichée (pas d'animation continue). Au-delà de 40 px, il coûte plus au GPU.

### Tests
- Rust (`npm run test:core`) : `tiktok/*` (pseudo, réglages et méthodes, oEmbed, réponse réelle d'un compte privé, page du live, délais, décodage protobuf et gzip avec des trames du vrai encodeur, service avec de fausses pages : live, cadeaux, hors live, compte privé, alternance légère/complète, diagnostic ; Euler simulé : signature, adresse du flux, trames envoyées et accusés, limite, clé refusée, live caché, session jamais envoyée ; adresse locale), `net` (en-têtes et trames binaires de la WebSocket sortante sur un vrai socket), `alerts/*` (validation, réglages, clé, débit, filtres, fichiers importés), `twitch/eventsub.rs` (conversion des événements Twitch).
- `tests/alerts.test.ts` : validation et réglages côté pages, file, fond pochette.
- Hors dépôt, dans Chromium contre le vrai serveur : 17 vérifications. Elles couvrent la page `/alerts`, l'image servie, l'adresse TikTok en GET et en POST, la mauvaise clé, la file ordonnée, les variables, les balises affichées comme du texte, le réglage en direct, « Passer » et le fond flouté sous le texte.

### Limites
- **Non testé** avec un vrai compte Twitch, un vrai live TikTok, dans OBS, dans TikTok LIVE Studio, ni avec un outil TikTok réel.
- **TikTok direct** : lecture non officielle de la page du live, contraire aux conditions de TikTok, qui peut cesser de fonctionner à tout moment (changement du site ou du format du flux).
- **Son** : la lecture automatique dépend du logiciel. OBS la permet ; c'est inconnu pour la source Lien de TikTok LIVE Studio.
- **Pas d'historique** : une alerte arrivée alors qu'aucune page n'est ouverte est perdue.

## 6undecies. Fenêtre « Live » (0.3.9)

```
AlertHub (résultat) ──────────┐
TwitchService (messages) ─────┼─▶ LiveHub (crates/batlay-core/src/live.rs) ──▶ événement live:update ──▶ page /live
TikTokService (commentaires) ─┤      journal · chats récents · compteurs        (fenêtre principale ET fenêtre détachée)
états Twitch / TikTok ────────┘
TikTokService (commentaires) ──▶ TwitchService::inject_chat ──▶ overlays du chat (src=all|twitch|tiktok)
```

- `crates/batlay-core/src/live.rs` : résumés (`describe`, mêmes textes qu'avant), compteurs (les tests ne comptent pas), commentaires TikTok, derniers spectateurs, audience. `shared/live.ts` garde les types, les libellés (`ALERT_OUTCOME_LABELS`) et les filtres du journal pour la page.
- Mémoire de session seulement (300 lignes de journal, 300 messages) : rien n'est écrit sur disque. Une ligne d'état identique n'est pas répétée pendant 30 min (nouvel essai TikTok toutes les 90 s).
- **Fenêtre détachée** : `#/live?window=1`, mêmes protections que la fenêtre principale (fenêtre « live » de `capabilities/main.json`). `App.tsx` n'y lance ni la musique, ni le chargement des overlays, ni les mises à jour, pour éviter une double diffusion vers OBS. `live:set-always-on-top` n'agit que sur cette fenêtre.
- **Chat** : `ChatMessage.platform` est facultatif (absent = Twitch). Overlay : `src=` (all, par défaut) et `pico=1` (repère : pastille de couleur avec une lettre, aucun logo de marque).
- **Polices** : `CHAT_FONTS` / `FONT_STACKS` / `FONT_LABELS` (twitch-overlay/chat-feed.ts). Les trois polices de Batlay sont importées dans `twitch-overlay/main.tsx` et servies par `/assets`. Les autres sont des polices de Windows, avec une police de repli.
- **Animations** : ajoutées dans `src/animation/effects.ts` et `config.ts` ; le chat, les alertes et l'éditeur les proposent automatiquement.

## 6duodecies. Overlays personnalisés (0.4.0)

- `OverlayConfig.kind = "custom"` (absent = musique). Dans un overlay personnalisé, `image`, `shape`, `text` et `clock` (`MUSIC_INDEPENDENT_TYPES`) restent affichés sans musique : `OverlayPreview` leur donne un morceau vide. Un `text` qui ne contient que des variables de musique disparaît alors.
- **Composant `image`** :
  - `mediaId` = fichier du dossier `alert-media`, importé par `alerts:import-media` (octets envoyés en base64, type vérifié par le contenu, 8 Mo max, nom = hash) ;
  - page OBS : `/api/alert-media/<id>` sur le même serveur ;
  - tableau de bord : data URL, mise en cache dans `src/components/media-cache.ts` (zustand). Le rendu (`OverlayMedia.tsx`) n'en dépend pas : il passe par un contexte React.
- **Éditeur** : ajout d'une image par la palette, par **glisser-déposer** sur le canvas (`EditorCanvas.onDropImage`, point du dépôt en coordonnées du canvas) ou par **collage** (Ctrl+V hors des champs de saisie). Taille : `fitImageSize` (proportions, 40 % du canvas au plus, jamais agrandie).
- **Composant `clock`** : `style.clockFormat` (`time`, `timeSeconds`, `date`, `dateTime`), formaté en `fr-FR`, mis à jour chaque seconde. Sa clé de transition ne change pas avec l'heure.
- **Modèles** : `src/presets/custom-templates.ts`. Rien ne dépasse du canvas (vérifié par les tests). L'écran vertical garde ses textes hors du tiers bas de l'écran.
- **Diagnostic TikTok** : `TikTokService::diagnose` (commande `tiktok:diagnose`). Il passe par le compte (oEmbed), la session, la recherche du live et le dernier état, sans connexion au live, et produit le rapport texte.
- **Infos du live** : les statistiques du `LiveHub` (cœur) sont poussées par `OverlayServer::broadcast_live_stats` (message WS `live`, renvoyé à chaque nouvelle page). La page OBS le revalide (`normalizeLiveStats`) et le fournit par `LiveStatsContext` ; l'éditeur montre `SAMPLE_LIVE_STATS`. `renderTemplate(template, music, live)` remplace `{lastFollower}`…
- **`countdown`** : `style.countdownSeconds`, texte de fin dans `content`, démarre au montage. **`goal`** : `style.goalMetric`, `style.goalTarget`, `fillColor`, `trackColor`, libellé dans `content`.
- **Mes images** : `MediaStore::list("image")` (commande `alerts:list-media`), les plus récentes d'abord, 60 au plus.
- Tests : `tests/custom-overlays.test.ts` ; infos du live et diagnostic dans les tests Rust (`live.rs`, `tiktok/service.rs`).

## 7. Build

```bash
npm run build:front  # tsc + build Vite (tableau de bord + pages OBS) dans dist/
npm run dist         # Tauri : application + installateur NSIS (Windows uniquement)
```

`npm run dist` produit `src-tauri/target/release/bundle/nsis/Batlay_x.y.z_x64-setup.exe` (installation « pour moi », comme avant, dans `%LOCALAPPDATA%\Batlay`). Les pages OBS sont copiées à côté de l'exécutable (`resources/dist`), d'où le cœur les sert. **Non vérifié dans cet environnement** : la compilation réelle se fait dans GitHub Actions (§13).

**Installateur et 0.4.x** (`src-tauri/windows/hooks.nsh`) : avant de copier les fichiers, l'installateur retire en silence l'ancienne version Electron (`%LOCALAPPDATA%\Programs\batlay`, ou le dossier lu dans la liste des programmes de l'utilisateur : seules les entrées dont le dossier contient « Uninstall Batlay.exe » sont concernées) avec son propre désinstallateur, qui ne touche pas à `%APPDATA%\Batlay`. Une ancienne installation « pour tous les utilisateurs » demande les droits administrateur : l'installateur le signale et la laisse en place.

## 8. Utilisation avec OBS

1. Dans Batlay, ouvrez un overlay dans l'**éditeur**.
2. Cliquez sur **URL OBS** (en-tête de l'éditeur, carte de la page *Mes overlays* ou barre latérale) : l'URL est copiée (ex. `http://localhost:3000/overlay/abc123`).
3. Dans OBS : **Sources > Browser Source** > collez l'URL.
4. Renseignez la taille recommandée affichée dans l'éditeur (largeur/hauteur du canvas de l'overlay).
5. Le fond de la page est transparent nativement (`overlay.html` force `background: transparent`) ; le fond que vous définissez dans l'éditeur (couleur, transparence, dégradé) s'y ajoute.
6. **Enregistrer** dans l'éditeur applique les changements **en direct** dans OBS (le serveur pousse la nouvelle configuration), sans recharger la Browser Source.
7. Polices : OBS n'a accès qu'aux polices installées sur son PC. Il n'y a volontairement aucun flou d'arrière-plan (`backdrop-filter`) : dans une Browser Source il n'y a rien derrière la page à flouter.

## 8bis. Autres logiciels de streaming (page Intégrations)

Guide utilisateur complet : **[INTEGRATIONS.md](INTEGRATIONS.md)**. Côté code :

- `src/integrations/` (pur, sans React, testé dans `tests/integrations.test.ts`) :
  - `platforms.ts` : registre OBS / TikTok LIVE Studio / Streamlabs / XSplit / Autre, avec statut, étapes, limites et alternatives. Règle : un statut ne dit jamais plus que ce qui a été vérifié ; seul OBS est « référence », et seul OBS a des capacités « vérifié ».
  - `formats.ts` : paysage / portrait / carré. La source d'un overlay musical a **toujours** la taille de son canvas ; seul un facteur de réduction *dans la scène* est conseillé.
  - `urls.ts` : variantes d'une URL **existante** (hôte `127.0.0.1`, `fit=contain`), refus de toute URL qui n'est pas une adresse http locale, identification d'une URL collée (overlay, chat, lien révoqué).
  - `diagnostics.ts` : vérifications affichées (serveurs, sources connectées).
- **Rendu inchangé** : `overlay/OverlayApp.tsx` lit `?fit=contain` et passe une échelle numérique à `OverlayPreview` (le même chemin que le zoom de l'éditeur) ; sans paramètre, c'est toujours `scale="1:1"`.
- `OverlayServer::client_counts` → commande `overlay:get-client-counts` : nombre de WebSocket ouverts par overlay (relevé toutes les 2 s par la page).
- Commande `integrations:open-docs` : le tableau de bord envoie un **identifiant** de logiciel ; le cœur ouvre l'adresse de sa liste fermée (`crates/batlay-core/src/integrations.rs`, https uniquement).
- Sécurité : les deux serveurs appliquent le même contrôle `Host` / `Origin` (`crates/batlay-core/src/loopback.rs`). Ils n'écoutent que sur 127.0.0.1 (un seul point d'écoute dans tout le code, vérifié par `tests/tauri-guards.test.ts`) ; aucun port supplémentaire n'est ouvert.

## 9. Import / Export

Chaque overlay peut être exporté en `.json` (`schemaVersion: 1`) depuis la page **Mes overlays** (menu « … » de la carte). L'import avec validation stricte du schéma **n'est pas encore implémenté** — actuellement seul l'export fonctionne.

## 10. Ajouter un nouveau provider musical

Implémentez l'interface `MusicProvider` (`src/services/music-provider.ts`) :

```ts
interface MusicProvider {
  connect(): Promise<void>;
  disconnect(): Promise<void>;
  getCurrentTrack(): Promise<Track | null>;
  getPlaybackState(): Promise<PlaybackState>;
  onStateChange(listener: (state: PlaybackState) => void): () => void;
  isConnected(): boolean;
}
```

Voir `src/services/demo-provider.ts` (aucune dépendance externe), `src/services/spotify-provider.ts` (via le cœur + OAuth) et `src/services/system-media-provider.ts` (via le cœur + Windows, section 6bis) comme références.

## 11. Limitations connues (honnêtes, à date)

- Import d'overlay `.json` non implémenté (export seulement).
- Mises à jour : voir §13 (via les Releases GitHub, versions installées uniquement).
- Identité visuelle 0.5 (§6octies-bis) : icône (`assets/icon.svg` = source vectorielle ; `icon.png` 512 px, `icon_2x.png` 1024 px, `icon.ico` 16→256 px, `favicon.png`) et visuels de l'installateur (`assets/installer/*.bmp`, format imposé par NSIS : pas de JPG). Rendu vérifié dans Chromium (pages du tableau de bord, clair et sombre) ; **jamais vu dans WebView2 sur Windows** : barre de titre sans cadre (glisser, double-clic, redimensionnement, Snap), installateur et désinstallateur refaits, mise à jour silencieuse.
- Rien de tout ceci n'a été testé avec un vrai compte Spotify, dans l'application installée ni dans OBS (voir « État actuel »).
- **Spotify Desktop, recoupement par la fenêtre** : écrit d'après le comportement décrit par d'autres projets (libspotifyctl) ; le script PowerShell correspondant n'a pas pu être exécuté hors Windows. Le titre `Advertisement` de la fenêtre est reconnu en plusieurs langues d'interface mais n'a pas été vérifié pour chacune.
- **Plusieurs artistes en Lecture système** : Batlay affiche la chaîne fournie par Windows. Je n'ai pas pu vérifier ici si Spotify Desktop y place tous les artistes ou seulement le principal ; si c'est le second cas, c'est une limite de la source, et seul le provider Spotify (API, compte Premium) donne la liste complète.
- **Polling PowerShell** : chaque relevé de la Lecture système lance un process PowerShell (quelques centaines de ms). La précision est bonne grâce à l'horodatage exact, mais la détection d'un changement peut prendre jusqu'à ~2 s. Un processus PowerShell persistant réduirait cette latence, mais n'a pas été tenté sans Windows pour le valider.
- **Recherche de pochette et écritures non latines** : `comparable()` (`src/services/artwork-lookup.ts`) ne garde que `[a-z0-9]`, donc un titre ou un artiste 100 % japonais/cyrillique/arabe y devient une chaîne vide et n'est jamais jugé « plausible » — pas de pochette en ligne pour eux (celle du lecteur, si elle existe, reste utilisée). Non modifié à ce jour (normalisation dédiée).
- **Reconnexion après la 0.4** : les jetons Twitch et Spotify chiffrés par Electron ne sont pas lisibles par la 0.5 (§2bis) : il faut se reconnecter une fois.
- `npm run lint` inutilisable (pas de `eslint.config.*`).
- Animations : voir les limites listées en §6nonies.
- Le provider "Lecture système" (section 6bis) n'a pas non plus été testé sur une vraie machine Windows — le script PowerShell est écrit selon le pattern WinRT documenté par la communauté, mais à valider en priorité.

## 12. Prochaines étapes suggérées

1. Pousser une branche : `.github/workflows/ci.yml` compile l'application Tauri sous Windows et lance tous les tests ; corriger ce qu'il signale.
2. `npm run dev` sur Windows : Mode Demo de bout en bout (tableau de bord → éditeur → OBS), fenêtre Live, thème.
3. Installer par-dessus une 0.4.x : vérifier le retrait de l'ancienne version, la reprise des overlays et réglages, la reconnexion Twitch / Spotify.
4. Lecture système sur un vrai Windows (accents, artistes, pochette, position).
5. Live TikTok réel (page invisible, compte privé connecté).
6. Implémenter l'import JSON avec validation de schéma.

## 13. Mises à jour depuis GitHub

Le bouton **Paramètres → Mises à jour** cherche la dernière version dans les *Releases* du dépôt GitHub,
la télécharge sur clic (SHA-512 vérifié), puis « Installer et redémarrer » affiche l'écran Batlay « Installation de
la mise à jour », lance l'installateur en silence (`/S /R` : aucune fenêtre d'installateur, puis relance de Batlay) et
ferme Batlay. Un bouton « Mettre à jour » apparaît aussi dans la barre de titre. Une recherche discrète a lieu 10 s après le démarrage (elle ne télécharge
rien). Fonctionne dans la version **installée** uniquement (pas avec `npm run dev`). Code : `crates/batlay-core/src/updater.rs`.

**Dépôt** : `BatStream-off/Batlay` (`updater.rs`, `package.json` → `repository`). Le dépôt doit être **public**.

**Publier une version** : changez la version partout (package.json, package-lock.json, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `crates/batlay-core/Cargo.toml` ;
`tests/version.test.ts` vérifie qu'elles concordent), puis `git tag v0.5.0 && git push --tags` :
`.github/workflows/release.yml` lance les tests (interface et cœur), construit l'installateur, et publie
`Batlay-Setup-X.Y.Z.exe` avec `latest.yml`. Un test en échec bloque la Release : lancez-les en local avant le tag.

**Passage depuis la 0.4.x** : `latest.yml` garde le format d'electron-builder (`files[0].url`, `sha512` en base64,
`size`, `path`), si bien que la recherche de mise à jour de la 0.4.x trouve la 0.5, la télécharge, vérifie son
empreinte et lance son installateur (en mode normal, avec ses fenêtres). L'installateur retire l'ancienne version
(§7). Les réglages sont repris ; Twitch et Spotify demandent une reconnexion.

**Revenir à la 0.4.1** : désinstaller Batlay (Paramètres > Applications), puis installer `Batlay-Setup-0.4.1.exe`
depuis les Releases. `%APPDATA%\Batlay` n'est pas supprimé : réglages et overlays restent (la 0.4.1 ignore les
nouvelles clés de jetons et redemande la connexion à Twitch et Spotify).

**Notes de version** : `docs/release-notes/vX.Y.Z.md` (texte pour les utilisateurs, plus court que le CHANGELOG),
reprises automatiquement comme description de la Release.

## 14. Chat Twitch dans OBS

Module **indépendant** de l'overlay musical : autre serveur, autre port, autre stockage, autre page OBS.

| | Overlay musical | Chat Twitch |
|---|---|---|
| Port | 3000 (réglable dans Paramètres) | **4000, fixe** (aucun réglage, aucun champ) |
| Code | `crates/batlay-core/src/overlay_server.rs` | `crates/batlay-core/src/twitch/`, `twitch-overlay/` |
| Page OBS | `/overlay/<id>` | `http://localhost:4000/overlay/chat` |

Le port 4000 est **réservé** : Paramètres refuse de l'attribuer à l'overlay musical, et le cœur l'ignore si un ancien réglage le contenait (`merge_settings`, `normalize_settings`).

### Configuration Twitch (une fois)
1. Sur https://dev.twitch.tv/console/apps, l'application (Client ID `vog5xlub7gikyjfoseohp2uo33di6x`) doit avoir l'URI de redirection **exactement** `http://localhost:4000`.
2. Type de client **Confidentiel** : le flux « Implicit » de Twitch n'est pas ouvert aux clients « Public » (ceux-ci n'ont que le Device Code). Le secret n'est **jamais utilisé ni saisi** par Batlay.

### Fonctionnement
```
Batlay ── ouvre le navigateur ──▶ id.twitch.tv/oauth2/authorize (response_type=token, scope user:read:chat, state aléatoire)
Twitch ── redirige ──▶ http://localhost:4000/#access_token=…   (jeton dans le fragment : jamais envoyé au serveur)
page de callback ── retire le jeton de l'URL, POST /auth/callback ──▶ cœur (twitch/server.rs)
cœur : contrôle du state, /validate (client_id, permission), profil, jeton chiffré (DPAPI)
EventSub WebSocket (channel.chat.message + suppression/purge) ──▶ serveur :4000 ──WS──▶ page OBS
```
- **Aucun Client Secret** dans le frontend, le cœur ni le dépôt (test `tests/tauri-guards.test.ts`). Le flux Implicit n'en a pas besoin.
- Le tableau de bord ne voit **jamais** le jeton : fichier séparé `batlay-twitch.json` (clé `tokenDpapi`), chiffré par DPAPI, absent de `config:get`. Si le chiffrement est indisponible, la session reste en mémoire et un avertissement s'affiche (jamais d'écriture en clair).
- Gestion des jetons : contrôle `/validate` au démarrage puis **toutes les heures** (exigé par Twitch) ; 401 → session effacée + « Reconnectez votre compte » ; réseau coupé → le jeton est conservé et la restauration réessayée ; « Déconnecter » révoque le jeton chez Twitch et l'efface.
- Erreurs OAuth gérées : refus (`access_denied`), `state` invalide (ignoré sans annuler la connexion en cours), délai de 5 min, mauvais `client_id`, permission manquante, URI de redirection incorrecte, port 4000 occupé.
- Le serveur n'écoute que sur `127.0.0.1`, refuse tout `Host`/`Origin` étranger (DNS-rebinding) et les WebSocket hors `/ws/chat`.
- Le jeton implicite n'a **pas de refresh token** : à l'expiration il faut se reconnecter (un clic).

### Choisir la chaîne affichée
Par défaut le chat affiché est celui du compte connecté. Page **Chat** → *Chaîne affichée* : collez un **pseudo** (`ninja`, `@ninja`) ou un **lien** (`https://www.twitch.tv/ninja`, lien pop-out, `m.twitch.tv`) pour lire le chat d'une autre chaîne ; vide ou « Ma chaîne » = la sienne.
- La **connexion Twitch reste obligatoire** (EventSub exige un jeton), avec la même unique permission `user:read:chat`, qui suffit pour lire le chat public d'une autre chaîne : `broadcaster_user_id` = chaîne visée, `user_id` = compte connecté.
- Le pseudo est extrait par `parse_channel` (`crates/batlay-core/src/twitch/model.rs`, même règle que `parseTwitchChannel` de `shared/twitch.ts`), puis résolu en identifiant par `GET /helix/users?login=`. Le tableau de bord n'envoie que cette saisie (revalidée par le cœur) ; le choix est mémorisé dans `batlay-twitch.json` et survit à la déconnexion.
- Chaîne introuvable : erreur affichée, chat **éteint** (pas de repli silencieux sur un autre chat) ; Twitch injoignable : nouvel essai toutes les 15 s. Changer de chaîne vide l'historique de l'overlay et recharge les badges de la nouvelle chaîne.
- Tests : `tests/twitch-channel.test.ts` (saisie), `twitch/*` en Rust (abonnement, recherche de la chaîne).

### Utilisation dans OBS
Page **Chat** → *Connecter Twitch* → copier l'URL → OBS : Source Navigateur (420 × 640 conseillé). *Envoyer un message de test* vérifie la source sans attendre le chat. Réglages par l'URL : `?size=28&max=15&fade=30&badges=0&bg=1&opacity=70&font=mono&shadow=0&nocmd=1&top=1&w=420&h=640&anim=pop&out=fade&ad=250&motion=auto` (`w`/`h` : largeur/hauteur de la zone en px, absents = toute la source ; `font` : `segoe`, `arial`, `serif`, `mono` ; `opacity` : fond en % ; `shadow=0` : sans ombre ; `nocmd=1` : masque les messages en « ! » ; `top=1` : nouveaux messages en haut). Le bouton *Redémarrer le lien* (Overlays → Chat) recharge les sources OBS connectées.

### Limites connues
- **Rien de ceci n'a été exécuté avec un vrai compte Twitch, dans l'application installée ni dans OBS.** Les tests Rust couvrent la logique (OAuth, EventSub simulé, session, reconnexion, flux de messages) et le serveur réel (port 0, vrais sockets).
- Cheermotes affichés en texte ; pas de badges de chat partagé propres à l'autre chaîne côté images.
