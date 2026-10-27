# Speedrun Splitter — Auto Splitter natif

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


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
│  - timer + contrôles │          │ (11 commandes Tauri) │  WS :4321  │  - rendu timer   │
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
- `speedrun_maj_settings_asl(valeurs)` → met à jour les settings ASL
  individuels (map code→bool, ex. 120+ pour MGS) dans le Context Boa ;
  appelé à la validation de la modale de config, persisté séparément
  par `speedrun_sauver_config`
- `speedrun_est_actif()` → true si actif
- `speedrun_action_manuelle(action)` → start/split/skip/undo/reset/pause
- `speedrun_aller_split(index)` → clic sur une ligne : arme l'index (timer
  arrêté) ou saute à l'index en retry de segment (run en cours)
- `speedrun_lire_config()` → lit speedrun.json (chemins + settings)
- `speedrun_sauver_config(config)` → sauve speedrun.json

### Events

- Tauri `speedrun:event` → store Svelte dashboard (types: started, split, reset,
  ended, goto, etat, gameTime, time, connected, disconnected, error, stopped,
  loaded, paused, resumed, splitSkipped, splitUndone, settings-list, saved).
- WS `speedrun-etat` → diffusion.html `majSpeedrunEtat` (maj DOM directe,
  pattern input-viewer : textContent/classList, zéro re-render). Sous-types
  spéciaux : `lss` (segments) et `etat` (resync complet émis via `EmitEtat`
  par `reemit_lss` — un reload OBS retrouve index + temps de la run).
- **En-tête identique des deux côtés** (réaligné 2026-10-10 — la diffusion
  était restée sur l'ancien layout en ligne) : `.sr-header` = colonne
  centrée — `.sr-titre-jeu` centré, `.sr-cat` sur sa propre ligne (0.7em),
  `.sr-status` en `absolute` coin haut-droit. `.sr-seg` n'assombrit PAS
  les noms (le gris est réservé à `.sr-seg-done`).

### Modèle de temps par split — référence LiveSplit (2026-10-09)

> Chaque ligne de split affiche `nom | temps | delta`. La référence du
> delta est le **PB cumulé du split** (`pb[i]` direct — les PB du .lss
> sont CUMULÉS, real time puis game time ; plus de soustraction
> `pb[i] − pb[i−1]` pour le delta). Il n'y a PAS de baseline « dernière
> fois » persistée (`speedrun_baselines.json` supprimé) — la sauvegarde
> de run se fait dans le .lss (voir « Handlers ASL + hot-reload +
> sauvegarde PB » plus bas).

- **Delta (colonne « +/− », droite)** : **avance/retard CUMULÉ de la run
  vs PB** (façon LiveSplit), format **EXACT LiveSplit**
  (`Delta::with_decimal_dropping` + `Accuracy::Tenths`) — `±S.d` sous
  60 s (`−2.4`, `+23.1`), `±M:SS` au-delà (`+12:34`, décimale
  ABANDONNÉE), `±H:MM:SS` au-delà d'1 h (`−1:02:33`). ⚠️ Le choix
  « secondes pures, jamais m:ss » du 2026-10-09 est **RÉVOQUÉ**
  (2026-10-13). Actif = chrono total vivant − `pb[i]` (défile à chaque
  tick `time` : `tempsJeu ?? tempsReel` côté dashboard,
  `srGameTime || srRealTime` côté diffusion) ; passé = cumul de run
  enregistré au split − `pb[i]` (figé) ; vide pour les non atteints /
  skippés / armés en NotRunning / sans PB. Couleur classée sur la valeur
  ARRONDIE au dixième (`+0.0` ne bascule pas) : `−x` vert
  (`--message-ok-color`), `+x` rouge (`--message-error-color`) — sur la
  valeur ET la bordure de la ligne (`.seg-ahead`/`.seg-behind`,
  `.sr-seg-ahead`/`.sr-seg-behind`). Pas de hex nouveau ; chrono
  principal teinté `--sr-grad-from`. Les deux `<span>` de temps ont une
  largeur de colonne (jamais `display:none` — la colonne delta entière
  peut être masquée via `srModeDelta="off"`).
- **Temps (milieu)** : deux modes via `srModeTemps` (modale layout,
  2026-10-13). **`"cumul"` (DÉFAUT, split time façon LiveSplit)** : temps
  TOTAL auquel le split est/sera bouclé — passé = cumul de run enregistré
  (`cumulsSplits`/`srSplitCumuls`, skippé → PB cumulé), actif en run =
  chrono total vivant, non atteint / armé en NotRunning = **PB cumulé**
  `pbCumul(i)` (= `pbRealTime ?? pbGameTime` direct — le .lss stocke les
  PB en CUMUL, PAS de soustraction — c'est le temps de comparaison
  LiveSplit). Format Regular accuracy Seconds `M:SS`/`H:MM:SS` SANS
  centièmes (`formatCumul`/`srFormatCumul`). **`"segment"`** (comportement
  d'avant 2026-10-13) : durée de segment — passé = durée réalisée figée,
  actif = temps vivant (0:00.00 si armé, jamais de signe), non atteint /
  skippé = durée PB du segment (`pb[i] − pb[i−1]` — la soustraction
  `segPb` reste ICI et en delta mode "segment", vide si indéterminable).
- **Contrat events** : `split`/`ended` portent `time` = **durée de
  segment** et `cumul` = temps total de run. Les deux consommateurs
  (store `speedrun.ts` + `diffusion.html`) lisent `time` comme durée ET
  `cumul` comme cumul du split réalisé (base du delta : `cumulsSplits`
  store / `srSplitCumuls` entry). `etat` porte `splitTimes` +
  `splitCumuls` (resync). Un oubli = régression visible.
- **Clic sur une ligne** (`speedrun_aller_split`, `.speedrun-segment`
  `pointer-events:auto` + `stopPropagation`, pas de `commitScene`) :
  en NotRunning → **arme** l'index (`armed_index` dans timer.rs, chrono à
  zéro, ne démarre pas — attend le start ASL/manuel ; `Started.index` porte
  l'index armé sinon le frontend reviendrait à 0) ; en Running/Paused/Ended →
  saute à l'index en **retry de segment** : chrono de segment à zéro, TOUS
  les temps de la tentative effacés (`temps_segs` + `cumuls_segs` Rust,
  `splits`/`cumulsSplits` + `srSplitTimes`/`srSplitCumuls` frontend — les
  précédents ne comptent pas comme finis), PAS un reset de run.
- **undo** : `seg_start` recule de `d_k` (durée enregistrée dans
  `EngineInner.temps_segs`, passée à `undo_split`) → le segment défait
  reprend là où il en était au lieu de repartir de zéro. Le `cumuls_segs`
  de l'entrée défaite est effacé (repris au re-split).
- **`disconnected`** (sortie du jeu) : handler ASL `exit()` invoqué puis
  timer reset → `reset` émis avant `disconnected` — l'affichage suit le
  timer sur les deux frontaux. `temps_segs`/`cumuls_segs` purgés (ils
  survivaient à la mort du processus — latence de mélange corrigée).

### Modale « Créer votre layout » — Layout Editor (2026-10-13)

> `SpeedrunLayoutModal.svelte` (bouton de la carte d'édition speedrun,
> flag `speedrunLayoutModalOpen`, montée dans App.svelte) — façon Layout
> Editor LiveSplit. Pilote UNIQUEMENT les éléments internes du splitter ;
> drag/resize, cadre SVG, média de fond, clic sur ligne (armer/retry)
> inchangés. Persistance PAR WIDGET : champs `Option<>` camelCase
> `srVoirTitre/srVoirCategorie/srVoirStatut/srVoirTimer/srVoirPhase/
> srVoirCompteur/srVoirBarre` (bool, absent = visible), `srModeTemps`,
> `srModeDelta` ("cumul"|"segment"|"off"), `srDeltaPrecision`,
> `srDeltaDrop`, `srTimerPrecision`, `srTimerMethode` ("auto"|"real"|
> "game"), `srCouleurTimer`, `srSegBordure`, `srSegBordureEpaisseur`,
> `srSegBordureRayon`, `srCouleurAhead/Behind/Actif`, `srNbLignes`,
> `srScrollAuto`, `srHauteurLigne` — dans scene.rs + interface Widget
> tauri.ts + `setSpeedrunConfig` (stores/scene.ts). Dashboard : lus via
> `$derived` (même pattern que srPolice). Diffusion : `srLireOpts(w)`
> dans renderScene → `entry.srOpts`, appliqués par `srAppliquerOpts`
> (display/classList/vars CSS — jamais innerHTML par event) au create ET
> à l'update ; `srMajHeader`/`srMajTimerEtPhase`/`srMajSegmentsVisuel`
> les consomment (pas d'accès à `w`). i18n : section `speedrunLayout.*`
> dans les 8 locales.

### Métronome du splitter — battement BPM dashboard SEUL (2026-10-19)

> Battement type métronome joué pendant la run, entendu UNIQUEMENT par le
> streamer sur son dashboard — JAMAIS envoyé à la diffusion (choix user ;
> un métronome pendant toute une run est risqué pour l'audience). Suit la
> phase du chrono : start ASL/manuel → il joue ; `paused` → stoppe net ;
> `resumed` → repart (un métronome n'exige pas de continuité de phase à
> travers une pause) ; `reset`/`ended`/`disconnected`/`stopped` → stoppe.

- **Runtime** : `stores/metronome.ts` (NOUVEAU) — Web Audio pur, zéro
  fichier audio (sinus 1250 Hz, attaque 1 ms, décroissance ~90 ms).
  Scheduler look-ahead (setInterval 25 ms → tics programmés ~120 ms à
  l'avance sur `AudioContext.currentTime`) → **dérive nulle** — PAS un
  setInterval+`new Audio()` (décalage ~100 ms/tic). Politique autoplay :
  `resume()` retenté à chaque passage du scheduler (se débloque au 1er
  geste utilisateur). Zéro Rust : le moteur ASL/la 15Hz/le timer ne sont
  pas touchés.
- **Déclenchement** : souscription unique à `phaseTimer` (speedrun.ts) —
  il centralise TOUTES les transitions (started/paused/resumed/reset/
  ended/disconnected/stopped/goto + resync `etat`) → pas de branchement
  dans le switch du store.
- **Config par widget** : champs `srMetronomeActif` (Option<bool>, absent
  = off), `srMetronomeBpm` (Option<u32>, 1-400, défaut 120), 
  `srMetronomeVolume` (Option<f32>, 0-1, défaut 0.5) — scene.rs + tauri.ts
  + `setSpeedrunConfig` (stores/scene.ts), persistés dans la scène AU
  MÊME TITRE que `speedrunCheminAsl`/`speedrunCheminLss` (choix user).
  `WidgetSpeedrun` pousse via $derived sur les champs PRIMITIFS (pas
  `contenu` entier — recréé à chaque pointermove) ; plusieurs widgets →
  le DERNIER monté/édité gagne (même règle de facto que les chemins).
- **UI** : `CarteEdition.svelte` (section speedrun) — 4e toggle
  `setting-toggle` dans la rangée des autos (Auto Start/Split/Reset) ;
  si actif : sliders BPM (1-400) + Volume (0-100 %, pattern copies
  locales + commit au change, comme srTaille) + bouton « Tester » (4 tics
  de préview, désactivé pendant une run via `battementEnCours`) + hint
  i18n explicitant « dashboard seulement, jamais dans le stream ».
  Changement de BPM/volume en cours de run : appliqué SANS reset de
  phase (le scheduler relit la config à chaque tic).
- **i18n** : clés `widgetSpeedrun.metronome*` (5) + `guide.pages.principe.l4`
  dans les 8 locales — la puce l4 pose la règle générale dashboard vs
  diffusion (métronome = pour le streamer ; alertes/bandeaux/attente =
  pour les viewers), demande user « claire pour un non-habitué ».
- **Audit deltas (parallèle, 2026-10-19)** : vérifié sur demande user —
  deltas verts/rouges = SECONDES (format LiveSplit exact), jumeaux
  dashboard (`formatDelta` WidgetSpeedrun) / diffusion (`srFormatDelta`) /
  pop points (`srPointEtatFranchi`). Aucun bug. Pas d'unité « s » ajoutée
  (convention LiveSplit universelle ; `+12:34 s` serait absurde ≥60 s).
  Cas signalé non modifié : run game time vs PB .lss RealTime importé →
  deltas étranges (contrat : .lss avec RealTime = RTA).

### Pop-out speedrun — fenêtre détachable avec contrôles (2026-10-19)

> « Comme le chat » (chat-popout) MAIS avec contrôles de run (choix user).
> Fenêtre toujours-au-dessus 420×640 (label `speedrun-popout`, custom
> protocol `streamos-speedrun://localhost/splitter`, HTML embarqué
> `speedrun-popout.html`, ASYNC obligatoire — même deadlock Webview2 que le
> pop-out chat). Le widget de scène, la diffusion OBS et le moteur ASL
> sont INCHANGÉS — le pop-out est un 3e consommateur.

- **Affichage** : WS `:4321` (frames `speedrun-etat`) — 3e rendu du
  splitter, bloc dupliqué VOLONTAIREMENT depuis diffusion.html (convention
  « pas de module JS partagé entre pages statiques », cf. chat-popout).
  Resync à l'ouverture : `speedrun-resync` upstream (server.rs, même
  mécanisme que diffusion.html) → ouverture en pleine run = phase + index
  + temps retrouvés (avance sur le pop-out chat, qui ne rejoue pas
  l'historique). Options de layout lues dans le SNAPSHOT (1er widget
  speedrun de la scène — majorité simple comme chatStyle du pop-out
  chat) + gradient = couleur du cadre de scène (`scene.cadreWidget`,
  comme les bulles du pop-out chat). Palette en dur = tokens glace.
- **Contrôles** : pattern RACCOURCIS — la fenêtre émet
  `speedrun-popout:action {action: demarrer|arreter|split|skip|undo|
  reset|pause|goto, index}` via `window.__TAURI__.event`, le store
  `speedrun.ts` écoute (initSpeedrun) et exécute via les actions
  existantes (demarrerSpeedrun/actionManuelle/allerSplit). PAS de logique
  métier dans la fenêtre satellite. État moteur (estActif — le WS ne le
  dit pas en NotRunning armé) poussé par le dashboard via
  `speedrun-popout:etat {actif}` + handshake `speedrun-popout:ready`
  (même mécanique que raccourcis:connexions).
- **Clic sur une ligne** : `goto` (armer en NotRunning / retry de
  segment) — double garde (moteurActif côté pop-out, moteur côté Rust).
- **Métronome** : PAS dans le pop-out (webview séparée → double
  battement désynchronisé sinon) — il reste dans le dashboard.
- **UI dashboard** : bouton « Détacher/Réattacher » dans la carte speedrun
  (CarteEdition, libellés `carteEdition.detacher/reattacher` réutilisés —
  aucune clé i18n nouvelle) + event Rust `speedrun-popout-closed` (croix
  native → reset du bouton ; distinct de `popout-closed` du chat pour ne
  pas réinitialiser les deux boutons mutuellement).
- **Fichiers** : `lib.rs` (2 commandes + protocol + capabilities
  `speedrun-popout` dans default.json), `resources/speedrun-popout.html`
  (NOUVEAU), `tauri.ts` (2 wrappers), `CarteEdition.svelte` (bouton),
  `stores/speedrun.ts` (listeners action + push etat).

### Pop points de validation — refonte « waouh » (2026-10-19)

> Refonte visuelle du pop affiché HORS splitter (diffusion seule) à chaque
> split franchi/skippé — demandé user : « pas juste un petit carré ».
> Base inchangée : `srVoirPoints`/`srPointsDureePop`/`srPointsSon(+Volume)`
> du widget, dédup multi-widgets par signature (index+cumul, 1.5 s), z 9993,
> caché sur started/reset/goto/stopped. JS quasi intact (srPopSplit :
> +classe `sans-delta` + var `--pop-couleur` inline).
> ⚠️ Position : squelette DÉDIÉ depuis le 2026-10-19 (frame `splitpop-pos`,
> `position_splitpop.json`, onglet Position section « Pop des splits ») —
> plus le squelette partagé, voir « Squelette de position unifié ».

- **Composition** (flex column, sans boîte — le texte flotte) : badge
  « SPLIT n » (chip ::before, letter-spacing) → **HERO = le temps gagné/
  perdu** géant (police **Anton**, `transform: skewX(-5deg)`, ombre dure
  noire multi-couches + glow néon `color-mix` de la couleur du résultat)
  → mot-clé dessous (`::after`) : **GAGNÉ** (ok+delta) / **PERDU** (ko+
  delta) / **SKIP** (ko sans delta) / **VALIDÉ** (franchi sans PB — le
  mot devient le hero, delta masqué). Aura sombre en `::before`
  (radial-gradients + blur) pour la lisibilité sur tout fond de jeu.
- **Animations** : impact écrasé-incliné → overshoot → recentrage
  (`splitpop-impact`, cubic-bezier rebond, 0.5 s) + flash luminosité sur
  le hero (`splitpop-flash`). Rejouées par le reflow remove/add
  `.visible` EXISTANT du root — rien à changer dans la mécanique.
- **Re-pops pendant l'affichage** (2026-10-19, demandé user « plus on
  augmente le timer, plus il y a un re-pop ») : timers à **2 s et 4 s**
  planifiés dans `srPopSplit` si `dureeMs > t + 600` (marge pour finir
  avant le fade) — durée 2 s = zéro re-pop, 3-4 s = un, ≥5 s = deux,
  au-delà stabilisation parfaite. Chaque re-pop pose `.repop` sur
  `splitPopCarte` (PAS le root — c'est la carte qui porte l'animation) :
  `splitpopRepop` (rebond 0.45 s) + `drop-shadow` blanc sur le hero/mot.
  Purge : `splitPopRepopTimers` vidé avec le timer principal dans
  `splitPopCacher` et à chaque nouveau `srPopSplit`.
- **Couleurs** : var `--pop-couleur` posée inline par `srPopSplit` =
  `srCouleurAhead`/`srCouleurBehind` du widget (modale layout) sinon
  `--message-ok-color`/`--message-error-color` — badge, glows et ombres
  en dérivent tous.
- **Tailles** : dérivées du squelette (carte `fontSize` = max(18,
  hauteur×0.4) posé par majSplitPopPos) — hero 1.5em, badge/mot 0.3em,
  mot-hero 0.85em.

### Export des splits — fichier HTML autonome (2026-10-21)

> « Exporter les splits » : un clic génère un `.html` auto-portant de la
> tentative — choix user : pas de `.md`, le HTML + CSS inline donne un
> rendu tabulaire soigné (demande user « minimaliste mais plutôt cool »).
> Exportable à TOUT moment (choix user) : reflète l'état courant des
> stores — run en cours = splits réalisés + ligne active live + non
> atteints en PB muté.

- **Boutons** : carte d'édition speedrun (sous « Détacher »,
  `widgetSpeedrun.exporter`) + pop-out (`.sr-ctrls` `.export`, teinté
  `--sr-grad-to`) → `emitAction("export")` → le store exécute
  (convention satellite, zéro logique dans la fenêtre).
- **Génération** : `exporterSplits()` dans `stores/speedrun.ts` — lit les
  stores (segments, durées/cumuls réalisés, phase, index, noms) et les
  clés `widgetSpeedrun.export*` (14 clés × 8 locales ; le fichier
  exporté SUIT la langue de l'UI). HTML = carte sombre centrée, barre
  gradient, chips « Partie n°N » + statut, hero temps total, tableau
  `# · split · segment · total · Δ vs PB` (vert/rouge, format LiveSplit
  dixième + drop ≥1 min) dans un `.table-wrap` arrondi (zebra + hover +
  `overflow-x:auto`, en-tête teinté gradient), tags SKIP / en cours,
  **ligne `gold`** = meilleur segment réalisé de la run (durée min >
  0 — convention LiveSplit, pastille dorée + durée `#fbbf24`, pas de
  comparaison vs BestSegmentTime : le parser .lss ne l'extrait pas).
  Responsif : padding carte en `clamp()` + `@media (max-width:560px)`
  (tailles réduites, scroll horizontal du tableau). `esc()` sur TOUTES
  les données utilisateur ; zéro ressource externe (s'ouvre hors-ligne —
  contrat vie privée).
- **Enregistrement** : commande `speedrun_exporter_splits(html,
  nom_fichier)` (commands.rs) — `blocking_save_file` filtre `.html`,
  extension forcée, `spawn_blocking` obligatoire (dialog natif, même
  raison que charger_asl). Retourne `Option<String>` (None = annulé).
  Nom suggéré `splits-<slug-jeu>-partie-<N>-<AAAAMMJJ-HHMM>.html`.
- **« Partie n°N » = compteur de tentatives** : `SpeedrunConfig.
  nb_tentatives` (u32, serde default) persisté dans `speedrun.json` ;
  store `tentativeRun` restauré par `chargerPreferencesSpeedrun`, +1 sur
  chaque event `started` (start manuel ou ASL) — undo/goto/reset
  n'incrémentent PAS. ⚠️ **Read-modify-write obligatoire** :
  `speedrun_sauver_config` écrase TOUT le fichier → `persisterTentative`
  relit la config avant écriture ; `sauverPreferencesSpeedrun` a été
  passé en merge pour la même raison (sinon `nb_tentatives` ET
  `settings_asl` étaient effacés à chaque choix de fichier/toggle —
  bug d'écrasement pré-existant corrigé au passage).
