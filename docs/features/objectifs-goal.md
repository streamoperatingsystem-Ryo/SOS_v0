# Widgets objectif (goal)

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


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
  dans les 8 locales — fr.json source de vérité.
