# Écran d'attente (BRB)

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Écran d'attente (BRB) — overlay plein écran + chrono (2026-09-26)

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
