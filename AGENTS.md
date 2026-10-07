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
| Twitch     | `twitch_chat.rs`   | IRC sur WebSocket (wss, 443)     | OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| YouTube    | `youtube_chat.rs`  | HTTP polling `/liveChat/messages`| OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| Kick       | `kick.rs`          | WebSocket Pusher cloud (us2)     | Aucune (anonyme) | `kick.json` (slug)| oui (slug) |
| TikTok     | `tiktok_chat.rs`   | WebSocket Webcast (PirateTok)    | Aucune (anonyme, ttwid) | `tiktok.json` (username) | oui (username) |

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

## Contrôles Twitch « En direct » — modale Modération (Helix)

Section « En direct » de l'onglet Twitch de `ModerationModal.svelte`
(master-detail, comme Communauté/VIPs/Bannis). Store `stores/twitchLive.ts`,
commandes `twitch_live_*` dans `lib.rs`, helpers Helix dans
`twitch_helix.rs`. **Helix uniquement — jamais IRC** pour ces actions.

- **Scopes ajoutés** (concaténés dans `SCOPES`, twitch_auth.rs) :
  `channel:manage:broadcast` (PATCH /channels + POST /streams/markers),
  `channel:edit:commercial`, `channel:manage:raids`,
  `moderator:read:chat_settings` + `moderator:manage:chat_settings`,
  `moderator:manage:announcements`, `moderator:read:shield_mode` +
  `moderator:manage:shield_mode`, `moderator:manage:automod`,
  `moderator:read:unban_requests` + `moderator:manage:unban_requests`.
  ⚠️ Un token déjà en keyring n'a PAS ces scopes → Helix 403 = `need_reauth`
  → l'utilisateur relance le device flow au clic (jamais automatique, le
  keyring n'est pas effacé tant qu'il n'a pas choisi de se reconnecter).
- **Pub** : POST `/channels/commercial` + scope `channel:edit:commercial`.
  `/channels/ads` = planning de pubs (Get Ad Schedule / snooze) — NE PAS
  l'utiliser pour lancer une pub. Durées figées : 30/60/90/120/150/180 s.
- **File AutoMod** : EventSub `automod.message.hold` / `automod.message.update`
  v1 sur la socket `twitch_eventsub.rs` existante. Souscription ISOLÉE après
  l'abonnement follows (qui reste fatal) : hors du `?`, un échec ne coupe ni
  la socket ni les follows ; un 403 émet `twitch:need_reauth` sans effacer le
  keyring ; 409 = déjà actif = succès. Actions : POST
  `/moderation/automod/message` (ALLOW/DENY). Pas de GET de file — cet
  endpoint n'existe pas dans Helix.
- **Shield Mode** : PUT `/moderation/shield_mode` UNIQUEMENT après
  confirmation explicite dans l'UI — l'activation vide le chat et force les
  modes AutoMod (le texte de confirmation le dit).
- **`parse_resp` (twitch_helix.rs)** : corps vide / 204 → `Value::Null` AVANT
  le parse JSON — PATCH /channels, POST /chat/announcements et DELETE /raids
  répondent 204 No Content.
- **Zéro persistance** : Helix relu à l'ouverture de la section, rien sur
  disque. Aucun contrôle Helix ne touche la scène ni OBS — **seule
  exception** : le bouton « Poser sur le layout » (jacquette du jeu
  courant, choisie via la recherche de catégorie ou le `game_id` relu).
  `twitch_live_poser_jacquette` (lib.rs) télécharge le JPEG avec gardes
  AVANT la requête (schéma https + hôte figé `static-cdn.jtvnw.net`,
  `game_id` alphanumérique ; corps ≤ `MAX_IMG_BYTES` et magic bytes
  FF D8 FF vérifiés) dans `medias/jacquette-<game_id>.jpg` (écrasé à
  chaque pose), puis `poserJacquette` (stores/scene.ts) fait l'upsert du
  widget média dédupliqué par nom de fichier — un second clic met à jour,
  n'empile pas — + UN seul `commitScene`.

## Planning Twitch — onglet Planning (modale « Interactions Live », Helix)

Onglet « Planning » de `InteractionViewerModal.svelte` (la modale
« Interactions chat » a été renommée « Interactions Live » : bouton Toolbar
`toolbar.interactions`, titre `interactions.titre`, largeur portée à
1200 px). Composant `PlanningTab.svelte`, store `stores/planning.ts`,
commandes `twitch_planning_*` dans `lib.rs`, helpers `schedule_*` dans
`twitch_helix.rs`. **Helix uniquement — le planning Twitch est la source
de vérité, zéro persistance locale** : relu à l'ouverture de l'onglet et
après chaque mutation.

- **Scope** : `channel:manage:schedule` (SCOPES). Vieux token → 403 =
  `need_reauth` → banner de reconnexion (jamais de device flow auto).
- **GET /schedule** : `data` est un OBJET `{segments[], vacation}` — pas un
  array ; curseur de pagination à la racine (`first=25`, borné MAX_PAGES).
  Un segment récurrent sort une occurrence par semaine, chacune avec son
  propre `id` Helix. ⚠️ Chaîne sans planning = **404 « schedule was not
  found »** (pas un array vide) — `helix_get` convertit ce 404 en
  `Value::Null` (garde limitée à `path == "/schedule"`, retry 401 inclus),
  `schedule_get` le lit comme planning vide.
- **POST /schedule/segment** : `duration` est une CHAÎNE JSON en minutes
  (30–1380), `timezone` IANA requis (`planningTimezone()` =
  `Intl.resolvedOptions()`). Glisser-déposer d'une jacquette → création
  PONCTUELLE 1 h ; chaîne non affiliée → Twitch refuse les non-récurrents
  avec un 403 « non-recurring » que `helix_write` distingue de NeedReauth
  (lecture du corps) → la commande retente en `is_recurring=true`
  automatiquement.
- **PATCH /schedule/segment** : corps partiel `ScheduleSegmentPatch`
  (clés snake_case — même convention que `ChatSettingsPatch`).
  ⚠️ Sur un récurrent, title/category/duration s'appliquent à TOUTE la
  série ; `start_time` n'est PAS modifiable sur un récurrent → changer
  jour/heure = DELETE + re-POST (confirmé « toute la série » dans l'UI,
  création d'abord pour ne pas perdre la série en cas d'échec).
  `is_canceled` annule la prochaine occurrence.
- **DELETE /schedule/segment** : supprime TOUTE la série d'un récurrent —
  confirmé dans l'UI. Drop d'une jacquette sur une carte segment =
  PATCH `category_id` (confirmé si récurrent).
- **Vacances** : PATCH `/schedule/settings` — tous les paramètres EN QUERY
  (endpoint sans corps) : `is_vacation_enabled`, `vacation_start_time`,
  `vacation_end_time` (RFC3339 — convertis depuis les inputs `date` dans
  `planningSetVacances`), `timezone`.
- **DnD** : dataTransfer JSON sous MIME `application/x-sos-planning`
  (`{type:"categorie"}` / `{type:"segment"}`). Seuls les segments ponctuels
  sont draggables ; les jacquettes de la colonne droite viennent de
  `twitch_live_search_categories` / `twitch_live_game_info` (cache
  `planningJacquettes` par category_id, placeholders `{width}x{height}`
  remplacés au rendu). Vues semaine (drop → heure), mois (drop → jour,
  heure 20h par défaut), liste.

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
    color: Option<String>,      // tag IRCv3 color (Twitch)
    message_id: Option<String>, // suppression Helix (Twitch)
    emotes: Option<Vec<EmoteSeg>>, // emotes Twitch (officielles + tierces)
}
```

**Rendu diffusion incrémental (2026-09-22)** : un nouveau message n'ajoute
qu'UN nœud DOM par widget (`appendChatMessage` → `creerNoeudChat`), plus
rebuild des 200 messages à chaque message. Le trim du buffer (MAX_CHAT=200)
retire les nœuds correspondants dans chaque widget filtré. `fillChatBody`
(rebuild complet) reste le chemin des changements rares : création du widget,
changement de filtre/taille/cadre (le border-image inline suit le gradient
du cadre — CEF ne recalcule pas les var() en dynamique).

**Emotes Twitch (twitch_emotes.rs)** : au démarrage de la tâche IRC, une
tâche parallèle charge les listes BTTV/7TV/FFZ (global + chaîne, APIs
publiques sans auth — 404 = chaîne non inscrite, non-fatal) dans un
`Arc<RwLock<CacheEmotes>>`. Par PRIVMSG, `construire_segments` produit des
`EmoteSeg { debut, fin, url, nom }` : les emotes **officielles** viennent du
tag IRC `emotes` (positions en **codepoints Unicode**, converties en UTF-16
pour `String.slice` JS — fin exclusive) ; les **tierces** sont matchées par
mot dans les zones non couvertes (priorité BTTV > 7TV > FFZ, case-sensitive,
ponctuation collée acceptée). `diffusion.html` et `chat-popout.html` rendent
`<img class="emote">` entre texte échappé (URL contrainte à `https://`,
XSS-safe). Les émojis Unicode passent en texte brut (transport UTF-8 intact,
fallback police OS).

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

## Widgets objectif (goal) — compteurs live + célébration (2026-09-22)

> Objectifs streamer façon Creator Goals/Streamlabs : compteur réel + cible
> choisie par l'utilisateur + célébration à l'atteinte. **Zéro tâche de
> polling ajoutée** — tout passe par la tâche centralisée `viewers.rs`
> (tick 15s, fetch 60s live / 180s offline par plateforme, déjà existante).

### Données (frames WS `chat_tx` → diffusion :4321, additive aux frames viewers)

- `{"type":"communaute","plateforme":p,...}` — compteurs réels dédupliqués
  par tuple complet (1/plateforme/fetch max). Champs **uniquement si
  disponibles** (jamais de valeur fabriquée) :
  - Twitch : `followers` (`followers_total`, 1 GET `first=1`→`total`),
    `subs` + `sub_points` (`subs_totaux`, 1 GET `first=1`) — twitch_helix.rs.
  - YouTube : `abonnes` (`channel_info` existant). Members = **N/A**
    (scope `channel-memberships.creator` absent du Device Flow).
  - Kick : `followers` (`followersCount` — mutualisé dans le GET
    `api/v2/channels/<slug>` des viewers, `fetch_channel_stats`, zéro GET
    supplémentaire).
  - TikTok : rien (pas de total public fiable) — viewers via RoomUserSeq.
- Emit frontend miroir : `communaute:update` → `stores/objectifs.ts`
  (`valeursGoals`, carte d'édition + aperçu dashboard).
- `{"type":"goal-atteint","widget_id","plateforme","metrique","count","cible"}`
  — émis par `viewers::verifier_objectifs` au **crossing ≥ cible, une fois** ;
  réarmé quand le compteur redescend sous la cible (unfollows/sub expiré —
  les objectifs Twitch redescendent aussi, doc officielle). Aussi appelé
  depuis `tiktok_chat.rs` (RoomUserSeq → métrique viewers). + `app.emit`.

### Widget `type: "goal"` (scene.rs / tauri.ts — champs Option, camelCase)

`goalPlateforme` ("twitch"|"youtube"|"kick"|"tiktok"), `goalMetrique`
("followers"|"subs"|"sub_points"|"abonnes"|"viewers"), `goalCible` (u64,
None = compteur seul sans atteinte). `titre` existant = label encadré.
Disponibilité réelle des métriques : `GOAL_METRIQUES` (tauri.ts) — la config
ne propose que ce qui est mesuré.

### Algo de suggestion (vérifié doc officielle Twitch Creator Goals)

`goalPalierSuivant(n)` : <100 → +10, <1000 → +100, <10000 → +1000
(exemples officiels : 11→20, 260→300). `goalSuggestion()` ajoute le cas
twitch/followers <25 → **25** (Path to Affiliate).

### Diffusion (`diffusion.html`)

`.widget.goal` (container-type + cqw comme .inputviewer) : entête logo
(`logoPlateforme`) + métrique, chiffres `actuel/cible`, barre gradient
`--sr-grad-*` (couleur du cadre SVG). `majGoalValeur`/`majGoalWidgets` =
textContent/width uniquement (zéro re-render). Célébration : voile
`.goal-celebration` + pulse `goalGlow` ~6s, **contenue au widget** — ne
touche pas la file d'alertes globale.

### Dashboard

`Widget.svelte` : vignette live (label + actuel/cible + mini-barre, store
`valeursGoals`). `CarteEdition.svelte` : selects plateforme/métrique (filtrés
par `GOAL_METRIQUES`), champ Cible, bouton **Suggérer** (palier sur valeur
live, `N/A` si non mesurée). `Toolbar` → « Créer un widget objectif ».

### Mode lié — Creator Goals Twitch déclarés (2026-10-03)

> Le widget peut aussi être LIÉ à un Creator Goal déclaré par le streamer
> dans son Creator Dashboard Twitch — actuel/cible/description tenus par
> Twitch sur les vrais events, pas par une cible locale. **Twitch
> uniquement** : aucune API d'objectifs chez YouTube/Kick/TikTok → le mode
> manuel reste leur seule option (et le fallback si goalId est retiré).
> ⚠️ Aucune API d'écriture : les goals se déclarent côté Twitch, jamais
> depuis StreamOS. GET /goals ne retourne que les objectifs ACTIFS.

- **Widget** : `goalId: Option<String>` (scene.rs + tauri.ts, serde default
  → rétro-compat). Some = mode lié (`goalMetrique`/`goalCible` ignorés,
  gardés comme fallback à la déliaison). `goalId: undefined` = délier.
  Changer de plateforme hors Twitch purge `goalId` (CarteEdition).
- **Scope** : `channel:read:goals` ajouté à `SCOPES` (twitch_auth.rs) →
  tokens en keyring SANS ce scope → 403 → `need_reauth` (même mécanique que
  les sections « En direct » : emit `twitch:need_reauth`, keyring conservé,
  reconnexion device flow au choix de l'utilisateur).
- **Helix** (`twitch_helix.rs`) : `CreatorGoal { id, goal_type, description,
  actuel, cible, atteint, termine }` — vocabulaire partagé commande Tauri +
  frame WS, zéro mapping. `creator_goals()` = GET /goals (`atteint` dérivé
  actuel≥cible). `eventsub_abonner_goals()` = channel.goal.begin/progress/
  end v1 — condition `broadcaster_user_id` SEULE (pas de moderator_user_id,
  contrairement à follows/automod).
- **EventSub** (`twitch_eventsub.rs`) : 3ᵉ abonnement isolé non-fatal après
  follows (fatal) et automod (isolé). Notifications `channel.goal.*` →
  `viewers::on_goal_event` (is_achieved null sur begin/progress → dérivé ;
  verdict final sur end).
- **Frames** (`viewers.rs`) : `{"type":"goal-twitch","id","goal_type",
  "description","actuel","cible","atteint","termine"}` sur chat_tx + emit
  `goal-twitch` miroir → `stores/objectifs.ts` `goalsTwitch` (id→goal).
  Deux sources : EventSub (temps réel) ET poll GET /goals au tick twitch
  (secours + late-joiners :4321). Dedup par tuple dans `goals_publies` ;
  disparus du GET → frame `termine:true` (goal fini/supprimé raté par
  EventSub). Un `termine` n'efface pas la map.
- **Atteinte liée** : `verifier_objectifs_goals` réutilise
  `evaluer_objectif` mais avec clé `widget_id:goal_id` (relier un AUTRE
  goal réarme la célébration ; le retain teste le préfixe avant `:`).
  `verifier_objectifs` (manuel) SKIP les widgets avec goalId.
- **Commande** `twitch_goals_lister` (lib.rs) : GET /goals pour la
  CarteEdition — zéro persistance, relu à chaque ouverture.
- **CarteEdition** : section « Objectifs Twitch » visible si plateforme
  twitch — auto-fetch à l'ouverture (`$effect`), lignes cliquables
  (description || label type, actuel/cible live via goalsTwitch, barre,
  badges Lié/Terminé) ; clic = lier, re-clic = délier (toggle). États :
  non-connecté / chargement / need_reauth (bouton « Reconnecter Twitch »
  → connecterTwitch) / erreur (bouton Recharger) / vide (hint Creator
  Dashboard). Mode lié : contrôles manuels masqués + « Délier » + hint
  goalDisparu si le goal n'est plus actif.
- **Diffusion** (`diffusion.html`) : `goalsTwitchMap` (id→goal) rempli par
  le handler `goal-twitch` ; `majGoalValeur` branche liée (label =
  description, `Terminé` dans `.goal-statut`, `goal-plein` sur pct 100) ;
  `majGoalWidgets` exclut les liés (sinon les frames communaute
  écraseraient l'affichage) ; `gKey` inclut `goalId` (rebuild entête au
  lier/délier) ; `goalLabelEntete` « Objectif Twitch » en attente de la
  1re frame.
- **i18n** : `goal.ttype.*` (7 types Helix : follower, subscription(_count),
  new_subscription(_count), new_bit, new_cheerer) + `carteEdition.goals*`
  dans fr/en/es/it/zh — fr.json source de vérité.

## Ordre de plan (z) des widgets — boutons −/+ au survol (2026-10-03)

> Réordonnancement z par l'utilisateur : tout au premier plan / tout en
> arrière-plan, via boutons au survol + entrées du menu contextuel widget.
> **Zéro Rust** — `Widget.z` est un champ JSON comme un autre ; côté
> diffusion le z est « style seulement » au snapshot (`zIndex = w.z` sur le
> wrap), jamais de rebuild.

- **Bande normale 0..N-1** (widgets non-caméra). Création/duplication :
  `z = max(non-caméra)+1` (trous possibles après delete). `mettreWidgetPlan`
  **renumérote la bande à chaque appel** → z contigus, trous comblés, borne
  basse ≥ 0 garantie (un z < 0 passerait SOUS `.bg-layer` z:0 en dashboard).
- **Caméra EXCLUE** : plage réservée z=5000 (sous les overlays autonomes
  diffusion 9995-9999 — welcome 9999, alertes 9997, commandes 9996).
  `mettreWidgetPlan(id)` est no-op sur la caméra ; boutons et entrées de
  menu masqués pour elle (`wgType !== "camera"` / `!isCamera`).
- **`mettreWidgetPlan(id, "avant" | "arriere")`** (`stores/scene.ts`) : tri
  par z (stable), splice + push/unshift, réassignation z=index, puis
  `commitScene` (save + snapshot :4321 — sync trous/caméra inchangée).
  ⚠️ Effet de bord voulu : un widget `trou` masque « fond + widgets
  dessous » → remonter/descendre un trou change ce qui est visible à
  travers lui.
- **Boutons −/+** (`Widget.svelte`, bloc `.z-controls`) : badge flottant
  AU-DESSUS du coin supérieur droit, visible au `:hover` du widget —
  dashboard uniquement, jamais en diffusion. `transform: scale(1/scale)`
  inline → taille écran constante quel que soit le zoom canvas ; ronds
  ambre `--message-user-action-color` ; `pointer-events:none` au repos +
  masqué pendant `.dragging` ; `stopPropagation` pointerdown/dblclick (pas
  de drag/sélection/modale titres/morph) ; `disabled` aux extrémités de la
  bande non-caméra (un widget à égalité de z peut encore bouger).
- **Menu contextuel widget** (`Canvas.svelte`, clic droit) : entrées
  `carteEdition.premierPlan` / `carteEdition.arrierePlan` après « Dupliquer »
  — action → ferme le menu (pattern établi, pas de disabled dans `.ctx-item`).

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
| `presets.ts`   | Galerie de scènes modèles : `presetsStore`, `chargerPresets`, `creerSceneDepuisPreset` (NOUVELLE scène), `initialiserSceneDepuisPreset` (onboarding + recharge UI + sceneSyncCaptures). |
| `langue.ts`    | Store i18n : `langue`, `t` (traducteur `$t("cle")` / `get(t)("cle")`), `localeTag` (toLocaleString), `initLangue`/`setLangue`, `LANGUES`. Voir section i18n + TRADUCTIONS.md. |

## Internationalisation (i18n dashboard) — 2026-09-28

> UI traduisible FR/EN/DE/ES/IT/RU/ZH/JA via dictionnaires JSON — **feuille de route
> et règles détaillées dans `TRADUCTIONS.md`** (lots d'extraction + traduction
> pour agents). Périmètre v1 : DASHBOARD UNIQUEMENT (diffusion.html /
> raccourcis.html / chat-popout.html non traduits).

- **Dictionnaires** : `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` — objets imbriqués
  par zone (`toolbar.widgets.creer`). **fr.json = source de vérité** : toute
  clé y existe d'abord ; fallback langue → fr → clé brute.
- **Store** `stores/langue.ts` : `$t("cle", {vars})` en template Svelte,
  `get(t)("cle")` en TS ; interpolation `{var}` (jamais de concaténation) ;
  `localeTag` (derived) remplace les `toLocaleString("fr-FR")` en dur.
- **Persistance** : `langue.json` via commandes `langue_etat`/`langue_sauver`
  (pattern onboarding — pas de whitelist Rust : ajouter une langue = 1 JSON +
  entrées `LANGUES`/`LOCALE_TAGS`/`DICTS` dans langue.ts, zéro Rust).
- **Sélecteur** : bouton « Langue » topbar (groupe outils à droite,
  `.langue-btn`) + dropdown `.langues-dropdown` — **recette `.ctx-menu` /
  `.ctx-item`** (app.css, même look que les menus clic droit ; seul
  l'ancrage diffère : absolu sous le bouton vs fixed à la souris).
  Chaque item affiche un mini-drapeau SVG (`Drapeau.svelte` — pas
  d'emoji drapeau, non rendus sous WebView2/Windows). Fermeture par
  sélection / clic extérieur `fermerLangues` / Escape `onSupprKey`.
  Noms de langues NATIFS, jamais traduits. Ordre figé : **en, fr, zh**
  en tête, puis de, es, it, ru, ja.
- **Détection au boot** (`initLangue`, onMount App.svelte étape 0b) :
  langue.json → navigator.language (préfixe 2) → "fr".
- **Ne PAS traduire** (liste fermée — détails dans TRADUCTIONS.md) : marques,
  contenu utilisateur, valeurs techniques, `src-tauri/`, pages OBS, commentaires.

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
- `onboarding.json` — `{ "fait": true }` (présence de la galerie de presets au 1er lancement ; absent = premier démarrage)
![alt text](image.png)- `langue.json` — `{ "langue": "fr" }` (langue UI du dashboard — i18n, voir section dédiée)
- `telemetrie/events.jsonl` — journal append-only des événements télémétrie (schéma v1, voir section dédiée)
- `ecran_attente.json` — config de l'écran d'attente BRB (duree_s, titre,
  afficher_titre, son, volume, media/kind/nom, source_audio OBS)
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

## Presets de scènes embarqués — parcours « tout prêt » (2026-09-20)

> Scènes modèles prêtes à l'emploi pour le user lambda : widgets positionnés
> + cadre actif, AUCUN média à importer (chat fonctionne dès qu'une
> plateforme est connectée, caméra via SOS-Caméra). Proposées au PREMIER
> lancement (onboarding) et à chaque création de scène (bouton « + Scène »).

### Données

- 4 presets JSON dans `src-tauri/resources/presets/` — `gaming`,
  `just-chatting`, `speedrun`, `minimal` — embarqués via `include_str!`
  (pattern diffusion.html / chat-popout.html : pas de ressource à bundler,
  recompile si changement).
- Format : `{ id, nom, description, scene: Scene complète }`. Widgets
  uniquement de types fonctionnels sans import (chat / camera / input-viewer /
  speedrun) + `cadreWidget` actif (règle figée cadres : s'applique à tous).
  Conçus 1920×1080 ; `appliquer_canvas_obs` ajuste le canvas sans rescaler
  les widgets (comportement des scènes importées).

### Flux

```
resources/presets/*.json ──include_str!──→ presets.rs
  ├─ presets_lister() → Vec<PresetInfo {id, nom, description}>  (galerie)
  │    parse NON-FATAL : JSON cassé → log::error + absent de la liste
  ├─ charger(id) + instancier(app) → Scene (uuid régénérés par widget
  │    + appliquer_canvas_obs)
  ├─ scene_creer_depuis_preset(nom, preset_id) → scenes::creer_avec →
  │    NOUVELLE scène (l'existante jamais touchée)
  └─ scene_initialiser_preset(preset_id) → ONBOARDING : si la scène courante
       est VIERGE (garde scene_vierge — le « Défaut » auto-créé au 1er boot),
       remplace son contenu + renomme l'entrée d'index ; SINON crée une
       NOUVELLE scène (rien n'est jamais écrasé)
```

Commandes Tauri (lib.rs) : `presets_lister`, `scene_creer_depuis_preset`,
`scene_initialiser_preset`, `onboarding_etat`, `onboarding_marquer_fait`.

### Onboarding (1er lancement)

- Flag `%APPDATA%\com.streamos.v0\onboarding.json` `{ "fait": true }`
  (pattern raccourcis.json — `lire_onboarding_fait` / `marquer_onboarding_fait`
  dans config.rs). Sémantique « sûr » : erreur de lecture = fait (on ne
  ré-affiche pas la modale à chaque boot si le fichier est illisible).
- `App.svelte` onMount (step 5b, APRÈS loadScene/loadScenesIndex/
  loadCurrentScene) : `onboarding_etat()` false → `chargerPresets()` +
  `presetsModalOpen.set("onboarding")`. Non-bloquant (try/catch).
- Choix preset → `initialiserSceneDepuisPreset` (store) : invoke +
  rechargement complet (index + courante + scène) + `sceneSyncCaptures`
  (caméra — même chaîne que openScene) + onboarding marqué fait.
- Fermer sans choisir (✕ / Escape / overlay / « Partir d'une page vierge »)
  → onboarding marqué fait, scène « Défaut » vide conservée.

### UI

- `PresetsModal.svelte` (montée dans App.svelte, pattern CadresModal) :
  mode porté par le store `presetsModalOpen` (`"creation" | "onboarding"`,
  ui.ts). Cards = icône SVG + nom + description (PAS de preview image en v1).
  - mode creation : champ nom (vide → nom du preset, ou « Scène N » pour la
    vierge) + card « Page vierge » (comportement historique `createScene`)
    en première position + cards presets.
  - mode onboarding : cards presets + footer « Partir d'une page vierge ».
- SceneBar bouton « + Scène » : ouvre la modale (`presetsModalOpen.set
  ("creation")`) — plus de création directe dans `onNouvelle`.

### Contrats

- ⚠️ `scene_initialiser_preset` (onboarding) ne remplace la scène courante
  QUE si elle est **vierge** (garde `scene_vierge` : 0 widget, pas de fond,
  cadre, titre ni morph — test `scene_vierge_detecte_contenu`). Un
  utilisateur EXISTANT (onboarding.json absent à la mise à jour → la modale
  s'affiche une fois) qui clique un preset obtient une NOUVELLE scène, son
  travail n'est JAMAIS touché. Pour toute création utilisateur :
  TOUJOURS `scene_creer_depuis_preset` (nouvelle scène).
- Widget caméra dans les presets : `obsSource: "SOS-Caméra"` (constante
  singleton), 1 max par preset, z=5000 (plage réservée). La source est créée
  par `ensure_camera` au `scene_sync_captures` (boot/openScene/connexion OBS)
  — le store presets appelle sceneSyncCaptures après application.
- Ids des widgets régénérés à chaque instanciation : 2 scènes issues du
  même preset ne partagent jamais leurs ids (propre pour duplication etc.).
- `appliquer_canvas_obs` est `pub(crate)` (utilisée par scenes.rs et
  presets.rs).

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
- `remux_to_mp4` est blocking sync, exécuté via les commandes d'import
  `async` + `spawn_blocking` (pool bloquant Tokio — plus sur le thread UI ;
  avant 2026-09-27 elles étaient `fn` sync = thread principal → gel
  « Ne répond plus »). Remux `-c copy` d'un fichier ≤80 Mo prend quelques
  secondes (I/O bound).

### Limitations

- **Codecs non-MP4** : `-c copy` échoue si un flux n'est pas compatible MP4
  (ex. HEVC vidéo, audio Opus hors profil). L'erreur ffmpeg est retournée
  telle quelle (pas de crash). Pas de fallback de transcodage (choix : remux
  seul). Un fallback `-c:v copy -c:a aac` (transcode audio uniquement) pourrait
  être ajouté ultérieurement si besoin.
- **Sous-titres** : `-sn` les dropped (compatibilité MP4/browser).
- **Binaire absent** : message clair « ffmpeg sidecar indisponible ».

## Carrousel d'images — widget média + fond de scène (2026-10-05)

Diaporama d'images sur deux surfaces : `mediaSlides` (widget média) et
`bgSlides` (fond de scène). **Images uniquement** (png/jpg/jpeg/gif/webp) —
pas de vidéos en v1, pas de shuffle, boucle séquentielle.

### Contrat

- `MediaSlide { rel, nom }` (scene.rs + tauri.ts, camelCase figé) — `rel` =
  chemin `medias/<uuid>.<ext>` servi par :4321, `nom` = nom original affiché
  dans la liste d'édition.
- `Option<Vec<MediaSlide>>` + `Option<f64>` (sec) avec `serde(default,
  skip_serializing_if = "Option::is_none")` → rétro-compat : les anciennes
  scènes n'ont pas les champs.
- **Priorité** : liste non vide > média unique (`media`/`bgMedia` restent
  stockés et redeviennent visibles si la liste est vidée) ; `bgObsSource`
  garde la priorité absolue sur le fond (règle figée).
- Intervalle : `mediaSlidesSec`/`bgSlidesSec`, défaut 5 s, min 1 s.
- Fondu enchaîné fixe ~800 ms **borné à la moitié de l'intervalle**
  (`min(800ms, sec×500)`).

### Le timer vit dans `diffusion.html` (jamais le dashboard)

Le dashboard ne commit QUE les mutations de liste/intervalle — **aucun
commit par slide** (un seul écrivain, pas de trafic snapshot par avance).
La boucle continue même dashboard inactif/déconnecté.

- **Widget** : pile d'`<img class="slide">` superposées (opacity 0/1,
  transition CSS bornée), `setInterval` recréé quand `slidesKey`
  (JSON des rels) change ; `slideSec` mémorisé pour recaler le timer sans
  reconstruire. `slidesArreter` à CHAQUE destruction d'entry (delete,
  transition kind↔morph, carrousel→média unique).
- **Chemin morph widget** : la source cachée swap de `src` par intervalle —
  l'élément est REMPLACÉ si la famille change (un `<video>` n'affiche pas
  d'image) ; re-render `morphRendreWidget` au `load` de chaque slide (le
  rendu image est one-shot — la boucle rAF ne couvre que les vidéos).
- **Fond** : `bgState.slides[]` d'imgs cachées (`#bg img` display:none,
  sources drawImage) ; le tick de `startBgLoop` avance à `nextAt` et
  `peindreFond` dessine courante + précédente avec `globalAlpha` pendant
  `fadeDur`. `stopBgLoop` à la sortie de carrousel (sinon rAF tourne à
  vide ; la branche vidéo la relance). Morph/pixel fond : le crossfade est
  dans `dessinerContenuFond` (srcPrev + alphaCur) → couvert par tous les
  chemins.

### Import multi

`import_medias_multi` (lib.rs) : `blocking_pick_files` filtré « Images »
(IMG_EXT) → `validate_and_copy_media` par fichier (mêmes gardes extension/
taille/magic bytes) → `ImportCheminsResult` (ok + erreurs concaténées).
AUCUNE mutation de scène côté Rust — le store pose les slides puis commit.

### Édition (`CarteEdition.svelte`)

Snippet partagé `carrouselBloc` (widget onglet Contenu + fond onglet
Contenu + branche « aucun fond ») : liste ordonnée (↑↓✕), « Ajouter des
images » / « Créer un carrousel », slider intervalle 1–60 s (brouillon
pendant drag, commit au change), « Vider la liste ».

### Preview dashboard

`Widget.svelte` + `Canvas.svelte` : index local cyclé par `$effect` (swap
de `src`, pas de fondu). Morph/pixel preview : source = slide courante.
Garde morph assouplie : `(!bgIsVideo || bgSlidesList)` (fond vidéo +
carrousel d'images → slides morphables).

## Widget caméra — source OBS "SOS-Caméra" (2026-09-12)

> ⚠️ **CONTRATS FIGÉS** (refonte 2026-09-12, remplace l'ancien système getUserMedia) :
> 1. **1 widget `type: "camera"` / scène** (singleton), source OBS fixe **"SOS-Caméra"** (`dshow_input`, constante `obs_trou::SOS_CAMERA`).
> 2. **Create même si OBS offline** → widget créé quand même ; la source est créée par `ensure_camera` au prochain `scene_sync_captures` (boot / openScene / connexion OBS).
> 3. **Delete → `SetSceneItemEnabled(false)` sur l'item, JAMAIS `RemoveInput`** — l'input OBS est conservé (conflits d'accès exclusif du capteur Windows, perte des settings OBS). Recréation = réutilisation de l'input + réactivation de l'item.
> 4. **Device change → `SetInputSettings video_device_id` seulement si `apply_device == true`** (jamais au boot — pas de restart du capteur dshow).
> 5. **Diffusion = trou + cadre SVG, dashboard = placeholder 📷, pas de getUserMedia** — le widget caméra passe par le chemin trou de `diffusion.html` (`renderScene` + `peindreFond` : `w.trou === true || w.type === "camera"`).
> 6. **Transform = sémantique `sync_trous`** : position = centre du widget, bounds w×h, `OBS_BOUNDS_SCALE_OUTER` (cover), alignment 0 → aucun saut entre create et move/resize. **+ crop ratio w/h** (`crop_source`, 2026-10-01) : SCALE_OUTER sans crop déborde de la boîte quand le ratio capteur ≠ ratio widget, et obs-websocket n'expose pas « rogner selon la boîte » → crop explicite calculé depuis `sourceWidth/Height`.
> 7. **Masque = filtre OBS sur la SOURCE, jamais de rustine diffusion.html** (2026-09-29) : `SOS-Caméra` ET `SOS-Caméra-Fond` sont découpés à la forme du cadre SVG via le filtre « Image mask/Blend » (**`mask_filter_v2`**, mode alpha) — le débord du rectangle caméra ne peut plus apparaître à travers les autres trous (jeu) ni sans média de fond.
> 8. **Bloc SOS tout en HAUT de la scène OBS, SAUF `SOS-Fond-*`** (2026-10-01) : `reorder_sos_sources` part de l'index max de la scène (`arr.len()-1`), pas du nombre d'items SOS — sinon le bloc se range en bas et toute source utilisateur passe devant SOS-Diffusion. Les `SOS-Fond-*` (captures plein canvas) restent aux indices 0..F-1, tout en bas : au-dessus des sources utilisateur, elles les couvriraient entièrement.

### ⚠️ Pièges obs-websocket v5 — NE JAMAIS RÉINTRODUIRE (2026-10-01)

> Trois bugs silencieux découverts ensemble (symptômes : angle droit de la
> caméra visible dans le trou jeu en dessous, puis « voile noir » sur la
> caméra). Tous dans `obs_trou.rs`, tous vérifiés par mesure réelle sur
> OBS 32.2.2 / obs-websocket 5.7.4.

1. **`GetSceneItemTransform` répond `{ "sceneItemTransform": { sourceWidth, sourceHeight, … } }`** —
   un niveau d'imbrication. Lire `resp["sourceWidth"]` à la racine renvoie
   toujours 0 → crop zoom, crop ratio et masque silencieusement inopérants
   (log `masque: « SOS-Caméra-Fond » sans taille de base → skip` alors qu'une
   color_source a TOUJOURS une taille = signal de ce bug).
2. **Deux kinds de filtre masque** (`GetSourceFilterKindList`) : `mask_filter`
   = **v1 OBSOLÈTE, `opacity` ENTIER 0-100** ; `mask_filter_v2` = `opacity`
   double 0-1. Créer `mask_filter` avec `opacity: 1.0` → lu comme **1 %** →
   source quasi transparente, plaque noire visible à travers. Toujours
   `SOS_MASQUE_KIND = "mask_filter_v2"` ; un filtre d'un autre kind est retiré
   puis recréé (le kind ne se change pas par `SetSourceFilterSettings`).
3. **`SetSceneItemIndex` : index 0 = bas, max = haut** — le reorder doit
   partir de `arr.len()-1` (taille totale de la scène) pour le bloc
   Diffusion/Caméra/Fond/Trou-* ; les `SOS-Fond-*` occupent explicitement
   les indices 0..F-1 (plein canvas → ne doivent couvrir aucune source).

> Méthode de vérification sans deviner : `GetSourceScreenshot` de `SOS-Caméra`
> avec/sans `SetSourceFilterEnabled` et moyenne d'alpha (1,5/255 vs 255 = le
> filtre est la cause) ; `GetSceneItemList` pour les indices réels.

### Architecture

```
Widget caméra (dashboard) ── commitScene ── obsSyncTrous (item caméra, fit "remplir")
   │                             └─ syncCameraMask (cache par forme : move = no-op)
   │ createCameraWidget / setCameraDevice (forcer) / deleteWidget
   │ camera_sync / camera_hide / camera_mask_sync (commandes Tauri)
   ▼
OBS scène "SOS" : [SOS-Diffusion (browser :4321)] ← au-dessus
                  [SOS-Caméra (dshow + filtre SOS-Masque)]  ← juste dessous
                  [SOS-Caméra-Fond (color + filtre SOS-Masque)] ← plaque noire
                  [SOS-Trou-* (captures)]          ← tout en bas
diffusion.html : widget caméra = trou dans le canvas de fond (destination-out,
forme du clip-path du cadre) → la caméra OBS apparaît à travers ; widgets DOM
chevauchant le rect caméra clippés via clipPath SVG evenodd (#sos-cam-clip).
```

### Masque caméra — découpe à la forme du cadre (2026-09-29)

> Problème : la source OBS est un RECTANGLE (AABB du widget, cover) mais le
> trou visible a la FORME du clip-path du cadre (biseaux cyberpunk…). L'écart
> n'était masqué que par le canvas de fond opaque → pixels caméra visibles à
> travers les autres trous (jeu) et sans média de fond (« débordement »).

- **Filtre** : « Image mask/Blend », kind OBS **`mask_filter_v2`**
  (constant `SOS_MASQUE_KIND` — PAS `mask_filter` v1 : opacity entière 0-100,
  voir l'encart « Pièges obs-websocket » plus haut), settings
  `{ type: "mask_alpha_filter.effect", image_path, color: 0xFFFFFF, opacity: 1.0, stretch: true }`
  — l'alpha du PNG multiplie l'alpha de la source, le PNG est étiré à la
  taille de BASE de la source (d'où le mapping en pixels source).
- **Polygone** : `pointsMasqueCamera` (scene.ts) → `pointsClipPathCadre`
  (cadres/points.ts = `svgPathEnPoints`, port du `svgPathToPoints` de
  diffusion.html) — **même forme que le trou destination-out**, mêmes
  conditions (`cadre.actif && strokeWidth > 0 && !sansCadre`), même parser
  des deux côtés (zéro dérive trou/masque). `carre`/`sansCadre` → null →
  filtres retirés, caméra rectangulaire.
- **Mapping** (`obs_trou::points_en_espace_source`) : inverse du pipeline
  `sync_trous` (`crop_source` = crop zoom centré **+ crop ratio w/h en
  « remplir »**, puis bounds fit alignés au centre — mêmes valeurs que
  l'item). La rotation
  de l'item est ignorée VOLONTAIREMENT : le filtre s'applique avant le
  transform → la forme tourne avec la source, comme le cadre avec le widget.
  La position (x, y) n'intervient pas → un move ne change pas le masque.
- **PNG** : `mask_png.rs` — rasterisation scanline even-odd anti-aliasée
  (couverture fractionnaire) + encodeur PNG RGBA maison (deflate « stored »,
  zéro dépendance). Fail-open : entrée invalide → couverture pleine (caméra
  rectangulaire), jamais de caméra invisible. Fichier versionné par hash
  sha256 dans `<data>/obs/masque-{camera|fond}-<hash16>.png` (cache d'images
  OBS contourné par le changement de chemin) + nettoyage des orphelins en
  gardant les fichiers encore référencés par un filtre.
- **Commande** `camera_mask_sync` (one-shot) : retour `bool` — `false` =
  capteur dshow froid (sourceWidth 0, retry 1,2 s inclus) → le dashboard ne
  met PAS en cache et retente au prochain commit (auto-guérison).
- **Dashboard** (`scene.ts`) : `syncCameraMask(forcer)` — cache par forme
  (`cleMasqueCamera` = JSON des points, mémorisé seulement si Rust retourne
  true) ; fit/zoom = les constantes de l'item caméra de `commitScene`
  (« remplir »/1, contrat lié). `setCameraDevice` → `forcer` (nouveau device
  = résolution native potentiellement différente). `syncCapturesOBS()` =
  sceneSyncCaptures + masque, appelé par les chargements de scène
  (openScene/duplicateScene/preset/boot/connexion OBS — scenes.ts,
  presets.ts, App.svelte, Toolbar.svelte).

### Fichiers

- `src-tauri/src/obs_trou.rs` — `SOS_CAMERA`, `ensure_camera()` (create/enable/transform/index), `camera_sync()`, `camera_hide()` ; `crop_source()` (crop zoom + crop ratio w/h, partagé par `sync_trous`/`ensure_camera`/`points_en_espace_source`) ; `reorder_sos_sources()` (bloc SOS = HAUT de scène, départ à `arr.len()-1`) ; `sync_scene_captures()` : ensure caméra (apply_device=false) + cache l'item orphelin si pas de widget caméra ; section « Masque caméra » : `camera_mask_sync()` + helpers (`points_en_espace_source`, `appliquer/retirer_filtre_masque` avec migration kind v1→v2, `taille_base_item`).
- `src-tauri/src/mask_png.rs` — rasterisation polygone (scanline even-odd AA) + encodeur PNG RGBA (deflate stocké) + tests ; `src-tauri/src/lib.rs` — commandes `camera_sync`, `camera_hide`, `camera_mask_sync` (dossier PNG = `config::data_dir/obs`).
- `src/lib/cadres/points.ts` — `svgPathEnPoints` (port de `svgPathToPoints` de diffusion.html) + `pointsClipPathCadre` (registre → polygone widget-local).
- `src/lib/tauri.ts` — wrappers `cameraSync` / `cameraHide` / `cameraMaskSync`.
- `src/lib/stores/scene.ts` — `createCameraWidget` (obsSource="SOS-Caméra" + cameraSync non-fatal), `setCameraDevice` (cameraSync avec device + syncCameraMask forcé), `commitScene` (items incluent la caméra, fit "remplir" + syncCameraMask), `deleteWidget` (cameraHide avant remove), `pointsMasqueCamera`/`syncCameraMask`/`syncCapturesOBS`.
- `src-tauri/resources/diffusion.html` — **PAS de getUserMedia** : le widget caméra est rendu comme un trou (élément transparent + `applyCadreWidget`), trou dans `peindreFond`. **Aucune modif pour le masque** (découpe au niveau de la source OBS).

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `getUserMedia` / `<video>` / `startCamera` / `stopCameraStream` dans diffusion.html — la caméra vient d'OBS, pas du navigateur.
2. ❌ `RemoveInput` sur "SOS-Caméra" dans le hot path delete widget — item caché seulement.
3. ❌ `SetInputSettings` au boot (sync_scene_captures) — restart du capteur dshow à chaque ouverture de scène.
4. ❌ Exclure le widget caméra du cadre SVG ou du trou `peindreFond` — il suit la règle figée des cadres.
5. ❌ Créer un second input si "SOS-Caméra" existe déjà — CreateSceneItem / SetInputSettings sur l'existant.
6. ❌ Masquer le débord caméra côté diffusion.html (peindre le fond sur le rect, plaque DOM…) — la découpe se fait au niveau de la SOURCE OBS (crop `crop_source` + filtre SOS-Masque), sinon le débord réapparaît à travers les autres trous.
7. ❌ Régénérer le masque à chaque move — cache par forme (`cleMasqueCamera`) : le masque est en pixels de la source, indépendant de la position.
8. ❌ `SCALE_OUTER` (fit « remplir ») sans `crop_source` — la source déborde de la boîte w×h et perce dans tout trou voisin (cf. « Pièges obs-websocket »).
9. ❌ Kind `mask_filter` (v1) — `opacity` entière, 1.0 = 1 % → voile noir ; toujours `SOS_MASQUE_KIND`.
10. ❌ Reorder avec `top_to_bottom.len()-1` comme index de départ — range le bloc SOS en BAS de scène ; départ = `arr.len()-1`.

### Hot-plug devices — Android UVC + caméras virtuelles (2026-10-06)

> Un téléphone Android branché en USB n'expose aucune caméra par défaut :
> **Android 14+** en mode « Webcam » (notification USB du téléphone) expose
> un device UVC en classe PnP `Camera`/`Image` ; les apps de caméra
> virtuelle (DroidCam, Iriun, Camo…) créent un device dshow, souvent en
> `Image`. Le mode « Webcam » ne peut pas être activé à distance — choix
> obligatoire sur le téléphone à chaque branchement.

- **Énumération** (`pc_enum::enumerate_cameras`) : `Get-PnpDevice -Class
  Camera,Image -Status OK` (Image ajoutée pour Android UVC + caméras
  virtuelles ; fallback WMI identique). `CarteEdition.svelte` y ajoute,
  quand OBS est connecté, l'union de `obs_enumerate_targets` kind `camera`
  (liste dshow autoritaire — couvre les devices non-PnP).
- **Watch Rust** (spawn `setup()`, lib.rs) : tick 3 s **uniquement si un
  widget caméra existe** dans la scène (zéro coût sinon — le spawn
  PowerShell est sauté) → emit `camera:devices` (`Vec<String>`) SEULEMENT
  quand la liste change.
- **Frontend** (`scene.ts`) : `cameraDevicesLive` (store, refresh du
  sélecteur sans rouvrir la carte) + `initCameraHotplug` (App.svelte boot).
  **Auto-sélection** : le 1er device NOUVELLEMENT apparu est appliqué via
  `setCameraDevice` si le widget caméra n'a pas de `cameraDeviceId` valide
  (absent ou disparu de la liste). Les devices déjà branchés au boot ne
  sont JAMAIS auto-sélectionnés (snapshot initial = référence, pas de
  détournement du device par défaut d'OBS sans action utilisateur).
- **i18n** : `carteEdition.hintAndroidCam` — aide « Android → USB → Webcam »
  affichée sous `aucuneCamera` quand la liste est vide.

## Fond de scène = capture OBS (app/jeu) — SOS-Fond-* (2026-09-21)

> Le fond de l'application peut être une CAPTURE OBS (fenêtre ou jeu) plein
> canvas au lieu d'un média — « comme un widget trou, mais pour le fond ».

### Mécanisme

```
CarteEdition (fond) → ObsSourceDialog mode "fond" (fenêtre/jeu seulement)
  → lierFondCapture(kind, target) (scene.ts)
      → obsCreateTrouFromPc("SOS-Fond-<sceneId8>", kind, target, 0,0,canvasW,canvasH)
        (create_trou_from_pc REUTILISÉ tel quel : CreateInput/reuse + transform + reorder)
  → scene.bgObsSource = <nom> + commitScene
        → obsSyncTrous : item plein canvas (fit "etirer")
OBS pile (bas → haut) : SOS-Fond-* < SOS-Trou-* < SOS-Caméra-Fond < SOS-Caméra < SOS-Diffusion
diffusion.html : renderBg — bgObsSource PRIORITAIRE sur bgMedia → même chemin
  que « pas de fond » (page transparente → la capture apparaît)
```

- La page :4321 est transparente (`html, body { background: transparent }`) —
  le canvas `#bg-canvas` est la seule couche opaque. Ne pas peindre = voir la
  capture en bas de pile. `peindreFond` n'a PAS besoin de changement (early
  return via `!bgState.mediaEl`).
- **Résolution adaptative + skip-frame (perf, 2026-09-22)** : le backing
  store de `#bg-canvas` suit la taille réelle de la Browser Source
  (`window.innerWidth/Height`, plafonné à 1×) — `bgScaleX/Y` projettent les
  coords scène via `setTransform` dans `peindreFond` (tous les calculs restent
  en pixels scène : fit, offsets, trous, clip-paths). La boucle rAF ne
  repeint que si `mediaEl.currentTime` a changé (pause/stall = zéro coût ;
  les changements de paramètres repeignent via le chemin snapshot).
- **Champ scène `bgObsSource`** (scene.rs + tauri.ts, serde default —
  rétro-compat) : présence = fond capture, PRIORITAIRE sur bgMedia (le média
  reste stocké, non peint ; « Retirer la capture » le restaure).
- **Source PAR SCÈNE** `SOS-Fond-<sceneId8>` (pas singleton — 2 scènes = 2
  fenêtres capturées différentes). `sync_scene_captures` (signature `&Scene`)
  : item == scene.bgObsSource → enable + transform plein canvas
  (0,0,canvasW,canvasH, STRETCH, alignment 5) ; les SOS-Fond-* des autres
  scènes → SetSceneItemEnabled false (orphan hiding).
- **Suppression** : `retirerFondCapture` → RemoveInput (pattern widget trou —
  window/game n'ont pas de conflit d'accès exclusif). La caméra n'est PAS
  proposée en mode fond (conflit dshow avec un éventuel widget caméra).
- **Dashboard** : `Canvas.svelte` — `hasFond` inclut bgObsSource, pastille
  `.lecture-obs-fond` « Capture app/jeu en fond — rendue par OBS » (recette
  .msg-pill), morphable fond désactivé (rien à morpher).
- `commitScene` (scene.ts) : l'item fond plein canvas est poussé dans
  obsSyncTrous (x=0, y=0, fit "etirer" — sync_trous travaille en alignment
  centre, cx = canvasW/2 → couvre exactement le canvas).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | Champ `bgObsSource: Option<String>` |
| `src-tauri/src/obs_trou.rs` | `reorder_sos_sources` (SOS-Fond-* bottom) + `sync_scene_captures` (&Scene, boucle SOS-Fond-*) |
| `src-tauri/src/lib.rs` | `scene_sync_captures` clone la scène complète |
| `src/lib/stores/scene.ts` | `lierFondCapture` / `retirerFondCapture` + item fond dans commitScene |
| `src/lib/components/ObsSourceDialog.svelte` | Prop `mode: "widget" \| "fond"` (état interne renommé `etape`) |
| `src/lib/components/CarteEdition.svelte` | Fond : boutons « Mettre une app/jeu en fond » / Changer / Retirer |
| `src/lib/components/Canvas.svelte` | `bgObsSource` derived + pastille + gate morphable |
| `src-tauri/resources/diffusion.html` | `renderBg` : `!rel \|\| scene.bgObsSource` → chemin transparent |

## Alertes multi-plateformes — follow/raid/sub/resub/subgift/bits/gift/superchat (2026-09-12)

> Architecture inspirée de l'ancienne app (RUST_SOS_2026) et réécrite pour la
> base v0 — même pattern que `bandeau.rs`. **Multi-plateformes : les 4 sources
> passent par le même point d'entrée** (`AlertesState::on_event` +
> `EventTwitch`). 8 types : les 6 historiques Twitch + `gift` (objet
> monétaire : TikTok gift, Kick KicksGifted) + `superchat` (don monétaire :
> YouTube SuperChat/SuperSticker).

### Flux

```
Twitch IRC (twitch_chat.rs)
  ├─ USERNOTICE (msg-id: sub|resub|subgift|raid, tags msg-param-*)
  └─ PRIVMSG tag bits=<n> (cheers)
Follows : EventSub WS channel.follow v2 (twitch_eventsub.rs,
push temps réel — plus de poll Helix)
YouTube polling (youtube_chat.rs) : superChatEvent/superStickerEvent
  → superchat, newSponsorEvent → sub, memberMilestoneChatEvent → resub,
  membershipGiftingEvent → subgift
Kick Centrifugo (kick.rs) : SubscriptionEvent → sub,
  GiftedSubscriptionsEvent → subgift, StreamHostEvent → raid,
  KicksGifted → gift
TikTok Webcast (tiktok_chat.rs) : Gift → gift (combos agrégés via
  GiftStreakTracker — 1 alerte en fin de streak), Follow → follow
                    │
                    ▼ moteur : config par type → cooldowns (viewer + global)
                              → file FIFO (max 5) → UNE alerte à la fois
                    ▼ WS chat_tx : alerte-config + alerte-play + alerte-stop
                    ▼ diffusion.html : overlay autonome z 9997 (haut centré,
                      icône emoji + texte template, son via /medias/)
```

### Règles et pièges (leçons de l'ancienne app)

1. **USERNOTICE exige `CAP REQ :twitch.tv/commands`** — avec `tags` seul,
   Twitch n'envoie JAMAIS les USERNOTICE : tout le parsing derrière est du
   code mort (bug "resub sans alerte" en live, corrigé 2026-09-30 — la
   capability est demandée dans `connect_irc` à chaque connexion). Le test
   bouton « Test » masque ce bug (il appelle `on_event` sans passer par
   l'IRC). Le message doit ensuite être **parsé explicitement** dans
   `twitch_chat.rs` — tombé dans un `_ =>` ignoré, aucun raid/sub n'est
   jamais émis (bug "raid sans alerte" ancienne app). La détection de
   commande utilise `irc_command()` (pas `contains` — un message peut
   contenir le mot "USERNOTICE"). msg-id couverts : `sub`, `resub`
   (+ continuations `giftpaidupgrade` / `primepaidupgrade` / `extendsub`),
   `subgift` (+ dons communautaires `submysterygift` / `standardpayforward` /
   `communitypayforward` → destinataire = mass-gift-count ou « la communauté »),
   `raid`. ⚠️ Un resub NON partagé en chat n'émet aucun USERNOTICE — limite
   Twitch : aucun event push (EventSub inclus) ne couvre les renouvellements
   silencieux.
2. **Le follow n'a pas d'event IRC** → EventSub WebSocket `channel.follow` v2
   (`twitch_eventsub.rs`, push temps réel). ⚠️ L'abonnement doit être créé
   dans les 10 s suivant le `session_welcome` (sinon Twitch ferme la socket),
   et 409 Conflict = abonnement identique déjà actif = SUCCÈS (cas
   session_reconnect, où Twitch migre les abonnements). L'ancien fallback
   poll Helix 60s côté frontend (diff snapshots + garde followed_at > boot)
   a été retiré avec l'arrivée d'EventSub (2026-09-26).
3. **Un cheer EST un message** : le PRIVMSG bits continue de circuler dans le
   chat normalement, l'alerte est émise EN PLUS.
4. **Une seule alerte à la fois** : file FIFO (max 5, oldest évincé) + timer
   Rust (`alerte-stop` autoritaire) + fallback timer diffusion (+500ms).
5. **Cooldowns** : par viewer (`type:user_id`, anti-spam) + global (rythme),
   par type, contournés par le bouton Test.
6. **Sons** : importés via `import_son` (mp3/ogg/wav ≤ 10 Mo, magic bytes) dans
   `medias/`, servis par :4321 sur `/medias/`, joués côté diffusion (viewers).
7. **Multi-plateformes (implémenté)** : tous les producteurs appellent
   `AlertesState::on_event(EventTwitch)` — `EventTwitch.plateforme` porte
   l'attribution (`"twitch"|"youtube"|"kick"|"tiktok"`, vide = test manuel)
   et remonte à la racine de la frame `alerte-play` pour la télémétrie.
   Variables template supplémentaires : `{montant}` `{objet}` `{nbObjet}`
   (superchat/gift). La commande Tauri `alerte_declencher` reste dispo pour
   tests/intégrations futures. Payloads Kick non documentés → chaque event
   non-chat loggé à sa 1re occurrence (`[Kick]`) pour ajuster le mapping.

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
  actif → `bpmResoudreCouleurs().p` (même résolution que le bandeau — `.p` =
  extrémité la plus lumineuse du gradient depuis 2026-10-04) ; sinon
  la couleur configurée du type/commande.
- **Import média alerte** : commande `import_alerte_media(kind_attendu)` —
  dialog filtré (Images/Vidéos) → `validate_and_copy_media` (helper commun
  refactoré depuis `pick_and_copy_media` : validation extension + taille +
  magic bytes + copie `medias/`) + garde-fou kind ≠ attendu. Servi par :4321
  sur `/medias/`.
- **Cadre SVG sur les overlays** : `appliquerCadreOverlay(carteEl, idCadre)` +
  `nettoyerCadreOverlay` dans diffusion.html — les cartes des overlays
  autonomes suivent `currentScene.cadreWidget` (le cadre unifié choisi dans
  la modale Cadres — un seul réglage, widgets + bord canvas). Mesure de la carte APRÈS affichage (rAF au play,
  re-mesure au `canplay` vidéo — la hauteur change) + refresh au snapshot si
  l'overlay est visible (changement de cadre à chaud). Clip-path appliqué à la
  carte pour les cadres à forme (cyberpunk/story/hexagon/arrondis). Le bandeau
  premier message reste en couleurs dérivées (pas de SVG — barre pleine largeur).

### Clip de bienvenue — mode spotlight plein écran + favoris (2026-10-05)

> Checkbox « utiliser le squelette de position » (section Général, défaut
> COCHÉ = comportement historique). Décochée → mode spotlight : voile noir
> opaque + présentation centrée à 85 %, indépendante du squelette.

- **Config** : `ConfigGlobale.utiliser_squelette: bool` (`serde(default=true)`
  → rétro-compat des `clips_bienvenue.json` existants) + miroir dans
  `QueueEtat` + flag `utiliser_squelette` dans les frames `welcome-clip-play`
  (queue ET test) et `welcome-clip-config`. Commande
  `welcome_set_utiliser_squelette`.
- **Timer** : `INTRO_SPOTLIGHT_MS = 3000` (welcome.rs) — ajouté au timer
  (`jouer_item` + `tester_clip`) en spotlight : le clip ne démarre qu'après
  l'intro avatar de 3 s, la durée configurée reste inchangée (elle s'applique
  à la présentation complète). Sync avec le `setTimeout(3000)` de
  diffusion.html.
- **Rendu diffusion** (`playWelcomeClip`) : `.welcome-clip-voile` (noir
  opaque plein écran, z 9999 inséré AVANT `welcomeRoot` → la carte reste
  au-dessus, fondu 350 ms via rAF) + `.welcome-clip-root.spot` (centré
  `translate(-50%,-50%)`, `width: 85%`, flex row). Phase `.intro` : avatar
  seul 44cqw sur toute la carte, textes/clip/favoris masqués ; au bout de
  3 s → split : colonne gauche 42 % (avatar + pseudo + message + CTA + bio +
  favoris), colonne droite = titre « STREAMER TWITCH » (dégradé violet
  `#9146ff`, letter-spacing, glow) + clip **16:9 strict** (`aspect-ratio`,
  `align-self: center`, rien en dessous). Transition `flex-basis` 600 ms.
  `welcomeDroiteEl` = la colonne droite entière — c'est elle qu'on affiche/
  masque (squelette : canplay ; spotlight : fin d'intro), pas le wrap vidéo.
- **Favoris du viewer** : `twitch_clips::favoris_channel(login, max)` → GQL
  public `user.channel.home.shelves.streamerShelf` (étagère AUTOHOST =
  streamers mis en avant sur la page d'accueil Twitch, Client-Id web, zéro
  auth, timeout 4 s, jamais fatal → `[]`). Fetch en `tokio::join!` avec la
  résolution MP4 (queue + test), spotlight+Twitch uniquement. Champ
  `favoris` de la frame play : `{login, display_name, avatar, en_live}`
  (max 6) — rendu sous la bio à gauche (avatars ronds + pastille verte si
  live). ⚠️ Test manuel : `tester_clip` n'a que `display_name` → login
  dérivé en lowercase (fetch silencieusement vide si display ≠ login).
- **i18n** : `interactions.bienvenue.utiliserSquelette` +
  `utiliserSqueletteHint` (fr uniquement, fallback convention TRADUCTIONS).

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

### Musique — "!rs <titre>" demandé par un viewer (bibliothèque LOCALE)

> Nouveau module `musique.rs`. Catalogue = fichiers audio importés par le
> streamer dans `medias/musique/` (servis par :4321 via le ServeDir `/medias`
> existant — sous-dossier = zéro changement serveur). **Rien n'est cherché
> en ligne** : le nom du fichier EST le titre demandable (nettoyé : `_` →
> espace). Le contraire de commandes.rs : les pistes durent des minutes →
> file NON bornée à 5 (refus explicite à `file_max`=20, jamais d'éviction
> silencieuse) et PAS de timer de libération — l'avancement est piloté par
> l'event `ended`/`error` de l'`<audio>` de la diffusion, remonté en WS
> upstream `musique-ended {id}` (id vérifié = dedup multi-clients).

- **Hook** : `MusiqueState::on_message(plateforme, pseudo, texte, est_mod)`
  appelé à côté du hook commandes dans les 4 modules chat. Retourne
  `Option<String>` = accusé chat (`✓ ajouté (position N)` / `✗ introuvable`
  / `⏳ cooldown` / `✗ file pleine`) — posté en PRIVMSG par twitch_chat.rs
  (seule plateforme avec canal d'envoi ; `reponses_chat` off = silence).
- **est_mod par plateforme** (`!skip` = mods + streamer) : Twitch = tag
  `badges` contient `moderator/`/`broadcaster/` OU pseudo == login ;
  YouTube = badges déjà normalisés (`moderator`/`broadcaster`) ; Kick =
  `sender.identity.badges[].type` OU `sender.slug` == `KickState.slug_courant()`
  (pub(crate) désormais) ; TikTok = `unique_id` == username suivi (pas de
  badge exposé par PirateTok).
- **Commandes chat** (noms configurables `musique_config.json`, défauts) :
  `!rs` demander (cooldown viewer + `max_par_viewer`=3 + `file_max`) ;
  `!skip` (mod uniquement — silence pour les viewers) ; `!retirer` (sa
  dernière demande en file) ; `!file` (en cours + 3 suivantes).
  **Collision** : `commande_set_config` refuse un nom repris par les
  commandes musique (`noms_reserves`), et inversement.
- **WS** : `musique-play {piste:{id,titre,src,demandeur}, volume}` /
  `musique-stop` / `musique-pause {pause}` / `musique-volume`. La diffusion
  joue UN `<audio>` global (`PREVIEW` → muet) + remplit `.chat-musique`
  (phrase défilante marquee, 2 copies + translateX -50%) dans l'en-tête de
  CHAQUE widget chat — élément SÉPARÉ de `.chat-viewers` (cachée en mode
  bulles, la ligne musique doit rester visible). `wsCourant`/`wsSend` =
  référence stable pour l'upstream (`ws` local à `connect()` est recréé
  aux reconnexions) ; `musique-resync` à l'open → `reemit` rejoue le play
  courant (reload OBS en cours de piste). Au passage : barre viewers
  centrée (`justify-content:center` sur `.chat-viewers`).
- **Dashboard** : event `musique:update` (état complet : config + en_cours
  + file + bibliothèque + progression + cooldowns_actifs — pas de diff).
  Onglet « Musique » de la modale Interactions Live (`MusiqueTab.svelte`,
  composant séparé comme PlanningTab) : encart explicite « catalogue
  local », lecteur Winamp (`LecteurMusique.svelte`, voir ci-dessous), file
  (monter/descendre/retirer/vider), bibliothèque (import multi, écouter
  = preview local `ecouterSon`, tester = play diffusion, supprimer).
- **Progression + cooldowns dans `etat()`** : `en_cours` reste sérialisé
  en `MusiqueItem` (contrat WS inchangé) ; le timing vit dans
  `LectureEnCours` {item, debut, pausee, debut_pause, pause_accum,
  duree_s}. `etat()` expose `progression {position_ms, pausee, duree_s}`
  (SNAPSHOT figé à l'emit — le lecteur l'avance en tick local 250 ms,
  zéro trafic périodique) et `cooldowns_actifs [{pseudo, restant_s}]`
  triés. `pause(bool)` trace `debut_pause`/`pause_accum` ; `reemit()`
  reset le timing (la piste repart à 0 après reload OBS, duree_s
  conservée) ; `on_meta(id, duree_s)` reçoit la durée remontée par
  `loadedmetadata` de l'<audio> diffusion (upstream `musique-meta`, seule
  source fiable — routée dans `server.rs` à côté de `musique-ended`).
- **Équalizer spectre réel** : côté diffusion, une `<audio>` « ombre »
  MUETTE joue le même fichier → `createMediaElementSource` → AnalyserNode
  → gain(0) → destination (l'<audio> AUDIBLE n'entre JAMAIS dans le
  graphe — un AudioContext suspendu couperait le son du stream). Tick
  66 ms → 16 bandes ; upstream `musique-spectrum {id, bands}` ~8 Hz →
  `server.rs` le re-émet en `musique:spectrum` (Tauri) pour le lecteur
  dashboard. Fallback bandes simulées si ctx indisponible ; pause de
  l'ombre synchronisée sur `musique-pause`.
- **Lecteur Winamp dashboard** (`LecteurMusique.svelte`, intégré dans le
  bloc « en cours » de l'onglet Musique — deck complet façon Winamp 2.x) :
  barre de titre dégradé nuit `WINAMP` + **shade cliquable** (tout le
  deck se replie sous la barre, état local), clutterbar `O A I D V`,
  grand compteur LED `mm:ss`, écran LCD vert phosphore (marquee `♪ titre
  — @demandeur [plateforme]`, 2 copies + translateX -50% ; indicateurs
  décoratifs kbps/kHz/stereo — pas de métadonnées bitrate, le fichier
  joue côté diffusion), équalizer 16 bandes (réel via `musique:spectrum`,
  simulé si frames >600 ms) + **peak dots** ambrés (crêtes accrochant la
  bande, retombées ~1,5 s), barre de progression crantée (affichage seul
  — pas de seek en v1), transport ⏮⏯⏹⏭⏏✕≡ (⏮ = `musiqueTester(src)`
  rejoue la piste, ⏏ = `importerMusique()` ; stop garde la confirmation
  de l'onglet via prop `demanderConfirm`), **playlist intégrée** (piste
  courante `1▸` surlignée + file numérotée ↑↓✕ — le bloc « File
  d'attente » de l'onglet a été SUPPRIMÉ, la file vit dans le deck),
  chips `CD pseudo Ns` décrémentés en local.
- **Widget scène `musique`** (`createMusiqueWidget`, 340×140) : rendu dans
  `diffusion.html` (branche `wtype === "musique"`, modèle `goal` — create/
  update/reuse via `widgetMap`, kind `musique`) : barre de titre `WINAMP`
  dégradé nuit + rangée écrans `.mu-lcd` (boîte LED temps `.mu-temps`
  pos/rest à gauche + écran marquee `.mu-defile-wrap` à droite, placeholder
  `♪ —` si rien ne joue) + 16 barres EQ avec **peak dots** (`eqPeaks[]`
  décrémenté dans `eqTick`, affiché si crête > bande) + barre de
  progression crantée ; tick 250 ms partagé démarré à la création.
  **Contrat visuel « skin classic »** (palette canonique : châssis
  #2e2e2e→#1c1c1c + bevel outset, écrans #0a140a/#050805 + inset #444,
  EQ #ff4040→#ffb040→#4aff4a + peak #ff9c40, fill #7dff7d→#2fd32f,
  "Lucida Console") — diffusion.html est la référence, toute retouche se
  reporte aux deux miroirs. Dimensionnement `--mu-w`/`--mu-h` inline.
  Vignette dashboard = `WidgetMusique.svelte` (miroir 1:1 du markup +
  CSS mu-*, tick local 250 ms sur `progression` + `musiqueBands`).
  Option `musiqueMode` : `"player"` (défaut, tout) / `"eq"` (bandes
  seules, classe `mu-eq-only` — titre masqué aussi) — `setMusiqueConfig`,
  carte d'édition.
- **Import** : `musique_importer` — dialog multi-fichiers, nom lisible
  CONSERVÉ (≠ uuid des alertes : c'est le titre demandé), exts
  `mp3/ogg/wav/m4a/flac`, magic bytes (`fLaC`, `ftyp` offset 4), ≤ 50 Mo
  (10 Mo des alertes est trop juste pour la musique), collision → `-2`.
- **Télémétrie** : `musique-play` whitelisté → kind `musique`, plateforme
  `multi` (attribution impossible depuis la frame seule).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/position_overlay.rs` | Source de vérité unifiée : `PositionOverlayConfig` + `PositionOverlayState` (persistée `position_overlay.json`), WS `position-overlay-config` |
| `src-tauri/src/alertes.rs` | Moteur : config 8 types (persistée `alertes_config.json`), file, cooldowns, templates, overlay (lit depuis PositionOverlayState), WS |
| `src-tauri/src/commandes.rs` | Commandes chat `!cmd` : config par commande (persistée `commandes_config.json`), file + cooldowns + timer, hooks chat, WS `commande-config/-play/-stop` |
| `src-tauri/src/musique.rs` | Musique `!rs` : bibliothèque locale `medias/musique/`, file non bornée + cooldowns + max/viewer, hooks chat (accusés Twitch), WS `musique-play/-stop/-pause/-volume`, upstream `musique-ended/-resync/-meta/-spectrum`, `LectureEnCours` (timing progression) + `cooldowns_actifs` dans `etat()` |
| `src/lib/stores/musique.ts` | Store musique : `musiqueEtat` (`musique:update`), `musiqueBands`/`musiqueBandsTs` (`musique:spectrum`), `initMusique`, config/import/contrôles file |
| `src/lib/components/LecteurMusique.svelte` | Lecteur « Winamp 2.x » de l'onglet Musique : barre de titre + shade, compteur LED, LCD marquee + indicateurs, EQ 16 bandes + peak dots (réel/simulé), barre progression, transport ⏮⏯⏹⏭⏏ + volume, playlist intégrée (file), chips cooldowns — tick local 250 ms sur snapshot `progression` — palette « skin classic » de référence |
| `src/lib/components/WidgetMusique.svelte` | Vignette dashboard du widget musique : miroir 1:1 du rendu `diffusion.html` (mu-titre / mu-lcd LED+marquee / mu-eq peaks / mu-posbar crantée), dimensionnement `--mu-w`/`--mu-h`, tick local 250 ms |
| `src/lib/components/MusiqueTab.svelte` | Onglet « Musique » de la modale Interactions Live : lecteur Winamp (file incluse), bibliothèque, réglages |
| `src-tauri/src/twitch_chat.rs` | `irc_command()` + `parse_irc_tags()` + `parse_usernotice()` + tag bits |
| `src-tauri/src/lib.rs` | `AlertesState` + `PositionOverlayState` + commandes `alertes_*` / `alerte_declencher` / `import_son` / `import_alerte_media` / `position_overlay_etat` / `position_overlay_set` |
| `src/lib/stores/positionOverlay.ts` | Store unifié : `positionOverlay`, `initPositionOverlay`, `chargerPositionOverlay`, `sauverPositionOverlay` |
| `src/lib/stores/alertes.ts` | Config store par type (événements gérés côté Rust — IRC + EventSub Twitch, polling YouTube, Centrifugo Kick, Webcast TikTok) — overlay retiré (géré par positionOverlay) |
| `src/lib/stores/commandes.ts` | Store commandes chat : `commandesStore`, `chargerCommandes`, `sauverCommande` (upsert), `supprimerCommande`, `testerCommande` |
| `src/lib/components/InteractionViewerModal.svelte` | Modale Interactions Live — 9 onglets : Clip de bienvenue / Bandeau premier message / Alertes / Commandes / Pad numérique / Présentation / Planning / Musique / Squelette de position |
| `src-tauri/resources/diffusion.html` | Overlay welcome (z 9999) + overlay alerte (z 9997) + overlay commande (z 9996) + handler `position-overlay-config` (applique aux trois) |

**NB organisation UI** : le panneau Alertes vit dans la modale **Interactions
Live** (ex "Interactions chat", ex "Interaction viewer") — 9 onglets : Clip de
bienvenue / Bandeau premier message / Alertes / Commandes / Pad numérique /
Présentation / Planning / Musique / Squelette de position. La section
"Alertes" de la sidebar Toolbar a été supprimée (tout passe par la modale).
La section sidebar "Interactions chat" est un **lanceur direct** (pas un
accordéon) : le clic ouvre la modale — seule section non dépliable de la
sidebar.

## Input Viewer — capture globale clavier + souris + manette (2026-10-03)

> Port du concept "Input Viewer" de RUST_SOS_2026 (E:\SteamOs - All Version\RUST_SOS_2026v0.1,
> features/input_viewer.rs + capture_clavier.rs). Widget de scène qui affiche
> les entrées en temps réel côté diffusion. Modes : **clavier** (AZERTY/QWERTY),
> **souris**, **numpad**, **manette** (gilrs → mapping standard Gamepad).

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
  - **Auto-stop (2026-10-03)** : la capture est GLOBALE (pas liée à la
    scène). `syncInputViewerCapture()` (scene.ts) coupe la capture quand la
    scène chargée n'a AUCUN widget `input-viewer` — appelé dans `loadScene`
    (couvre openScene/createScene/deleteScene/duplicateScene/importScene/
    presets) et `deleteWidget` (suppression du dernier widget). Jamais
    d'auto-START : le bouton ON/OFF reste le choix de l'utilisateur.
- **Widget de scène** `type: "input-viewer"` : champs `inputMode`
  ("clavier"|"souris"|"numpad"|"manette"), `inputLayout`
  ("azerty"|"qwerty"), `inputSkin` ("xbox"|"ps"|"8bitdo", manette seulement),
  `inputCouleur` (couleur presse, défaut #8b5cf6), `inputMapping` (remap
  boutons manette + inversion D-pad). Créé via Toolbar →
  `createInputViewerWidget()` (500×220). Placeholder dashboard « Input
  viewer ». Cadre SVG automatique (applyCadreWidget).
- **Diffusion** (`diffusion.html`) : branche renderScene `input-viewer` —
  markup généré UNE FOIS (recréé si `ivKey` = mode|layout|skin change), maj
  d'état par `classList.toggle('presse')` sur les éléments qui changent
  seulement (union ancien/nouveau). WS `input-viewer-etat` →
  `majInputViewerEtat` (touches `[data-touche]`/`[data-touche-alt]`, souris
  `[data-souris]` + X/Y, manette `[data-bouton]`/`[data-dpad]`/`[data-stick-indicateur]`).

### Skins input-viewer — SVG réaliste (2026-10-03)

> Toutes les manettes et la souris sont des **SVG inline** générés par
> `ivGenererManette8BitDoSVG` / `ivGenererManetteXboxSVG` /
> `ivGenererManettePsSVG` / `ivGenererSourisHTML` ; clavier et numpad gardent
> leurs touches DOM (layout dynamique) posées sur un châssis SVG
> (`ivChassisHTML` : plaque + plateau encaissé + câble + LEDs CAPS/NUM/SCRL
> qui s'allument via `data-touche`). Ancien markup DOM de la manette
> supprimé (boutons verre génériques + labels `::after`).

- **Contrat pressé intact** : `.presse` est togglée sur des `<g>` SVG
  (`classList` fonctionne sur SVG) ; les fills de base sont des gradients
  `fill="url(#ivg*)"`, la règle CSS `.presse` les écrase (attribut de
  présentation < CSS). Glow = `filter: drop-shadow` (pas de
  outline/box-shadow en SVG). Labels = `<text>` (`::after` ne marche pas en
  SVG) ; couleurs par classes `iv-fig-lbl-*` (Xbox lettres / PS symboles).
- **Sticks SVG** : `<g class="iv-fig-stick" data-stick-indicateur data-bouton>`
  — le cap bouge (axes) ET s'illumine au clic L3/R3. Branche dédiée dans
  `majInputViewerEtat` : `translate(px)` en unités viewBox (le `calc(-50%+%)`
  DOM ne s'applique pas aux éléments SVG).
- **Boutons bonus** : home=`data-bouton="16"`, touchpad PS=`17`.
- **Responsive** : `viewBox` + `preserveAspectRatio="xMidYMid meet"` partout
  (sauf le châssis clavier, volontairement étiré `none`).
- **8BitDo fallback** : `data-skin="8bitdo"` sur `.iv-manette` → si D-pad
  inactif, axes LeftStick lus comme D-pad (émulation DualShock4).

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
| `src-tauri/src/scene.rs` | Champs Widget `inputMode` / `inputLayout` / `inputSkin` / `inputCouleur` / `inputMapping` |
| `src/lib/stores/scene.ts` | `createInputViewerWidget()` + `setInputViewerConfig()` |
| `src/lib/components/CarteEdition.svelte` | Options input-viewer (mode/layout/skin/couleur + ON/OFF capture) |
| `src/lib/components/Widget.svelte` | Placeholder « Input viewer » (dashboard = config, pas un player) |
| `src-tauri/resources/diffusion.html` | 6 rendus : clavier/numpad (touches DOM + châssis SVG `ivChassisHTML`), souris SVG, manettes SVG (8bitdo/xbox/ps) + maj DOM directe (WS `input-viewer-etat`) |

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
| `--message-ok-color` | `#22c55e` (vert) | Succès / état connecté | Toolbar `.obs-status.ok` + `.header.etat-ok` (en-tête section OBS connecté : texte vert), `.dot.on` partagée (bandeau top App.svelte + lignes Connexions) |
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
  couleur EFFECTIVE du cadre unifié (`cadreWidget` — source unique)
  dans `--cadre-couleur` / `--cadre-couleur-fin` sur `:root`, résolue via
  `gradientCadre()` de `lib/cadres/registre` — le même resolver que le
  rendu : TOUS les styles → `couleur`/`couleurFin` (les tons internes des
  presets sont dérivés de ces couleurs ; `variante` est legacy, désérialisé
  mais ignoré).
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
  liseré rouge `inset 3px 0 0` à gauche. Chaque `.section` porte aussi une
  classe permanente `.sec-*` (couleurs → crochet, cf. « Crochets colorés »).
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

### Crochets colorés — sections + modales (2026-10-04)

> Le « crochet » = liseré vertical de 3px `inset 3px 0 0` sur le bord gauche.
> Présent EN PERMANENCE sur chaque section de la sidebar et sur le flanc
> gauche de chaque modale — couleur = celle du bouton/section d'origine.

- **Sections** (`Toolbar.svelte`) : chaque `.section` porte une classe
  PERMANENTE `.sec-*` (`sec-widgets`, `sec-connexions`, `sec-interactions`,
  `sec-attente`, `sec-jc`, `sec-moderation`, `sec-diagnostic`, `sec-obs`,
  `sec-perso`, `sec-ergo`) qui pose DEUX variables
  indépendamment de l'ouverture :
  - `--accent-carte` : teinte carte SOMBRE (`--carte-*` 950/700 — fond +
    bordure pointillée de la carte ouverte, `.cible-*` reste conditionnel) ;
  - `--accent-crochet` : teinte VIVE (`--crochet-*` niveaux 400-500,
    glace.css) — la barre `.header` l'utilise pleine teinte
    (`box-shadow: inset 3px 0 0 var(--accent-crochet, var(--accent-ui))`).
  - Hiérarchie au survol : `.header:hover` repasse la barre en `--accent-ui`
    + glow ; `.etat-erreur` (OBS non connecté) la repasse en
    `--message-error-color` — le rouge sémantique prime toujours.
- **Modales** (`Modal.svelte`) : prop `crochet` (couleur CSS, défaut
  `var(--accent-ui)`) → `--crochet-modal` sur `.modal-shell`, empilé dans le
  box-shadow après le glow existant (`inset 3px 0 0`, suit le
  border-radius). Chaque modale passe la couleur de son bouton lanceur :
  Interactions `rose`, Écran d'attente `cyan`, Modération `indigo`,
  Diagnostic `teal`, Cadres `violet`, ObsSource `dore`, Device `vert`,
  widgets (suppr/titre/speedrun) `--message-error-color` (carte widget
  rouge), Titre du fond `orange`, InputBinding `jaune`, scènes (presets/
  dupliquer) `--accent-rose` saumon.
- **Tokens** (`tokens.css`) : les teintes vives dédiées sont
  `--t-bleu-500/emeraude-500/jaune-500/ardoise-400/rose-500/cyan-500/
  indigo-500/teal-500` ; violet/ambre/orange réutilisent la palette. Ne
  JAMAIS utiliser les `--carte-*` (950/700) pour une barre de 3px —
  invisibles sur `--fond-panneau`.

### Pastille de message centré — recette `.msg-pill` (2026-09-19)

> Badge centré sur un widget ou le fond du canvas (lecture OBS, input
> viewer). Source unique : `src/app.css`. **Ne jamais re-copier le bloc**
> dans un composant — la triple duplication (`.lecture-obs` /
> `.input-viewer-obs` / `.lecture-obs-fond`) a été unifiée.

- `.msg-pill` : conteneur (centrage `translate(-50%,-50%)`, fond `--fond` 82%, `--rayon-petit`, `font-size: clamp(0.5rem, 9cqw, 1.25rem)`).
- `.msg-pill-text` : sur le `<span>` texte — 4 lignes max + ellipsis (`-webkit-line-clamp:4` ; `min-width:0` requis pour le shrink flex).
- `.msg-pill-icon` : icône ▶ à `1.08em` (suit le clamp cqw).
- **Prérequis cqw** : un ancêtre `container-type: inline-size` (`.widget-3d`, `.bg-layer`, `.titre-zone`), sinon fallback % viewport.
- Couleur de texte : `--texte` par défaut ; les badges « rendu dans OBS » (`.lecture-obs` widget vidéo, `.lecture-obs-fond` fond canvas, `.input-viewer-obs`) partagent `--message-user-action-color` (orange) via une règle globale app.css — un seul message = une seule couleur, lisible quel que soit le cadre (2026-10-04 : remplace l'ancien `color:` inline = couleur du cadre, illisible quand l'utilisateur choisissait une couleur sombre — NE PAS réintroduire).

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

## Titres multi-positions — widget + fond (2026-09-19, refondu 2026-09-28)

> Jusqu'à 4 titres SIMULTANÉS par cible (widget / fond), style partagé.
> Latéraux (gauche/droite) : texte VERTICAL (`writing-mode: vertical-rl`).
> Vocabulaire unifié « haut/bas/gauche/droite » pour les deux cibles :
> widget = HORS du widget (au-dessus/dessous/à côté), fond = INTÉRIEUR
> le long du bord correspondant.

### Champs (tauri.ts + scene.rs — contrat JSON direct, camelCase)

- Widget : `titreHaut` / `titreBas` / `titreGauche` / `titreDroite`
  (`Option<String>`, vide/None = aucun titre à cette position) + style
  partagé `titrePolice` (id Google Font, fonts.ts — woff2 embarqués via
  `server.rs`), `titreTaille` (12–72 px, défaut 24),
  `titreGras`/`titreItalique`/`titreSouligne`, `titreEspacement`
  (−2 à 20 px), `titreContourCouleur` (hex, défaut `"#fff"`),
  `titreContourEpaisseur` (0–6 px, 0 = aucun contour, défaut 2),
  `titreCouleur`/`titreCouleurFin` (hex — dégradé du texte personnalisé ;
  absents = auto, dégradé repris du cadre ; `titreCouleur` seule = couleur
  unie).
- Fond : miroirs `bgTitreHaut/Bas/Gauche/Droite` + `bgTitre*` style
  (`bgTitreCouleur`/`bgTitreCouleurFin` idem).
- **LEGACY** : `titre`/`titrePosition` (« dessus »/« dessous ») et
  `bgTitre`/`bgTitrePosition` (« haut »/« bas ») — désérialisés puis migrés
  par `Scene::migrer_titres()` (pattern `migrer_cadres`) aux mêmes 4 sites
  que la migration cadres (load_scene_file, migrer_ancien_config, importer,
  presets::instancier) : « dessus »→titreHaut, « dessous »/« bas »→Bas, un
  champ nouveau déjà présent gagne, legacy vidé dans tous les cas (le JSON
  se nettoie à la prochaine sauvegarde). Jamais écrits par l'UI.

### Édition (dashboard)

Double-clic sur un widget : bord le plus proche du clic (relX/relY, min des
4 distances haut/bas/gauche/droite) → `titreModalOpen` (ui.ts — porte
`{ id, position }`) → `TitreModal.svelte` → `TitreEditForm.svelte` (corps
partagé widget + fond : 4 champs texte + style + 2 pickers de dégradé
(perso/auto-cadre) + aperçu mini-canvas avec les textes ancrés à leur
position) → `setWidgetTitre` (scene.ts). Zones
`.titre-zone` : haut/bas 25% pleine largeur, gauche/droite 22% × 50%
centrées — affichées au `:hover` du widget sélectionné, avec ou sans titre
existant. Fond : bouton « Titre du fond » (CarteEdition) →
`TitreFondModal.svelte` (même TitreEditForm) → `setBgTitre`.

### Rendu

- Dashboard : `.widget-titre.haut/.bas/.gauche/.droite` hors `.widget-3d`
  (non clippés par le cadre SVG) — haut/bas `bottom/top:100%` centrés
  horizontalement, gauche/droite `right/left:100%` centrés verticalement +
  vertical-rl. `Canvas.svelte` `.bg-titre.*` (intérieur, 8 px des bords).
  Gradient de texte = `titreCouleur`/`bgTitreCouleur` définis → dégradé
  personnalisé (`*CouleurFin` absente → couleur unie) ; sinon couleurs du
  cadre actif (`gradientCadre`, fallback blanc→gris ; `sansCadre` →
  fallback). Priorité : perso > cadre > fallback.
- Diffusion : `applyTitreWidget` (entry.titreEls par position —
  create/update/remove selon les 4 champs) / `applyTitreFond` (bgTitreEls,
  `.bg-titre` classe). Mêmes formules inline (top/left/transform/marges)
  + writing-mode vertical-rl pour les latéraux.
- **Contour des lettres** : `-webkit-text-stroke: <N>px <couleur>` réglable
  (partagé par les 4 titres) — inline dans les templates Svelte / via
  `el.style.webkitTextStroke` dans les apply* de diffusion.html.
  `paint-order: stroke fill` reste fixé dans les règles CSS (le contour est
  peint SOUS le gradient — sans paint-order il mange les glyphes).
- `scene_vierge` (scenes.rs) : couvre les 4 `bgTitre*` + legacy `bgTitre`.

## Cadres SVG — règle absolue (NE PAS REGRESSER)

> ⚠️ **RÈGLE FIGÉE — ne jamais modifier ce comportement sans accord explicite.**
> Cette section documente un fix définitif (2026-08-31) après plusieurs
> régressions. Tout changement ici casse l'affichage côté diffusion/OBS.

### Principe

Les cadres SVG (`scene.cadreWidget`) s'appliquent à **TOUS les widgets**, sans
exception : médias, chat, welcome-clip, et **widgets troués**. Aucun type de
widget ne doit être exclu du cadre.

**Cadre unifié (2026-09-26) — UN SEUL système, deux rendus.** Une seule
`CadreConfig` (`scene.cadreWidget`, champ canonique) alimente EN MÊME TEMPS
le cadre de chaque widget ET le cadre du bord extérieur du canvas
(`Canvas.svelte` `.cadre-app-overlay`, diffusion `applyCadreApp` →
`#cadre-app-overlay` — les deux lisent `cadreWidget`). `CadresModal` n'a
plus d'onglets : un « Cadre activé », un writer unique `mettreAJourCadre`
(stores/scene.ts — écrit `cadreWidget` et purge `cadreApp`). Le champ
`scene.cadreApp` est LEGACY : conservé pour désérialiser les vieilles
scènes, migré vers `cadreWidget` par `Scene::migrer_cadres()` à la lecture
(load_scene_file, importer, migrer_ancien_config, presets::instancier —
cadreWidget actif gagne, sinon cadreApp repris même inactif, cadreApp vidé
dans tous les cas). Jamais écrit par l'UI ; ne jamais réintroduire une
seconde config divergente.

**Portée du bord (`bordActif`)** : `CadreConfig.bordActif` (bool, défaut
`true` — `#[serde(default = "defaut_vrai")]` côté Rust, optionnel côté TS)
masque UNIQUEMENT l'overlay du bord extérieur (`Canvas.svelte` gate
`cadreBordActif`, `applyCadreApp` → `bordActif !== false`) ; le cadre des
widgets reste actif. Ce n'est PAS une seconde config : même CadreConfig,
juste une cible de rendu masquable. Vieilles scènes sans le champ → `true`
(rétro-compat). Toggles : `CadresModal` (« Bord de l'app », désactivé si
cadre inactif) + carte fond de `CarteEdition`.

**Animation (`animer`, 2026-10-05)** : `CadreConfig.animer` (bool,
`#[serde(default)]` → false, opt-in via le toggle « Animation » de
`CadresModal`). Quand true, chaque renderer émet UNE animation signature en
SMIL **sur le tracé du bord** — jamais dans la marge extérieure : la marge
est rognée par `overflow:hidden` + `clip-path` du parent sur les widgets
(.widget / .widget-3d / .apercu de la modale) et hors canvas pour le bord
de l'app → une anim en marge est invisible (leçon 2026-10-05 : halo diode
mgstyle et étincelles story ne s'affichaient nulle part). Signatures :
cyberpunk = runner lumineux, hexagon = nœud voyageur — les deux en
`animateMotion` avec attribut `path=` direct (PAS de `mpath`/`xlink:href` :
non résolus par les vieux CEF d'OBS → runner figé) + grappes d'alvéoles
rotatives (`animateTransform`, décor marge), story = étincelle voyageuse
(`animateMotion` sur borderPath), thin-line/mgstyle/carre/arrondis =
segment comète (`stroke-dashoffset` sur `pathLength="100"`). Jamais de
filtre animé. `animer` est propagé :
`CadreOpts.animer` → `genererSVG`/`genererCadreSVG` (les 3 sites : TS +
diffusion.html + chat-popout.html), `cadreKey`/`majEtatCadre` l'incluent
(re-render au toggle). EXCEPTION perf : les bulles chat forcent
`animer=false` (`decorerCadreMsg`/`decorerCadreMsgDims` — ~200 SVG animés
par widget hors budget).

**Cadre par défaut** (`CadreConfig::defaut_scene`, scene.rs — MIROIR de
`CADRE_WIDGET_DEFAUT`, scene.ts) : MGStyle, noir → violet #8b5cf6, épaisseur
50, actif, bordActif. Posé dès la **naissance** d'une scène (Scene::new —
nouvelle sauvegarde « + Scène », premier boot, fallback scène illisible) →
cadre actif sur widgets + bord dès le départ. Les sauvegardes existantes ne
sont PAS touchées (`#[serde(default)]` → cadreWidget reste None au load) ;
`sceneAvecWidget` (scene.ts) le pose encore au 1er widget créé sur ces
vieilles scènes (`??` — un cadre explicitement désactivé n'est jamais
réactivé). ⚠️ `scene_vierge` (scenes.rs — garde onboarding) traite le
défaut auto comme « pas de contenu utilisateur » : `cadreWidget ==
defaut_scene()` compte vierge, toute autre valeur = choix utilisateur →
pas vierge. CadreConfig dérive PartialEq pour ce test.

**Opt-out par widget** (`w.sansCadre`, 2026-09-19) : les widgets créés par
drop dans la zone Widgets (`ajouterALaBibliotheque` → `poserMediaDepuisBibliotheque`,
auto-pose — l'entrée reste dans `bibliothequeMedias`) portent `sansCadre: true`
— le cadre de scène ne s'applique pas à EUX (clip-path + overlay absents, trou
éventuel rectangulaire, titre en gradient fallback). Le bouton « Transformer
en widget » (`transformerEnWidget`) repasse `sansCadre` à false sur le widget
déjà posé (pas de doublon) et consomme la ligne. `setWidgetSansCadre(id, v)`
est le setter générique exposé dans la carte d'édition pour TOUS les types
(toggle « cadre sur ce widget », caméra incluse — réactiver consomme aussi
l'entrée bibliothèque si le widget porte un média droppé). Flag par instance
sérialisé dans la scène, pas par type : la règle ci-dessus reste vraie pour
tous les autres widgets.

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

**Invoker Boa pré-compilé (perf, 2026-09-22)** : `call_method` n'interpole
plus les JSON dans un wrapper JS re-`ctx.eval`-ué à chaque appel (2-6
parse+compile/tick à 15Hz). `globalThis.__asl_invoke(name, derefOn,
timerJson, oldJson, currentJson, procJson|null, settingsJson)` est installé
UNE FOIS dans `ScriptContext::new()` et appelé via `JsObject::call` avec les
JSON en vraies chaînes JS (plus d'échappement de quotes — args natifs, pas
du code interpolé). `__asl_vars`/`__asl_settings`/`__deref_callback`
(mutés en place) inchangés.

### Dépendances

- `boa_engine = "0.20"` — moteur JS pur Rust (remplace Roslyn C#). ~5MB.
- `windows` 0.58 features Win32 supplémentaires : `Win32_System_Diagnostics_Debug`
  (ReadProcessMemory), `Win32_System_Diagnostics_ToolHelp` (CreateToolhelp32Snapshot),
  `Win32_System_ProcessStatus` (EnumProcessModulesEx, GetModuleBaseNameW).

### Commandes Tauri (11)

- `speedrun_charger_asl(path)` → charge .asl, retourne settings
- `speedrun_charger_lss(path)` → charge .lss, retourne segments + PB ;
  mémorise la run (slot Rust) pour le resync diffusion (payload WS `lss`)
- `speedrun_demarrer(totalSplits, start, split, reset)` → démarre boucle polling
- `speedrun_arreter()` → arrête boucle
- `speedrun_maj_settings(start, split, reset)` → met à jour settings
- `speedrun_est_actif()` → true si actif
- `speedrun_action_manuelle(action)` → start/split/skip/undo/reset/pause
- `speedrun_aller_split(index)` → clic sur une ligne : arme l'index (timer
  arrêté) ou saute à l'index en retry de segment (run en cours)
- `speedrun_lire_config()` → lit speedrun.json (chemins + settings)
- `speedrun_sauver_config(config)` → sauve speedrun.json

### Events

- Tauri `speedrun:event` → store Svelte dashboard (types: started, split, reset,
  ended, goto, etat, gameTime, time, connected, disconnected, error, stopped,
  loaded, paused, resumed, splitSkipped, splitUndone, settings-list).
- WS `speedrun-etat` → diffusion.html `majSpeedrunEtat` (maj DOM directe,
  pattern input-viewer : textContent/classList, zéro re-render). Sous-types
  spéciaux : `lss` (segments) et `etat` (resync complet émis via `EmitEtat`
  par `reemit_lss` — un reload OBS retrouve index + temps de la run).

### Modèle de temps par segment — référence LiveSplit (2026-10-02)

> Chaque ligne de split affiche `nom | temps | delta`. La référence est
> le **PB du segment** (`pb[i] − pb[i−1]` — les PB du .lss sont CUMULÉS,
> real time puis game time). Si `pb[i−1]` est absent la durée est
> indéterminable → delta/temps vides, jamais le cumul entier. Il n'y a PAS
> de baseline « dernière fois » persistée (`speedrun_baselines.json`
> supprimé — pas de sauvegarde de run).

- **Delta (colonne « +/− », droite)** : écart au PB du segment, format
  LiveSplit `m:ss` sans décimales (`−1:01`, `+0:12`). Actif = live à chaque
  tick (`segmentTime` des events `time`/`etat` = `segment_elapsed()` =
  primary − `seg_start`), figé au split pour les passés, vide pour les non
  atteints / skippés / armés / sans PB. `−x` vert (`--message-ok-color`),
  `+x` rouge (`--message-error-color`) — sur la valeur ET la bordure de la
  ligne (`.seg-ahead`/`.seg-behind`, `.sr-seg-ahead`/`.sr-seg-behind`).
  Pas de hex nouveau ; chrono principal teinté `--sr-grad-from`. Les deux
  `<span>` de temps ont une largeur de colonne (jamais `display:none`).
- **Temps (milieu)** : durée de segment — passé = durée réalisée figée,
  actif = temps vivant (0:00.00 si armé, jamais de signe), non atteint /
  skippé = durée PB du segment (vide si indéterminable).
- **Contrat events** : `split`/`ended` portent `time` = **durée de segment**
  et `cumul` = temps total de run (champ séparé). Les deux consommateurs
  (store `speedrun.ts` + `diffusion.html`) lisent `time` comme durée — un
  oubli = régression visible.
- **Clic sur une ligne** (`speedrun_aller_split`, `.speedrun-segment`
  `pointer-events:auto` + `stopPropagation`, pas de `commitScene`) :
  en NotRunning → **arme** l'index (`armed_index` dans timer.rs, chrono à
  zéro, ne démarre pas — attend le start ASL/manuel ; `Started.index` porte
  l'index armé sinon le frontend reviendrait à 0) ; en Running/Paused/Ended →
  saute à l'index en **retry de segment** : chrono de segment à zéro, TOUS
  les temps de la tentative effacés (`temps_segs` Rust + `splits`/
  `srSplitTimes` frontend — les précédents ne comptent pas comme finis),
  PAS un reset de run.
- **undo** : `seg_start` recule de `d_k` (durée enregistrée dans
  `EngineInner.temps_segs`, passée à `undo_split`) → le segment défait
  reprend là où il en était au lieu de repartir de zéro.
- **`disconnected`** (sortie du jeu) : le timer Rust reset → `reset` émis
  avant `disconnected` — l'affichage suit le timer sur les deux frontaux.

> **Sujet suivant (non implémenté)** : à la sortie du jeu détectée par l'ASL,
> proposer à l'utilisateur de sauver ou non ses splits (nom + dossier) ;
> l'ASL serait chargé par layout (3 layouts → 3 jeux, le switch recharge).

### Disposition « ligne » horizontale (2026-09-29)

> Façon LiveSplit horizontal : champ widget `srLayout` (`"auto"` | `"colonne"`
> | `"ligne"`, Option serde — absent = auto). En **auto**, le splitter bascule
> en ligne quand `hauteur < ~4×srTaillePolice` (même seuil des deux côtés ;
> fallbacks taille = 16 diffusion / 27 dashboard, identiques aux fontSize
> appliqués). En ligne : `.sr-ligne` toggle sur `.widget.speedrun` /
> `.speedrun-widget` → `flex-direction: row`, header/meta/barre masqués,
> splits en scroll horizontal (cellules empilées nom / delta+temps via
> `flex-wrap`, index masqué), timer épinglé à droite. Le seul JS dépendant
> de l'axe est le scroll auto vers le split actif (`scrollTop`↔`scrollLeft`)
> dans `srMajSegmentsVisuel` (diffusion) et le `$effect` de `WidgetSpeedrun`.

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
- Écran d'attente : `ecran-attente` (toggle lancer/arrêter — pastille verte
  `data-plat="attente"` alimentée par `raccourcis:connexions.attente`)
- Just Chatting : `just-chatting` (toggle overlay — pastille verte
  `data-plat="justchatting"` alimentée par `raccourcis:connexions.justchatting`)
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
init/appliquer/masquer), bloc en queue de la section « Ergonomie » de la
Toolbar (show/hide, 4 bords, `<select>` moniteurs, hint borderless —
l'ancienne section accordéon « Raccourcis » a été supprimée, son contenu
migré tel quel dans Ergonomie).

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
- Topbar (App.svelte) : `.output-buttons` (2 boutons texte+icône ▶/● → ■,
  proéminents, avant `.ctrl-buttons`). Vert `--message-ok-color` au repos,
  rouge `--message-error-color` + glow quand `.actif` ; `--btn-tint` local
  (pattern `.speedrun-start`). Busy PAR bouton (`streamBusy`/`recordBusy`),
  `disabled` + grisé neutre quand OBS non connecté. Aucun polling : état =
  stores `obsStreaming`/`obsRecording`.
- Barre raccourcis : icônes `data-id="obs-stream"` / `data-plat="stream"` et
  `data-id="obs-record"` / `data-plat="record"` — la pastille `.dot` existante
  suffit, `setConnexions` itère tous les `[data-plat]` (zéro JS ajouté).
  Payload `raccourcis:connexions` étendu : `stream` / `record` (bools),
  poussés par le même `$effect`/`pushRaccourcisConnexions` d'App.svelte.

## Section Prévisualisation — miniature live + projecteur OBS (2026-XX)

Section **accordéon standard** de la barre latérale (`.sec-preview`,
carte jaune `.cible-preview`, section `"preview"` dans `ui.ts`) : pliée =
tout démonté (l'iframe est détruite → zéro coût, c'est le lazy-loading) ;
ouverte = miniature + sélecteur écran + bouton projecteur. Deux fonctions :

- **Miniature live** : `<iframe src="http://127.0.0.1:4321/?preview">`
  rendue aux dims du canvas scène puis réduite par `transform: scale()`
  dans une boîte `aspect-ratio` (~224 px de large). `pointer-events:none`
  sur l'iframe : le clic monte au bouton → ouvre le projecteur OBS.
  `previewMonIdx` (écran choisi) persisté en `localStorage` ; les moniteurs
  OBS sont relus à l'ouverture de la section ($effect).
- **`?preview` dans `diffusion.html`** : const `PREVIEW`
  (`URLSearchParams`). Tout le son coupé (alertes, welcome, présentation,
  commande, pad, écran d'attente) et vidéos **persistantes figées**
  (widgets média, fond, média-fond — `currentTime` posé pour afficher la
  bonne frame). Le reste reste live (chat, alertes visuelles, JC, écran
  d'attente). Sans le param (source OBS), la page est **inchangée**.
  ⚠️ La miniature ne montre que la couche overlay : caméra et captures
  OBS sont des trous dans la page (rendues par OBS sous la source
  navigateur — le vrai rendu = le projecteur OBS).
- **Projecteur OBS** (équivalent du clic droit OBS → « Ouvrir le
  projecteur (prévisualisation) ») : obs-websocket
  `OpenVideoMixProjector` (`videoMixType: OBS_WEBSOCKET_VIDEO_MIX_TYPE_PREVIEW` — l'enum
  côté websocket est préfixé `OBS_WEBSOCKET_VIDEO_MIX_TYPE_*`, PAS
  `OBS_VIDEOMIX_TYPE_*` → "invalid enum value" sinon),
  `monitorIndex` choisi, **-1 = fenêtré**). Le select des écrans vient de
  `GetMonitorList` — les indices sont **ceux d'OBS**, pas ceux de
  `raccourcis_moniteurs` (Win32). One-shot comme tout `obs.rs` :
  connect → requête → close. Pas de suivi d'ouverture : fermer le
  projecteur se fait dans Windows/OBS. Après `result=true`, la nouvelle
  fenêtre projecteur est remontée au premier plan (poll ~2 s, la fenêtre
  arrive en différé sur le thread UI d'OBS).
- **Fenêtres projecteur = titre + obs64.exe** : les fenêtres Qt d'OBS
  partagent la classe générique `Qt<ver>QWindowIcon` (PAS "OBSProjector")
  → `hwnds_projecteurs()` matche titre contenant "project" + exe
  obs64.exe. `fermer_projecteurs()` envoie WM_CLOSE à chacune.
- **Par défaut au boot = projecteur fermé** : OBS restaure ses projecteurs
  d'une session à l'autre → `obsConnect` appelle `obs_projecteur_fermer`
  à chaque connexion réussie (fermeture active, pas d'option). Non-fatal
  et silencieux.
- Chaînage : `stores/obs.ts` (`obsMoniteurs`, `ouvrirProjecteurPreview`
  — credentials via `obsCreds()`) → `tauri.ts` → commandes
  `obs_projecteur_moniteurs` / `obs_projecteur_preview` /
  `obs_projecteur_fermer` (lib.rs) → `moniteurs_liste` /
  `ouvrir_projecteur_preview` / `fermer_projecteurs` (obs.rs).

## Just Chatting — overlay non-destructif caméra + chat (2026-XX)

> Mode bascule « scene IRL » : en diffusion, seuls la caméra et le(s) chat(s)
> restent visibles, repositionnés sur un layout calculé. **La scène réelle
> n'est JAMAIS mutée** — ni `commitScene`, ni `update_scene`, ni JSON. Pas de
> titre au-dessus de la caméra, pas de zoom fond (écarts assumés vs la
> version React). `scene.cadreWidget` reste actif (règle figée cadres).

### État — runtime, OFF au boot, non persisté

`just_chatting.rs` (pattern `ecran_attente` sans timer ni JSON) :
`JustChattingState { actif: Mutex<bool>, derniere_frame, chat_tx, app }`.
Commandes `just_chatting_etat` / `just_chatting_set_actif` ; frame WS
`just-chatting-etat` `{etat:{actif, paysage, portrait}}` sur `chat_tx` +
event `just-chatting:etat` → dashboard. **Resync** : la diffusion envoie
`just-chatting-resync` au `ws.onopen` → `server.rs` → `reemit()` —
différence volontaire avec `ecran_attente::reemit` : **jamais un no-op**,
une frame `actif:false` est reconstruite si aucune n'a été émise (une
Browser Source connectée avant le premier toggle doit recevoir l'état).

### Formule de layout — unique, en Rust (fractions 0–1)

La frame porte les DEUX variantes (`paysage` + `portrait`) ; la diffusion et
le store choisissent selon `canvasW >= canvasH` puis × canvas réel en px.
Paysage : `m=0.04, g=0.02, h=0.78, y=(1-h)/2, inner=1-2m-g`, caméra
`x=m, w=0.62*inner`, chat `x=m+wCam+g, w=0.38*inner` (même hauteur/centre).
Portrait : mêmes marges 0.04 sur les 4 côtés, empilés, caméra 62 % de la
hauteur utile au-dessus. Plusieurs chats → tous au même rect. Les fractions
du preset embarqué `presets/just-chatting.json` NE SONT PAS utilisées (le
preset reste un modèle de scène distinct).

### Dérivation — deux copies indépendantes, même résultat

- **Diffusion** (`diffusion.html`, `jcDeriverScene` en tête de
  `renderScene`) : copie `{...scene, widgets: caméra+chats aux rects JC}` —
  le sweep `seen` retire le DOM des widgets filtrés, `peindreFond` ne perce
  plus leurs trous, `#sos-cam-clip` suit le rect JC. Aucun rendu dédié.
- **OBS** (`stores/justChatting.ts` → `sceneDerivee`) : copie complète
  identique + **`bgObsSource: undefined`** (sinon la capture plein canvas
  resterait affichée) passée à `scene_sync_captures` via le paramètre
  optionnel `scene: Option<Scene>` de la commande (absent = scène chargée,
  comportement historique). `sync_scene_captures` (contrat unique dans
  `obs_trou.rs`) : caméra `ensure_camera` au rect JC, `SOS-Trou-*`/`SOS-Fond-*`
  absents de la copie → `SetSceneItemEnabled false`. Jamais `RemoveInput`,
  jamais d'écriture (`state.scene` ni disque).

### Toggle (dashboard, non-fatal OBS offline)

- **ON** : `setActif` → store rafraîchi AVANT les syncs →
  `sceneSyncCaptures(copie dérivée)` + `cameraSync(null, rect JC)` +
  `cameraMaskSync(points cadre, jcW, jcH, "remplir", 1)` — le masque PNG
  dépend des DIMS du widget (sync_scene_captures cale le transform, pas le
  masque). Pas de widget caméra → skip cameraSync/masque, masquage OK.
- **OFF** : `syncCapturesOBS()` (scène réelle — réactive trous + fond,
  recale la caméra) + **`syncCameraMask(true)` obligatoire** : le cache
  `cleMasqueCamera` détient encore la clé réelle alors que le PNG OBS est
  aux dims JC → sans `forcer`, le rétablissement serait sauté.
- **Ordre critique au OFF** : `jcActif` doit être à `false` AVANT
  `syncCapturesOBS` — sinon le hook post-sync renverrait la copie dérivée
  et le mode ne se couperait pas dans OBS.

### Hook post-sync captures (`scene.ts` — `registerApresSyncCaptures`)

Enregistré par `initJustChatting`, exécuté à la fin de `syncCapturesOBS`
(openScene/duplicate/preset/boot/reconnexion OBS) **ET** de `commitScene` :
si JC actif → `sceneSyncCaptures(copie dérivée)` re-cache les trous de la
nouvelle scène et recale `SOS-Caméra`/`SOS-Caméra-Fond` au rect JC
(`obsSyncTrous` repositionne les items sans jamais toucher
`sceneItemEnabled`, mais il remet la caméra au rect réel à chaque commit).
**Un hook n'appelle JAMAIS `syncCapturesOBS`** (re-enchaînement → boucle) —
wrapper Tauri direct uniquement. Coût assumé en v1 : un one-shot OBS à
chaque pointerup tant que le mode est actif (documenté, pas optimisé).

### UI

Toolbar : section toggle directe `header output-btn` + `.dot` (état) ;
topbar : pillule `.indicateur-btn` visible si `jcActif`, clic = coupe ;
raccourcis : `data-id="just-chatting"` `data-plat="justchatting"`.
i18n : `toolbar.justChatting` + `toolbar.justChattingTitle` (5 dicts).

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
  `commande` | `welcome` | `bandeau` | `pad` | **`stream-info`** |
  **`event`**.
- **Whitelist frames** : `chat`, `viewers`, `alerte-play`, `commande-play`,
  `welcome-clip-play`, `bandeau-premier-message-play`, `pad-play`,
  `stream-info`. Les frames
  de synchro UI (`*-config`, `*-stop`, `input-viewer-etat` 30 Hz,
  `speedrun-etat`, `position-overlay-config`, `snapshot`) ne sont **PAS**
  enregistrées — ajouter un type = décision documentée dans `collecte.rs`.
- **`stream-info`** : frame émise par `viewers.rs` (Twitch) au changement du
  tuple `(en_live, jeu_id, titre)` — dédupliquée, jamais à chaque count.
  `{"type":"stream-info","plateforme":"twitch","en_live":true,"titre","jeu",
  "jeu_id","debut","tags"}` ; `en_live:false` à la transition live→offline.
  Inerte côté `diffusion.html` (type inconnu → ignoré par le dispatch).
- **`event`** : kind rempli depuis le listener Tauri `chat:event`
  (`collecte.rs` — émis par Twitch avant `alertes.on_event`, pré-filtre =
  vérité terrain). `data` porte `type_alerte`, `nb_viewers` (raids),
  `nb_bits`, `montant`, `objet`, `nb_objet`, `user_id`. ⚠️ Twitch seul
  émet `chat:event` — kick/youtube/tiktok appellent `on_event` directement.
- `titre_stream` / `jeu` / `jeu_id` : estampillés sur **tous** les events
  d'une plateforme tant qu'un contexte live est connu
  (`contextes: HashMap<plateforme, ContexteLive>` dans `TelemetrieInner` —
  insert sur `stream-info en_live:true`, remove sur `en_live:false` et
  `deconnexion`). Rétro-compatible : absents ⇒ non sérialisés.
- Attribution plateforme des overlays : `alerte`→`/plateforme` racine de la
  frame `alerte-play` (portée par `EventTwitch.plateforme` ; fallback
  `twitch` pour les tests manuels à plateforme vide),
  `bandeau`→`viewer.plateforme` (réel, porté par la frame),
  `commande`/`welcome`→`multi` (attribution exacte = hooks explicites dans
  les émetteurs, LOT ultérieur si besoin).

### Architecture

| Fichier | Rôle |
|---|---|
| `src-tauri/src/telemetrie/mod.rs` | `TelemetrieState` (compteurs, sessions courantes, anneau 500 events) + `record()` non bloquant (mpsc non borné → writer) |
| `src-tauri/src/telemetrie/modele.rs` | `EventTelemetrie` schéma v1 + `maintenant_iso()` |
| `src-tauri/src/telemetrie/stockage.rs` | Writer JSONL thread dédié — flush 500 ms / 64 lignes, dégradé mémoire-only si fichier inouvrable |
| `src-tauri/src/telemetrie/collecte.rs` | Whitelist + extraction plateforme/pseudo/data + listeners `*:connecte`/`*:deconnecte` + `chat:event` + subscriber `chat_tx` |
| `src-tauri/src/telemetrie/commands.rs` | `telemetrie_etat` (compteurs + sessions + fichier), `telemetrie_derniers(limit)` (anneau, ≤500), `audience_sessions(depuis_jours)`, `audience_session(session_id)` |
| `src-tauri/src/telemetrie/analyse.rs` | Moteur audience : parse JSONL borné → sessions → série viewers 60 s → segments jeu → détection de pics + causes + agrégats (tests unitaires) |

Init dans `lib.rs` setup : `TelemetrieState::new` + `demarrer` **avant** les
auto-resume (pour capter leurs connexions) — non-fatal si le fichier est
inaccessible. `chat:message`/`viewers:update`/etc. continuent d'aller au
frontend et à :4321 exactement comme avant — le tap est un abonné de plus.

## Diagnostic — modale à onglets (consommation 2026-09-22, audience 2026-10-XX)

> La modale « Diagnostic » (`ConsoModal.svelte` — nom conservé) a deux
> onglets : **« Application »** (l'existant : consommation 1 Hz) et
> **« Audience »** (analyse des pics de viewers, Twitch d'abord).
> ⚠️ **Le module backend s'appelle `conso`, pas `diagnostic`** : un module
> racine nommé `diagnostic` masque le namespace built-in rustc
> `#[diagnostic::*]` que la macro `tauri::command` génère → E0433 sur
> TOUTES les commandes.

### Onglet « Application » — consommation (`conso.rs`)

### Ce qui est mesuré (Windows, échec = 0, jamais de crash)

- **Processus** (snapshot ToolHelp32) : StreamOS, WebView2 dashboard
  (enfants `msedgewebview2` agrégés), OBS, jeu (nom suivi via les events
  speedrun `connected`/`disconnected` du tap `chat_tx`), Système (total
  `GetSystemTimes` + `GlobalMemoryStatusEx`). CPU % du total + RAM Mo.
- **Sous-systèmes** (snapshot threads) : CPU % **d'un cœur** agrégé par
  label. Les threads s'enregistrent au spawn via
  `crate::conso::nommer_thread("Label")` (registre TID→label — pas de
  `SetThreadDescription`). Non enregistrés → bucket
  « App + runtime (chat, WS, serveur) ». Threads nommés actuels :
  Splitter (moteur ASL), Input viewer (manette/clavier/souris),
  Télémétrie (écriture).
- **GPU par processus** : compteurs PDH `\GPU Engine(*)\Utilization
  Percentage` (PdhAddEnglishCounterW — vendor-agnostic Intel/AMD/NVIDIA/
  ARM, WDDM 2.x). Instance `pid_<PID>_...` → somme des moteurs par PID.
  Requête échouée (VM/vieux Windows) → `gpu_pct: null` → « N/A » UI.
- **Compteurs d'activité** : tap passif `chat_tx` (même pattern télémétrie)
  — `chat` msgs/s, `speedrun-etat`/s, `input-viewer-etat`/s.

### Contrats

- **Non énergivore** : 1 snapshot processus + 1 snapshot threads + 1
  collecte PDH par seconde. Historique roulant **300 pts en mémoire**
  (`TAILLE_HISTORIQUE`), jamais sur disque.
- **Commande `conso_etat()` → `ConsoEtat { nb_coeurs, historique }`** :
  le frontend la pole **uniquement** quand la modale est ouverte (store
  `conso.ts` : `demarrerPollingConso`/`arreterPollingConso`, 1 Hz).
- **Init** : `conso::demarrer(chat_tx.clone())` dans `lib.rs` setup
  (après la télémétrie, avant les auto-resume).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/conso.rs` | Échantillonneur 1 Hz, registre threads, PDH GPU, commande `conso_etat`, tests du tap |
| `src/lib/stores/conso.ts` | Store `consoEtat` + polling 1 Hz borné à la modale |
| `src/lib/components/ConsoModal.svelte` | Modale à onglets (`application`/`audience`, pattern `.tabs` d'InteractionViewerModal) : onglet Application = lignes nom + % + RAM/GPU + sparkline SVG (survol = point précis « il y a N s ») — variables thème uniquement |
| `src/lib/stores/ui.ts` | Flag `consoModalOpen` ; lanceur Toolbar « Diagnostic » ; montage `App.svelte` |

### Onglet « Audience » — analyse des pics de viewers (Twitch d'abord)

> Dashboard historique **consultatif**, alimenté par `events.jsonl` —
> zéro polling, re-fetch au changement de période/session uniquement.

**Pipeline** : `telemetrie/analyse.rs` parse le JSONL (borné `depuis_jours`,
lignes cassées ignorées) → reconstruction des sessions par `session_id` →
série viewers rééchantillonnée grille **60 s** (carry-forward, le kind
`viewers` n'étant émis qu'au changement) → segments jeu aux changements de
`jeu_id` → détection des pics → attribution d'une cause.

**Algorithme de pic** : baseline `b(t)` = médiane de l'historique dispo
(robuste aux pics) ; ouverture si `v ≥ max(1.25×b, b+8)` sur ≥2 points,
fermeture si `v < 1.10×b` sur 2 points ; retenu si `delta ≥ 10` ou `≥30 %`.
Causes ordonnées (1re qui matche dans `[t_debut−300s, +120s]`) : `raid`
(avec `nb_viewers` → confiance certaine/probable) → `contenu` (changement
jeu/titre) → `communaute` (≥3 events bits/sub/gift) → `debut_live`
(≤3 min du début) → `organique`. Sessions <5 min ou <2 points ignorées.

**Commandes** : `audience_sessions(depuis_jours) → { periode_jours,
sessions: [SessionResume], agregats }` et `audience_session(session_id) →
{ meta, serie, segments, pics, timeline }`. Agrégats : top jeux/titres/
créneaux (jour_sem×heure), top pics, totaux events pendant les lives.

**Fichiers** : `stores/audience.ts` (résultats + chargement + erreur, pas
de polling), `AudienceTab.svelte` (graphe SVG aire+ligne, bandes jeu,
marqueurs timeline, zones de pics, cartes agrégats, liste de pics).
`tauri.ts` porte les types miroir.

**Limites documentées** : résolution 60 s (pic <1-2 min invisible),
`viewer_count` Twitch approximatif, `chat:event` Twitch-seul (autres
plateformes → état « Twitch uniquement »), aucune valeur fabriquée en
l'absence de données, tags stockés mais pas de stats par tag en v1.

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

## Écran d'attente (BRB) — overlay plein écran + chrono (2026-XX)

> Porté de l'ancienne app (`features/ecran-attente/`), réécrit pour
> l'architecture v0 — **même pattern que `pad_numerique.rs`/`bandeau.rs`** :
> état détenu par Rust, broadcast `chat_tx` → `diffusion.html`. Le chrono est
> une tâche tokio 1 Hz **source de vérité** : diffusion et dashboard ne font
> que refléter `temps_restant`.

### Contrats

- **Config persistée** `ecran_attente.json` : `duree_s` (défaut 600), `titre`
  (**défaut vide** — le wordmark du layout signature fait office de titre),
  `afficher_titre`, `son` (**défaut ON** — consigne utilisateur), `volume`
  (0.0-1.0, défaut 0.5), `media`/`media_kind`/`media_nom`, `source_audio`,
  et trois éléments positionnables par mini-canvas drag/resize dans la
  modale (même mécanique que le « Squelette de position » des
  alertes/welcome — centre x/y en px canvas + échelle) : `minuteur_x/y`
  (défaut 960/540) + `minuteur_echelle` (0.4-3.0), `streamy_x/y` (défaut
  170/180, haut-gauche) + `streamy_echelle` (0.3-2.0, défaut 0.6),
  `logo_x/y` (défaut 960/140, haut-centre) + `logo_echelle` (0.3-2.0).
  **`layout`** : `"signature"` (défaut — marque StreamOS) | `"minimal"`
  (rendu historique), choisi dans la modale ; passé dans la frame WS,
  appliqué live par classe CSS.
- **Frame WS `ecran-attente-etat`** (auto-réparatrice : config + runtime
  complets à chaque tick/transition — un client reconnecté se réaligne à la
  frame suivante). Resync : la diffusion envoie `ecran-attente-resync` au
  `onopen` WS → `server.rs` appelle `ecran_attente::reemit()` (dernière
  frame gardée en mémoire) — couvre les pauses sans tick. **Non enregistrée
  en télémétrie** (frame de synchro UI, whitelist `collecte.rs` inchangée).
- **Commandes Tauri** : `ecran_attente_etat`, `ecran_attente_set_config`,
  `ecran_attente_lancer(host,port,password)`, `ecran_attente_pause_toggle`,
  `ecran_attente_arreter(host,port,password)`, `ecran_attente_importer_media`
  (helper `pick_and_copy_media(app, video_sans_limite)` — **pas de cap de
  taille vidéo pour l'écran d'attente** : `validate_and_copy_media_limite`,
  consigne utilisateur — la vidéo BRB peut être longue ; les images gardent
  10 Mo. Tous les autres imports gardent les caps 10/80 Mo),
  `ecran_attente_lister_sources_audio(host,port,password)`.
- **Event `ecran-attente:etat`** → dashboard (modale + pillule header).
- **Règle Mutex** : un seul `.lock()` par statement (piège non-réentrant).

### Diffusion (`diffusion.html`)

- Overlay `.ecran-attente-root` plein écran **z 9990** : recouvre la scène
  mais reste **SOUS** les overlays éphémères (pad 9995, commandes 9996,
  alertes 9997, bandeau 9998, welcome/voile 9999) — comportement validé.
- Média plein cadre `object-fit:cover` (`<img>` / `<video loop>`) — reconstruit
  UNIQUEMENT si `media|kind` change (signature `eaMediaSig`, sinon la vidéo
  se rechargerait à chaque tick 1 Hz). Son porté par la vidéo
  (`muted = !son`, `volume` appliqués à chaque frame → réglages live).
- Sans média → dégradé animé teinté par les couleurs du cadre widget de la
  scène (`bpmResoudreCouleurs` → `--ea-rgb-prim/sec`). Carte minuteur centrée
  portant le **cadre SVG de la scène** (`appliquerCadreOverlay`,
  `data-overlay="ecran-attente"`), ré-appliqué au snapshot (changement de
  scène en live) ; titre ET chiffres du chrono en dégradé de texte aux mêmes
  couleurs (chiffres nuancés de blanc pour la lisibilité).
- **Layout `signature`** (`.layout-signature` sur la racine — tout le bloc
  `ecran-attente-sig-*` est `display:none` sinon) : 4 **stries néon**
  diagonales aux coins + comète glissante (`ea-comet`), **mascotte Streamy**
  (`/brand/streamy-loading.png`, flottement `ea-streamy-flotte`) avec
  **anneaux orbitaux** rotatifs (`.sig-orbite`, conic + mask annulaire —
  clin d'œil aux `.orb` du dashboard), **wordmark** `/brand/wordmark.png`
  (`mix-blend-mode:screen` → fond noir neutralisé sans retouche) et
  **ligne morse ···−−−···** (9 `<i>` p/t flashés en séquence, cycle 9,5 s).
  Streamy et wordmark sont positionnés par `streamy_x/y/echelle` et
  `logo_x/y/echelle` (left/top = centre + `scale`, comme le minuteur —
  live à chaque frame). Palette fixe cyan→violet→magenta (identité) —
  l'aurora reste teintée cadre. En pause → swap `streamy-offline.png`
  (même `<img>`, src changée dans `majEcranAttente`). Le média importé
  reste le fond (`cover`) sous les éléments de marque. Assets servis par
  `GET /brand/{name}` (server.rs, `include_bytes!` depuis `src/assets/` —
  même pattern que `/fonts/`).
- Fin de chrono : « 0:00 » affiché ~1,5 s puis frame `actif:false` de Rust →
  fade-out + nettoyage. La pause ne fige que le chrono — le média continue.

### Routage audio OBS (porté de l'ancienne app, one-shot)

Au `lancer` : connexion `obs::connect` → source `wasapi_output_capture` choisie
ou créée (« SOS Audio Écran d'attente », idempotent) + `SetInputVolume` +
`SetSceneItemEnabled false` sur les items `desktop|bureau` de la scène SOS.
À l'arrêt (manuel OU fin de chrono — creds mémorisés au lancer) : réactivation
par motif de nom. **Non-fatal** si OBS absent (l'overlay fonctionne quand
même). Scène `SOS` uniquement — jamais les autres scènes de l'utilisateur.

### Dashboard

- **Aucun rendu média** dans le dashboard (consigne utilisateur) : la modale
  `EcranAttenteModal` (Toolbar « Écran d'attente ») montre nom du fichier +
  config (durée, titre, son, volume, picker source audio OBS) + contrôles
  (lancer / pause / arrêter). Commit immédiat par champ via
  `ecran_attente_set_config`.
- Pillule « Attente M:SS » dans le bandeau header quand actif (clic → modale),
  bouton toggle `ecran-attente` dans la barre de raccourcis (pastille verte).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/ecran_attente.rs` | `EcranAttenteState` (config + runtime + timer tokio + frame WS + audio OBS), `reemit`, `lister_sources_audio` |
| `src-tauri/src/lib.rs` | `mod ecran_attente`, `app.manage`, 7 commandes |
| `src-tauri/src/server.rs` | Handler WS `ecran-attente-resync` → `reemit()` + route `GET /brand/{name}` (PNG marque embarqués) |
| `src-tauri/resources/diffusion.html` | Overlay `.ecran-attente-*`, `majEcranAttente`, dispatch + resync |
| `src/lib/stores/ecranAttente.ts` | Store + listener `ecran-attente:etat` + actions (creds OBS des stores `obs.ts`) |
| `src/lib/components/EcranAttenteModal.svelte` | Modale config/contrôle (pas de preview média) |
| `src/lib/stores/ui.ts` | Flag `ecranAttenteModalOpen` |
| `src/lib/components/Toolbar.svelte` | Lanceur « Écran d'attente » |
| `src/App.svelte` | Init, pillule header, dispatch `ecran-attente`, montage modale |
| `src-tauri/resources/raccourcis.html` | Bouton `data-id="ecran-attente"` + `data-plat="attente"` |

## Guide du streamer — livre feuilletable intégré (2026-10-03)

> Guide utilisateur en langage simple, rendu comme un livre ouvert dont
> les feuilles basculent en 3D autour de la reliure (référence : carnet
> Tomb Raider). **Dashboard uniquement** — jamais dans `diffusion.html`
> ni aucune page OBS. Plan complet : `PLAN-GUIDE-STREAMER.md`.

### Lanceur & montage

- **Lanceur du guide** = section LANCEUR DIRECT `.section.sec-guide`
  (`Toolbar.svelte`, icône livre + `toolbar.guide` + chevron — même
  mécanique que Modération / Diagnostic, pas un accordéon). Pied fixe de
  la sidebar **juste au-dessus d'Ergonomie** (sibling de l'aside dans
  `.sidebar-col`, hors zone scrollable). Crochet `--crochet-ardoise` —
  même famille qu'Ergonomie, les deux entrées du bas forment un bloc.
  (Sorti d'Ergonomie le 2026-10-06 : avant, c'était un `.action` dans le
  contenu de l'accordéon.)
- **Section « Ergonomie »** = pied fixe de la sidebar (`Toolbar.svelte`,
  `.section.dash-fond` — sibling de l'aside, `flex-shrink:0`, hors zone
  scrollable, sous le lanceur du guide). Accordéon standard
  (`toggleSection("ergonomie")`, union `Section` de `ui.ts`) regroupant
  le fond du chrome dashboard (ex-« Fond du dashboard », déplacé là) +
  fenêtre de démarrage + barre de raccourcis. Carte **ardoise** à
  l'ouverture : `--carte-ardoise` (glace.css ← `--t-ardoise-700` — niveau
  700 exigé : un 950 serait invisible en bordure pointillée sur
  `--fond-panneau`), variante `.cible-ergo` au même mécanisme que les
  autres `.cible-*`. La `.dot` marque le fond dashboard posé ; surcharge
  `.dash-fond .chevron` (margin-left réduit — sinon trou entre la `.dot`
  poussée à droite par `margin-left:auto` et le chevron).
- Flag `guideLivreOpen` (`ui.ts`) → `{#if $guideLivreOpen}<GuideLivre/>`
  dans `App.svelte`, à la suite des autres modales.

### Mécanique du livre (`GuideLivre.svelte`)

- **Modèle** : `N = ceil(PAGES.length / 2)` feuilles physiques ; feuille
  `i` : recto = `PAGES[2i]`, verso = `PAGES[2i+1]`. État `tournees` =
  feuilles tournées (0..N) ; à t=0 la base gauche = page de garde, à t=N
  la base droite = 4ᵉ de couverture.
- **3D CSS, pas SVG** : `perspective:1600px` sur `.livre-scene`,
  `preserve-3d` sur `.livre` ET `.feuille`, `transform-origin: left
  center` (reliure), `backface-visibility:hidden` (avec `-webkit-`) sur
  les deux faces, flip `rotateY(-180deg)` en 0.65 s. Le SVG ne sert qu'au
  décor (ornements dorés) — les éléments SVG ne supportent pas les
  transforms 3D.
- **Z-index** : pile droite `calc(--n-feuilles - --i)`, pile gauche
  `calc(--i + 1)` — les vars sont posées en style inline par feuille.
- **État** : verrou `enTransition` 680 ms ; `feuilleEnMouvement` reçoit
  seul `will-change`. **Reprise de lecture** :
  `localStorage["sos-guide-page"]` (bornée [0,N], lecture/écriture
  non-fatales).
- **Navigation** : zones tranche gauche/droite (72px, à cheval `-8px` sur
  le bord du livre) — flèche SVG `opacity:0→0.9` au hover + soulèvement
  du coin (`rotateY(-7deg)` recto / `-173deg` verso, classes `survol-*`
  sur `.livre`) ; masquées aux extrémités. Clavier `←`/`→` + `Escape` ;
  clic-overlay pointerdown+pointerup (pattern `Modal.svelte`) ; ✕
  recette `.modal-close`.
- **Sommaire cliquable (2026-10-06)** : bloc `{ type: "sommaire" }` rendu
  depuis `chapitres()` (contenu.ts). `allerA(folio)` → cible
  `ceil((folio-1)/2)` feuilles ; les feuilles intermédiaires tournent **en
  cascade** (`basculer(dir)` toutes les `ECART_CASCADE_MS = 120` ms), verrou
  `enTransition` tenu jusqu'à `(n-1)×120 + DUREE_FLIP_MS` ; `timersCascade`
  annulés dans `onDestroy`. `tourner()` = `basculer` + `armerFinTransition`.
  Entrées = `<button class="sommaire-entree">` (titre + pointillés de
  conduite + folio) → même neutralisation du chrome global que
  `.zone-tourne`. Étiquette `.chapitre-label` (dorée, capitales) au-dessus
  du titre quand la page porte `chapitreCle`.
- **Retour au sommaire (2026-10-06)** : `<button class="retour-sommaire">`
  UNIQUE, posé dans `.livre-scene` (**sibling de `.livre`, HORS du
  contexte 3D des feuilles** — ni transform ni miroir : toujours au
  même endroit, z-index 70), aligné sur le coin haut-gauche de la page
  de gauche. Affiché via `pageGauche = PAGES[2*tournees-1]` : caché à
  t=0 (garde), sur verso orphelin et quand la page de gauche EST le
  sommaire. `allerA(FOLIO_SOMMAIRE)` avec `FOLIO_SOMMAIRE` calculé
  depuis `PAGES.findIndex(id === "sommaire")` — jamais de folio en
  dur. Label = `guide.retourSommaire` (8 locales). Même neutralisation
  du chrome global que `.sommaire-entree`.
- ⚠️ **Piège chrome global des boutons** : `app.css` applique
  `backdrop-filter:blur(6px)` + pseudo-éléments shine/reflet à TOUS les
  `<button>` → les `.zone-tourne` (72px pleine hauteur sur les tranches)
  floutaient le texte des bords de page. Correctif : `backdrop-filter:
  none` + `::before/::after{content:none}` sur `.zone-tourne` (même
  neutralisation que `.header` de Toolbar — réflexe à avoir pour tout
  bouton « zone invisible »).
- **Contenu** : `{#snippet face(pg, folio, cote)}` — couverture
  (`id:"couverture"` = mise en page dédiée), verso orphelin (PAGES
  impair) et bases = pages de garde décoratives. Blocs : `para` /
  `liste` / `etapes` / `note` / `sep`. Titres et lettrine
  (`::first-letter`) en **Caveat** (déjà injectée par `fonts.ts` —
  zéro ajout de font) **sauf ru/zh/ja** : Caveat ne couvre pas le
  cyrillique ni le CJK → `--fonte-titre` sur `.livre` bascule sur une
  police système du pays (Segoe UI / Microsoft YaHei / Yu Gothic —
  map `FONTES_TITRES`). Signature « Ryo Shinkawa » et marqueurs
  d'étapes (chiffres) restent en Caveat. Folios bas-droite (recto) /
  bas-gauche (verso).
- **Palette** : `var(--livre-*)` (glace.css ← tokens `--t-parchemin`,
  `--t-encre`, `--t-cuir`, `--t-dore`, `--t-ruban`…) — artwork interne
  du composant ; le chrome de l'overlay reste sur les rôles existants.
  Zéro hex dans le composant (règle figée Variables CSS UI).

### Contenu (`src/lib/guide/contenu.ts`)

- `BlocGuide` (`para|liste|etapes|note|sep|sommaire`) + `PageGuide { id,
  titreCle, chapitreCle?, blocs }` ; `PAGES[]` = ordre des faces.
  **Textes = clés i18n uniquement** (`guide.pages.<id>.*` +
  `guide.chapitres.*` dans les 8 JSON — fr.json source de vérité).
  Ajouter une page = 1 entrée `PAGES` + les clés ×8.
- **Chapitres** : `chapitreCle` posée UNIQUEMENT sur la première page du
  chapitre → crée l'entrée de sommaire ; `chapitres()` calcule le folio
  (`index + 1`) — **jamais de numéro de page saisi à la main**. Insérer
  une page décale les folios automatiquement.
- **Parcours « principal » (24 faces, 9 chapitres, 2026-10-06)** :
  couverture → sommaire → Comprendre (principe) → Trois réglages
  (premierPas, obs, modeles) → Se repérer (cartes, barreLaterale, scenes)
  → Chat (plateformes, chat) → Widgets & médias (widgets, medias,
  carteEdition) → Cadres (cadres, cadresModale) → Faire vivre le live
  (alertes, bandeau, attente, justChatting) → Passer en direct
  (previsualisation, demarrer) → Pour aller plus loin (plusLoin) → fin.
  L'ordre suit ce qu'un nouveau streamer FAIT, pas l'ordre de la sidebar.
  Garder un nombre PAIR de faces (sinon verso orphelin décoratif).
- **Capacité d'une face** : titre + 1 para + 4-6 puces courtes + 1 note
  (`overflow:hidden`, pas de scroll). Page qui déborde → raccourcir le
  TEXTE de la locale, jamais la mise en page.
- **Marqueurs `**…**`** : rendus `<strong class="mot-cle">` bordeaux par
  `segments()` (split sur `**`). Les traductions doivent conserver le
  même nombre de marqueurs, autour des mots équivalents (jamais autour
  de la ponctuation).
- **Registre figé : jamais de tutoiement**, aucune langue — infinitif /
  impersonnel privilégié pour l'UI, sinon pronom formel (vous / Sie /
  usted / Lei / вы-impératif pluriel / 您 / です・ます調). S'applique aux
  fichiers i18n entiers, pas seulement au guide.
- **Règle de rédaction** : seules des fonctionnalités **vérifiées dans
  le code** sont documentées (pas de sommaire exhaustif, pas de features
  dormantes) — re-grep la feature avant d'écrire ses textes.

### Fichiers

| Fichier | Rôle |
|---|---|
| `src/lib/components/GuideLivre.svelte` | Overlay + mécanique du livre (flip 3D, zones tranche, clavier, reprise localStorage) |
| `src/lib/guide/contenu.ts` | Modèle de données `PAGES` / `BlocGuide` / `PageGuide` / `chapitres()` (sommaire calculé) |
| `src/lib/stores/ui.ts` | `"ergonomie"` dans `Section` + flag `guideLivreOpen` |
| `src/lib/components/Toolbar.svelte` | Lanceur `.sec-guide` (pied fixe) + section Ergonomie + variante `.cible-ergo` |
| `src/App.svelte` | Import + montage `{#if $guideLivreOpen}` |
| `src/theme/tokens.css` | Tokens `--t-*` palette livre + `--t-ardoise-700` |
| `src/theme/glace.css` | Rôles `--livre-*` + `--carte-ardoise` |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `toolbar.ergonomie`/`guide`/`guideTitle` + section `guide` complète |

## Fenêtre principale — écran + mode au démarrage (2026-10-04)

> Section « Ergonomie » de la Toolbar : choix du moniteur de démarrage +
> mode d'affichage (fenêtré / maximisé / fullscreen borderless). Appliqué
> **immédiatement** (la fenêtre saute en direct) ET persisté pour le
> prochain boot — même pattern que `raccourcis_appliquer`.

- **Persistance** : `fenetre.json` (`config.rs::FenetreConfig`) —
  `{ monitor_index, mode, geo? }`. `mode` ∈ `"fenetre"|"maximise"|
  "fullscreen"` (`String` normalisé par `mode_valide`, défaut `"maximise"`
  = comportement historique → migration douce, fichier absent = défauts).
  `geo` = dernière géométrie fenêtrée en **px physiques**
  (`outer_position` + `inner_size` — restaurée par `set_position` +
  `set_size`, pas de dérive de décorations) ; `None` → centrage 1280×800
  (= taille `tauri.conf.json`).
- **Application** (`fenetre.rs::appliquer`) : normalisation AVANT le
  déplacement — `set_fullscreen(false)` + `unmaximize()` (une fenêtre
  maximisée/fullscreen ne se déplace pas par `set_position`), puis pose
  dans le rect du moniteur cible (`maximize()`/`set_fullscreen(true)`
  s'appliquent sur l'écran contenant la fenêtre). Index moniteur hors
  bornes (écran débranché) → primaire, même règle que raccourcis.
  `geo_intersecte` rejete une géométrie sauvée sur un autre écran
  (pointerait hors écran) → repli centrage.
- **Sauvegarde de `geo`** : recapturée par `capturer_geo` (no-op si
  fullscreen/maximisé — jamais de pollution) à trois moments : hook
  `CloseRequested` de la fenêtre main (enregistré dans
  `appliquer_au_boot`), `app_arreter`/`app_redemarrer` (exit()/restart()
  ne déclenchent pas forcément CloseRequested), et chaque
  `fenetre_appliquer` (post-apply si mode fenêtré, pré-apply sinon).
- **Anti-flash boot + splash (2026-10-06)** : `"visible": false` dans
  `tauri.conf.json` → `appliquer_au_boot` (setup) positionne/mode SANS
  `show()`. Le splash `streamos-splash://localhost/` (fenêtre « splash »
  440×280 topmost centrée créée par `splash::demarrer` en tête de setup —
  wallpaper Windows via `SPI_GETDESKWALLPAPER` + mascotte + « Bienvenue
  <pseudo Windows> » traduit via langue.json) couvre le chargement.
  `splash::armer_affichage` montre `main` + ferme le splash à
  max(4 s de splash, event `frontend:pret` émis par App.svelte en fin
  d'onMount) — garde-fou : timeout 8 s supplémentaires si le frontend ne
  répond pas (jamais de fenêtre invisible définitive, splash ≤ ~12 s).
  `show()`/`close()` idempotents (HMR, double appel = no-op). Wallpaper
  indisponible (couleur unie/fichier illisible) → 404 → dégradé CSS,
  jamais bloquant. **Dev** (`tauri::is_dev()`) : le splash devient une
  fenêtre normale maximisée (barre de titre → draggable, resizable, dans
  la barre des tâches) et n'est JAMAIS fermée automatiquement — elle
  reste ouverte à côté du dashboard ; fermeture manuelle par sa croix.
- **Frontend** : sélecteur moniteur **partagé** avec la section
  Raccourcis (`raccourcisMoniteurs` / `monsOptions` — la commande
  `raccourcis_moniteurs` liste les écrans, pas de doublon). 3 boutons
  `.action.small` façon `.bord-row` + `<select>` moniteur + hint i18n.

| Fichier | Rôle |
|---|---|
| `src-tauri/src/fenetre.rs` | Commandes `fenetre_etat`/`fenetre_appliquer`, `appliquer_au_boot`, `sauver_geo`/`capturer_geo`, `geo_intersecte` |
| `src-tauri/src/config.rs` | `FenetreConfig`/`FenetreGeo` + lire/sauver `fenetre.json` |
| `src-tauri/src/lib.rs` | `mod fenetre`/`mod splash`, handler, `appliquer_au_boot` + `splash::demarrer`/`armer_affichage` dans setup, protocole `streamos-splash`, `sauver_geo` dans `app_arreter`/`app_redemarrer` |
| `src-tauri/src/splash.rs` | `demarrer` (fenêtre splash), `repondre` (routes `/`, `/wallpaper`, `/mascotte`), `armer_affichage` (transition splash→main) |
| `src-tauri/resources/splash.html` | Page du splash (placeholders `__USER__`/`__GREETING__`) |
| `src-tauri/tauri.conf.json` | `"visible": false` (anti-flash — show() différé par `splash::armer_affichage` sur `frontend:pret`) |
| `src/lib/tauri.ts` | `ModeFenetre`/`FenetreEtat` + `fenetreEtat`/`fenetreAppliquer` |
| `src/lib/stores/fenetre.ts` | `fenetreMonitorIndex`/`fenetreMode` + `initFenetre`/`appliquerFenetre` |
| `src/lib/components/Toolbar.svelte` | Contrôles écran+mode dans la section Ergonomie |
| `src/App.svelte` | `initFenetre()` au boot + `emit("frontend:pret")` en fin d'onMount |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `toolbar.mode.{fenetre,maximise,fullscreen}` + `toolbar.demarrageHint` |

## Bandeau premier message — teinte cadre + mascotte animée (2026-10-04)

> Correctifs sur l'overlay `.bandeau-pm-*` (z 9998, `diffusion.html` —
> barre pleine largeur bas/haut, couleurs dérivées du cadre widget).

- **Teinte = extrémité LA PLUS LUMINEUSE du gradient cadre** :
  `bpmResoudreCouleurs` compare `couleur`/`couleurFin` via `bpmLuminance`
  (rec.601, 0-255) et choisit la plus lumineuse comme primaire `p`. Raison :
  le cadre défaut StreamOS est **noir→violet** (`#000000`→`#8b5cf6`) —
  prendre `couleur` rendait la barre entière, le badge, le liseré et le
  glow Streamy noirs (invisibles). `.p` sert aussi à la couleur de texte
  des alertes/commandes (même résolution, cf. section alertes) — elles
  gagnent la même lisibilité.
- **Pop d'entrée de la mascotte** (`bpm-streamy-pop`, 0,7 s avec dépassement,
  délai 0,35 s pour surgir après la glisse de la barre) : l'animation
  n'existe que sous `.bandeau-pm-root.visible` — retirer la classe la
  réarme pour le prochain message, zéro JS. ⚠️ `scaleX(-1)` (miroir) doit
  être **répété dans chaque keyframe** — un keyframe `transform` remplace
  le transform statique. Variante `bpm-streamy-pop-haut` inversée pour
  `pos-haut` (Streamy est sous la barre, elle descend ; transform-origin
  50% 0%).
- **Limite connue** : un second premier message pendant que le bandeau est
  déjà visible ne relance PAS le pop (`.visible` déjà posée, la classe
  n'est pas re-appliquée dans `playBandeauPM`).

## Présentation streamer — rideaux + bio + clips + sons (2026-10-05)

> Portage de la feature « streamers favoris » de l'ancienne app React :
> présentation théâtrale d'un streamer (rideaux qui s'ouvrent, bio, infos
> chaîne, jeux récents, clips jouables, voix + musique). Déclenchement
> **manuel uniquement** depuis le dashboard — pas de trigger sur arrivée chat.

### Rust — `src-tauri/src/presentation.rs`

- **`PresentationState`** (tauri::State, `lib.rs`) : `chat_tx` + `favoris`
  (HashMap login→`FavoriConfig`, persisté `presentation_favoris.json` dans
  `config::data_dir`) + `courant` (`Option<PresentationEtat>`, RAM — une
  présentation ne survit pas au redémarrage, pas de resync diffusion).
- **`FavoriConfig`** : `user_id`, `login`, `display_name`, `avatar`,
  `son_voix`, `son_musique` (chemins `medias/...` validés par préfixe +
  existence), `mode` (`"theatrical"` | `"card"` — bascule à chaud via frame
  `presentation-mode` si le streamer est en cours d'affichage).
- **`lancer(user_id, mode, access)`** : `tokio::join!` des 4 fetchs Helix
  indépendants — `broadcaster` (fatal), `channel_info`, `stream_viewers`,
  `liste_clips` (gracieux) → dédup des `game_id` des clips → `game_info`
  (max 4 jeux récents) → frame `presentation-show` + `courant` + refresh
  du cache favori. `ClipInfo.game_id` (`#[serde(default)]`, additif) a été
  ajouté pour la déduction des jeux. `PresentationDonnees.banniere` =
  `offline_image_url` Helix (bannière/entête de chaîne, "" si absente) —
  `BroadcasterInfo.offline_image_url` mappé dans `twitch_helix.rs`.
- **Contrôles** : `fermer` (`presentation-hide`), `clip_jouer` (résolution
  MP4 signée au clic via `twitch_clips::resoudre_mp4` — les URL GQL
  expirent, jamais résolues à l'avance), `clip_stopper`, `audio_jouer` /
  `audio_stopper` (piste `voix`|`musique`, loop pour la musique).
- **12 commandes** `presentation_*` dans `lib.rs` : recherche
  (`search_channels` Helix), favoris CRUD + son + mode, lancer/fermer/
  clip/audio/etat. Émet `presentation:etat` (`{favoris, courant}`) au
  frontend à chaque mutation.

### Frames WS `chat_tx` → diffusion :4321

`presentation-show` (données complètes + `sons`), `presentation-hide`,
`presentation-mode`, `presentation-clip-play` (`{mp4_url,titre,duree_ms}`),
`presentation-clip-stop`, `presentation-audio-play` (`{piste,url,loop}`),
`presentation-audio-stop` (`{piste}` — ou toutes si absent).

### Diffusion (`diffusion.html`, vanilla JS — z-index 10000-10004)

- **Overlay autonome** (theatrical) : `.pres-voile` contient dans l'ordre
  `.pres-fond` (bannière de chaîne `donnees.banniere`, plein écran
  `object-fit:cover` + scrim dégradé `::after` — 1er enfant pour que tout
  le décor reste au-dessus ; masqué si bannière absente), orbes animés,
  `.pres-sol` (plancher), 3 `.pres-spot` (faisceaux clip-path balancés)
  et ~20 `.pres-poussiere` (particules en dérive).
- **Rideaux** `.pres-rideau` : plis larges 88 px + ourlet doré, ouverture
  `translateX(±27 %) scaleX(0.5)` en 1,2 s → les panneaux restent froncés
  en bordure (~220 px/côté), retenus par `.pres-embrasse` dorées (fade
  retardé 0,75 s — le `transition-delay` n'est posé que dans `.open`).
  `.pres-valance` : cantonné festonné permanent en haut (proscenium).
- **Contenu plaqué sur scène** : `.pres-carte` SANS panneau (plus de fond/
  bordure — lisibilité par text-shadows + scrim), 2 colonnes (profil/bio/
  stats/lien chaîne | EN DIRECT + jeu + jeux récents + grille 4 clips),
  max-width 73 % pour rester dans l'ouverture des rideaux.
- **Public** `.pres-public` (z 10003 — devant le contenu, sous le clip) :
  3 rangées de fauteuils bordeaux + têtes silhouettées générées par
  `construirePublic` (~15 % de places vides, balancement `pres-tete-sway`).
- **Mode carte** : `.pres-carte` compacte (colonne) — voile + fondu seuls,
  sans rideaux ni décor ni public.
- **Lecteur clip** : `<video>` MP4 plein écran (z 10004), contrôlé par
  `presentation-clip-play` / `presentation-clip-stop`. Pas d'iframe Twitch.
- **Audio** : `Audio()` voix (once) + musique (loop) servis depuis
  `/medias/...` sur :4321 — même plomberie que les sons alertes/pad.
- Esc/clic voile = fermeture locale uniquement (l'état Rust persiste).

### Dashboard — onglet « Présentation » (`InteractionViewerModal.svelte`)

3 sous-onglets : **Recherche** (`presentation_recherche` → résultats
Helix, ajout favori / présentation directe), **Favoris** (master-detail :
mode, sons voix/musique importés via `import_son` existant, retrait),
**Diffusion** (état courant, clip, stop sons/clip, fermeture). Store
`stores/presentation.ts` (`initPresentation` + `chargerPresentation`
appelés au boot dans `App.svelte`), wrappers `tauri.ts`. i18n
`interactions.presentation.*` + `tabPresentation` dans les 8 locales.

### Limites / hors scope (validé)

- Pas de déclenchement automatique à l'arrivée d'un streamer dans le chat.
- Pas de resync : une présentation lancée avant l'ouverture du navigateur
  source n'est pas rejouée (comportement voulu, état RAM).
- Aucun scope Twitch supplémentaire — tout passe par les helpers Helix
  existants et les scopes déjà en keyring.

## Modules HUD — horloge / musique / objectifs (2026-10-05)

> Couche d'affichage scène-persistée SÉPARÉE des widgets
> (`Scene.modules`, pas un `type` de widget). Trois modules fixes
> (clés `MODULE_IDS`), chacun affichable/cachable/configuré par
> scène, positionné en **px canvas** par drag/resize direct sur le
> canvas dashboard. Rendus au z 9000+ en diffusion (au-dessus de
> tous les widgets) + cadre optionnel réutilisant le gradient du
> cadre de scène (jamais le CadreSVG imposé — le module est une
> carte, pas un widget).

- **`ModuleHud` (scene.rs)** : struct UNIQUE pour les 3 modules —
  `actif/x/y/largeur/hauteur/police/taille/opacite/cadre` communs
  + options spécifiques optionnelles ignorées par les autres
  (`afficherDate/secondes/format12h`, `afficherTemps`,
  `titre`/`lignes` pour objectifs). Patch générique
  `Partial<ModuleHud>` côté frontend. camelCase volontaire —
  contrat JSON direct avec scene.ts.
- **`LigneObjectif`** : une ligne du bloc objectifs =
  `{ id, label?, plateforme?, metrique?, cible?, goalId? }`.
  `goalId` = Creator Goal Twitch lié (valeur lue en direct via le
  frame `goal-twitch`, comme le widget goal) ; sinon ligne libre
  plateforme+métrique+cible manuelle. Ordre persistant
  (monter/descendre dans la modale).
- **Setters** (`stores/scene.ts`) : `setModule(id, patch)` (upsert
  sur `MODULE_DEFAUTS` + commit), `ajouterLigneObjectif`,
  `setLigneObjectif`, `retirerLigneObjectif`, `deplacerLigneObjectif`.
  `moduleCourant(id)` = config effective (défauts si absent).
  `scene_vierge` compte les modules comme du contenu (test
  `scene_vierge_detecte_contenu` couvre les 3 cas).
- **`ModuleHud.svelte`** : carte drag/resize (mêmes 8 poignées et
  règles min 80×36 que l'overlay d'interactions — diviseur de
  `scale` obligatoire, même invariant que les widgets). Clic =
  `modulesModalOpen.set(id)`. Aperçu statique réel (date/heure du
  jour, première piste de la file, lignes objectifs avec valeurs
  live `$valeursGoals`/`$goalsTwitch` + suggestion `goalSuggestion`).
- **`ModulesModal.svelte`** : master-detail 3 modules (badge
  ON/OFF). Champs X/Y/L/H = complément précis du drag/resize.
  Bloc objectifs : éditeur de lignes libres (label, plateforme,
  métrique, cible + Suggérer, réordonner, supprimer) + liaison
  Creator Goal Twitch (même liste `twitch_goals_lister` que la
  CarteEdition goal — relue, zéro persistance, `need_reauth` →
  bouton reconnexion).
- **Diffusion** (`diffusion.html`) : `renderModules` (create/update/
  remove par id dans `#modules-root`, z 9000 — DOM conservé entre
  snapshots, rebuild uniquement au changement de structure) —
  appelé dans `renderScene`. `.module` + `appliquerStyleModule`
  (position/dims/police/`--mod-fs`/`--mod-opacite`/`--mod-accent`
  depuis `bpmResoudreCouleurs`) ; `cadre: true` → `appliquerCadreOverlay`
  (même overlay SVG de cadre que les overlays existants, opt-in).
  Ticks : `assurerTickHorloge` (1 s, auto-arrêt), `assurerTickMusique`
  (250 ms partagé avec les widgets musique — même invariant) ;
  `majModuleMusique` est appelée depuis `majMusiqueWidgets`
  (transitions play/stop, jamais de rebuild DOM) ;
  `majModulesObjectifs` est appelée depuis les sinks `viewers` /
  `communaute` / `goal-twitch` (valeurs patchées in-place via
  `ligneEls`, jamais de re-render).
- **i18n** : `modules.*` + `toolbar.modules*` dans les 8 locales
  (rendu diffusion non traduit — labels objectifs viennent du
  module, `goalTwitchLabel`/`goalMetriqueLabel` restent FR comme
  les widgets goal).

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | `ModuleHud`, `LigneObjectif`, `ScenesModules`, `Scene.modules` (serde default) |
| `src-tauri/src/scenes.rs` | `scene_vierge` compte les modules + test |
| `src/lib/tauri.ts` | `ModuleHud`/`LigneObjectif`/`ModuleId`/`MODULE_IDS`/`ScenesModules` + `Scene.modules` |
| `src/lib/stores/scene.ts` | `moduleDefaut`/`moduleCourant`/`setModule` + setters lignes |
| `src/lib/components/ModuleHud.svelte` | Carte drag/resize + aperçu statique |
| `src/lib/components/ModulesModal.svelte` | Master-detail config (modale Modules) |
| `src/lib/components/Canvas.svelte` | Couche modules après les widgets |
| `src/lib/components/Toolbar.svelte` | Lanceur « Modules » (lanceur direct) |
| `src/lib/stores/ui.ts` | `modulesModalOpen: ModuleId \| null` |
| `src/App.svelte` | Montage `{#if $modulesModalOpen}` |
| `src-tauri/resources/diffusion.html` | CSS `.module`/`.mh-*`/`.mm-*`/`.mo-*` + `renderModules`/`majModule*`/`assurerTick*` |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `modules.*` + `toolbar.modules*` + `commun.hauteur` |
