# Planning Twitch — onglet Planning

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


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
