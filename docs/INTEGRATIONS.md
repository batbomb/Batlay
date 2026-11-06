# Batlay dans les logiciels de streaming

> Page de l'application : **Intégrations** (barre latérale). Les textes affichés viennent de `src/integrations/platforms.ts` ; ce document dit la même chose. En cas de doute, c'est le code qui fait foi.

Batlay ne fabrique **qu'un seul rendu** par overlay : la page Web servie en local (`http://localhost:3000/overlay/<id>` pour la musique, `http://localhost:4000/overlay/chat` pour le chat). **N'importe quel logiciel capable d'afficher une page Web dans une scène peut l'utiliser.** La même URL sert dans OBS, Streamlabs, XSplit et TikTok LIVE Studio. Modifier un overlay dans l'éditeur le met à jour partout, sans le recréer pour chaque logiciel.

## ⚠️ Ce qui a été vérifié, et ce qui ne l'a pas été

**Aucun de ces logiciels n'a été lancé avec Batlay pendant le développement.** L'environnement de développement tourne sous Linux, sans Windows, sans OBS et sans TikTok LIVE Studio.

Ce qui a été vérifié automatiquement :
- La vraie page overlay (bundle de production) a été chargée dans **Chromium** contre le vrai serveur d'overlay. Chromium est le moteur des sources Web d'OBS et de Streamlabs. Les points vérifiés :
  - taille native et `fit=contain` en 1080 × 1920 ;
  - fond transparent, aucun défilement ;
  - configuration et état de lecture poussés **en direct, sans rechargement** ;
  - variante `127.0.0.1` ;
  - coupure puis reconnexion automatique ;
  - refus d'une page d'une autre origine.
- Les statuts ci-dessous viennent de la **documentation des éditeurs** quand elle existe. Sinon, ils viennent de guides tiers, et c'est alors indiqué.

**Pour vérifier chez vous**, ouvrez *Intégrations* : chaque overlay affiche un compteur **« sources connectées »**. Il passe à 1 dès qu'un logiciel a chargé la page **et** ouvert sa connexion temps réel. C'est la preuve que l'affichage et les mises à jour en direct fonctionnent dans ce logiciel.

## Compatibilité par logiciel

| Logiciel | Source à utiliser | Statut | Mises à jour en direct | Transparence | Adresse locale |
|---|---|---|---|---|---|
| **OBS Studio** | Navigateur (Browser Source) | Méthode de référence | Oui (vérifié dans Chromium) | Oui | Oui |
| **TikTok LIVE Studio** | Link | **Partielle — à vérifier** | À vérifier (compteur) | À vérifier | À vérifier (variante 127.0.0.1) |
| **Streamlabs Desktop** | Browser Source | Méthode documentée | Attendu (même moteur qu'OBS) | Attendu | Attendu |
| **XSplit Broadcaster** | Webpage | Méthode documentée | Attendu | Réglable (Display) | Attendu |
| **Autre logiciel** | Navigateur / Web / Webpage / Link / URL | Non vérifiée | Conditions ci-dessous | Conditions ci-dessous | Conditions ci-dessous |

### OBS Studio
1. **Sources → + → Navigateur**, puis collez l'URL copiée depuis Batlay (décochez « Fichier local »).
2. Saisissez la **largeur / hauteur** indiquées par Batlay. Pour un overlay musical, c'est la taille du canvas de l'éditeur.
3. Laissez le CSS personnalisé par défaut : il rend le fond transparent.
4. Si la source semble figée : **Actualiser le cache de la page actuelle**.

Documentation : [OBS — Browser Source](https://obsproject.com/kb/browser-source)

### TikTok LIVE Studio (priorité)
**Méthode principale : la source « Link ».** LIVE Studio (Windows) propose une source qui affiche une page Web. TikTok **ne la documente pas publiquement**. La procédure ci-dessous vient de guides d'éditeurs tiers (octobre 2026), et TikTok peut renommer ou déplacer cette source.

1. Ouvrez LIVE Studio et choisissez l'orientation du live (portrait conseillé).
2. **Une seule fois**, rendez `batlay.test` utilisable. Au premier lancement sous Windows, Batlay le propose de lui-même : acceptez la fenêtre d'autorisation de Windows. Sinon, cliquez sur **Configurer automatiquement** dans *Intégrations*. Batlay ajoute alors la ligne `127.0.0.1 batlay.test` au fichier hosts ; une marche à suivre manuelle reste disponible (« Le faire à la main »). Pourquoi cette étape : le champ « Lien » refuse `localhost` et `127.0.0.1` (« Saisis l'URL correcte ») mais accepte `http://batlay.test:3000/…`. `.test` est un nom réservé, qui n'existe pas sur Internet. Une fois configuré, **toutes** les URL de Batlay utilisent `batlay.test`, y compris pour OBS ; les anciennes URL en `localhost` restent valables.
3. **+ Ajouter une source → Lien (Link)**, puis collez l'URL copiée depuis **Batlay → Intégrations → TikTok LIVE Studio** : elle commence par `http://batlay.test:`.
4. Si une **résolution personnalisée** est proposée, saisissez la taille indiquée par Batlay. Sinon, ou si l'overlay apparaît coupé, cochez **Ajuster à la source** dans Batlay et recollez l'URL (elle se termine alors par `?fit=contain`).
5. Placez la source. En portrait, évitez le **tiers inférieur** et le **bord droit**, que l'interface de TikTok recouvre (commentaires, cadeaux).
6. Retournez dans Batlay : **« sources connectées »** doit passer à 1. Sinon, cochez **Variante 127.0.0.1**, recollez l'URL, puis passez à la méthode de secours si besoin.

Points non garantis par TikTok :
- **Adresse locale.** Batlay n'écoute que sur `127.0.0.1`. La variante `http://127.0.0.1:…` contourne un éventuel problème de résolution de `localhost`, sans rien ouvrir de plus.
- **Transparence.** Les guides tiers se contredisent. Si un fond apparaît, donnez à l'overlay son propre fond (carte) dans l'éditeur, ou utilisez la méthode de secours.
- **Source vide (0 × 0).** Des éditeurs tiers ont signalé des sources qui s'affichent vides. Fixez la résolution, retirez puis rajoutez la source, ou utilisez `fit=contain`.
- **Ressources.** Chaque source Link lance son propre navigateur : limitez-vous à ce qui est utile.
- **Chat.** Les réglages du chat sont dans l'URL : après un changement dans *Overlays → Chat*, modifiez l'URL de la source Link.
- **Éligibilité.** L'accès à LIVE Studio dépend de critères de compte fixés par TikTok.

**Méthode de secours : la caméra virtuelle d'OBS.**
1. Dans OBS, réglez *Paramètres → Vidéo* sur 1080 × 1920.
2. Ajoutez les overlays Batlay comme sources « Navigateur », avec votre caméra.
3. Cliquez sur **Démarrer la caméra virtuelle**.
4. Dans LIVE Studio, ajoutez une source **Caméra**, choisissez **OBS Virtual Camera** et étirez-la sur toute la scène.

Limites de cette méthode :
- L'image est opaque : composez toute la scène dans OBS.
- Démarrez la caméra virtuelle avant LIVE Studio.
- Des guides tiers indiquent que TikTok peut verrouiller les caméras virtuelles sur les comptes récents.

**Autre option : diffuser depuis OBS avec une clé de stream TikTok**, si TikTok vous en a accordé une. Les overlays s'ajoutent alors comme dans OBS. Batlay ne fournit, ne génère et ne contourne aucune clé.

Page officielle (téléchargement ; TikTok ne publie pas de documentation de la source Link) : [TikTok LIVE Studio](https://www.tiktok.com/studio/download). Cette adresse est relevée par plusieurs sources ; elle n'a pas pu être ouverte directement depuis l'environnement de développement.

### Streamlabs Desktop
1. **Studio → Sources → +**, choisissez **Browser Source**, nommez-la, puis cliquez sur **Add Source**.
2. Collez l'URL dans le champ **URL**, saisissez **Width / Height**, puis cliquez sur **Done**.

Streamlabs Desktop est bâti sur OBS : sa source utilise le même moteur. Documentation : [Streamlabs — Browser Source](https://streamlabs.com/content-hub/post/introducing-browser-source-interaction-for-streamlabs-desktop)

### XSplit Broadcaster
1. **Add Source → Webpage**, collez l'URL, puis cliquez sur **OK**.
2. Dans les propriétés, section **Display** : réglez la zone d'affichage sur la taille indiquée, gardez la transparence et masquez les barres de défilement.
3. Laissez *Custom Code* vide. Si la page apparaît décalée, cochez **Ajuster à la source**.

Documentation : [XSplit — Webpage](https://support.xsplit.com/en/article/webpage-1y1l6v)

### Autre logiciel
Cela peut fonctionner si le logiciel :
- accepte une adresse **http** (pas seulement https) sur **127.0.0.1** ;
- exécute **JavaScript** et les **WebSocket** ;
- respecte un **fond transparent**.

Le compteur de sources connectées le confirme. Si le logiciel n'accepte qu'une caméra, passez par la caméra virtuelle d'OBS.

## Dimensions

| Format de scène | Taille |
|---|---|
| Paysage | 1920 × 1080 |
| Portrait (TikTok LIVE) | 1080 × 1920 |
| Carré | 1080 × 1080 |

- **Overlay musical.** La taille de la source est **toujours celle de son canvas** : Batlay ne modifie aucune dimension. Si l'overlay dépasse le format choisi, la page indique de combien le réduire *dans la scène* (au plus 90 % de la largeur ou de la hauteur).
- **Chat.** Batlay utilise la zone fixée dans *Overlays → Chat*. Si elle est sur « Auto », il conseille 420 × 640 (paysage), 860 × 640 (portrait, au-dessus du tiers inférieur) ou 480 × 640 (carré).
- **Copier la taille** met `largeurxhauteur` (ASCII) dans le presse-papiers.

## Options d'URL (facultatives, les anciennes URL restent valables)

| Option | Effet |
|---|---|
| Variante `127.0.0.1` | Même serveur, même port. Seule l'écriture de l'adresse change (`localhost` → `127.0.0.1`). |
| `?fit=contain` (overlay musical) | Le canvas s'adapte à la taille de la source, proportions gardées, ancré en haut à gauche. Absent = taille native, comme avant. Toute autre valeur est ignorée. |

Les réglages du chat (`size`, `max`, `w`, `h`…) sont conservés tels quels dans la variante. **Régénérer le lien** (Overlays ou Paramètres) invalide un lien dans **tous** les logiciels à la fois. L'outil *« Quel overlay est derrière cette URL ? »* indique si une URL collée correspond à un overlay existant ou à un lien révoqué.

## Sécurité et fonctionnement local

- Les serveurs n'écoutent que sur `127.0.0.1` (jamais `0.0.0.0`). Aucun port n'a été ajouté, et aucun tunnel ni service cloud n'est utilisé.
- **Nouveau** : le serveur d'overlay musical (port 3000) refuse, comme celui du chat, toute requête dont l'en-tête `Host` n'est pas `localhost:<port>` ou `127.0.0.1:<port>`. Cela protège contre le DNS-rebinding. Il refuse aussi tout WebSocket ouvert depuis une autre origine (une page Web quelconque ne peut plus recevoir l'état de lecture). Les sources Web des logiciels chargent la page depuis la même origine : elles ne sont pas concernées.
- Les pages d'overlay ne reçoivent aucun jeton Twitch ou Spotify.
- Le bouton de documentation n'envoie au cœur de Batlay qu'un **identifiant de logiciel**. L'adresse ouverte vient d'une liste fermée (`crates/batlay-core/src/integrations.rs`) : une page ne peut pas faire ouvrir une URL arbitraire.
- Le tableau de bord n'a qu'une seule commande vers le cœur, réservée à ses propres fenêtres ; les pages d'overlay n'ont accès à aucune commande.

## Diagnostic

1. **Batlay → Intégrations → Vérifications** : serveurs en ligne, nombre de sources connectées.
2. **Compteur à 0 alors que la source est ajoutée** :
   - l'URL est-elle complète ?
   - la source est-elle visible (icône œil) ?
   - essayez la variante 127.0.0.1 ;
   - vérifiez qu'aucun pare-feu ou logiciel de sécurité ne bloque les connexions locales de ce logiciel.
3. **Overlay coupé ou décalé** : fixez la taille indiquée, ou cochez *Ajuster à la source*.
4. **Fond noir ou blanc** : la source ne gère pas la transparence. Ajoutez un fond à l'overlay dans l'éditeur, ou passez par OBS (caméra virtuelle).
5. **Rien ne s'affiche après une régénération de lien** : collez l'URL dans *« Quel overlay est derrière cette URL ? »*. Un lien révoqué y est signalé.
6. **Serveur hors ligne** : le port est pris par une autre application. Changez le port musical dans Paramètres. Le port du chat est fixe (4000).
