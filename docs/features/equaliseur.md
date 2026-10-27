# Module Équaliseur — spectre audio

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Module Équaliseur — overlay spectre audio sous les widgets (2026-10-13)

> Quatrième module (`modules.equaliseur`, même `ModuleHud` + config/drag/
> resize que les 3 autres) MAIS rendu différemment : ce n'est **pas** un
> module HUD au-dessus des widgets — c'est un **overlay translucide en
> bas de la diffusion, SOUS les widgets et au-dessus du fond**, uniquement
> en diffusion (le dashboard n'affiche qu'un placeholder statique
> positionnable). Spectre audio système calculé en Rust, poussé par WS.
>
> ⚠️ **Le spectre n'est PAS le mix OBS** — obs-websocket n'expose aucun
> niveau audio. La source est la capture WASAPI Windows :
> **loopback du périphérique de sortie par défaut** (≈ « Desktop Audio »
> d'OBS) **+ périphérique d'entrée par défaut** (la voix — absente du
> loopback sauf monitoring OBS), fusionnés par bande (max).

- **`equaliseur.rs`** : `EqualiseurState` (champ de `ScenesState`, pas
  `app.manage` — vit avec `chat_tx` comme input_viewer). `sync(module)`
  appelée par `scenes::push_snapshot` (point unique des mutations, après
  libération des locks) + une fois au boot dans `lib.rs`. Démarre 1–2
  threads capture WASAPI (sortie Render→loopback + micro optionnel
  `eqMicro`), FFT 2048 (rustfft, fenêtre Hann) → 24 bandes log 32 Hz–16 kHz
  normalisées dB (−55..0 dBFS → 0..1) → frame `{"type":"eq-spectrum",
  "bandes":[24]}` ~30 Hz sur `chat_tx`. **Zéro thread quand inactif**
  (invariant input_viewer) ; config changée → cancel + respawn (threads
  détachés, ~150 ms max via `wait_for_event`). Échec device → eprintln! +
  thread mort, jamais de panic. Stubs no-op hors Windows ; `wasapi` +
  `rustfft` sous `[target.'cfg(windows)'.dependencies]` (wasapi 0.24 —
  0.25 exige Rust 1.85 > MSRV 1.77).
- **Options** (`ModuleHud.eqMicro`/`eqBulles`, camelCase, défauts ON) :
  micro dans le spectre / bulles animées. Bloc Style de la modale masqué
  pour ce module (pas de texte/fond/cadre — ignorés).
- **Diffusion** : `#eq-root` inséré dans le DOM **juste après `#bg`**
  (même `z-index:0` → l'ordre DOM départage : au-dessus du fond, sous
  tous les widgets z ≥ 0 et `.bg-titre` z 1), canvas plein stage,
  `pointer-events:none`. `renderModuleEqualiseur` dans `renderScene`
  sync rect/options + boucle rAF (aucune rAF si inactif). `eq-spectrum`
  → `eqCible` ; `eqDessiner` fait le lissage attack ~70 ms / decay ~220 ms,
  barres néon arrondies + bulles néon translucides qui montent au-dessus
  du bandeau (`eqBulles`), spawn proportionnel à l'énergie. Frames
  plates dédupliquées côté Rust.
  ⚠️ Bug corrigé 2026-10-10 : `#eq-root` porte `display:none` en feuille de
  style — l'affichage DOIT être `style.display = "block"` (`""` ne retire
  que l'inline → couche jamais visible + rAF tournant à vide). Garde
  `!eqCanvasCtx` dans `renderModuleEqualiseur` : un throw y figerait la
  couche widgets (appel au milieu de renderScene).
  ⚠️ **Bug corrigé 2026-10-13 — collision `var eqCtx`** : le spectre
  MUSIQUE (audio ombre Winamp) déclare aussi `var eqCtx` (AudioContext)
  plus bas dans le MÊME scope script — le dernier déclaré gagnait, le
  contexte 2d du module valait `null` → garde `!eqCtx` toujours vraie →
  **module jamais affiché** (frames `eq-spectrum` bien reçues, jamais
  dessinées ; si la musique joue, l'AudioContext truthy faisait passer
  la garde puis `clearRect` sur un AudioContext → TypeError → rAF morte).
  Correctif : le contexte canvas du module s'appelle `eqCanvasCtx`
  (le bloc musique garde `eqCtx`). Même classe de bug que la collision
  `eqTick` documentée dans diffusion.html — **toute nouvelle variable
  du module doit vérifier qu'aucun bloc plus bas ne la redéclare**.
- **Couleurs des barres (2026-10-13)** : CHOIX USER prioritaire
  (`eqCouleur`/`eqCouleurFin` du module, pickers dans la modale —
  décision explicite respectée telle quelle, `couleurFin` posée sans
  `couleur` est ignorée), sinon **l'accent du cadre de scène**
  (extrémité la plus lumineuse, `p` de `bpmResoudreCouleurs` — même
  résolution que `accentCadre` du placeholder dashboard). Le rendu
  DIFFUSION mire l'apparence du placeholder dashboard : dégradé qui
  FOND vers le haut (`rgba(accent, 0.23)` au sommet ≈ le
  `linear-gradient to top, accent, transparent 130%` de `.eq-ph i`),
  sauf `couleurFin` user posée (dégradé p→s complet). **Pas de fallback
  gris** : la couleur du cadre est un choix utilisateur, sombre compris
  (une garde de lisibilité essayée le 2026-10-13 a été retirée à la
  demande de l'utilisateur — elle écrasait sa couleur de cadre).
  Résolution partagée `couleursEqualiseur` (stores/scene.ts) pour les
  pickers de la modale.
- **Dashboard** : placeholder statique dans `ModuleHud.svelte` (barres
  figées + label « rendu en diffusion »), drag/resize inchangé.

| Fichier | Rôle |
|---|---|
| `src-tauri/src/equaliseur.rs` | Capture WASAPI + FFT + frames `eq-spectrum` |
| `src-tauri/src/scenes.rs` | `ScenesState.eq` + sync dans `push_snapshot` |
| `src-tauri/src/lib.rs` | `mod equaliseur` + init + sync boot |
| `src-tauri/resources/diffusion.html` | `#eq-root` + `renderModuleEqualiseur`/`eqDessiner` + handler WS |
| `src/lib/components/ModuleHud.svelte` | Placeholder statique drag/resize |
| `src/lib/components/ModulesModal.svelte` | Toggles micro/bulles + pickers couleur (bloc Style masqué) |
