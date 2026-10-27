# Sorties OBS — stream & record

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Sorties OBS — stream & record (2026-09-20, set explicite 2026-10-19)

> Boutons « Démarrer/Arrêter » le stream ET l'enregistrement OBS : 2 actions
> dans la section OBS de la Toolbar + 2 icônes topbar + 2 icônes dans la
> barre de raccourcis.
> **La connexion OBS reste one-shot** (connect → RPC → close) comme toutes les
> commandes OBS — aucune connexion persistante, **aucun polling**.
>
> **Start/Stop explicites, JAMAIS ToggleStream/ToggleRecord** (fix
> désynchronisation 2026-10-19) : le libellé affiché vient d'un cache
> (`obsStreaming`/`obsRecording`) alors qu'un toggle agit sur l'état RÉEL
> d'OBS — dès que l'utilisateur touche OBS directement (ou que le stream
> tombe), le bouton promettait l'inverse de ce qu'il faisait (« Démarrer »
> qui coupe le live). Désormais : l'état est RELU à l'ouverture de la
> modale de confirmation (« Oui » grisé pendant la lecture), la direction
> confirmée est envoyée en `Start*`/`Stop*` explicite et idempotent (déjà
> dans l'état voulu → no-op), et le store est alimenté par un re-GET post-
> action. Resync aussi à l'ouverture de la section OBS et au retour de
> focus de la fenêtre — ponctuel, pas un timer.
>
> **Confirmation obligatoire (2026-10-19)** — dans OBS un clic suffit ; dans
> StreamOS NON : démarrer ET arrêter passent par `ConfirmObsOutputModal.svelte`
> (pattern `ConfirmOuvrirSceneModal` : `Modal` 24rem, `.confirm-body` +
> `.modal-actions.confirm-actions`, Enter = Oui / Escape = Non). Le store
> `confirmObsOutputOpen` (ui.ts, `"stream" | "record" | null`) est posé par
> les 3 lanceurs — Toolbar, topbar et barre de raccourcis (la modale s'ouvre
> dans le dashboard). Titre + hint dynamiques selon la sortie ET le sens
> (`confirmObsOutput.titre|hint{Stream,Record}{Start,Stop}`, 8 locales) ;
> crochet + bouton Oui `--accent-ui`/`primary` au démarrage,
> `--message-error-color`/`danger` à l'arrêt. À l'ouverture, la modale
> appelle `rafraichirOutputs()` et grise « Oui » pendant la re-vérification
> (Enter garé aussi — `verif` dans `onConfirm`) : le libellé Start/Stop
> reflète l'état RÉEL relu, pas le cache. Oui → `set*Obs(!actif)` —
> l'intention affichée est passée en paramètre, pas un toggle aveugle.

### Contrats

- `obs.rs` : `set_stream` / `set_record` (`(host, port, password, actif)` —
  helper `set_output` : GetStreamStatus/GetRecordStatus → `Start*`/`Stop*`
  explicite UNIQUEMENT si l'état diffère (idempotent, jamais Toggle*) →
  re-GET → retourne `outputActive` relu ; Start*/Stop* répondent un
  responseData vide, d'où le re-GET) et `outputs_status` (GetStreamStatus +
  GetRecordStatus dans UNE connexion → `(stream, record)`). Réutilisent
  `connect()` + `rpc()` comme `refresh_diffusion`.
- Commandes Tauri : `obs_set_stream` / `obs_set_record` → `bool` (param
  `actif: bool` = intention confirmée dans l'UI), `obs_outputs_status` →
  `{ stream, record }` — signature `(host, port, password)` comme les autres.
- `stores/obs.ts` : `obsStreaming` / `obsRecording` (writable bool),
  `obsStreamBusy` / `obsRecordBusy` (set en vol — grise les lanceurs, garde
  anti-réentrance ET anti-race : `rafraichirOutputs` skip si l'un est true,
  sinon un GET lu avant l'action écraserait l'état retourné par le set),
  `rafraichirOutputs()` (non-fatal, AWAITÉ à la fin d'`obsConnect` réussi +
  appelé à l'ouverture de `ConfirmObsOutputModal`, de la section OBS
  (Toolbar) et au focus de la fenêtre),
  `setStreamObs(actif)` / `setRecordObs(actif)` (le booléen retourné = état
  réel relu, alimente le store ; erreur traduite via `messageClair` →
  `obsError`).
- **PAS de polling** (choix validé) : l'état est relu aux points de
  resync listés + après chaque set. Si l'utilisateur agit directement dans
  OBS, la pastille peut rester fausse entre deux resync — mais l'action
  confirmée est TOUJOURS celle affichée (Start/Stop explicite idempotent),
  jamais l'inverse.
- Toolbar : `.action output-btn` + `<span class="dot">` (vert
  `--message-ok-color` quand actif) + `.output-btn.actif` (bordure/texte
  `--accent-ui`, recette `.action.small.actif`). Clic → `confirmObsOutputOpen`
  (jamais le toggle direct) ; `disabled` si OBS non connecté ou toggle en vol
  (`obsStreamBusy`/`obsRecordBusy` — chaque toggle = 1 connexion).
- Topbar (App.svelte) : `.output-buttons` (2 boutons texte+icône ▶/● → ■,
  proéminents, avant `.ctrl-buttons`). Vert `--message-ok-color` au repos,
  rouge `--message-error-color` + glow quand `.actif` ; `--btn-tint` local
  (pattern `.speedrun-start`). Clic → `confirmObsOutputOpen` ; busy PAR bouton
  (`$obsStreamBusy`/`$obsRecordBusy`),
  `disabled` + grisé neutre quand OBS non connecté. Aucun polling : état =
  stores `obsStreaming`/`obsRecording`.
- Barre raccourcis : icônes `data-id="obs-stream"` / `data-plat="stream"` et
  `data-id="obs-record"` / `data-plat="record"` — clic → `confirmObsOutputOpen`
  via `dispatchRaccourcis` (confirmation dans le dashboard, délibéré — la
  barre n'est PAS un « appui matériel » exempt comme l'API Stream Deck). La
  pastille `.dot` existante
  suffit, `setConnexions` itère tous les `[data-plat]` (zéro JS ajouté).
  Payload `raccourcis:connexions` étendu : `stream` / `record` (bools),
  poussés par le même `$effect`/`pushRaccourcisConnexions` d'App.svelte.
