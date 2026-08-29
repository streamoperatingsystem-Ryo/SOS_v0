# StreamOS v0 — Documentation technique

## Commandes de build/vérification

```bash
# Rust (backend Tauri)
cd src-tauri && cargo check          # vérification compilation
cd src-tauri && cargo build          # build debug

# Frontend (Svelte + Vite)
npm run build                        # build production
npm run tauri dev                    # dev complet (Rust + frontend)
```

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
| `communaute.ts`| Followers/subs/viewers (Twitch Helix + YouTube Data API) |

## Persistance (config.rs)

Fichiers dans `%APPDATA%/StreamOS/` :
- `kick.json` — `{ "slug": "..." }`
- `tiktok.json` — `{ "username": "..." }`
- `scenes/` — scènes SOS (JSON)
- `medias/` — médias importés

Twitch et YouTube utilisent keyring (coffre OS) pour les tokens OAuth2 — pas de fichier JSON.
