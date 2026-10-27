# Carrousel d'images

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Carrousel d'images — widget média + fond de scène (2026-10-05)

Diaporama d'images sur deux surfaces : `mediaSlides` (widget média) et
`bgSlides` (fond de scène). **Images uniquement** (png/jpg/jpeg/gif/webp) —
pas de vidéos en v1, pas de shuffle, boucle séquentielle.

### Contrat

- `MediaSlide { rel, nom }` (scene.rs + tauri.ts, camelCase figé) — `rel` =
  chemin `medias/<uuid>.<ext>` servi par :4321, `nom` = nom original affiché
  dans la liste d'édition.
- `Option<Vec<MediaSlide>>` + `Option<f64>` (sec) avec `serde(default,
  skip_serializing_if = "Option::is_none")` → rétro-compat : les anciennes
  scènes n'ont pas les champs.
- **Priorité** : liste non vide > média unique (`media`/`bgMedia` restent
  stockés et redeviennent visibles si la liste est vidée) ; `bgObsSource`
  garde la priorité absolue sur le fond (règle figée).
- Intervalle : `mediaSlidesSec`/`bgSlidesSec`, défaut 5 s, min 1 s.
- Fondu enchaîné fixe ~800 ms **borné à la moitié de l'intervalle**
  (`min(800ms, sec×500)`).

### Le timer vit dans `diffusion.html` (jamais le dashboard)

Le dashboard ne commit QUE les mutations de liste/intervalle — **aucun
commit par slide** (un seul écrivain, pas de trafic snapshot par avance).
La boucle continue même dashboard inactif/déconnecté.

- **Widget** : pile d'`<img class="slide">` superposées (opacity 0/1,
  transition CSS bornée), `setInterval` recréé quand `slidesKey`
  (JSON des rels) change ; `slideSec` mémorisé pour recaler le timer sans
  reconstruire. `slidesArreter` à CHAQUE destruction d'entry (delete,
  transition kind↔morph, carrousel→média unique).
- **Chemin morph widget** : la source cachée swap de `src` par intervalle —
  l'élément est REMPLACÉ si la famille change (un `<video>` n'affiche pas
  d'image) ; re-render `morphRendreWidget` au `load` de chaque slide (le
  rendu image est one-shot — la boucle rAF ne couvre que les vidéos).
- **Fond** : `bgState.slides[]` d'imgs cachées (`#bg img` display:none,
  sources drawImage) ; le tick de `startBgLoop` avance à `nextAt` et
  `peindreFond` dessine courante + précédente avec `globalAlpha` pendant
  `fadeDur`. `stopBgLoop` à la sortie de carrousel (sinon rAF tourne à
  vide ; la branche vidéo la relance). Morph/pixel fond : le crossfade est
  dans `dessinerContenuFond` (srcPrev + alphaCur) → couvert par tous les
  chemins.

### Import multi

`import_medias_multi` (lib.rs) : `blocking_pick_files` filtré « Images »
(IMG_EXT) → `validate_and_copy_media` par fichier (mêmes gardes extension/
taille/magic bytes) → `ImportCheminsResult` (ok + erreurs concaténées).
AUCUNE mutation de scène côté Rust — le store pose les slides puis commit.

### Édition (`CarteEdition.svelte`)

Snippet partagé `carrouselBloc` (widget onglet Contenu + fond onglet
Contenu + branche « aucun fond ») : liste ordonnée (↑↓✕), « Ajouter des
images » / « Créer un carrousel », slider intervalle 1–60 s (brouillon
pendant drag, commit au change), « Vider la liste ».

### Preview dashboard

`Widget.svelte` + `Canvas.svelte` : index local cyclé par `$effect` (swap
de `src`, pas de fondu). Morph/pixel preview : source = slide courante.
Garde morph assouplie : `(!bgIsVideo || bgSlidesList)` (fond vidéo +
carrousel d'images → slides morphables).
