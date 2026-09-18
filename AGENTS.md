# StreamOS v0 — Documentation technique

## Commandes de build/vérification

```bash
# Rust (backend Tauri)
cd src-tauri && cargo check          # vérification compilation
cd src-tauri && cargo clippy         # lints Rust (ERREURS réelles + style)
cd src-tauri && cargo build          # build debug

# Frontend (Svelte + Vite)
npm run check                        # svelte-check : types + identifiants inconnus (0 erreur exigé)
npm run build                        # build production
npm run tauri dev                    # dev complet (Rust + frontend)
```

> ⚠️ **BASE ZÉRO-WARNING (2026-09-14)** : `cargo clippy` (exit 0) et
> `npm run check` (0 erreur) sont le niveau de référence du dépôt. Toute
> nouvelle erreur/warning doit être corrigée AVANT de pousser — une base propre
> rend les régressions visibles immédiatement. Les warnings `non_snake_case`
> ont été neutralisés par `#[allow(non_snake_case)]` sur `CadreConfig`/`Widget`/
> `Scene` (scene.rs) : les champs camelCase y sont VOLONTAIRES (contrat JSON
> direct avec le frontend). Ne pas "corriger" ces champs sans mettre à jour
> scene.ts + les scènes JSON sur disque.

## Piège Rust — Mutex non réentrant (écran noir figé au boot, 2026-09-14)

> ⚠️ **NE JAMAIS appeler `.lock()` plusieurs fois sur le même `std::sync::Mutex`
> dans une même expression** (args d'un `eprintln!`/`format!`, conditions, …).
> Les temporaires (MutexGuard) vivent jusqu'à la fin du statement → le 2e
> `lock()` du même thread bloque à jamais → thread principal Tauri figé dans
> `setup()` : fenêtre noire, impossible à déplacer/fermer, AUCUN log `eprintln!`
> du setup dans la sortie `tauri dev`.
> Correctif : lire UNE fois (`let c = m.lock().unwrap().clone();`) puis logger.
> Correctif appliqué : `position_overlay.rs` (`new`, causait le gel au boot
> 2026-09-14) et `bandeau.rs` (`set_config`, gel dès la sauvegarde bandeau).

## Plateformes de chat — méthodes de connexion

| Plateforme | Fichier Rust       | Méthode                          | Auth             | Persistance       | Auto-resume |
|------------|--------------------|----------------------------------|------------------|-------------------|-------------|
| Twitch     | `twitch_chat.rs`   | IRC (port 6697, TLS)             | OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| YouTube    | `youtube_chat.rs`  | HTTP polling `/liveChat/messages`| OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| Kick       | `kick.rs`          | WebSocket Pusher cloud (us2)     | Aucune (anonyme) | `kick.json` (slug)| oui (slug) |
| TikTok     | `tiktok_chat.rs`   | WebSocket Webcast (PirateTok)    | Aucune (anonyme, ttwid) | `tiktok.json` (username) | oui (username) |

### Détails par plateforme

#### Twitch (`twitch_auth.rs` + `twitch_chat.rs` + `twitch_helix.rs`)
- **Auth** : OAuth2 Device Code Flow (TVs & Limited Input devices).
  Scopes : `chat:read`, `channel:read:subscriptions`, `moderator:read:followers`, `user:read:email`.
  Token stocké dans keyring (Windows native).
- **Chat** : IRC TLS vers `irc.chat.twitch.tv:6697`. PING/PONG toutes 60s.
- **Communauté** : Helix API (followers, subs, viewers, broadcaster).
- **Events emit** : `twitch:connecte`, `twitch:deconnecte`, `twitch:device`, `twitch:erreur`.
- **Auto-resume** : lit token keyring au boot → valide (refresh si 401) → IRC direct.

#### YouTube (`youtube_auth.rs` + `youtube_chat.rs` + `youtube_data.rs`)
- **Auth** : OAuth2 Device Code Flow (TVs & Limited Input devices).
  Scope : `youtube.readonly` uniquement (`channel-memberships.creator` non supporté par Device Flow).
  Token stocké dans keyring.
- **Chat** : HTTP polling `GET /liveChat/messages` (polling interval reçu dans la réponse).
  `live_chat_id` résolu via `/search?channelId=...&eventType=live` → `/videos?id=`.
- **Communauté** : Data API v3 (channel info, members, live viewers).
  ⚠️ `members` endpoint retourne 403 avec Device Flow (scope `channel-memberships.creator` requis) — géré gracieusement.
- **Events emit** : `youtube:connecte`, `youtube:deconnecte`, `youtube:device`, `youtube:erreur`.
- **Auto-resume** : lit token keyring au boot → valide → chat polling direct.

#### Kick (`kick.rs`)
- **Auth** : Aucune. Connexion anonyme au cloud Pusher public (cluster us2).
- **Chat** : WebSocket `wss://ws-us2.pusher.com/app/<key>` → subscribe `chatrooms.<id>.v2`.
  `chatroom_id` résolu via `GET kick.com/api/v2/channels/<slug>` (headers User-Agent + Referer pour Cloudflare).
- **Events emit** : `kick:connecte`, `kick:deconnecte`.
- **Persistance** : `kick.json` (slug du canal).
- **Auto-resume** : lit `kick.json` au boot → resolve slug → WS direct.

#### TikTok (`tiktok_chat.rs`)
- **Auth** : Aucune. Reverse engineering du protocole Webcast via crate `piratetok-live-rs`.
  Cookie `ttwid` acquis anonymement (GET tiktok.com). Pas de signing server, pas de x_bogus.
- **Chat** : WebSocket Webcast (protobuf-encoded events).
  `room_id` résolu depuis le username. Auto-reconnect géré par PirateTok (50 retries, backoff exponentiel).
- **Events gérés** : `Chat` (message), `RoomUserSeq` (viewer count), `Gift` (log), `Like` (ignoré), `Connected`, `Reconnecting`, `Disconnected`.
- **Events emit** : `tiktok:connecte`, `tiktok:deconnecte`, `tiktok:erreur`, `tiktok:viewers`, `chat:message`.
- **Persistance** : `tiktok.json` (username du streamer).
- **Auto-resume** : lit `tiktok.json` au boot → WS direct.
- **Dépendance** : `piratetok-live-rs` vendored dans `vendor/piratetok-live-rs/` (build.rs neutralisé — bug Windows path separators).

## Architecture chat — flux des messages

```
Plateforme (IRC/WS/HTTP) → Rust (twitch_chat/kick/tiktok_chat/youtube_chat)
  → app.emit("chat:message", ChatMessage)     → frontend Svelte (chat widget)
  → chat_tx.send(json)                        → serveur :4321 (diffusion OBS)
```

`ChatMessage` struct (dans `twitch_chat.rs`) :
```rust
struct ChatMessage {
    plateforme: String,  // "twitch" | "youtube" | "kick" | "tiktok"
    pseudo: String,
    texte: String,
    badges: Option<...>,
    avatar: Option<String>,
}
```

## Compteur viewers — widget chat en diffusion (2026-XX)

> UNE seule tâche centralisée (`viewers.rs`) pour toutes les plateformes — PAS
> de boucle de polling par plateforme. Tick toutes les 15s ; chaque plateforme
> connectée est fetchée selon son propre rythme : **60s si en live, 180s si
> hors-ligne/erreur** (back-off). Max ~3 GET/min au total.

- **Twitch** : `twitch_helix::stream_viewers(user_id, access)` → `Option<u32>`.
- **YouTube** : `youtube_data::live_viewers(access, channel_id)` → `Option<u32>`.
- **Kick** : `kick::fetch_viewers(slug)` → `livestream.viewer_count` du même
  endpoint `api/v2/channels/<slug>` que `resolve_chatroom` (GET partagé via
  `get_channel`, headers anti-Cloudflare).
- **TikTok** : AUCUN polling — `RoomUserSeq` (push WS natif) est forwardé vers
  `chat_tx` dans `tiktok_chat.rs` avec dedup locale.

**Contrat WS** (sur `chat_tx` → diffusion :4321) :
`{"type":"viewers","plateforme":"twitch","count":123|null}` — `null` =
hors-ligne/déconnecté. Envoyé SEULEMENT quand le count change (dedup) ou qu'une
plateforme se déconnecte. Emit frontend : `viewers:update` `{plateforme,count}`.

**Diffusion** (`diffusion.html`) : `.chat-viewers` = barre en tête du widget
chat (flex column : barre + `.chat-body` flex:1). `:empty` → `display:none`
(aucun count = layout identique à avant). Widget `unifie` → une pastille
(logo + count compact « 1,2k ») par plateforme live ; widget filtré → seulement
la plateforme filtrée. `viewersParPlateforme` + `fillViewersBar` +
`rerenderViewersBars` (handler WS `type==="viewers"`).

## État partagé (tauri::State)

| State         | Champs clés                          | Commandes associées |
|---------------|--------------------------------------|---------------------|
| `TwitchState` | irc_handle, cancel, connected, login | twitch_connecter, twitch_deconnecter, twitch_etat, twitch_reconnecter |
| `YoutubeState`| chat_handle, cancel, connected, login, channel_id, access | youtube_connecter, youtube_deconnecter, youtube_etat |
| `KickState`   | ws_handle, cancel, connected, slug   | kick_connecter, kick_deconnecter, kick_etat, kick_slug_courant |
| `TiktokState` | chat_handle, cancel, connected, username | tiktok_connecter, tiktok_deconnecter, tiktok_etat, tiktok_username_courant |

## Frontend — stores

| Store         | Rôle                                              |
|---------------|---------------------------------------------------|
| `chat.ts`     | Store central : `connexions`, `initChat()`, listeners events, stores d'erreur |
| `twitch.ts`   | `connecterTwitch`, `deconnecterTwitch`            |
| `youtube.ts`  | `connecterYoutube`, `deconnecterYoutube`          |
| `kick.ts`     | `connecterKick`, `deconnecterKick`, `lireSlugSauve` |
| `tiktok.ts`   | `connecterTiktok`, `deconnecterTiktok`, `lireUsernameSauve` |
| `communaute.ts`| Followers/subs/viewers (Twitch Helix + YouTube Data API). Affichage intégré dans la modale Modération & Rôles (onglet Twitch → section « Communauté » lecture seule ; onglet YouTube activé). Chargement réactif dans `App.svelte` ($effect sur `connexions.twitch`/`connexions.youtube`). |

## Persistance (config.rs)

## Persistance (config.rs)

Fichiers dans `%APPDATA%\com.streamos.v0\` (identifiant Tauri `com.streamos.v0` —
PAS "StreamOS" ; `config.rs` → `app_data_dir()`) :
- `config.json` — `{ "sceneId": "..." }` (scène courante, migration legacy)
- `obs_canvas.json` — résolution canvas OBS (globale)
- `alertes_config.json` — config des alertes par type
- `position_overlay.json` — squelette de position unifié (clip de bienvenue + alertes)
- `kick.json` — `{ "slug": "..." }`
- `tiktok.json` — `{ "username": "..." }`
- `obs_canvas.json` — résolution canvas OBS (globale)
- `followers_snapshot.json` — snapshot des followers Twitch (user_id, login, followed_at + timestamp). Sert à détecter les unfollows au démarrage suivant.
- `unfollows.json` — historique append-only des unfollows détectés (user_id, login, date_unfollow). Dédupliqué par user_id.
- `scenes/` — scènes SOS (JSON)
- `medias/` — médias importés

Twitch et YouTube utilisent keyring (coffre OS) pour les tokens OAuth2 — pas de fichier JSON.

## Unfollows Twitch — détection au démarrage (2026-09-15)

> À chaque démarrage de StreamOS, la liste des followers est comparée au
> snapshot précédent (`followers_snapshot.json`). Les user_id présents dans
> l'ancien snapshot mais absents du nouveau = unfollows. Ils sont enregistrés
> dans `unfollows.json` (append-only, dédupliqué par user_id) avec la date de
> détection (timestamp ISO 8601 du démarrage). Le nouveau snapshot écrase
> l'ancien.

### Flux

```
twitch_communaute_followers (lib.rs)
  ├─ Helix /channels/followers → liste followers (login, user_id, followed_at, display_name)
  ├─ enrichir_avatars (twitch_helix.rs) : batch /users?id=... (max 100 par requête)
  │   → remplit profile_image_url + display_name pour chaque follower
  ├─ lire_followers_snapshot (config.rs) → anciens user_id
  ├─ diff anciens vs courants → unfollows détectés
  ├─ ajouter_unfollows (config.rs) → append dans unfollows.json (dédupliqué)
  └─ sauver_followers_snapshot (config.rs) → écrase followers_snapshot.json
```

### Affichage

- **Modale Modération & Rôles → onglet Twitch → section « Communauté »** :
  followers affichés avec avatar + display_name + date de follow.
- **Modale Modération & Rôles → onglet Twitch → section « Unfollows »** :
  historique des unfollows (login + date de détection), trié plus récent en premier.
- Store `communaute.ts` : `unfollows` (writable), `chargerUnfollows()` (lecture disque via `twitch_communaute_unfollows`).
- `chargerCommunaute()` inclut `chargerUnfollows()` (Promise.allSettled).

### Limitations

- La détection ne se fait qu'au démarrage (pas en temps réel — Twitch n'a pas
  d'event IRC pour les unfollows). Si un follower unfollow puis re-follow
  entre deux démarrages, il n'apparaîtra pas dans les unfollows.
- Le login d'un unfollow vient du snapshot précédent. Si le snapshot est
  corrompu ou absent (premier démarrage), le login peut être vide (user_id
  seulement).
- `enrichir_avatars` est non-fatal : si le batch /users échoue (rate limit,
  réseau), les followers sont retournés sans avatar (profile_image_url vide).

## Import média — MKV remuxé en MP4 via sidecar ffmpeg (2026-09-14)

> Chromium (dashboard + source navigateur OBS) ne lit **pas** le MKV dans un
> `<video>` — seul MP4 (H.264) et WebM le sont. MKV et WebM partagent les magic
> bytes EBML (`1A 45 DF A3`) → `check_magic_video` accepte déjà le MKV ; seul le
> filtre d'extension bloquait `.mkv`.

### Flux

```
Dialog fichier (mp4/webm/mkv + images) → validate_and_copy_media (lib.rs)
  ├─ extension + taille (≤80 Mo) + magic bytes
  ├─ MKV → remux_to_mp4 : sidecar ffmpeg `-c copy -sn -movflags +faststart`
  │        → medias/<uuid>.mp4 (sans réencodage, rapide)
  └─ MP4/WebM → fs::copy → medias/<uuid>.<ext>
```

- `VIDEO_EXT = ["mp4", "webm", "mkv"]` (lib.rs). Le kind retourné est `"video"`.
- Le chemin stocké en scène finit en `.mp4` (même pour un MKV importé) → le
  frontend `kindFromMedia` (VIDEO_EXT `["mp4","webm"]`) retourne `"video"`
  naturellement. **Aucun changement frontend** — on ne propage pas "mkv".
- Sidecar ffmpeg embarqué : `tauri-plugin-shell` + `externalBin` dans
  `tauri.conf.json` (`binaries/ffmpeg`). Binaire à
  `src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` (target triple
  Windows x64). **Exclu de git** (`.gitignore` `src-tauri/binaries/`) — chaque
  env dev/CI doit le fournir (build statique gyan.dev ffmpeg-release-essentials).
- Permission : `shell:allow-execute` pour `binaries/ffmpeg` (capabilities).
- `remux_to_mp4` est blocking sync mais tourne sur un thread worker Tauri (pas
  le thread UI) → pas de gel du dashboard. Remux `-c copy` d'un fichier ≤80 Mo
  prend quelques secondes (I/O bound).

### Limitations

- **Codecs non-MP4** : `-c copy` échoue si un flux n'est pas compatible MP4
  (ex. HEVC vidéo, audio Opus hors profil). L'erreur ffmpeg est retournée
  telle quelle (pas de crash). Pas de fallback de transcodage (choix : remux
  seul). Un fallback `-c:v copy -c:a aac` (transcode audio uniquement) pourrait
  être ajouté ultérieurement si besoin.
- **Sous-titres** : `-sn` les dropped (compatibilité MP4/browser).
- **Binaire absent** : message clair « ffmpeg sidecar indisponible ».

## Widget caméra — source OBS "SOS-Caméra" (2026-09-12)

> ⚠️ **CONTRATS FIGÉS** (refonte 2026-09-12, remplace l'ancien système getUserMedia) :
> 1. **1 widget `type: "camera"` / scène** (singleton), source OBS fixe **"SOS-Caméra"** (`dshow_input`, constante `obs_trou::SOS_CAMERA`).
> 2. **Create même si OBS offline** → widget créé quand même ; la source est créée par `ensure_camera` au prochain `scene_sync_captures` (boot / openScene / connexion OBS).
> 3. **Delete → `SetSceneItemEnabled(false)` sur l'item, JAMAIS `RemoveInput`** — l'input OBS est conservé (conflits d'accès exclusif du capteur Windows, perte des settings OBS). Recréation = réutilisation de l'input + réactivation de l'item.
> 4. **Device change → `SetInputSettings video_device_id` seulement si `apply_device == true`** (jamais au boot — pas de restart du capteur dshow).
> 5. **Diffusion = trou + cadre SVG, dashboard = placeholder 📷, pas de getUserMedia** — le widget caméra passe par le chemin trou de `diffusion.html` (`renderScene` + `peindreFond` : `w.trou === true || w.type === "camera"`).
> 6. **Transform = sémantique `sync_trous`** : position = centre du widget, bounds w×h, `OBS_BOUNDS_SCALE_OUTER` (cover), alignment 0 → aucun saut entre create et move/resize.

### Architecture

```
Widget caméra (dashboard) ── commitScene ── obsSyncTrous (item caméra, fit "remplir")
   │ createCameraWidget / setCameraDevice / deleteWidget
   │ camera_sync / camera_hide (commandes Tauri)
   ▼
OBS scène "SOS" : [SOS-Diffusion (browser :4321)] ← au-dessus
                  [SOS-Caméra (dshow_input)]       ← juste dessous
                  [SOS-Caméra-Fond (color_source)] ← plaque noire
                  [SOS-Trou-* (captures)]          ← tout en bas
diffusion.html : widget caméra = trou dans le canvas de fond (destination-out,
forme du clip-path du cadre) → la caméra OBS apparaît à travers ; widgets DOM
chevauchant le rect caméra clippés via clipPath SVG evenodd (#sos-cam-clip).
```

### Fichiers

- `src-tauri/src/obs_trou.rs` — `SOS_CAMERA`, `ensure_camera()` (create/enable/transform/index), `camera_sync()`, `camera_hide()` ; `sync_scene_captures()` : ensure caméra (apply_device=false) + cache l'item orphelin si pas de widget caméra.
- `src-tauri/src/lib.rs` — commandes `camera_sync` (device Option + géométrie) et `camera_hide`.
- `src/lib/tauri.ts` — wrappers `cameraSync` / `cameraHide`.
- `src/lib/stores/scene.ts` — `createCameraWidget` (obsSource="SOS-Caméra" + cameraSync non-fatal), `setCameraDevice` (cameraSync avec device), `commitScene` (items incluent la caméra, fit "remplir"), `deleteWidget` (cameraHide avant remove).
- `src-tauri/resources/diffusion.html` — **PAS de getUserMedia** : le widget caméra est rendu comme un trou (élément transparent + `applyCadreWidget`), trou dans `peindreFond`.

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `getUserMedia` / `<video>` / `startCamera` / `stopCameraStream` dans diffusion.html — la caméra vient d'OBS, pas du navigateur.
2. ❌ `RemoveInput` sur "SOS-Caméra" dans le hot path delete widget — item caché seulement.
3. ❌ `SetInputSettings` au boot (sync_scene_captures) — restart du capteur dshow à chaque ouverture de scène.
4. ❌ Exclure le widget caméra du cadre SVG ou du trou `peindreFond` — il suit la règle figée des cadres.
5. ❌ Créer un second input si "SOS-Caméra" existe déjà — CreateSceneItem / SetInputSettings sur l'existant.

## Alertes Twitch — follow/raid/sub/resub/subgift/bits (2026-09-12)

> Architecture inspirée de l'ancienne app (RUST_SOS_2026) et réécrite pour la
> base v0 — même pattern que `bandeau.rs`. **Twitch d'abord ; les autres
> plateformes passeront par le même point d'entrée** (`alerte_declencher`).

### Flux

```
Twitch IRC (twitch_chat.rs)
  ├─ USERNOTICE (msg-id: sub|resub|subgift|raid, tags msg-param-*) ─┐
  └─ PRIVMSG tag bits=<n> (cheers) ─────────────────────────────────┤
                                                                    ▼
Follows : diff snapshots Helix côté frontend (stores/alertes.ts,   alertes.rs
poll 60s, baseline au 1er poll + garde followed_at > boot)  ──→ alerte_declencher
                                                                    │
                    ┌───────────────────────────────────────────────┘
                    ▼ moteur : config par type → cooldowns (viewer + global)
                              → file FIFO (max 5) → UNE alerte à la fois
                    ▼ WS chat_tx : alerte-config + alerte-play + alerte-stop
                    ▼ diffusion.html : overlay autonome z 9997 (haut centré,
                      icône emoji + texte template, son via /medias/)
```

### Règles et pièges (leçons de l'ancienne app)

1. **USERNOTICE doit être parsé explicitement** dans `twitch_chat.rs` — tombé
   dans un `_ =>` ignoré, aucun raid/sub n'est jamais émis (bug "raid sans
   alerte" en live). La détection de commande utilise `irc_command()` (pas
   `contains` — un message peut contenir le mot "USERNOTICE").
2. **Le follow n'a pas d'event IRC** → diff des snapshots Helix côté frontend.
   Double garde : baseline au 1er poll (pas d'alerte sur l'existant) +
   `followed_at > bootIso` (élimine les faux positifs de la fenêtre paginée).
3. **Un cheer EST un message** : le PRIVMSG bits continue de circuler dans le
   chat normalement, l'alerte est émise EN PLUS.
4. **Une seule alerte à la fois** : file FIFO (max 5, oldest évincé) + timer
   Rust (`alerte-stop` autoritaire) + fallback timer diffusion (+500ms).
5. **Cooldowns** : par viewer (`type:user_id`, anti-spam) + global (rythme),
   par type, contournés par le bouton Test.
6. **Sons** : importés via `import_son` (mp3/ogg/wav ≤ 10 Mo, magic bytes) dans
   `medias/`, servis par :4321 sur `/medias/`, joués côté diffusion (viewers).
7. **Extensibilité multi-plateformes** : `alerte_declencher(type, pseudo,
   user_id, nb_viewers, nb_bits, nb_mois, destinataire)` est LE point d'entrée
   — Kick/YouTube/TikTok appelleront la même commande (TikTok gifts = candidat
   naturel via PirateTok).

### Squelette de position unifié (clip de bienvenue + alertes) — 2026-09-14

> Refonte : avant, welcome.rs et alertes.rs avaient chacun leur `OverlayConfig`
> (deux fichiers JSON, deux réglages séparés dans la modale). Désormais, un
> seul réglage dans l'onglet "Squelette de position" pilote les deux overlays
> via une source de vérité Rust unifiée.

- `PositionOverlayConfig { x, y, largeur, hauteur }` dans `position_overlay.rs`,
  persistée dans `position_overlay.json` (source de vérité unique).
- `PositionOverlayState` enregistré dans `app.manage()` AVANT welcome_state et
  alertes_state (ils y accèdent via `app.state::<PositionOverlayState>()`).
- `welcome.rs` (`emit_overlay_config`, `set_overlay_config`, `QueueEtat.overlay`)
  et `alertes.rs` (`emit_config_ws`, `set_overlay_config`) lisent la position
  depuis `PositionOverlayState` — les anciens champs `overlay` dans
  `ConfigGlobale` (welcome) et `AlertesConfig` (alertes) sont gardés pour
  rétro-compat deserialization mais inutilisés.
- Poussée à la diffusion via WS `position-overlay-config` → `diffusion.html`
  applique la position au root welcome ET au root alerte en live
  (`applyWelcomeOverlayConfig` + `applyAlerteOverlayConfig`). Les handlers
  `welcome-clip-config` et `alerte-config` sont conservés (ré-émission au
  moment d'un play/test, garantit la position si diffusion reconnecte).
- Réglage dashboard : modale Interactions chat → onglet "Squelette de position"
  (mini-canvas drag/resize rectangle LIBRE — pas de ratio contraint, presets
  6 positions + champs X/Y/Largeur/Hauteur) → `position_overlay_set`.
- **Le DOM de chaque overlay reste spécifique** (carte texte alerte ≠ carte
  avatar+bio+vidéo welcome) — c'est la position/taille qui est partagée, pas
  le rendu. La hauteur est auto côté diffusion (la carte s'ajuste à son contenu).

### Médias par alerte + cadre SVG sur les overlays — 2026-09-14

> Refonte modale Alertes (master-detail) + médias (image/vidéo) + cadre SVG
> choisi par l'utilisateur sur les cartes alerte ET clip de bienvenue.

- **Modale Alertes = liste + panneau détail** (master-detail) : colonne gauche
  = 6 types (icône + nom + badge ON/OFF, clic = sélection), panneau droit =
  config du type sélectionné en 4 blocs (Média / Texte / Comportement / Test).
- **Modale Interactions chat = 5 onglets, TOUS en master-detail** (2026-09-14) :
  Clip de bienvenue (sections Général / File d'attente / Attribution), Bandeau
  premier message (Général / Durée / Position / Test & session / Apparence),
  Alertes (6 types), **Commandes** (commandes chat `!cmd`), Squelette de
  position. Mêmes classes CSS (`.alertes-layout` / `.alertes-liste` /
  `.alerte-select` / `.alerte-detail`).
- **Média par type** : `AlerteTypeConfig.media` + `media_kind` ("image" |
  "video", `Option` + `serde(default)` → rétro-compat des anciens
  `alertes_config.json`). Mode dérivé : vidéo → vidéo seule (son porté par la
  vidéo, pas d'audio séparé) + texte ; image → image + son (`son`) + texte ;
  aucun média → icône emoji + texte (comportement historique).
- **Rendu diffusion** (`playAlerte`) : texte au-dessus, média dessous
  (`.alerte-media`, width 100%). Son image via `alerteAudio` ; vidéo en
  `<video autoplay playsinline>` (son inclus, pas d'audio séparé). `stopAlerte`
  pause + reset la vidéo. Fix au passage : `cfg.duree_ms` → `cfgType.duree_ms`
  (référence indéfinie latente, masquée par court-circuit `||`).
- **Couleur du texte = couleur du cadre widget** : si `currentScene.cadreWidget`
  actif → `bpmResoudreCouleurs().p` (même résolution que le bandeau) ; sinon
  la couleur configurée du type/commande.
- **Import média alerte** : commande `import_alerte_media(kind_attendu)` —
  dialog filtré (Images/Vidéos) → `validate_and_copy_media` (helper commun
  refactoré depuis `pick_and_copy_media` : validation extension + taille +
  magic bytes + copie `medias/`) + garde-fou kind ≠ attendu. Servi par :4321
  sur `/medias/`.
- **Cadre SVG sur les overlays** : `appliquerCadreOverlay(carteEl, idCadre)` +
  `nettoyerCadreOverlay` dans diffusion.html — les cartes des overlays
  autonomes suivent `currentScene.cadreWidget` (le « Cadre des widgets » choisi
  dans la modale Cadres). Mesure de la carte APRÈS affichage (rAF au play,
  re-mesure au `canplay` vidéo — la hauteur change) + refresh au snapshot si
  l'overlay est visible (changement de cadre à chaud). Clip-path appliqué à la
  carte pour les cadres à forme (cyberpunk/story/hexagon/arrondis). Le bandeau
  premier message reste en couleurs dérivées (pas de SVG — barre pleine largeur).

### Commandes chat — "!commande" tapée par un viewer (2026-09-14)

> Nouveau module `commandes.rs` — même moteur que `alertes.rs` (file FIFO max
> 5, UNE carte à la fois, cooldowns viewer + global, timer Rust autoritaire +
> fallback diffusion) mais déclenché par un MESSAGE chat : le 1er mot (ex:
> `!hype`) est comparé aux commandes enregistrées. Le message continue de
> circuler dans le chat (comme un cheer : l'overlay est EN PLUS).

- **Config** : `CommandeConfig { id (uuid), actif, commande (sans `!`,
  normalisée lowercase), duree_ms, texte_template, couleur, taille_px,
  cooldown_viewer_s, cooldown_global_s, son, media, media_kind }` — liste
  persistée `commandes_config.json`. Variables template : `{pseudo}`
  `{commande}` `{message}` (reste de la ligne après la commande).
- **Hooks chat** : `CommandesState::on_message(pseudo, display_name, texte)`
  appelé à côté du hook bandeau dans twitch_chat.rs / kick.rs / tiktok_chat.rs
  / youtube_chat.rs (non-fatal si état absent). Le 1er mot du message (sans
  `!`, lowercase) match une commande enregistrée + active.
- **Cooldowns** : par viewer (clé `id:pseudo` — ChatMessage n'a pas de
  user_id) + global par commande, contournés par le bouton Test.
- **WS** : `commande-config` (liste complète, poussée avant chaque play) +
  `commande-play` + `commande-stop` → diffusion.html overlay `.commande-root`
  z 9996 (sous alertes 9997). Même carte que les alertes : texte au-dessus +
  média dessous + cadre SVG (`appliquerCadreOverlay(carte, "commande")`) +
  couleur du texte = couleur du cadre widget si actif.
- **Squelette de position** : partagé (position_overlay.rs) — le WS
  `position-overlay-config` positionne les 3 overlays (welcome + alerte +
  commande).
- **Commandes Tauri** : `commandes_etat`, `commande_set_config` (upsert par
  id, id généré si vide, nom normalisé trim/!-/lowercase), `commande_supprimer`,
  `commande_tester`. Frontend : `stores/commandes.ts` + onglet "Commandes"
  (master-detail : liste gauche + bouton « + Ajouter », détail = Commande /
  Média / Texte / Comportement / Test & suppression).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/position_overlay.rs` | Source de vérité unifiée : `PositionOverlayConfig` + `PositionOverlayState` (persistée `position_overlay.json`), WS `position-overlay-config` |
| `src-tauri/src/alertes.rs` | Moteur : config 6 types (persistée `alertes_config.json`), file, cooldowns, templates, overlay (lit depuis PositionOverlayState), WS |
| `src-tauri/src/commandes.rs` | Commandes chat `!cmd` : config par commande (persistée `commandes_config.json`), file + cooldowns + timer, hooks chat, WS `commande-config/-play/-stop` |
| `src-tauri/src/twitch_chat.rs` | `irc_command()` + `parse_irc_tags()` + `parse_usernotice()` + tag bits |
| `src-tauri/src/lib.rs` | `AlertesState` + `PositionOverlayState` + commandes `alertes_*` / `alerte_declencher` / `import_son` / `import_alerte_media` / `position_overlay_etat` / `position_overlay_set` |
| `src/lib/stores/positionOverlay.ts` | Store unifié : `positionOverlay`, `initPositionOverlay`, `chargerPositionOverlay`, `sauverPositionOverlay` |
| `src/lib/stores/alertes.ts` | Config store par type + diff follows (poll 60s) — overlay retiré (géré par positionOverlay) |
| `src/lib/stores/commandes.ts` | Store commandes chat : `commandesStore`, `chargerCommandes`, `sauverCommande` (upsert), `supprimerCommande`, `testerCommande` |
| `src/lib/components/InteractionViewerModal.svelte` | Modale Interactions chat — 5 onglets : Clip de bienvenue / Bandeau premier message / Alertes / Commandes / Squelette de position |
| `src-tauri/resources/diffusion.html` | Overlay welcome (z 9999) + overlay alerte (z 9997) + overlay commande (z 9996) + handler `position-overlay-config` (applique aux trois) |

**NB organisation UI** : le panneau Alertes vit dans la modale **Interactions chat**
(ex "Interaction viewer", renommée 2026-09-12) — 4 onglets : Clip de bienvenue /
Bandeau premier message / Alertes / Squelette de position. La section "Alertes"
de la sidebar Toolbar a été supprimée (tout passe par la modale). Les commandes
chat sont prévues mais non implémentées (pas d'onglet placeholder). La section
sidebar "Interactions chat" est
un **lanceur direct** (pas un accordéon) : le clic ouvre la modale — seule section
non dépliable de la sidebar.

## Input Viewer — capture globale clavier + souris (2026-09-14)

> Port du concept "Input Viewer" de RUST_SOS_2026 (E:\SteamOs - All Version\RUST_SOS_2026v0.1,
> features/input_viewer.rs + capture_clavier.rs). Widget de scène qui affiche
> les entrées en temps réel côté diffusion. **Lot 1 : clavier + souris.**
> Lot 2 (non implémenté) : manette (gilrs + XInput), pavé numérique flash.

### Flux

```
Clavier (poll GetAsyncKeyState 60Hz, thread Rust) + Souris (hook WH_MOUSE_LL)
  → thread moteur : état central (touches BTreeSet + souris) + throttle 30Hz
  → chat_tx.send({"type":"input-viewer-etat","etat":{"touches":[...],"souris":{...}}})
  → serveur :4321 → diffusion.html
  → maj DOM directe (classList.toggle('presse') sur [data-touche]) — zéro re-render
```

- **Rust** (`input_viewer.rs`) : `InputViewerState` (manage, inactif au boot).
  - Poll clavier 60Hz (256 vkCodes, fronts down/up seulement) + mapping
    `vk_vers_code_html` (vkCode → KeyboardEvent.code, table portée de
    capture_clavier.rs). Modificateurs génériques 0x10/0x11/0x12 ignorés
    (variantes L/R 0xA0-0xA5 à la place).
  - Souris : hook `WH_MOUSE_LL` installé dans un thread dédié avec sa boucle
    de messages (GetMessageW bloquant — zéro CPU au repos) ; boutons
    immédiats, position throttlée 50ms. Le hook est installé ET désinstallé
    sur le MÊME thread (leçon ancienne app) ; arrêt via
    `PostThreadMessageW(WM_QUIT)`.
  - Publication WS : throttle 30Hz + skip si signature identique (le CEF
    d'OBS saccade si bombardé).
  - **Bouton ON/OFF** dans les options du widget (Toolbar) → commandes
    `input_viewer_set_actif` / `input_viewer_etat_actif`. Zéro thread/hook
    quand OFF.
- **Widget de scène** `type: "input-viewer"` : champs `inputMode`
  ("clavier"|"souris"), `inputLayout` ("azerty"|"qwerty"), `inputCouleur`
  (couleur presse, défaut #8b5cf6). Créé via Toolbar →
  `createInputViewerWidget()` (500×220). Placeholder dashboard « Input
  viewer ». Cadre SVG automatique (applyCadreWidget).
- **Diffusion** (`diffusion.html`) : branche renderScene `input-viewer` —
  markup keycaps généré UNE FOIS (recréé si mode/layout change), maj d'état
  par `classList.toggle('presse')` sur les éléments qui changent seulement
  (union ancien/nouveau). WS `input-viewer-etat` → `majInputViewerEtat`.

### Pièges (leçons ancienne app + v0)

1. **Hook WH_MOUSE_LL** : SetWindowsHookExW exige que la boucle de messages
   tourne sur le MÊME thread que l'installation ; la désinstallation aussi.
   Thread dédié : install → GetMessageW (bloquant, zéro CPU) → unhook. Arrêt
   propre via `PostThreadMessageW(thread_id, WM_QUIT)`.
2. **Throttle souris 20Hz** sur les déplacements purs (boutons toujours
   immédiats) — sinon la diffusion OBS saccade (leçon ancienne app).
3. **Throttle WS 30Hz + skip si signature identique** — le CEF d'OBS décode
   la vidéo en logiciel : chaque message WS inutile affame le décodeur.
4. **Maj DOM directe** (classList.toggle) — jamais de innerHTML par event.
5. **Modificateurs** : poller les variantes L/R (0xA0-0xA5), jamais les
   génériques 0x10/0x11/0x12 (sinon Shift gauche/droite indiscernables).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/input_viewer.rs` | Capture : poll clavier 60Hz (table vkCode→KeyboardEvent.code portée), hook souris LL, état central, publication WS throttlée 30Hz |
| `src-tauri/src/lib.rs` | `InputViewerState` + commandes `input_viewer_set_actif` / `input_viewer_etat_actif` |
| `src-tauri/src/scene.rs` | Champs Widget `inputMode` / `inputLayout` / `inputCouleur` |
| `src/lib/stores/scene.ts` | `createInputViewerWidget()` + `setInputViewerConfig()` |
| `src/lib/components/Toolbar.svelte` | Bouton création + options (mode/layout/couleur + ON/OFF capture) |
| `src/lib/components/Widget.svelte` | Placeholder « Input viewer » (dashboard = config, pas un player) |
| `src-tauri/resources/diffusion.html` | Rendu keycaps AZERTY/QWERTY + visuel souris + maj DOM directe (WS `input-viewer-etat`) |

## Morphing (bulge/pinch) — widgets média + fond de scène (2026-09-12)

> Concept repris de MorphLens (projet externe, code NON porté) : clic sur une
> zone → agrandir/rétrécir avec rayon + intensité. Réécrit pour StreamOS en
> **morphs non destructifs** (liste de paramètres persistée, jamais baked dans
> les pixels).

### Principe

```
morphs: [{ x, y, rayon, intensite, mode }]  (normalisés 0-1, ordre = cumul)
   │ rendu : média → offscreen 2D (fit/zoom/rot existants) → shader WebGL
   │         (bulge/pinch : échantillonnage plus près/loin du centre, courbe
   │          hermite) → canvas visible
   ├─ Widget média : dashboard (Widget.svelte, one-shot vignette) + diffusion
   │   (diffusion.html, rAF pour vidéos / one-shot images)
   └─ Fond : dashboard (Canvas.svelte) + diffusion (peindreFond → GL → drawImage
       → trous destination-out inchangés)
```

### Règles

1. **ACTIVATION PARESSEUSE** : contexte GL créé SEULEMENT si le média a des
   morphs. Sans morph → rendu natif img/video (zéro coût — clips, interactions
   et le reste de l'app jamais impactés). WebGL indisponible → fallback rendu
   natif (log, pas de crash).
2. **Non destructif** : le média source n'est jamais modifié — le rendu
   réapplique la chaîne de morphs dans l'ordre. Reset = vide la liste.
3. **Coordonnées normalisées (0-1)** de la zone visible → survivent au resize,
   au changement de scène, au reload.
4. **Moteur GL en DEUX exemplaires synchronisés** : `src/lib/morph/gl-morph.ts`
   (dashboard, module TS) + moteur inliné dans `diffusion.html` (vanilla JS —
   la page est autonome sans Tauri). **Garder les deux synchronisés** (shaders
   identiques).
5. **Mode morphing** (toggle sidebar) : clic sur un widget média = bulge/pinch
   au point cliqué (pas de drag) ; clic sur le canvas vide = morph du FOND.
   Params du prochain morph : stores/morph.ts (intensité 10-200%, rayon 5-50%,
   sens agrandir/rétrécir).
6. **Persistance** : `morphs` sur Widget, `bgMorphs` sur Scene (JSON scène,
   commit + snapshot WS comme les autres champs).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | `MorphPoint` + `morphs` (Widget) + `bgMorphs` (Scene) |
| `src/lib/morph/gl-morph.ts` | Moteur GL dashboard + `dessinerFitCanvas` (object-fit canvas partagé) |
| `src/lib/stores/morph.ts` | Params UI du prochain morph (mode/sens/intensité/rayon) |
| `src/lib/stores/scene.ts` | `ajouterMorphWidget` / `resetMorphsWidget` / `ajouterMorphFond` / `resetMorphsFond` |
| `src/lib/components/Widget.svelte` | Rendu canvas morphé widget + clic-pour-morpher |
| `src/lib/components/Canvas.svelte` | Rendu canvas morphé fond + clic fond |
| `src-tauri/resources/diffusion.html` | Moteur GL inliné + rendu widgets morphés (rAF vidéos) + fond morphé dans peindreFond |
| `src/lib/components/Toolbar.svelte` | Blocs Morphing (section Widgets pour le média, section Arrière-plan pour le fond) |

## Variables CSS UI — messages & états (NE PAS CODER LES COULEURS EN DUR)

> ⚠️ **RÈGLE — à lire avant d'écrire TOUT style de message ou d'indicateur d'état.**
> Cette section existe parce que les couleurs d'état étaient fragmentées
> (`--ok`, `--connexion-ok`, `#3c3`, `#5c5`, `#c0392b`, `#e0a060` en dur dans
> 5 composants). Unification du 2026-09-12. **NE PAS RECRÉER ce qui existe.**

### Les 3 variables (source unique : `src/app.css`, bloc `:root`)

| Variable | Couleur | Sémantique | Usages |
|---|---|---|---|
| `--message-user-action-color` | `#e0a060` (orange) | Action requise de l'utilisateur | Toolbar `.obs-status.erreur` (mot de passe OBS invalide, OBS injoignable), ObsSourceDialog `.hint.warn` |
| `--message-ok-color` | `#22c55e` (vert) | Succès / état connecté | Toolbar `.obs-status.ok` + `.header.etat-ok` (en-tête section OBS) + `.dot.on`, App.svelte `.dot.on` (bandeau top), DevPanel `.check` |
| `--message-error-color` | `#c0392b` (rouge) | Erreur / danger | Toolbar `.header.etat-erreur` (en-tête section OBS non connecté : idle/error → texte + bordure rouges, couleur préservée au hover), App.svelte `.dot` (bandeau top, état off), boutons destructifs au survol (ConfirmDeleteWidgetModal `.action.danger:hover`, SceneBar `.ctx-item.danger:hover`) |

### Palette & arrondis — thème HUD sci-fi (source unique : `src/app.css`)

> Thème HUD sci-fi (2026-09-15) — remplace le thème sobre noir/blanc/gris.
> Palette noir bleuté + accents néon violet/orange + dégradé signature
> `135deg #f97316→#ec4899→#8b5cf6`. Toute la palette passe par ces variables
> pour qu'un futur thème = réécrire `app.css` uniquement.

| Variable | Valeur | Rôle |
|---|---|---|
| `--fond` / `--texte` | `#08080f` / `#e5e7eb` | Fond app (noir bleuté) + texte principal |
| `--gris` | `#8b8fa3` | Texte secondaire : placeholders, hints, outline canvas (Canvas.svelte) |
| `--gris-fonce` | `#12121e` | Fonds secondaires : hover subtil, inputs |
| `--bordure` | `#1e1e30` | Bordures UI passives (gris-violet foncé) |
| `--bordure-active` | `#2a2a40` | Bordures hover/contrôles (plus visible) |
| `--fond-panneau` | `#0d0d16` | Fond sidebar/panneaux (légèrement + clair que `--fond`) |
| `--fond-controle` | `#15151f` | Fond boutons/inputs (encore + clair) |
| `--accent-violet` | `#8b5cf6` | Accent néon principal (bordures actives, glows) |
| `--accent-orange` | `#f97316` | Accent néon secondaire (cornières, scène active) |
| `--accent-magenta` | `#ec4899` | Milieu du dégradé signature |
| `--gradient-sig` | `linear-gradient(135deg, #f97316, #ec4899, #8b5cf6)` | Dégradé signature (logo, bordures actives) |
| `--glow-violet` | `0 0 12px rgba(139,92,246,0.35)` | Glow externe section/bouton actif |
| `--glow-orange` | `0 0 8px rgba(249,115,22,0.4)` | Glow pastille scène active |
| `--rayon-petit` | `5px` | Boutons, inputs, selects (appliqué globalement dans app.css) |
| `--rayon` | `8px` | Blocs, barres, conteneurs |
| `--rayon-grand` | `14px` | Modales (`.modal` / `.dialog` des 5 modales) |

### Carte d'édition contextuelle — 3 cartes colorées (2026-09-15)

> La carte d'édition (`CarteEdition.svelte`, panneau latéral gauche) adopte une
> couleur selon la cible sélectionnée. Les boutons internes `.action` suivent
> cette couleur dès l'état repos (bordure + surface teintée via `--btn-tint` =
> `--accent-carte`), pas seulement au hover/active.

| Cible | Classe CSS | Couleur | Variable | Condition |
|---|---|---|---|---|
| Widget classique | `.cible-widget` | Rouge bordeaux | `--dash-danger` (`#7f1d1d`) | widget sélectionné (sauf input-viewer) |
| Fond de l'application | `.cible-fond` | Orange | `--dash-orange` (`#9a3412`) | fond sélectionné (clic canvas vide) |
| Widget input-viewer | `.cible-input-viewer` | Jaune | `--dash-jaune` (`#a16207`) | widget input-viewer sélectionné |

- La classe CSS est **dérivée** dans `CarteEdition.svelte` (`carteClasse`) :
  `cible === "widget" && isInputViewerWidget` → `cible-input-viewer`, sinon
  `cible-${cible}`. La cible du store reste `"widget"` (pas de nouveau type) —
  seul le rendu CSS change.
- Variables `--dash-orange` / `--dash-jaune` définies dans `app.css` (palette
  `--dash-*`, couleurs froides assombries — cohérent avec `--dash-danger`).

### Boutons — recette « verre Aero HUD » + reflet animé (2026-09-15)

> Surface interne des boutons du chrome dashboard. Brillance moitié
> supérieure teintée `--btn-tint` (signature Aero) + relief interne (inset
> lumineux haut / sombre bas = verre bombé) + **reflet animé** : un sweep
> lumineux diagonal traverse le bouton une fois au `:hover` (one-shot 0.7s,
> pas de boucle → pas énergivore). Recette partagée par toute l'UI — un
> contexte = surcharger `--btn-tint` (et `--btn-border` si besoin), jamais
> copier le CSS. Source unique : `src/app.css` (tokens + `@keyframes` +
> `button::before` global).

| Variable | Valeur | Rôle |
|---|---|---|
| `--btn-tint` | `var(--dash-accent)` (indigo) | Teinte du verre + du reflet. Surchargé par contexte |
| `--btn-border` | `var(--dash-gradient)` | Bordure border-box (dégradé dash) |
| `--btn-surface` | dégradé 180° teinté | Surface Aero (padding-box) — repos |
| `--btn-surface-hover` | dégradé 180° teinté + intense | Surface Aero — survol |
| `--btn-inset` | inset haut lumineux + bas sombre | Relief verre bombé — repos |
| `--btn-inset-hover` | inset + intense | Relief — survol |

**Teintes par contexte (surchargent `--btn-tint`) :**
- Sidebar (Toolbar) : `--dash-accent` (indigo) — défaut
- En-tête OBS erreur : `--dash-danger` (bordeaux)
- CarteEdition : `--accent-carte` (bordeaux widget / orange fond / jaune input-viewer) — posé sur `.carte-edition`
- SceneBar : `--accent-orange` (néon) — posé sur `.scene-bar`
- Modales : `--accent-violet` (néon) — posé sur `.modal-shell`
- ctrl-btn Arrêter/Redémarrer/Refresh : `--ctrl-arreter`/`--ctrl-redemarrer`/`--ctrl-refresh`

**Reflet animé** : `button::before` (global dans `app.css`) en `z-index: -1`
(au-dessus du fond, sous le texte), `isolation: isolate` sur le bouton pour
contenir le z-index négatif. `@keyframes btn-shine` = sweep `translateX` +
`skewX(-18deg)`, 0.7s ease-out, one-shot au `:hover:not(:disabled)`.

### Couleurs de marque des plateformes (décoratif, 2026-09-15)

> Exception documentée à la règle Variables CSS UI : `--plat-*` ne doit
> JAMAIS servir pour un message/état ok/erreur — uniquement décoratif
> (pastille plateforme connectée dans le bandeau + modales). Connecté =
> couleur de marque + glow ; déconnecté = gris (`--gris-fonce`). OBS et
> Diffusion gardent le vert sémantique (`--message-ok-color`).

| Variable | Valeur | Plateforme |
|---|---|---|
| `--plat-twitch` | `#a855f7` | Twitch (violet) |
| `--plat-kick` | `#22c55e` | Kick (vert) |
| `--plat-youtube` | `#f43f5e` | YouTube (rouge) |
| `--plat-tiktok` | `#ec4899` | TikTok (magenta) |
| `--plat-obs` | `#d1d5db` | OBS (gris clair) |
| `--plat-diffusion` | `#22c55e` | Diffusion (vert) |

### Couleurs contrôles & snap (centralisées, 2026-09-15)

> Source unique pour les boutons topbar (Arrêter/Redémarrer/Refresh OBS dans
> `App.svelte`), le gizmo 3D (`Gizmo3D.svelte`, `var()` dans les attributs SVG)
> et le guide magnétique (`AlignmentGuides.svelte`, lu via `getComputedStyle`
> car rendu canvas). Aliases des variables sémantiques — changer la couleur de
> base propage partout.

| Variable | Valeur | Usages |
|---|---|---|
| `--ctrl-arreter` | `var(--message-error-color)` (rouge `#c0392b`) | Bouton Arrêter ■, traits bords du guide, cadre du gizmo |
| `--ctrl-redemarrer` | `var(--accent-orange)` (orange `#f97316`) | Bouton Redémarrer ↻, axes/croix du gizmo |
| `--ctrl-refresh` | `var(--message-ok-color)` (vert `#22c55e`) | Bouton Refresh ⟳, viseur du guide, boule du gizmo |
| `--ctrl-centre` | `#ffee00` (jaune) | État centré/snap : viseur du guide proche du centre, boule du gizmo à (0,0) |

**Exclusions volontaires (WYSIWYG + règle figée cadres) :**
- `Widget.svelte` : `.widget-3d` garde `border: 1px solid var(--texte)` + outline
  de sélection blancs — le widget canvas représente ce qu'OBS affiche
  (diffusion.html `.widget` a sa propre bordure rgba blanche).
- `diffusion.html` : rendu OBS, JAMAIS touché par le style dashboard.
- `src/lib/cadres/*.ts` + `registre.ts` : palettes de rendu des cadres SVG,
  exception volontaire aux variables CSS (couleurs de données, pas de style UI).
- `AlignmentGuides.svelte` : guides snap dessinés en canvas via JS, couleurs
  lues depuis les variables `--ctrl-*` via `getComputedStyle` (cf. section
  ci-dessous) — pas de `#hex` en dur.
- Base globale `button/input/select/textarea` (radius + transition) + **accent-color
  violet néon sur tous les sliders/checkbox/radios** (`app.css` — jamais le bleu
  navigateur) — ne pas la dupliquer dans les composants.

### Règles

1. **Tout nouveau message, hint d'avertissement, indicateur d'état ou bouton
   destructif DOIT utiliser ces variables** — jamais de `#hex` dans un composant.
2. **Ajuster la teinte = modifier UNE ligne dans `app.css`** — jamais dans les composants.
3. Une nouvelle sémantique de couleur (ex. info bleu) = ajouter une variable
   `--message-<sémantique>-color` dans `app.css` + documenter dans le tableau ci-dessus.
4. Les messages d'erreur destinés à l'utilisateur doivent être **clairs et
   actionnables** (traduits depuis l'erreur technique, cf. `messageClair()` dans
   `src/lib/stores/obs.ts` qui ouvre en plus la section concernée).

### Exceptions volontaires (ne PAS migrer)

- **Palettes des cadres SVG** (`src/lib/cadres/cadre-*.ts`, `CadresModal.svelte`) :
  couleurs de rendu des cadres, pas des messages UI.
- **Guide de snap canvas** (`AlignmentGuides.svelte`) : dessiné en canvas via
  JS, couleurs `--ctrl-arreter` (traits) / `--ctrl-refresh` (viseur) /
  `--ctrl-centre` (viseur centré) lues via `getComputedStyle`.
- **`color: #fff` sur fond coloré** (texte des boutons ON/danger) : constante de
  contraste sur une couleur variable, pas une couleur d'état.
- **Valeurs de DONNÉES** (couleur par défaut des cadres/alertes stockée en
  config : `scene.ts`, `alertes.ts`) : contenu persisté, pas du style UI.

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `color: #e0a060` (ou tout hex de message/état) dans un composant Svelte.
2. ❌ Recréer une variable doublon (`--ok`, `--connexion-ok`, `--warning`, …) —
   elles ont été SUPPRIMÉES au profit des 3 variables `--message-*-color`.
3. ❌ Afficher une erreur technique brute à l'utilisateur (ex. "OBS: connexion
   fermée par le serveur") sans traduction actionnable.
4. ❌ Ajouter un état visuel sans vérifier si une variable existe déjà (grep
   `--message-` dans `app.css` AVANT d'écrire du style).
5. ❌ `border: … solid var(--texte)` sur le chrome dashboard (boutons, inputs,
   modales, sections) — utiliser `var(--bordure)`. Seule exception : les
   widgets canvas de `Widget.svelte` (WYSIWYG diffusion).
6. ❌ Arrondir les widgets canvas ou toucher `diffusion.html` pour le style —
   règle figée cadres (voir section dédiée).

## Cadres SVG — règle absolue (NE PAS REGRESSER)

> ⚠️ **RÈGLE FIGÉE — ne jamais modifier ce comportement sans accord explicite.**
> Cette section documente un fix définitif (2026-08-31) après plusieurs
> régressions. Tout changement ici casse l'affichage côté diffusion/OBS.

### Principe

Les cadres SVG (`scene.cadreWidget`) s'appliquent à **TOUS les widgets**, sans
exception : médias, chat, welcome-clip, et **widgets troués**. Aucun type de
widget ne doit être exclu du cadre.

### Comportement attendu

| Type de widget | Cadre SVG dashboard | Cadre SVG diffusion/OBS | Trou canvas de fond |
|----------------|---------------------|-------------------------|---------------------|
| média          | oui (clip-path + overlay SVG) | oui | n/a |
| chat           | oui (clip-path + overlay SVG) | oui | n/a |
| welcome-clip   | oui (clip-path + overlay SVG) | n/a (overlay autonome) | n/a |
| **trou**       | **oui** (clip-path + overlay SVG) | **oui** | **forme du clip-path du cadre** |

### Fichiers concernés

#### `src/lib/components/Widget.svelte` (dashboard)
- `cadreActif` = `!!cadre?.actif && (cadre?.strokeWidth ?? 0) > 0` — **PAS de `!isTrou`**.
  L'exclusion des trous était une ancienne décision de design ("un trou n'a pas
  de bordure") qui a été **supprimée**. Ne jamais la réintroduire.
- Le bloc `{#if cadreActif && cadre}` s'applique à tous les types (chat, media, trou).
- `trou-overlay` (bordure pointillée) est masqué quand un cadre est actif
  (`{#if isTrou && !cadreActif}`) pour éviter la superposition pointillé + cadre.

#### `src-tauri/resources/diffusion.html` (diffusion OBS :4321)
- **CSS** : `.widget.trou` = `background: transparent; border: none;` — le widget
  trou est invisible (le trou est dans le canvas de fond), seul le cadre SVG est
  visible au-dessus.
- **`renderScene()`** : les widgets troués ne sont **PAS skipés**. Un élément DOM
  `div.widget.trou` est créé (transparent) pour recevoir `applyCadreWidget()`
  (clip-path + overlay SVG). Ne jamais réintroduire un `continue` sans DOM pour
  les trous.
- **`renderScene()` — chat** : `applyCadreWidget()` est appelé sur le widget chat
  (UPDATE + CREATE). L'entry chat a les champs `cadreOverlayEl` et `cadreKey`.
  Ne jamais les retirer.
- **`peindreFond()` — trous** : quand un cadre avec clip-path est actif, le trou
  dans le canvas de fond suit la **forme du clip-path** (coins biseautés cyberpunk,
  hexagone, story, arrondis) et non un rectangle. Utilise `svgPathToPoints()` +
  `getCadreClipPathPoints()` (cache) pour parser le path SVG en points, puis
  `projectCorner()` pour la projection 3D. Sans cadre actif → rectangle simple
  (fast path `fillRect`).

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `if (w.trou === true) { seen.add(w.id); continue; }` dans `renderScene()` —
   les trous doivent avoir un élément DOM pour recevoir le cadre.
2. ❌ `&& !isTrou` dans `cadreActif` (Widget.svelte) — les trous ont le cadre.
3. ❌ Omettre `applyCadreWidget()` sur le chemin chat dans `renderScene()`.
4. ❌ Dessiner le trou comme un rectangle quand un cadre avec clip-path est actif —
   les coins débordent du cadre côté OBS.
5. ❌ Retirer les champs `cadreOverlayEl`/`cadreKey` de l'entry chat dans
   `widgetMap`.
6. ❌ Dessiner des éléments décoratifs **à l'intérieur** de la zone du widget
   (crosshair, radar, grille, scan lines, data ticks, cadran, etc.) — un cadre
   **borde**, jamais n'obscurcit le contenu (média, chat, caméra). Tous les
   éléments décoratifs doivent être dans la **marge extérieure** (coordonnées
   négatives ou > largeur/hauteur). Leçon MGStyle (2026-09-15) : un crosshair
   au centre du widget = cible par-dessus la vidéo → contresens fonctionnel.

## Speedrun Splitter — Auto Splitter natif Rust (2026-09-14)

> Port du Speedrun Splitter de `RUST_SOS_2026v0.1` (E:\SteamOs - All Version).
> Moteur ASL (Auto Split Language) 100% Rust : parser ASL + transpileur C#→JS
> + moteur Boa (JS pur Rust) + scan mémoire Win32 + parser LSS + timer.
> Plus léger que LiveSplit (pur Rust + Boa ~5MB vs .NET WPF + Roslyn ~50MB).

### Architecture

```
Dashboard (Svelte + TS)                Backend Rust                    Diffusion (CEF OBS)
┌──────────────────────┐  invoke  ┌─────────────────────┐  chat_tx   ┌──────────────────┐
│ WidgetSpeedrun.ts    │ ───────→ │ speedrun/commands.rs │ ─────────→ │ diffusion.html   │
│  - timer + contrôles │          │  (9 commandes Tauri) │  WS :4321  │  - rendu timer   │
│  - charge .asl/.lss  │ ←─────── │ speedrun/asl_engine  │ ←───────── │  - splits LSS    │
│  - settings toggles  │  event   │ speedrun/timer       │  event     │  - maj DOM WS    │
│  speedrun-store.ts   │  Tauri   │ speedrun/transpiler  │  Tauri     │  (zéro re-render) │
│                      │          │ speedrun/script_bridge│            │                  │
│                      │          │ speedrun/pont_memoire│            │                  │
│                      │          │ speedrun/memory/     │            │                  │
│                      │          │ speedrun/asl/        │            │                  │
└──────────────────────┘          └─────────────────────┘            └──────────────────┘
```

### Flux

```
Dashboard (WidgetSpeedrun.svelte)
  ├─ Charger .asl → speedrun_charger_asl(path) → parser ASL → settings list
  ├─ Charger .lss → speedrun_charger_lss(path) → parser LSS → segments + PB
  ├─ Démarrer → speedrun_demarrer(totalSplits, start, split, reset)
  │   └─ Thread moteur 15Hz : connexion processus → init/update/start/split/reset
  │      └─ emit "speedrun:event" (Tauri → store dashboard)
  │      └─ chat_tx.send (WS :4321 → diffusion.html majSpeedrunEtat)
  └─ Contrôles manuels → speedrun_action_manuelle("start"|"split"|"skip"|"undo"|"reset"|"pause")
```

### Dépendances

- `boa_engine = "0.20"` — moteur JS pur Rust (remplace Roslyn C#). ~5MB.
- `windows` 0.58 features Win32 supplémentaires : `Win32_System_Diagnostics_Debug`
  (ReadProcessMemory), `Win32_System_Diagnostics_ToolHelp` (CreateToolhelp32Snapshot),
  `Win32_System_ProcessStatus` (EnumProcessModulesEx, GetModuleBaseNameW).

### Commandes Tauri (9)

- `speedrun_charger_asl(path)` → charge .asl, retourne settings
- `speedrun_charger_lss(path)` → charge .lss, retourne segments + PB
- `speedrun_demarrer(totalSplits, start, split, reset)` → démarre boucle polling
- `speedrun_arreter()` → arrête boucle
- `speedrun_maj_settings(start, split, reset)` → met à jour settings
- `speedrun_est_actif()` → true si actif
- `speedrun_action_manuelle(action)` → start/split/skip/undo/reset/pause
- `speedrun_lire_config()` → lit speedrun.json (chemins + settings)
- `speedrun_sauver_config(config)` → sauve speedrun.json

### Events

- Tauri `speedrun:event` → store Svelte dashboard (types: started, split, reset,
  ended, gameTime, time, connected, disconnected, error, stopped, loaded, paused,
  resumed, splitSkipped, splitUndone, settings-list).
- WS `speedrun-etat` → diffusion.html `majSpeedrunEtat` (maj DOM directe,
  pattern input-viewer : textContent/classList, zéro re-render).

### Bugs corrigés (portés depuis le legacy, déjà fixés dans le code source)

1. **vars/settings persistants** — `globalThis.__asl_vars` / `__asl_settings` en
   mémoire Boa (sinon fonctions/instances détruites par JSON round-trip).
2. **strip .exe** — Process::find_by_name matche "mgsi" (ASL) contre "mgsi.exe".
3. **new_state_var prepend 0** — LiveSplit InitializeOffsets : deref base en premier.
4. **module vide → module principal** — pas process_name (RCA-1).
5. **gameTime Option** — fallback realTime (RTA) si null.
6. **init guard** — ne pas appeler start/split/reset si init a échoué.
7. **transpiler char-safe** — char_indices() pas bytes[i] (panics multi-byte).
8. **Mutex non réentrant** (AGENTS.md) — singleton AslEngine : lire UNE fois.
9. **Boa Context non Send** — ScriptContext vit dans un seul thread (mpsc channels).
10. **Throttle WS** — skip si signature identique (CEF OBS saccade sinon).
11. **Maj DOM directe** diffusion.html — jamais innerHTML par event.

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/speedrun/mod.rs` | Déclaration modules + cfg(windows) + stubs non-Windows |
| `src-tauri/src/speedrun/commands.rs` | 9 commandes Tauri + singleton AslEngine + emit (Tauri + WS) |
| `src-tauri/src/speedrun/asl/parser.rs` | Parser .asl manuel (states + 13 méthodes) |
| `src-tauri/src/speedrun/asl/lss.rs` | Parser .lss XML (segments + PB) |
| `src-tauri/src/speedrun/engine/timer.rs` | TimerModel (start/split/reset/pause/undo/skip) |
| `src-tauri/src/speedrun/engine/asl_engine.rs` | Boucle polling 15Hz + emit events |
| `src-tauri/src/speedrun/engine/transpiler.rs` | Transpileur C#→JS (29 étapes) |
| `src-tauri/src/speedrun/engine/script_bridge.rs` | ScriptContext Boa + API compat C# |
| `src-tauri/src/speedrun/engine/pont_memoire.rs` | Pont JS↔Rust thread_local |
| `src-tauri/src/speedrun/memory/` | Scan Win32 (process, deep_pointer, memory_watcher, signature_scanner) |
| `src-tauri/src/config.rs` | Persistance speedrun.json (chemins + settings) |
| `src-tauri/src/scene.rs` | Type Widget "speedrun" + champs speedrunCheminAsl/Lss/Settings |
| `src/lib/stores/speedrun.ts` | Store TS (écoute event Tauri + actions invoke) |
| `src/lib/components/WidgetSpeedrun.svelte` | Composant dashboard (timer + contrôles + LSS) |
| `src-tauri/resources/diffusion.html` | Rendu diffusion (branche speedrun + WS speedrun-etat + majSpeedrunEtat) |

### Plateforme

- **Windows** : scan mémoire Win32 (ReadProcessMemory) + Boa + timer manuel + LSS.
- **Non-Windows** : stubs renvoient erreur explicite (pas de crash). Timer manuel
  + LSS fonctionnent partout (pas de scan mémoire).

## Média de fond sur les widgets à contenu (chat / input-viewer / speedrun) — 2026-09-15

> Les widgets `chat`, `input-viewer` et `speedrun` acceptent désormais un média
> (image/vidéo) en **arrière-plan**, avec le contenu (messages chat, keycaps,
> timer+splits) affiché **par-dessus** — comme un widget média, avec les mêmes
> contrôles (Affichage/fit, zoom, rotation, offsets, PlayerBar vidéo).
> **Zéro changement Rust** : les champs `media`/`kind`/`mediaFit`/`mediaZoom`/
> `mediaRot`/`mediaOffsetX/Y`/`mediaPaused`/`mediaTime` existent déjà sur TOUS
> les widgets (`scene.rs`), et `import_media` (lib.rs) accepte n'importe quel
> widget.

### Contrat de rendu (règle figée)

1. **Le média = PREMIER enfant** du `.widget` (diffusion) / `.widget-3d`
   (dashboard), `class="media-fond"`, `position:absolute; inset:0; z-index:0`.
2. **Le contenu au-dessus** : `.chat-body` (diffusion) / `.chat-actif`
   (dashboard), `.widget.inputviewer > *:not(.media-fond):not(.cadre-overlay)`,
   `.widget.speedrun > *:not(.media-fond):not(.cadre-overlay)` (diffusion),
   `.contenu-dessus` (wrapper WidgetSpeedrun dashboard), badge
   `.input-viewer-obs` (z 2) — tous `position:relative; z-index:1`.
   JAMAIS de z-index négatif sur le média (passerait derrière le fond du widget).
   ⚠️ **`:not(.cadre-overlay)` OBLIGATOIRE** dans les sélecteurs `> *` :
   l'overlay du cadre SVG est un ENFANT DIRECT du widget (`applyCadreWidget`
   fait `el.appendChild`). Sans l'exclusion, le sélecteur `> *` (spécificité
   0,3,0) écrase `.cadre-overlay` (0,1,0 : `position:absolute; z-index:5`)
   → le cadre SVG disparaissait sur input-viewer et speedrun (régression
   2026-09-15).
3. **Le cadre SVG reste au-dessus de tout** (z-index 5) — règle cadres
   inchangée, le clip-path découpe aussi le média de fond.
4. **Fit "ajuster" (défaut)** : le fond glass sombre du widget reste visible
   dans les bandes ; "remplir" couvre tout. Les messages chat gardent leur
   glass `backdrop-filter` (floute le média derrière eux).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/resources/diffusion.html` | `syncMediaFond(entry, w)` : (re)crée le média de fond si `mediaKey` change ou l'entry n'en a plus, `insertBefore(firstChild)`, applique `applyMediaStyle` + `applyMediaState` à chaque snapshot ; appelé dans UPDATE+CREATE des branches chat / input-viewer / speedrun |
| `src/lib/stores/scene.ts` | `clearWidgetMedia(id)` : retire `media`/`kind` + commit (pattern `clearFond` — le fichier dans medias/ n'est PAS supprimé) |
| `src/lib/components/Widget.svelte` | Snippet `{#snippet mediaFond()}` rendu dans les branches isChat/isInputViewer/isSpeedrun (img figée ou vidéo vignette via `videoEl` — les $effect src/registerVideo/mediaTime existants pilotent l'élément) |
| `src/lib/components/Toolbar.svelte` | Snippet `{#snippet controlesMedia()}` (Affichage + zoom/rot/offsets + Reset + Retirer le média + PlayerBar) rendu dans les branches chat / input-viewer / média (le speedrun passe par la branche média) |

### Pièges

1. **`applyMediaState` garde par tagName** (`entry.mediaEl.tagName !== "VIDEO"`),
   PAS par `entry.kind` — les entries chat/input-viewer/speedrun ont un kind
   propre ("chat", …) mais portent une vidéo de fond dans `entry.mediaEl`.
2. **Input-viewer UPDATE + `innerHTML`** : quand mode/layout/skin change,
   `ivEntry.el.innerHTML = ivGenererHTML(w)` DÉTRUIT le média de fond →
   resetter `ivEntry.mediaEl = null; ivEntry.mediaKey = ""` juste après
   (comme pour l'overlay cadre SVG), puis `syncMediaFond` recrée.
3. **Le bouton Morphing est limité aux widgets type "media"** (`isMediaWidget`
   dans Toolbar) — les morphs ne sont PAS rendus sur les widgets à contenu
   (les branches chat/input-viewer/speedrun de `renderScene` précèdent la
   branche morph).
4. **Dashboard = config, pas un player** (règle d'or) : le média de fond y est
   une vignette figée (vidéo : `preload="metadata"`, pause, frame `mediaTime`)
   ; la lecture réelle est pilotée par la PlayerBar via snapshot WS (:4321).
5. **Pas de composant séparé pour les contrôles média** : Svelte 5 snippet
   (`controlesMedia`) dans Toolbar — les classes CSS (`.fit-group`, `.fit-btn`,
   `.media-ctrl`, …) sont scopées à Toolbar.svelte et ne s'appliqueraient pas
   à un composant enfant sans duplication.
