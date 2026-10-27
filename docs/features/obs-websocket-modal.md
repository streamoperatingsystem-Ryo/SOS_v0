# Modale « OBS WebSocket » — 3 options

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Modale « OBS WebSocket » — inventaire + 3 options (2026-10-19)

Surface avancée lancée depuis la section OBS de la barre latérale (bouton
« WebSocket et options », `obsWsModalOpen` dans `ui.ts`, crochet doré OBS).
`ObsWebSocketModal.svelte` = 4 blocs : explication protocole (v5, OBS ≥ 28,
one-shots `connect→RPC→close`), état connexion, inventaire des features qui
passent par le canal (état réel via stores — PAS d'interrupteurs : chaque
feature se pilote dans sa propre section), puis 3 vrais toggles.

### Options persistées

- `obsAutoConnect` / `obsFermerProjecteurs` : `localStorage` (même pattern que
  `obsHost`, voir `stores/obs.ts` — `obtenirOptionObsBool`). `bootObs` est
  gaté par `obsAutoConnect` dans `App.svelte`.
- `muter_bureau` : champ `EcranAttenteConfig` (json `ecran_attente.json`,
  default true via `#[serde(default = ...)]`). `demarrer` fait un early
  return après `assurer_source_audio` si false ; `restaurer_audio` reste
  **inconditionnel** (idempotent). Miroir TS : `tauri.ts`.

### Anti-patterns

- Ne JAMAIS transformer les lignes d'inventaire en interrupteurs : décocher
  `sync_trous`, la création `SOS-Diffusion` ou le reorder z-order casse la
  feature elle-même — les toggles structurels sont interdits par design.
- Pas de client OBS persistant : l'inventaire n'a donc pas d'état « branché »
  par opération — seule l'étape `connecter` de la section OBS reflète la
  liaison.
- Badge de statut : `badge(f, $t)` avec `$t` passé depuis le template —
  `get(t)` dans le helper tuerait la réactivité au changement de langue.

Guide : page `websocket` du chapitre 9 renvoie vers la modale
(i18n `guide.pages.websocket` + `obsWs.*` dans les 8 locales).
