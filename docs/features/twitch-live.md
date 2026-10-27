# Contrôles Twitch « En direct »

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


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
