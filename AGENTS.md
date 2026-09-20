# StreamOS v0 — Documentation technique

## Protocole d'exécution — périmètre et validation avant écriture

> Règles de méthode pour tout agent travaillant sur ce dépôt. Elles priment
> sur la tâche en cours, quelle qu'elle soit.

1. **Diagnostic avant écriture, systématiquement** — même sur une tâche qui
   semble simple. Vérifier le code/la doc réels (read, grep, git log) et
   afficher le diagnostic AVANT toute modification.
2. **Un fichier, un diff, une pause** — jamais plusieurs fichiers modifiés
   d'affilée sans montrer le diff de chacun et attendre validation.
3. **Le périmètre annoncé est une liste blanche fermée** — tout ce qui n'y
   figure pas est hors de portée. Un besoin hors périmètre se signale, il ne
   se corrige pas de soi-même.
4. **Les sections figées de ce fichier priment toujours** — « RÈGLE FIGÉE » /
   « NE JAMAIS RÉINTRODUIRE » / « CONTRAT FIGÉ » / « RÈGLE ABSOLUE »
   l'emportent sur toute instruction de la tâche en cours.
5. **Le doute s'exprime, il ne se résout pas seul** — ambiguïté ou
   contradiction → poser la question, ne pas trancher seul.

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
- `raccourcis.json` — `{ "visible": bool, "bord": "gauche|droite|haut|bas", "monitor_index": n }` (barre de raccourcis)
- `telemetrie/events.jsonl` — journal append-only des événements télémétrie (schéma v1, voir section dédiée)
- `speedrun_baselines.json` — référence « dernière fois » du splitter : temps de split **cumulés** (secondes) par segment, keyed par chemin .lss. Mis à jour à chaque split réalisé ; mergé avec les PB du .lss pour la colonne delta (voir section Speedrun).
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

### Les 3 variables (source unique : `src/theme/glace.css` → tokens `--t-*`)

| Variable | Couleur | Sémantique | Usages |
|---|---|---|---|
| `--message-user-action-color` | `#f59e0b` (orange) | Action requise de l'utilisateur | Toolbar `.obs-status.erreur` (mot de passe OBS invalide, OBS injoignable), ObsSourceDialog `.hint.warn` |
| `--message-ok-color` | `#22c55e` (vert) | Succès / état connecté | Toolbar `.obs-status.ok` + `.header.etat-ok` (en-tête section OBS connecté : texte vert), `.dot.on` partagée (bandeau top App.svelte + lignes Connexions), DevPanel `.check` |
| `--message-error-color` | `#c0392b` (rouge) | Erreur / danger | Toolbar `.header.etat-erreur` (en-tête section OBS non connecté : texte + liseré gauche rouges), boutons destructifs au survol (ConfirmDeleteWidgetModal `.action.danger:hover`, SceneBar `.ctx-item.danger:hover`), `.ctrl-btn.ctrl-arreter:hover` (topbar) |

### Système de thème `src/theme/` — 3 couches (2026-09-20, spec IDEE/IDEA02.md)

> Le thème du chrome dashboard vit dans `src/theme/`, importé par `main.ts`
> dans cet ordre : `tokens.css` → `roles.css` → `glace.css` → `app.css`.
> **UN SEUL THÈME (décision 2026-09-20) : `glace` est la base définitive —
> la « variante » est la couleur utilisateur `--accent-ui` (couleur du
> cadre SVG), qui re-thème tout le chrome à chaud. Pas de sélecteur de
> thème. Si un jour un 2e thème est nécessaire : nouveau fichier
> `theme-*.css` + changer l'import dans `main.ts`, zéro touch aux
> composants.**

- **`tokens.css`** : primitives brutes `--t-*` (hex + composites : dégradés,
  glows). Seule source de `#hex` — un composant ne contient JAMAIS de hex.
- **`roles.css`** : le contrat — recettes dérivées `--btn-surface`,
  `--btn-surface-hover`, `--btn-surface-active` + reliefs `--btn-inset`,
  `--btn-inset-hover`, `--btn-inset-active` + arrondis `--rayon-*`.
  Calculées en fonction des rôles assignés par le thème ; un thème
  n'assigne QUE des couleurs.
- **`glace.css`** : LE THÈME — assigne chaque rôle (`--fond`, `--btn-tint`,
  `--accent-*`, `--dash-*`, `--message-*`…) à un token.
- **`app.css`** : structure uniquement (reset, shine/gloss `button::before`/
  `::after`, scrollbars, `.modal-*`, `.msg-pill`, couche décor `.fond-decor`
  + classes partagées `.ico` et `.dot`). Les composants consomment les
  rôles — JAMAIS un `--t-*` directement.

**Règles du thème « glace » :**
- **Pas de fill de bouton coloré au repos** : `--btn-tint: transparent`,
  `--btn-base` = verre sombre translucide (`--fond-controle` à 80%). La
  couleur vit dans les bordures, glows, textes et états actifs.
- **Exceptions sémantiques** (reposent leur `--btn-tint` coloré) :
  `.action.danger`, `.modal-action.danger`, `.speedrun-start`/`-stop` ;
  `.ctrl-arreter`/`-redemarrer`/`-refresh` (topbar) le reposent **au
  survol uniquement** — icônes neutres au repos, couleur `--ctrl-*` au
  hover (icône + teinte + bordure + glow).
- **Bordures en dégradé** : technique `background: <gradient> border-box
  border-area` — `border-area` clippe la couche à l'anneau de bordure
  (requis depuis que les fills sont translucides ; sans ça le dégradé
  transparaît sous tout le bouton). Chrome/WebView2 ≥ 128.
- **Fond d'ambiance** : `--fond-ambiance` (aurora en radial-gradients) est
  le fond de `.fond-decor` — PAS sur `body` (un fond body, bloc in-flow,
  se peindrait par-dessus le calque z-index -1 et le masquerait). `body`
  n'a aucun fond ; le fond plat `--fond` reste sur `:root`. `.fond-decor`
  (`position: fixed; z-index: -1; pointer-events: none`, définie dans
  `app.css`, montée dans `App.svelte`)
  contenant, dans l'ordre de peinture : `.fond-image` (image utilisateur
  pleine fenêtre + voile assombrissant/lavé `--cadre-couleur`), les orbs
  (arcs orbitaux — cercles géants hors-champ), `.ligne` (diagonale fine) et
  les `.boule` (points variés 3–17px remontant le bord gauche). Tout est
  SOUS le contenu : rien ne masque le texte. Les panneaux fixes (header,
  `.sidebar-col`, `.scene-bar`, `.drop-bar`) sont `transparent`.
  `--fond-panneau` reste pour les panneaux flottants (modales, dropdowns,
  carte d'édition).
- **Accent utilisateur (couleur du cadre)** : `App.svelte` pousse la
  couleur EFFECTIVE du cadre actif (`cadreApp`, fallback `cadreWidget`)
  dans `--cadre-couleur` / `--cadre-couleur-fin` sur `:root`, résolue via
  `gradientCadre()` de `lib/cadres/registre` — le même resolver que le
  rendu : cadre personnalisable → `couleur`/`couleurFin` ; preset →
  palette de la `variante` (le champ `couleur` n'y est PAS la vérité).
  `--accent-ui` = `var(--cadre-couleur, var(--accent-violet))` + dérivés
  `--glow-ui`, `--bordure-ui`, `--bordure-ui-active`, `--gradient-cadre`
  (glace.css).
  Tout le chrome les consomme (bordures de boutons, hovers, focus,
  anneaux, modales). `--bordure`/`--bordure-active` RESTENT neutres —
  consommés aussi par le canvas (outline de sélection widget), hors
  périmètre. Les signaux sémantiques (`--accent-carte`, `--dash-*`,
  `--ctrl-*`, `--plat-*`, `--message-*`) ne suivent PAS `--accent-ui`.
  `--accent-violet` reste la marque (logo, `--gradient-sig`).
- **Image de fond dashboard** : `stores/fondSidebar.ts` (localStorage
  `sidebarFond` + `tauri.importAlerteMedia("image")` — pipeline média
  commun, zéro Rust ajouté) ; boutons dans Personnalisation.
- **Sidebar** = liste plate : icône SVG `.ico` + label + chevron `›` ;
  lignes **transparentes au repos**, pilule verre au survol ; section
  ouverte = pilule neutre discrète `color-mix(--texte 7%)` ; chaque
  `.section` porte un encadré `--bordure-ui` (accent cadre), renforcé
  `--bordure-ui-active` quand ouverte (le contenu déplié reste dans le
  cadre) ; section OBS non connectée = texte `--message-error-color` +
  liseré rouge `inset 3px 0 0` à gauche.
- **Accent primaire** : `--accent-rose` (`#d4888f`) — bouton « Vos scènes »
  (pilule pleine : dégradé vertical léger + liseré sombre, glow au survol).
  Quand le cadre SVG de l'app est actif, la pilule suit `--cadre-couleur`
  (via `var(--cadre-couleur, var(--accent-rose))`, comme le décor).

| Variable | Valeur | Rôle |
|---|---|---|
| `--fond` / `--texte` | `#08080f` / `#e5e7eb` | Fond app (noir bleuté) + texte principal |
| `--gris` | `#8b8fa3` | Texte secondaire : placeholders, hints, outline canvas (Canvas.svelte) |
| `--gris-fonce` | `#12121e` | Fonds secondaires : hover subtil, inputs |
| `--bordure` | `#1e1e30` | Bordures NEUTRES — canvas (outline sélection widget) + base des variantes teintées |
| `--bordure-active` | `#2a2a40` | Bordure neutre renforcée — idem, canvas uniquement |
| `--bordure-ui` / `--bordure-ui-active` | `color-mix(--accent-ui 35%/65%, --bordure*)` | Bordures du CHROME teintées couleur cadre (boutons, sections, séparateurs) |
| `--fond-panneau` | `#0d0d16` | Fond des panneaux FLOTTANTS (modales, dropdowns, carte d'édition) — les panneaux fixes sont transparents |
| `--fond-controle` | `#15151f` | Substrat du verre boutons (`--btn-base`) + fonds inputs |
| `--fond-ambiance` | composite radial-gradients | Fond aurora — fond de `.fond-decor` (PAS sur `body`) |
| `--accent-ui` | `var(--cadre-couleur, --accent-violet)` | Accent du chrome entier : bordures, focus, glows, anneaux — suit la couleur du cadre |
| `--glow-ui` | `0 0 8px color-mix(--accent-ui 40%)` | Glow externe teinté cadre (hovers, dropdowns) |
| `--gradient-cadre` | `linear-gradient(135deg, --cadre-couleur, --cadre-couleur-fin)` | Anneaux de boutons SceneBar — dégradé exact du cadre |
| `--accent-violet` | `#8b5cf6` | Marque (logo) + fallback de `--accent-ui` sans cadre |
| `--accent-orange` | `#f97316` | Accent néon secondaire (scène active, glows SceneBar) + fallback décor |
| `--accent-magenta` | `#ec4899` | Milieu du dégradé signature |
| `--accent-rose` | `#d4888f` | Fallback de « Vos scènes » quand aucun cadre n'est actif |
| `--gradient-sig` | `linear-gradient(135deg, #f97316, #ec4899, #8b5cf6)` | Dégradé signature — marque uniquement (logo, titre) |
| `--glow-orange` | `0 0 8px rgba(249,115,22,0.4)` | Glow pastille scène active |
| `--rayon-petit` | `5px` | Boutons, inputs, selects (appliqué globalement dans app.css) |
| `--rayon` | `8px` | Blocs, barres, conteneurs |
| `--rayon-grand` | `14px` | Modales (`.modal` / `.dialog` des 5 modales) |

### Carte d'édition contextuelle — 3 cartes colorées (2026-09-15)

> La carte d'édition (`CarteEdition.svelte`, panneau latéral gauche) adopte une
> couleur selon la cible sélectionnée. La couleur vit dans la bordure des
> boutons `.action` (`--btn-border` = `--accent-carte` plein) + le glow
> hover/active — le fill reste le verre sombre neutre commun (IDEA02).

| Cible | Classe CSS | Couleur | Variable | Condition |
|---|---|---|---|---|
| Widget classique | `.cible-widget` | Rouge bordeaux | `--dash-danger` (`#7f1d1d`) | widget sélectionné (sauf input-viewer) |
| Fond de l'application | `.cible-fond` | Orange | `--dash-orange` (`#9a3412`) | fond sélectionné (clic canvas vide) |
| Widget input-viewer | `.cible-input-viewer` | Jaune | `--dash-jaune` (`#a16207`) | widget input-viewer sélectionné |

- La classe CSS est **dérivée** dans `CarteEdition.svelte` (`carteClasse`) :
  `cible === "widget" && isInputViewerWidget` → `cible-input-viewer`, sinon
  `cible-${cible}`. La cible du store reste `"widget"` (pas de nouveau type) —
  seul le rendu CSS change.
- Variables `--dash-orange` / `--dash-jaune` définies dans `theme/glace.css`
  (palette `--dash-*`, couleurs froides assombries — cohérent avec
  `--dash-danger`).

### Boutons — recette « verre sombre » + reflet animé (thème glace, 2026-09-20)

> Recette partagée par toute l'UI — un contexte = surcharger les rôles
> (`--btn-tint`, `--btn-border` local), jamais copier le CSS.
> Recettes dérivées : `src/theme/roles.css` ; valeurs : `src/theme/glace.css` ;
> keyframes + `button::before` global : `src/app.css`.

| Variable | Valeur (glace) | Rôle |
|---|---|---|
| `--btn-tint` | `transparent` | Teinte du verre + du reflet. `transparent` par défaut — PAS de fill coloré au repos ; surchargé UNIQUEMENT par les états sémantiques |
| `--btn-base` | `--fond-controle` à 80% | Substrat translucide du verre (le fond ambiant transparaît) |
| `--btn-border` | pas de défaut global | Anneau de bordure — assigné localement par contexte (porte la couleur d'identité) |
| `--btn-surface` | dégradé 180° tint→base | Surface verre — repos |
| `--btn-surface-hover` | dégradé + intense | Surface — survol |
| `--btn-surface-active` | dégradé inversé (substrat plein en haut) | Surface — pression (`:active`) |
| `--btn-inset` / `--btn-inset-hover` | inset haut lumineux + bas sombre + halo `color-mix(--accent-ui 16%/30%)` | Relief verre bombé + lueur cadre — repos / survol |
| `--btn-inset-active` | `inset 0 2px 4px rgba(0,0,0,.4)` | Relief creusé — pression (`:active`) |

**Qui surcharge `--btn-tint` (sémantique, exhaustif) :**
`.speedrun-start`/`.speedrun-stop` (CarteEdition), `.action.danger`
(DropBar), `.modal-action.danger:hover` (app.css) ; `.ctrl-arreter`/
`.ctrl-redemarrer`/`.ctrl-refresh` (topbar) **au `:hover` seulement**. Tout
autre usage = bug à corriger.

**Couleur de contexte sans fill** : `--btn-border` porte l'identité
(`--accent-carte` sur CarteEdition `.action`, `--dash-danger` sur DropBar
danger, `--gradient-cadre` sur les anneaux SceneBar). Technique anneau :
`<gradient> border-box border-area` (clip à la bordure — voir règle
`border-area` ci-dessus).

**Verre dépoli** : `button { backdrop-filter: blur(6px) }` (global dans
`app.css`) — le décor (aurora, image, boules) transparaît flouté derrière
chaque bouton. La lueur cadre au repos vit dans `--btn-inset` (halo
`--accent-ui`, cf. table ci-dessus).

**Reflet animé** : `button::before` (global dans `app.css`) en `z-index: -1`
(au-dessus du fond, sous le texte), `isolation: isolate` sur le bouton pour
contenir le z-index négatif. `@keyframes btn-shine` = sweep `translateX` +
`skewX(-18deg)`, 0.7s ease-out, one-shot au `:hover:not(:disabled)`.
Invisible par défaut (teinté `--btn-tint` = transparent) ; réapparaît sur
les boutons sémantiques qui reposent un `--btn-tint` coloré.

### Pastille de message centré — recette `.msg-pill` (2026-09-19)

> Badge centré sur un widget ou le fond du canvas (lecture OBS, input
> viewer). Source unique : `src/app.css`. **Ne jamais re-copier le bloc**
> dans un composant — la triple duplication (`.lecture-obs` /
> `.input-viewer-obs` / `.lecture-obs-fond`) a été unifiée.

- `.msg-pill` : conteneur (centrage `translate(-50%,-50%)`, fond `--fond` 82%, `--rayon-petit`, `font-size: clamp(0.5rem, 9cqw, 1.25rem)`).
- `.msg-pill-text` : sur le `<span>` texte — 4 lignes max + ellipsis (`-webkit-line-clamp:4` ; `min-width:0` requis pour le shrink flex).
- `.msg-pill-icon` : icône ▶ à `1.08em` (suit le clamp cqw).
- **Prérequis cqw** : un ancêtre `container-type: inline-size` (`.widget-3d`, `.bg-layer`, `.titre-zone`), sinon fallback % viewport.
- Couleur de texte par contexte : classe locale (`.input-viewer-obs` = `--message-user-action-color`) ou inline (`lectureCouleur`/`bgLectureCouleur` = couleur réelle du cadre SVG).

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

## Titre de widget (2026-09-19)

> Titre optionnel affiché au-dessus ou en dessous d'un widget, rendu en
> dashboard ET en diffusion :4321. Pendant pour le fond de l'application :
> champs `bgTitre*` (`.bg-titre` dashboard / `#bg-titre` diffusion).

### Champs `Widget` (tauri.ts + scene.rs)

`titre` (vide/undefined = pas de titre), `titrePosition` ("dessus"/"dessous"),
`titrePolice` (id Google Font, `fonts.ts` — woff2 embarqués via `server.rs`),
`titreTaille` (12–72 px, défaut 24), `titreGras`/`titreItalique`/`titreSouligne`,
`titreEspacement` (−2 à 20 px).

### Édition (dashboard)

Double-clic sur le widget : `relY < 0.25` → position "dessus", le reste →
"dessous" → `titreModalOpen` (ui.ts) → `TitreModal.svelte` → `majTitreWidget`
(scene.ts). Zones `.titre-zone` (25% haut/bas) : affichées au `:hover` dès
que le widget est `selected`, **avec ou sans titre existant**, avec label
explicite ambre (« Double-cliquez ici pour ajouter/modifier un titre
au-dessus/en dessous de votre widget »).

### Rendu

- Dashboard : `.widget-titre` hors `.widget-3d` (non clippé par le cadre
  SVG), gradient de texte = couleurs du cadre actif (`gradientCadre`,
  fallback blanc→gris). Position `bottom:100%`/`top:100%`.
- Diffusion : `applyTitreWidget` / `entry.titreEl` (diffusion.html) —
  créé/maj/supprimé selon `w.titre`, mêmes champs.
- **Contour des lettres** : `-webkit-text-stroke: 2px #fff` +
  `paint-order: stroke fill` (contour SOUS le gradient — sans paint-order il
  mange les glyphes). Appliqué aux 4 règles : `.widget-titre` + `.bg-titre`
  (dashboard) et `.widget-titre` + `#bg-titre` (diffusion).

## Cadres SVG — règle absolue (NE PAS REGRESSER)

> ⚠️ **RÈGLE FIGÉE — ne jamais modifier ce comportement sans accord explicite.**
> Cette section documente un fix définitif (2026-08-31) après plusieurs
> régressions. Tout changement ici casse l'affichage côté diffusion/OBS.

### Principe

Les cadres SVG (`scene.cadreWidget`) s'appliquent à **TOUS les widgets**, sans
exception : médias, chat, welcome-clip, et **widgets troués**. Aucun type de
widget ne doit être exclu du cadre.

**Opt-out par widget** (`w.sansCadre`, 2026-09-19) : les widgets créés par
drop dans la zone Widgets (`ajouterALaBibliotheque` → `poserMediaDepuisBibliotheque`,
auto-pose — l'entrée reste dans `bibliothequeMedias`) portent `sansCadre: true`
— le cadre de scène ne s'applique pas à EUX (clip-path + overlay absents, trou
éventuel rectangulaire, titre en gradient fallback). Le bouton « Transformer
en widget » (`transformerEnWidget`) repasse `sansCadre` à false sur le widget
déjà posé (pas de doublon) et consomme la ligne. Flag par instance sérialisé
dans la scène, pas par type : la règle ci-dessus reste vraie pour tous les
autres widgets.

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

### Commandes Tauri (11)

- `speedrun_charger_asl(path)` → charge .asl, retourne settings
- `speedrun_charger_lss(path)` → charge .lss, retourne segments + PB ; fige
  aussi la baseline mergée (slot Rust) poussée dans le payload WS `lss`
- `speedrun_demarrer(totalSplits, start, split, reset)` → démarre boucle polling
- `speedrun_arreter()` → arrête boucle
- `speedrun_maj_settings(start, split, reset)` → met à jour settings
- `speedrun_est_actif()` → true si actif
- `speedrun_action_manuelle(action)` → start/split/skip/undo/reset/pause
- `speedrun_lire_config()` → lit speedrun.json (chemins + settings)
- `speedrun_sauver_config(config)` → sauve speedrun.json
- `speedrun_baseline_lire(chemin_lss)` → baseline sauvegardée du .lss
  (`Vec<Option<f64>>`, cumul secondes/segment ; vide si jamais enregistrée)
- `speedrun_baseline_enregistrer(chemin_lss, index, cumul_secs)` → écrit le
  temps cumulé d'un split réalisé dans speedrun_baselines.json (ne touche PAS
  la référence affichée — figée pour le run en cours)

### Events

- Tauri `speedrun:event` → store Svelte dashboard (types: started, split, reset,
  ended, gameTime, time, connected, disconnected, error, stopped, loaded, paused,
  resumed, splitSkipped, splitUndone, settings-list).
- WS `speedrun-etat` → diffusion.html `majSpeedrunEtat` (maj DOM directe,
  pattern input-viewer : textContent/classList, zéro re-render). Sous-types
  spéciaux : `lss` (segments + champ `baseline` mergée) et `baseline`
  (référence delta fraîche, émise à chaque `started` — figée pour le run).

### Colonne delta « dernière fois » (2026-09-19)

> 1ère colonne temps de chaque ligne de split = **delta cumulé vs la référence**
> (baseline `speedrun_baselines.json` « dernière fois », fallback PB du .lss) —
> convention LiveSplit (colonne « +/− », signe toujours affiché, dixième).

- **Format** : `-1.6` / `+2.3` / `-1:23.4` (cumulé au split : temps courant −
  référence cumulée). `0.0` neutre (gris) si non parcouru / skippé / sans
  référence (premier run : « compteurs à zéro »).
- **Couleurs** : vert `--message-ok-color` si négatif (gagné), rouge
  `--message-error-color` si positif (perdu) — sur la valeur ET la **bordure
  de la ligne entière** (`.seg-ahead`/`.seg-behind` dashboard,
  `.sr-seg-ahead`/`.sr-seg-behind` diffusion ; déclarées après `.actif` →
  l'emportent aussi sur le split en cours).
- **Cycle de vie de la référence** : figée à chaque `started` (slot Rust +
  emit WS `baseline` + store `referenceSplits`) — les enregistrements en
  cours de run préparent la référence du run SUIVANT sans modifier l'affiché.
- **Ordre des colonnes aligné** dashboard/diffusion : n° | nom | delta | temps.
- **Dernier split** : émis via `ended` (pas `split`) → son temps est stocké
  dans `splits` et enregistré en baseline dans le handler `ended`.

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

## Effet miroir média — flip H/V (2026-09-20)

> Retournement horizontal/vertical du contenu média (sémantique OBS « Flip »).
> `mediaMiroirH`/`mediaMiroirV` sur `Widget`, `bgMiroirH`/`bgMiroirV` sur
> `Scene` — booléens `serde(default)` (false = pas de miroir, rétro-compat).

- **Rendu** : `scale(±1)` **combiné au zoom** dans la chaîne transform
  existante, **après `rotate`** dans la liste → le flip s'applique à la source
  dans son espace local (« flipper l'image puis la tourner »). Formule unique
  partout : `scale(zoom·mH, zoom·mV)` avec `mH/mV ∈ {−1, 1}`.
- **Chemins couverts (tous doivent rester synchrones)** :
  - Widget média : `mediaTransform` (Widget.svelte) + `dessinerOffscreen`
    (chemin morph, 2 sous-chemins normal/pixel) ; `applyMediaStyle` +
    `morphRendreWidget` (diffusion.html).
  - Media-fond des widgets à contenu (chat/input-viewer/speedrun) : automatique
    via `applyMediaStyle` (diffusion) et le snippet `mediaFond` (dashboard).
  - Fond : `bgMediaTransform` + `rendreBgMorph` + `rendreBgPixel`
    (Canvas.svelte) ; `dessinerContenuFond` dans `peindreFond` (diffusion.html
    — couvre le chemin morphé via l'offscreen).
- **UI** : onglet **Effets** de CarteEdition — groupe « Miroir média »
  (gated `selectedWidget?.media`) et « Miroir fond » ; boutons toggle
  `fit-btn` « Miroir → » / « Miroir ↓ » → `toggleMediaMiroir` /
  `toggleBgMiroir` (stores/scene.ts, commit immédiat).
- **Reset** : `resetMedia` / `resetFond` remettent les flags à false.
- **Exclusions** : widget caméra (source OBS `SOS-Caméra`, pipeline
  `camera_sync` — un flip caméra serait un transform OBS séparé) et widgets
  trou/captures OBS (pas de média rendu — flags inertes, comme zoom/rot).
- Ne pas confondre avec le « Retournement » de l'onglet Dupliquer
  (`rotateX`/`rotateY` ±180° = flip du widget ENTIER en 3D, pas du média).

## Barre de raccourcis — fenêtre TOPMOST dockée à un bord d'écran (2026-09-19)

> Fenêtre `WebviewWindow` dédiée (label `raccourcis`, `raccourcis.rs` +
> `resources/raccourcis.html` via custom protocol `streamos-raccourcis://`) :
> bande noire translucide `rgba(0,0,0,0.7)` collée à un bord d'écran, toujours
> au-dessus, icônes d'actions du dashboard. **AUCUNE logique métier dans la
> barre** — chaque clic émet `raccourcis:action` {id} vers le dashboard.

### Contrats figés

1. **TOPMOST ≠ plein écran exclusif.** `always_on_top` place la fenêtre dans la
   bande topmost Win32 ; un jeu en **plein écran exclusif** (DirectX flip)
   recouvre TOUT — la barre disparaît, c'est accepté. Cible = **fenêtré sans
   bordure (borderless) + bureau**. JAMAIS d'overlay injecté (DLL, DX/VK) —
   interdit, certains anti-cheat le sanctionnent.
2. **Ré-armement topmost** : un jeu borderless lui aussi topmost peut passer
   devant. Timer tokio 2s → `SetWindowPos(HWND_TOPMOST, SWP_NOMOVE|NOSIZE|
   NOACTIVATE)` (z-order public Win32, cross-thread sûr). La tâche meurt avec
   la fenêtre (check `get_webview_window` à chaque tick).
3. **Flags fenêtre** : `decorations(false)`, `resizable(false)`,
   `always_on_top(true)`, `skip_taskbar(true)`, `focusable(false)` (= ne vole
   pas le focus au jeu ; les clics souris restent reçus), `transparent(true)`
   (sinon `rgba(0,0,0,0.7)` composite sur fond opaque → gris),
   `visible(false)` → géométrie physique → `show()`.
4. **Géométrie physique** : épaisseur 56px logiques × `scale_factor`, rect
   moniteur COMPLET (pas work_area — la barre recouvre la taskbar si même
   bord). `monitor_index` hors bornes (écran débranché) → primaire.
5. **`withGlobalTauri: true`** dans tauri.conf.json : REQUIS — le HTML vanilla
   de la barre utilise `__TAURI__.event.emit/listen` (contrairement au pop-out
   chat qui passe par le WS :4321 sans IPC).
6. **Orientation** : `?bord=` dans l'URL à la création + `emit_to("raccourcis",
   "raccourcis:bord", bord)` sur ré-application → bascule colonne (G/D) ↔
   rangée (H/B) dans le HTML.

### Commandes (`raccourcis.rs`)

| Commande | Rôle |
|----------|------|
| `raccourcis_moniteurs` | Liste moniteurs (index, nom, résolution, primaire) pour le `<select>` Toolbar |
| `raccourcis_etat` | `{visible, bord, monitor_index}` — visible = fenêtre réellement ouverte |
| `raccourcis_appliquer` | Crée/montre la barre dockée + persiste `raccourcis.json`. **Async obligatoire** (deadlock Webview2 en sync) |
| `raccourcis_masquer` | Détruit la fenêtre + `visible:false` (bord/moniteur conservés) |

### Ids d'actions (`raccourcis:action` → dispatch App.svelte)

- Widgets : `widget-media`, `widget-chat`, `widget-camera` (garde singleton
  caméra identique à la Toolbar), `widget-input-viewer`, `widget-speedrun`
- Connexions (toggle selon `connexions`) : `connexion-twitch`,
  `connexion-kick`, `connexion-youtube`, `connexion-tiktok`, `connexion-obs`.
  Kick/TikTok sans slug/username sauvegardé → `openSectionExplicit("connexions")`.
  **OBS = one-shot** (pas de déconnexion) : clic = `obsConnect` +
  `sceneSyncCaptures` (re-sync, comme le bouton « Connecter »).
- Modales : `modale-interactions`, `modale-moderation`, `modale-cadres`
- Système : `systeme-masquer`

### Événements

- `raccourcis:action` {id} — barre → dashboard (dispatch).
- `raccourcis:ready` — barre → dashboard : handshake, la barre demande l'état
  des connexions (pastilles vertes) après init de ses listeners.
- `raccourcis:connexions` {twitch,youtube,kick,tiktok,obs} — dashboard → barre
  (`emitTo`), poussé par `$effect` sur `connexions`/`obsStatus` + au `ready`.
- `raccourcis:bord` — Rust → barre : nouvelle orientation sans recréer.
- `raccourcis-closed` — Rust → dashboard : fermeture externe (sync Toolbar).

Frontend : `stores/raccourcis.ts` (visible/bord/monitorIndex/moniteurs +
init/appliquer/masquer), section accordéon « Raccourcis » dans Toolbar
(show/hide, 4 bords, `<select>` moniteurs, hint borderless).

## Sorties OBS — stream & record (2026-09-20)

> Boutons « Démarrer/Arrêter » le stream ET l'enregistrement OBS : 2 toggles
> dans la section OBS de la Toolbar + 2 icônes dans la barre de raccourcis.
> **La connexion OBS reste one-shot** (connect → RPC → close) comme toutes les
> commandes OBS — aucune connexion persistante, **aucun polling**.

### Contrats

- `obs.rs` : `toggle_stream` / `toggle_record` (ToggleStream / ToggleRecord,
  retournent `outputActive` de la réponse = nouvel état, zéro requête en plus)
  et `outputs_status` (GetStreamStatus + GetRecordStatus dans UNE connexion →
  `(stream, record)`). Réutilisent `connect()` + `rpc()` comme
  `refresh_diffusion`.
- Commandes Tauri : `obs_toggle_stream` / `obs_toggle_record` → `bool`,
  `obs_outputs_status` → `{ stream, record }` — signature `(host, port,
  password)` comme les autres.
- `stores/obs.ts` : `obsStreaming` / `obsRecording` (writable bool),
  `rafraichirOutputs()` (non-fatal, appelé à la fin d'`obsConnect` réussi),
  `toggleStreamObs()` / `toggleRecordObs()` (le booléen retourné alimente le
  store directement ; erreur traduite via `messageClair` → `obsError`).
- **PAS de polling** (choix validé) : l'état est lu au connect + après chaque
  toggle. Si l'utilisateur agit directement dans OBS, le dashboard et la barre
  restent désynchronisés jusqu'au prochain toggle dans StreamOS.
- Toolbar : `.action output-btn` + `<span class="dot">` (vert
  `--message-ok-color` quand actif) + `.output-btn.actif` (bordure/texte
  `--accent-ui`, recette `.action.small.actif`). `disabled` si OBS non
  connecté ou toggle en vol (`outputBusy` — chaque toggle = 1 connexion).
- Barre raccourcis : icônes `data-id="obs-stream"` / `data-plat="stream"` et
  `data-id="obs-record"` / `data-plat="record"` — la pastille `.dot` existante
  suffit, `setConnexions` itère tous les `[data-plat]` (zéro JS ajouté).
  Payload `raccourcis:connexions` étendu : `stream` / `record` (bools),
  poussés par le même `$effect`/`pushRaccourcisConnexions` d'App.svelte.

## Télémétrie multiplateforme — collecte passive JSONL (2026-09-19)

> Module `telemetrie/` : observateur **PASSIF** des flux existants — zéro
> modification des émetteurs. Listeners Tauri `*:connecte` / `*:deconnecte`
> (bornes de session, uuid par fenêtre connexion→déconnexion) + subscription
> `chat_tx` (même canal broadcast que le WS :4321) avec whitelist fermée.

### Contrat d'événement (schéma v1 — `telemetrie/events.jsonl`)

```
{"v":1,"ts":"<ISO8601 UTC>","plateforme":"...","kind":"...",
 "session_id"?,"titre_stream"?,"jeu"?,"jeu_id"?,"pseudo"?,"data":{...}}
```

- `plateforme` : `twitch` | `youtube` | `kick` | `tiktok` | **`multi`**
  (commande/welcome — la frame ne porte pas la plateforme d'origine) |
  **`local`** (pad numérique) | `systeme`.
- `kind` : `connexion` | `deconnexion` | `chat` | `viewers` | `alerte` |
  `commande` | `welcome` | `bandeau` | `pad`.
- **Whitelist frames** : `chat`, `viewers`, `alerte-play`, `commande-play`,
  `welcome-clip-play`, `bandeau-premier-message-play`, `pad-play`. Les frames
  de synchro UI (`*-config`, `*-stop`, `input-viewer-etat` 30 Hz,
  `speedrun-etat`, `position-overlay-config`, `snapshot`) ne sont **PAS**
  enregistrées — ajouter un type = décision documentée dans `collecte.rs`.
- `titre_stream` / `jeu` / `jeu_id` : champs réservés — remplis par
  l'enrichissement stream_info (LOT 2), ignorés à la sérialisation tant
  qu'absents.
- Attribution plateforme des overlays : `alerte`→`twitch` (seul émetteur
  actuel), `bandeau`→`viewer.plateforme` (réel, porté par la frame),
  `commande`/`welcome`→`multi` (attribution exacte = hooks explicites dans
  les émetteurs, LOT ultérieur si besoin).

### Architecture

| Fichier | Rôle |
|---|---|
| `src-tauri/src/telemetrie/mod.rs` | `TelemetrieState` (compteurs, sessions courantes, anneau 500 events) + `record()` non bloquant (mpsc non borné → writer) |
| `src-tauri/src/telemetrie/modele.rs` | `EventTelemetrie` schéma v1 + `maintenant_iso()` |
| `src-tauri/src/telemetrie/stockage.rs` | Writer JSONL thread dédié — flush 500 ms / 64 lignes, dégradé mémoire-only si fichier inouvrable |
| `src-tauri/src/telemetrie/collecte.rs` | Whitelist + extraction plateforme/pseudo/data + listeners `*:connecte`/`*:deconnecte` + subscriber `chat_tx` |
| `src-tauri/src/telemetrie/commands.rs` | `telemetrie_etat` (compteurs + sessions + fichier), `telemetrie_derniers(limit)` (anneau, ≤500) |

Init dans `lib.rs` setup : `TelemetrieState::new` + `demarrer` **avant** les
auto-resume (pour capter leurs connexions) — non-fatal si le fichier est
inaccessible. `chat:message`/`viewers:update`/etc. continuent d'aller au
frontend et à :4321 exactement comme avant — le tap est un abonné de plus.

## Grille magnétique + guides symétriques + duplication miroir (2026-09-19)

> Guides visuels pendant drag/resize — **dashboard UNIQUEMENT, jamais en
> diffusion**. Le snap existait depuis le 2026-08-29 (dev-log #29) ; cette
> passe ajoute symétrie axe central, espacements égaux et duplication de
> widget avec gizmo miroir.

### Snap (`scene.ts` — `buildSnapTargets` / `computeSnap`, seuil `SNAP_PX = 6`)

Cibles par axe (`xs`/`ys`, `SnapTarget {pos, side}`), figées au pointerdown
(snapshot — pas de `get()` par frame), bypass **Alt** = free-drag, actif en
drag ET resize :

- bords + centre du canvas, bords + centres des autres widgets (existant)
- `side: "mirror"` : positions miroir des autres widgets (`cw − o.x`,
  `cw − o.x − o.largeur`, idem en y) → la marge opposée du widget déplacé
  égale la marge d'un autre widget
- `side: "gap"` : pour chaque paire non chevauchante, prolongation de la série
  au même espacement + centrage entre les deux (si le widget déplacé tient
  dans le gap — ses dims sont lues depuis la scène au snapshot)

### Visuels (`AlignmentGuides.svelte`, `<canvas>` pointer-events:none z-50)

- 4 traits aux bords + labels distance px — passés en jaune `--ctrl-centre`
  quand les marges opposées sont égales (±0.5 px ; le snap produit l'égalité
  exacte)
- Viseur central + label « CENTRE » (existant)
- **Fantôme miroir** : rect pointillé vert `--ctrl-refresh` à la position
  symétrique (axe vertical du canvas), masqué si centré (= même place)
- **Espacements égaux** : segments pointillés jaunes sur le gap créé ET le
  gap de référence + label « N px ». Les autres widgets sont lus depuis
  `sceneStore` ; le widget déplacé est exclu par égalité de rect (le store
  reçoit exactement les valeurs snappées poussées dans `guidesEtat`)

### Duplication (`dupliquerWidget` + onglet « Dupliquer » de CarteEdition)

- Onglet « Dupliquer » dans la carte d'édition — branche widgets non-caméra
  (la caméra reste exclue : singleton figé, `createCameraWidget` refuse).
- Bouton → copie : uuid neuf, z au-dessus (max non-caméra + 1), `obsSource`
  **strippé** (une source OBS = un seul emplacement ; le trou copié reste non
  lié), position initiale = miroir axe vertical du canvas (+24 px si
  l'original est centré). Copie sélectionnée + `commitScene` (save + snapshot).
- Flag `garderTab` (CarteEdition) : la copie devient sélectionnée SANS reset
  de l'onglet vers « Contenu ».
- **Gizmo miroir** : `Gizmo3D` réutilisé avec prop `maxDeg={180}` (défaut 90 =
  inclinaison seule). `rotateY = ±180°` = miroir horizontal, `rotateX = ±180°`
  = miroir vertical — `perspective(800px)` sans `backface-visibility` : le
  widget montre son dos. Aimant bord ±maxDeg (8°), label « Miroir H / V /
  H+V » (`--ctrl-centre`) dès |angle| > 90°. Raccourcis « Miroir → » /
  « Miroir ↓ » / « Reset » (`rotateWidgetLocal` + `commitScene`).
- **Zéro nouveau champ `Widget`** : `rotateX`/`rotateY` existants, rendus
  identiques dashboard (`Widget.svelte`) et diffusion (`diffusion.html`),
  persistés dans la scène JSON comme avant.
