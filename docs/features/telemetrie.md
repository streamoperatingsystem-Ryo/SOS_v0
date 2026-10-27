# Télémétrie multiplateforme — JSONL local

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


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
