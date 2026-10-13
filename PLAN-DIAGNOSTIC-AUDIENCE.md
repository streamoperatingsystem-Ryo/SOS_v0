# Diagnostic — onglet « Audience » : analyse des pics de viewers (Twitch d'abord)

Transformer la modale « Diagnostic » (ConsoModal/conso.rs) en modale à onglets —
« Application » (existant) + « Audience » : reconstruction des sessions live
Twitch depuis `telemetrie/events.jsonl` enrichi par le contexte stream
(titre/jeu/tags) et les événements détaillés (`chat:event`), détection
algorithmique des pics avec attribution de la cause probable, agrégats par
jeu/titre/créneau et cartes résumé. **Consultatif uniquement** — pas
d'injection dans le planning.

## Contexte vérifié dans le code

- `src-tauri/src/conso.rs` — échantillonneur 1 Hz, historique 300 pts mémoire,
  commande `conso_etat`, `conso::demarrer` en setup (lib.rs:3864). Module nommé
  `conso` car `diagnostic` masque le namespace rustc (E0433) — **ne jamais**
  nommer un module `diagnostic`.
- `ConsoModal.svelte` — sparklines SVG + survol, montée par
  `{#if $consoModalOpen}`, `Modal title={$t("consoModal.titre")}`
  (« Diagnostic — Consommation », fr.json:207), polling 1 Hz borné à la
  modale via `stores/conso.ts`.
- `telemetrie/` : `mod.rs` (`TelemetrieState`,
  `sessions: HashMap<plat, session_id>`, `record()` non bloquant mpsc→writer,
  anneau 500), `modele.rs` (`EventTelemetrie` avec
  `titre_stream`/`jeu`/`jeu_id` déjà réservés « LOT 2 »,
  `skip_serializing_if=None` → rétro-compatible, zéro migration),
  `collecte.rs` (whitelist FERMÉE `kind_depuis_frame` + `plateforme_frame` lit
  `/plateforme` racine + `extraire`, listeners `*:connecte`/`*:deconnecte`
  via `app.listen_any`), `commands.rs` (`telemetrie_etat`,
  `telemetrie_derniers`), `stockage.rs` (writer thread, `fichier_events` →
  `config::data_dir/telemetrie/events.jsonl`, flush 500 ms/64 lignes).
- `viewers.rs` — UNE tâche, tick 15 s, Twitch via
  `twitch_helix::stream_viewers` (GET /streams) 60 s live / 180 s offline ;
  frame `{"type":"viewers","plateforme","count"}` dédupée sur count →
  `chat_tx` + `viewers:update`. `Plat{prochain, dernier}` ; pattern
  dedup-par-map identique pour `commus`.
- `twitch_helix.rs:662-679` — `stream_viewers` lit SEUL `viewer_count` ;
  `title`, `game_id`, `game_name`, `started_at`, `tags` sont jetés.
- ⚠️ **`stream_viewers` a 3 appelants** : `viewers.rs:78`, `lib.rs:1718`
  (commande `twitch_communaute_viewers` → `Result<Option<u32>, String>`),
  `presentation.rs:296` (`tokio::join!`, `live.ok().flatten()` → `Option<u32>`).
- `chat:event` — `app.emit` AVANT `alertes.on_event` (twitch_chat.rs:221
  USERNOTICE, :254 bits ; twitch_eventsub.rs:211 follows). `EventTwitch` :
  `type_alerte`, `plateforme` (=« twitch » posé dans les 3 constructeurs),
  `pseudo`, `user_id`, `nb_viewers`, `nb_bits`, `niveau_sub`, `nb_mois`,
  `montant`, `objet`, `nb_objet`. Aucun consommateur frontend. Seul Twitch
  l'émet — cohérent avec « Twitch d'abord ».
- `diffusion.html` + `chat-popout.html` — dispatch `ws.onmessage` = chaîne
  `if/else if` sur `msg.type` : **les types inconnus tombent en silence,
  inertes** → la frame `stream-info` peut passer sur `chat_tx` sans toucher
  la diffusion.
- Onglets : pattern `.tabs`/`.tab` d'`InteractionViewerModal.svelte:814-854` ;
  gros onglets externalisés (`PlanningTab.svelte`, `MusiqueTab.svelte`).
- i18n : 8 locales (`fr` source de vérité). `lib.rs` : `mod telemetrie` (38),
  `use telemetrie::commands::{…}` (54), handler ~4270-4272 ; setup :
  `viewers::demarrer` (3846) puis `TelemetrieState::new`+`demarrer`
  (3852-3858) puis `conso::demarrer` (3864).
- `normaliser_titre` cité dans le commentaire de `modele.rs:37` n'existe pas
  (référence forward) — v1 : `jeu` = `game_name` Helix verbatim.

## Décision

`stream_viewers` change de signature → `Result<Option<StreamInfo>, HelixError>` ;
les 3 appelants sont adaptés (`presentation.rs` + hunk
`twitch_communaute_viewers` dans lib.rs au périmètre LOT 1).

## Contrats de données

### Frame WS `stream-info` (chat_tx, émise au changement du tuple `(en_live, jeu_id, titre)`)

```json
{"type":"stream-info","plateforme":"twitch","en_live":true,"titre":"…","jeu":"…","jeu_id":"…","debut":"<RFC3339 started_at>","tags":["…"]}
```

`en_live:false` (titre/jeu vides) à la transition live→offline ET à la
déconnexion Twitch (perte des creds dans `deconnecte`).

### Events JSONL (schéma v1 inchangé — champs réservés remplis)

- `{"kind":"stream-info","plateforme":"twitch","data":{"en_live","titre","jeu","jeu_id","debut","tags"}}`
  (+ `jeu`/`jeu_id`/`titre_stream` estampillés à la racine).
- `{"kind":"event","plateforme":"twitch","pseudo":"…","data":{"type_alerte","nb_viewers","nb_bits","montant","objet","nb_objet","user_id","niveau_sub","nb_mois"}}`
  (depuis `chat:event`, pré-filtre = vérité terrain ; `kind:"alerte"` reste =
  « jouée à l'écran »).
- Tous les kinds gagnent `jeu`/`jeu_id`/`titre_stream` quand un contexte live
  est connu.

### Commandes Tauri (nouvelles, `async` + `spawn_blocking` pour le parse fichier)

- `audience_sessions(depuis_jours: u32) -> AudienceSessions` :
  `{ periode_jours, sessions: [SessionResume { session_id, debut, fin, duree_s,
  pic, moyenne, jeu_principal, nb_msgs_chat, nb_events_par_type }],
  agregats: Agregats }`
- `audience_session(session_id: String) -> SessionDetail` :
  `{ meta, serie: [Point {ts, count}] (~1 pt/min, carry-forward),
  segments: [Segment {debut, fin, jeu, jeu_id, titre, moyenne, pic}],
  pics: [Pic {t_debut, t_pic, t_fin, v_avant, v_pic, delta, delta_pct, jeu,
  titre, cause, confiance, chat_par_min}],
  timeline: [Evt {ts, genre, detail}] }` (icônes résolues côté Svelte)
- `Agregats` : `top_jeux: [StatJeu { jeu, jeu_id, temps_s, moyenne, pic_max,
  viewer_minutes, nb_segments, nb_pics }]`, `top_titres`,
  `top_creneaux: [{ jour, heure, moyenne, echantillons }]`, `top_pics`,
  `totaux {raids, subs, resubs, subgifts, bits_total, follows, supers, gifts}`
  pendant les lives.
- Nommage **snake_case** partout (convention `ConsoEtat`/`EventTelemetrie`),
  types mirroring dans `tauri.ts`.

## Algorithme (`telemetrie/analyse.rs`, fonctions pures testables)

1. **Parse** : `BufReader` ligne à ligne → `EventTelemetrie` (lignes cassées
   ignorées), filtre `plateforme=="twitch"` + `ts ≥ now−depuis_jours`.
2. **Sessions live** : groupe par `session_id` ; intervalle « en live » =
   plage où le dernier `viewers.count` connu est non-null (carry-forward),
   bornée par `stream-info en_live:false`, `deconnexion`, ou fin des données.
   Garde : `<2 points` ou durée `<5 min` → ignorée des stats.
3. **Série** : rééchantillonnage grille 60 s (dernier count connu ; `debut`
   (started_at) recale le début si antérieur au 1er event).
4. **Segments jeu** : split aux changements de `jeu_id`.
5. **Baseline** `b(t)` = médiane des 15 points précédents (robuste aux pics ;
   <3 points d'historique → baseline = v(t), pas de détection — le début de
   live est couvert par la règle `debut_live`).
6. **Pic** : ouverture `v ≥ max(1.25×b, b+8)` ≥2 points consécutifs ;
   fermeture `v < 1.10×b` 2 points. Retenu si `delta ≥ 10` ou `≥30%` du
   baseline.
7. **Attribution** (fenêtre `[t_debut−300s, t_debut+120s]`, règles ordonnées —
   1re qui matche gagne) : `event type_alerte="raid"` → « Raid de {pseudo} »
   (confiance `certaine` si `nb_viewers ≥ 0.7×delta`, sinon `probable`) →
   changement `jeu`/`titre` → `contenu` → ≥3 events
   `bits`/`sub`/`resub`/`subgift`/`superchat`/`gift` → `communaute` →
   `t_debut ≤ 3 min` après début live → `debut_live` → sinon `organique`.
   `chat_par_min` calculé dans la fenêtre (corroboration).
8. **Agrégats** : par `jeu_id` (viewer-minutes/temps), par `titre`, par
   créneau `jour_sem × heure`, totaux events pendant lives, `top_pics` triés
   par delta.

## Étapes d'implémentation (par LOT — un fichier = un diff = une pause)

### LOT 1 — Collecte enrichie

- `twitch_helix.rs` : `pub struct StreamInfo { viewers:u32, titre:String,
  jeu_id:String, jeu:String, debut:String, tags:Vec<String> }` ;
  `stream_viewers` → `Result<Option<StreamInfo>, HelixError>` (même GET
  /streams, zéro appel en plus).
- `viewers.rs` : branche twitch — map `contextes` à côté de `plats`/`commus` ;
  sur changement → `chat_tx.send` frame `stream-info` ; `Ok(None)` avec
  contexte live → `en_live:false` + clear ; `deconnecte` → idem. `publier`
  reçoit `v.map(|s| s.viewers)`.
- `presentation.rs` : `live.ok().flatten().map(|s| s.viewers)`.
- `lib.rs` : `twitch_communaute_viewers` → `.map(|o| o.map(|s| s.viewers))` ;
  **réordonner le setup** : bloc télémétrie AVANT `viewers::demarrer` —
  sinon la 1re frame `stream-info` peut partir avant la subscription chat_tx
  de la télémétrie → contexte perdu pour toute la session.
- `telemetrie/modele.rs` : doc kinds +`"stream-info"`/`"event"` ; corriger la
  référence morte `normaliser_titre`.
- `telemetrie/mod.rs` : `contextes: HashMap<String, ContexteLive>` dans
  `TelemetrieInner` ; maj sur kind `stream-info` (en_live→insert,
  !en_live→remove) AVANT estampillage + remove à `deconnexion` ;
  estampillage `jeu`/`jeu_id`/`titre_stream` dans `on_frame` (un lock par
  statement — règle mutex non réentrant).
- `telemetrie/collecte.rs` : whitelist `"stream-info"`→kind homonyme ;
  `extraire` arm ; listener `app.listen_any("chat:event")` → parse payload →
  `record` kind `"event"`.
- Validation : cargo check/clippy ; test réel = connecter Twitch en live →
  events.jsonl porte `stream-info` + events estampillés.

### LOT 2 — Moteur + commandes

- `telemetrie/analyse.rs` (nouveau) : algorithme ci-dessus + structs
  Serialize. `mod analyse;` dans `telemetrie/mod.rs`.
- `telemetrie/commands.rs` : `audience_sessions`, `audience_session` —
  `async`, chemin via `stockage::fichier_events(&app)`, parse dans
  `spawn_blocking`. `lib.rs` : `use` + handler.
- Tests `#[cfg(test)]` : pic détecté/ignoré, attribution raid, carry-forward,
  médiane baseline, session <5 min ignorée.
- Validation : `cargo test` + check/clippy.

### LOT 3 — UI onglet « Audience »

- `ConsoModal.svelte` : `let tab = $state("application")` + barre `.tabs`
  (pattern InteractionViewerModal) ; contenu actuel sous
  `{#if tab==="application"}` ; `{#if tab==="audience"}<AudienceTab/>{/if}` ;
  titre → « Diagnostic » ; polling conso piloté par `$effect` sur `tab`.
- `src/lib/components/AudienceTab.svelte` (nouveau) : 2 colonnes
  (graphe ~65 % | cartes ~35 %) ; selects période (7/30/90, défaut 30) +
  session ; graphe SVG (aire+ligne, bandes jeu alternées+labels, marqueurs
  timeline, zones pic surlignées, survol) ; cartes « Top pic », « Meilleure
  catégorie », « Meilleur titre », « Meilleur créneau », « Pendant la
  période » ; liste des pics (clic→highlight) ; états vides/chargement +
  mention « Twitch uniquement ».
- `src/lib/stores/audience.ts` (nouveau) : invoke + stores + chargement ;
  re-fetch au changement période/session (pas de polling).
- `src/lib/tauri.ts` : types mirroring (snake_case) +
  `audienceSessions`/`audienceSession`.
- Validation : `npm run check` 0 erreur + test manuel données réelles.

### LOT 4 — i18n + doc

- `src/lib/i18n/*.json` ×8 : `consoModal.titre`→« Diagnostic »,
  `consoModal.tabApp`/`tabAudience`, section `audience.*` complète (fr
  source, jamais de tutoiement) ; `toolbar.diagnosticTitle` mis à jour.
- `AGENTS.md` : section « Diagnostic consommation » → modale à onglets ;
  nouvelle section « Audience » ; télémétrie : LOT 2 fait, whitelist
  +`stream-info`, listener `chat:event`, contexte live. Commentaire LOT 2
  dans `telemetrie/mod.rs` mis à jour.

## Fichiers à toucher

| Fichier | LOT | Changement |
|---|---|---|
| `src-tauri/src/twitch_helix.rs` | 1 | `StreamInfo`, signature `stream_viewers` |
| `src-tauri/src/viewers.rs` | 1 | frame `stream-info` dedupée (branche twitch + deconnecte) |
| `src-tauri/src/presentation.rs` | 1 | appelant `stream_viewers` adapté |
| `src-tauri/src/lib.rs` | 1+2 | `twitch_communaute_viewers` adaptée ; réordre setup ; `use` + handler |
| `src-tauri/src/telemetrie/modele.rs` | 1 | doc kinds + commentaire `normaliser_titre` |
| `src-tauri/src/telemetrie/mod.rs` | 1+2 | `contextes` + estampillage ; `mod analyse` |
| `src-tauri/src/telemetrie/collecte.rs` | 1 | whitelist + `extraire` + listener `chat:event` |
| `src-tauri/src/telemetrie/analyse.rs` | 2 | **nouveau** — moteur + tests |
| `src-tauri/src/telemetrie/commands.rs` | 2 | 2 commandes |
| `src/lib/components/ConsoModal.svelte` | 3 | onglets + titre |
| `src/lib/components/AudienceTab.svelte` | 3 | **nouveau** |
| `src/lib/stores/audience.ts` | 3 | **nouveau** |
| `src/lib/tauri.ts` | 3 | types + wrappers |
| `src/lib/i18n/*.json` ×8 | 4 | clés |
| `AGENTS.md` + `PLAN-DIAGNOSTIC-AUDIENCE.md` | 0/4 | doc |

## Fichiers à NE PAS toucher

- `diffusion.html` / `chat-popout.html` / pages OBS (frame `stream-info`
  vérifiée inerte — chaîne if/else).
- `scene.rs`/`scene.ts`, widgets, OBS, `alertes.rs` (on lit `chat:event`, on
  ne touche pas l'émetteur), `tiktok_chat.rs`/`youtube_chat.rs`/`kick.rs`
  (hors scope v1), `server.rs`, `stockage.rs` (rotation = LOT ultérieur).
  Aucune dépendance ajoutée.

## Risques & limites (à documenter dans l'UI)

- Résolution 60 s : pic <1-2 min potentiellement invisible (cadence Helix
  figée). `viewer_count` Twitch approximatif.
- Volume JSONL dominé par `chat` — parse streamé et borné par `depuis_jours` ;
  rotation fichier = sujet futur.
- Premier run sans données → états vides explicites, pas de valeurs
  fabriquées.
- `chat:event` Twitch-seul → badge « Twitch uniquement ».
- Mutex non réentrant (règle figée) : contextes lus sous un seul lock par
  statement.

## Décisions par défaut

- Tags capturés dans `stream-info.data.tags`, pas de stats par tag en v1.
- Fenêtre par défaut 30 jours (sélecteur 7/30/90). Onglets « Application » /
  « Audience ».
- `chat:event` listener choisi vs enrichir `alerte-play` (pré-filtre =
  vérité, zéro modif émetteur).

## Critères de fin

`cargo check` + `cargo clippy` exit 0 ; `cargo test` (analyse) vert ;
`npm run check` 0 erreur ; modale Diagnostic à 2 onglets ; events.jsonl porte
jeu/titre ; une session live Twitch réelle affiche série + pics + cartes avec
causes ; AGENTS.md à jour.
