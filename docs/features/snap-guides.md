# Grille magnétique + guides + duplication miroir

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Grille magnétique + guides symétriques + duplication miroir (2026-09-19)

> Guides visuels pendant drag/resize — **dashboard UNIQUEMENT, jamais en
> diffusion**. Le snap existait depuis le 2026-08-29 (dev-log #29) ; cette
> passe ajoute symétrie axe central, espacements égaux et duplication de
> widget avec gizmo miroir.

### Snap (`scene.ts` — `buildSnapTargets` / `computeSnap`, seuil `SNAP_PX = 6`)

Cibles par axe (`xs`/`ys`, `SnapTarget {pos, side}`), figées au pointerdown
(snapshot — pas de `get()` par frame), bypass **Alt** = free-drag, actif en
drag ET resize :

- bords + centre du canvas, bords + centres des autres widgets (existant)
- `side: "mirror"` : positions miroir des autres widgets (`cw − o.x`,
  `cw − o.x − o.largeur`, idem en y) → la marge opposée du widget déplacé
  égale la marge d'un autre widget
- `side: "gap"` : pour chaque paire non chevauchante, prolongation de la série
  au même espacement + centrage entre les deux (si le widget déplacé tient
  dans le gap — ses dims sont lues depuis la scène au snapshot)

### Visuels (`AlignmentGuides.svelte`, `<canvas>` pointer-events:none z-50)

- 4 traits aux bords + labels distance px — passés en jaune `--ctrl-centre`
  quand les marges opposées sont égales (±0.5 px ; le snap produit l'égalité
  exacte)
- Viseur central + label « CENTRE » (existant)
- **Fantôme miroir** : rect pointillé vert `--ctrl-refresh` à la position
  symétrique (axe vertical du canvas), masqué si centré (= même place)
- **Espacements égaux** : segments pointillés jaunes sur le gap créé ET le
  gap de référence + label « N px ». Les autres widgets sont lus depuis
  `sceneStore` ; le widget déplacé est exclu par égalité de rect (le store
  reçoit exactement les valeurs snappées poussées dans `guidesEtat`)

### Duplication (`dupliquerWidget` + onglet « Dupliquer » de CarteEdition)

- Onglet « Dupliquer » dans la carte d'édition — branche widgets non-caméra
  (la caméra reste exclue : singleton figé, `createCameraWidget` refuse).
- Bouton → copie : uuid neuf, z au-dessus (max non-caméra + 1), `obsSource`
  **strippé** (une source OBS = un seul emplacement ; le trou copié reste non
  lié), position initiale = miroir axe vertical du canvas (+24 px si
  l'original est centré). Copie sélectionnée + `commitScene` (save + snapshot).
- Flag `garderTab` (CarteEdition) : la copie devient sélectionnée SANS reset
  de l'onglet vers « Contenu ».
- **Gizmo miroir** : `Gizmo3D` réutilisé avec prop `maxDeg={180}` (défaut 90 =
  inclinaison seule). `rotateY = ±180°` = miroir horizontal, `rotateX = ±180°`
  = miroir vertical — `perspective(800px)` sans `backface-visibility` : le
  widget montre son dos. Aimant bord ±maxDeg (8°), label « Miroir H / V /
  H+V » (`--ctrl-centre`) dès |angle| > 90°. Raccourcis « Miroir → » /
  « Miroir ↓ » / « Reset » (`rotateWidgetLocal` + `commitScene`).
- **Zéro nouveau champ `Widget`** : `rotateX`/`rotateY` existants, rendus
  identiques dashboard (`Widget.svelte`) et diffusion (`diffusion.html`),
  persistés dans la scène JSON comme avant.
