# Effet miroir média — flip H/V

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Effet miroir média — flip H/V (2026-09-20)

> Retournement horizontal/vertical du contenu média (sémantique OBS « Flip »).
> `mediaMiroirH`/`mediaMiroirV` sur `Widget`, `bgMiroirH`/`bgMiroirV` sur
> `Scene` — booléens `serde(default)` (false = pas de miroir, rétro-compat).

- **Rendu** : `scale(±1)` **combiné au zoom** dans la chaîne transform
  existante, **après `rotate`** dans la liste → le flip s'applique à la source
  dans son espace local (« flipper l'image puis la tourner »). Formule unique
  partout : `scale(zoom·mH, zoom·mV)` avec `mH/mV ∈ {−1, 1}`.
- **Chemins couverts (tous doivent rester synchrones)** :
  - Widget média : `mediaTransform` (Widget.svelte) + `dessinerOffscreen`
    (chemin morph, 2 sous-chemins normal/pixel) ; `applyMediaStyle` +
    `morphRendreWidget` (diffusion.html).
  - Media-fond des widgets à contenu (chat/input-viewer/speedrun) : automatique
    via `applyMediaStyle` (diffusion) et le snippet `mediaFond` (dashboard).
  - Fond : `bgMediaTransform` + `rendreBgMorph` + `rendreBgPixel`
    (Canvas.svelte) ; `dessinerContenuFond` dans `peindreFond` (diffusion.html
    — couvre le chemin morphé via l'offscreen).
- **UI** : onglet **Effets** de CarteEdition — groupe « Miroir média »
  (gated `selectedWidget?.media`) et « Miroir fond » ; boutons toggle
  `fit-btn` « Miroir → » / « Miroir ↓ » → `toggleMediaMiroir` /
  `toggleBgMiroir` (stores/scene.ts, commit immédiat).
- **Reset** : `resetMedia` / `resetFond` remettent les flags à false.
- **Exclusions** : widget caméra (source OBS `SOS-Caméra`, pipeline
  `camera_sync` — un flip caméra serait un transform OBS séparé) et widgets
  trou/captures OBS (pas de média rendu — flags inertes, comme zoom/rot).
- Ne pas confondre avec le « Retournement » de l'onglet Dupliquer
  (`rotateX`/`rotateY` ±180° = flip du widget ENTIER en 3D, pas du média).
