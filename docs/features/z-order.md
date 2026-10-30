# Ordre de plan (z) des widgets

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Ordre de plan (z) des widgets — boutons −/+ au survol (2026-10-03)

> Réordonnancement z par l'utilisateur : tout au premier plan / tout en
> arrière-plan, via boutons au survol + entrées du menu contextuel widget.
> **Zéro Rust** — `Widget.z` est un champ JSON comme un autre ; côté
> diffusion le z est « style seulement » au snapshot (`zIndex = w.z` sur le
> wrap), jamais de rebuild.

- **Bande normale 0..N-1** (widgets non-caméra). Création/duplication :
  `z = max(non-caméra)+1` (trous possibles après delete). `mettreWidgetPlan`
  **renumérote la bande à chaque appel** → z contigus, trous comblés, borne
  basse ≥ 0 garantie (un z < 0 passerait SOUS `.bg-layer` z:0 en dashboard).
- **Caméra EXCLUE** : plage réservée z=5000 (sous les overlays autonomes
  diffusion 9995-9999 — welcome 9999, alertes 9997, commandes 9996).
  `mettreWidgetPlan(id)` est no-op sur la caméra ; boutons et entrées de
  menu masqués pour elle (`wgType !== "camera"` / `!isCamera`).
- **`mettreWidgetPlan(id, "avant" | "arriere")`** (`stores/scene.ts`) : tri
  par z (stable), splice + push/unshift, réassignation z=index, puis
  `commitScene` (save + snapshot :4321 — sync trous/caméra inchangée).
  ⚠️ Effet de bord voulu : un widget `trou` masque « fond + widgets
  dessous » → remonter/descendre un trou change ce qui est visible à
  travers lui.
- **Badges de survol** (composant **`BadgeSurvol.svelte`**, recette CSS
  `.wbadge`/`.z-controls`/`.z-btn` dans `app.css` — mutualisée widgets +
  modules HUD depuis 2026-10-30, avant : copie renommée `.mod-*` dans
  ModuleHud.svelte) : ancre `.wbadge` FRÈRE de `.widget`/`.module` +
  bloc `.z-controls`, badge flottant contre-scalé
  (`transform: scale(1/scale)` inline → taille écran constante quel que
  soit le zoom canvas), ronds ambre `--message-user-action-color`,
  dashboard uniquement, jamais en diffusion. **2026-10-26 : le badge est
  sorti du widget** — enfant de `.widget`, il était avalé par son
  stacking context (`z-index:{w.z}`) → recouvert par tout voisin à z
  supérieur et par les modules HUD (8000). `.wbadge` = ancre absolue
  plaquée sur le rect de la carte, `pointer-events:none` (la zone invisible
  ne mange aucun clic), z-index **9400** : au-dessus des widgets ≤5000,
  des modules 8000 et des aides 9000 ; **sous** `.sel-overlay` 9500 (les
  poignées resize gardent la priorité). Visibilité CSS pure (app.css) :
  `.widget:hover + .wbadge` / `.module:hover + .wbadge` +
  `.wbadge:hover` (le badge déborde du rect — le survoler seul le garde
  affiché) ; masqué pendant `.dragging`/`.actif` ;
  `stopPropagation` pointerdown/dblclick (pas de drag/sélection/modale
  titres/morph) ; `data-widget-id`/`data-module-id` pour le menu
  contextuel (`closest("[data-*-id]")`, Canvas.svelte — le premier est
  porté aussi par `.sel-overlay`). ⚠️ Adjacence obligatoire : `.wbadge`
  doit suivre immédiatement `.widget`/`.module` dans le DOM
  (sélecteur `+`). **Placement adaptatif (`badgePos`, interne au
  composant)** : le bord du badge est choisi selon la place disponible en
  unités canvas (px écran / scale, nb de boutons, carte à moitié hors
  canvas) — haut → bas → droite → gauche → dedans (overlay interne,
  dernier recours) — variantes `.haut`/`.bas`/`.droite`/`.gauche`/`.dedans`,
  `transform-origin` du coin collé + `flex-direction:column` sur les
  côtés. État épinglé : prop `visible` → `.z-controls.visible`.
- **Boutons −/+** : 2 boutons sur tout widget non-caméra (prop `boutons`
  de BadgeSurvol), `disabled` aux extrémités de la bande non-caméra (un
  widget à égalité de z peut encore bouger). Caméra : pas de −/+ (z=5000
  figé) — œil seul épinglé via `visible` quand `cameraCachee` (détails
  `docs/features/camera-details.md`). Modules : œil seul aussi — pas de
  z-order module.
- **Poignées de resize** (`SelectionOverlay.svelte`, monté dans
  `Canvas.svelte` après la boucle des widgets, prop `widgetSelectionne`)
  : overlay transparent plaqué sur le rect du widget sélectionné,
  `z-index 9500` (au-dessus des widgets <5000 et des modules HUD 8000) —
  JAMAIS masquées par un voisin chevauchant à z supérieur (avant :
  enfants du widget à `z-index: w.z`, avalées par son stacking context).
  Hit-box **26 px ÉCRAN constante** quel que soit le zoom
  (`calc(Npx * var(--hs))`, `--hs` = 1/scale posé inline — même
  contre-scale que `.z-controls`), centrée pile sur le coin/bord
  (`-13px`) ; carré visible 12 px ambre `--message-user-action-color` +
  liseré `--fond` via `::after` (on vise large, on voit fin).
  `pointer-events:none` sur l'overlay sauf sur les poignées. Le drag
  reste sur `.widget` (Widget.svelte) ; TOUTE la mécanique resize vit
  dans l'overlay (snap `buildSnapTargets`/`computeSnap`, guides,
  MIN=80, pointer capture, `commitScene` au pointerup).
- **Menu contextuel widget** (`Canvas.svelte`, clic droit) : entrées
  `carteEdition.premierPlan` / `carteEdition.arrierePlan` après « Dupliquer »
  — action → ferme le menu (pattern établi, pas de disabled dans `.ctx-item`).
  Caméra : entrée œil « Masquer/Réafficher la caméra » (`cameraCachee`) à
  la place des −/+ ; « Dupliquer » masqué (`dupliquerWidget` refuse, no-op
  silencieux sinon — singleton figé).
