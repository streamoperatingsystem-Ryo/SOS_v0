# Plateformes de chat — détails

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


### Détails par plateforme

#### Twitch (`twitch_auth.rs` + `twitch_chat.rs` + `twitch_eventsub.rs` + `twitch_helix.rs`)
- **Auth** : OAuth2 Device Code Flow (TVs & Limited Input devices).
  Scopes réels (`twitch_auth.rs` const `SCOPES`) : `chat:read`, `chat:edit`,
  `moderator:read:followers`, `channel:read:subscriptions`,
  `user:read:broadcast`, `moderator:manage:banned_users`, `moderation:read`,
  `channel:read:vips`, `channel:manage:vips`, `channel:manage:moderators`,
  `moderator:manage:chat_messages` + les scopes « En direct » listés dans la
  section « Contrôles Twitch « En direct » » plus bas.
  Token stocké dans keyring (Windows native).
- **Chat** : IRC sur WebSocket `wss://irc-ws.chat.twitch.tv:443` (capabilities
  `twitch.tv/tags` + `twitch.tv/commands` — commands requis pour recevoir
  les USERNOTICE sub/resub/subgift/raid, sinon jamais envoyés par Twitch).
  PING/PONG réactif (le serveur envoie PING, on répond PONG immédiatement —
  pas de timer) + Pong natif au niveau frame WebSocket.
- **Follows** : EventSub WebSocket `channel.follow` v2 (`twitch_eventsub.rs`,
  `wss://eventsub.wss.twitch.tv:443/ws`) — push temps réel, zéro polling.
  Démarré/arrêté avec la connexion Twitch (même `cancel` que l'IRC,
  `stop_irc` aborte les deux). Abonnement créé après le `session_welcome`
  (fenêtre Twitch 10 s ; 409 = déjà actif = OK pour session_reconnect).
  Keepalive ~10 s → read timeout keepalive×2+30 (socket morte détectée).
  `session_reconnect` → reconnexion immédiate sans backoff.
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
- **Events gérés** : `Chat` (message), `RoomUserSeq` (viewer count), `Gift` (alerte `gift` — combos agrégés via `GiftStreakTracker`), `Follow` (alerte `follow`), `Like` (ignoré), `Connected`, `Reconnecting`, `Disconnected`.
- **Events emit** : `tiktok:connecte`, `tiktok:deconnecte`, `tiktok:erreur`, `tiktok:viewers`, `chat:message`.
- **Persistance** : `tiktok.json` (username du streamer).
- **Auto-resume** : lit `tiktok.json` au boot → WS direct.
- **Dépendance** : `piratetok-live-rs` vendored dans `vendor/piratetok-live-rs/` (build.rs neutralisé — bug Windows path separators).
