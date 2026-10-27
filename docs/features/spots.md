# Module Spots lumineux — deux faisceaux sur le fond

> Cinquième module (`modules.spots`, même `ModuleHud` + config que les 4
> autres) MAIS rendu différemment : ce n'est **pas** un module HUD
> au-dessus des widgets — c'est un **éclairage du fond** : deux
> faisceaux « moving head » aux coins hauts de la diffusion, balayage
> pendulaire + cycle de couleurs fluo, SOUS les widgets et au-dessus du
> fond. Uniquement en diffusion (le dashboard n'affiche qu'un aperçu
> statique de deux cônes sur le fond). **Zéro state Rust** — rendu 100 %
> diffusion, aucune frame dédiée, aucun thread.

- **Couche** : `<canvas id="spots-canvas">` **sibling de `#bg` dans
  `#stage`** (entre `#bg` et `#eq-root`) — espace ÉCRAN, comme
  `#eq-root`. ⚠️ Ne PAS le remettre enfant de `#bg` : il y hériterait
  du `transform` tilt/**zoom** du fond — avec `bgZoom: 1.5` les apex
  (6 %/94 % de large) partent hors cadre (−192 px / +2112 px à 1920 de
  large) et les faisceaux deviennent quasi invisibles (bug constaté sur
  la scène « SPEED - MGS1 2 »). En espace écran : les spots vivent « dans
  la pièce », indépendants du zoom du mur, restent sous `.bg-titre`
  (z 1) et tous les widgets. `display:none` en feuille de style → le
  renderer DOIT poser `style.display = "block"` (même leçon que
  `#eq-root` : `""` ne retire que l'inline).
- **« Ne perce pas les widgets »** : les widgets `trou`/`camera` sont des
  trous destination-out dans `#bg-canvas` — une couche au-dessus les
  recouvrirait. Le bloc trous de `peindreFond` est donc extrait en
  **`peindreTrous(ctx, scene, enEspaceEcran)` partagé**, rejoué sur le
  canvas spots à chaque frame après les faisceaux — caméra et captures
  restent propres. Le 3e param désactive la compensation tilt/zoom de
  `trouToCanvas` : le canvas spots n'étant pas dans `#bg`, ses coords
  SONT les coords écran (les projections 3D/clip-path des widgets
  eux-mêmes s'appliquent toujours). Ne pas rendre les spots dans un
  calque sans cet évidage.
- **Rendu** (`renderModuleSpots`/`spotsOverlayTick`/`spotsDessiner`,
  appelé dans `renderScene`) : sprite de cône blanc pré-rendu UNE fois
  (dégradé vertical + plumé latéral destination-in à plateau décentré),
  projeté en `globalCompositeOperation = "lighter"` depuis l'apex
  (asymétrique : 5 %/95 % de large, hauteurs différentes). **Faisceau
  multi-teinte en 4 passes** — chaque spot teinte le sprite (offscreen
  `spotsTeinter`, source-in) avec : la teinte de BORD (palette décalée
  +0,07 → les bords annoncent la couleur suivante, plus sombres) en
  halo ambiant très large ×1,5 puis cône externe, la couleur pure en
  cône central ×0,66, et le cœur blanc-chaud (`spotsMix` → 55 % blanc)
  en cône étroit ×0,26 + halo radial à la source + mare de lumière à
  l'impact. Palette fluo cyclique `SPOTS_PALETTE` (vert → jaune →
  orange → rouge → violet → bleu, palier ~55 % + transition smoothstep),
  couleurs des deux spots déphasées d'un demi-cycle. **Mouvement
  organique** : chaque spot a sa cadence (×1,17 à droite), l'angle
  combine DEUX sinus non harmoniques (0,5·v·±0,30 + 0,83·v·±0,07) et
  la largeur respire (±7 %) — pas de balayage mécanique symétrique.
  ⚠️ **Orientation toujours INTÉRIEURE** (fix 2026-10-20) : `rotate(θ)`
  bascule le faisceau vers la gauche quand θ > 0 (axe y vers le bas) —
  l'angle est mirroré par `dir = gauche ? -1 : 1` (base 0,34 / 0,44)
  pour que chaque faisceau balaie la scène, de la verticale à ~40°
  vers le centre. Un signe non mirroré = faisceaux pointant hors cadre.
- **Perf** : backing store ~40 % de la résolution scène (l'upscale CSS
  fait le flou — ~6× moins de pixels), rAF plafonnée ~30 fps, seulement
  quand `actif`. ⚠️ Le `clearRect` de désactivation exige
  `setTransform(1,0,0,1,0,0)` AVANT — le transform réduit persiste sinon
  et n'efface qu'une fraction du canvas.
- **Options** (`ModuleHud`, camelCase) : `opacite` = intensité lumineuse
  (0-1, défaut 0.8 — champ générique réutilisé, le bloc Style de la
  modale reste masqué), `spotsVitesse` (0,25-3×, défaut 1 — balayage +
  cycle couleurs), `spotsReagir` (défaut true) = intensité modulée par
  le max des bandes `eqCible` — réutilise la frame `eq-spectrum`
  existante, donc **no-op tant que le module Équaliseur ne capture
  pas** (ambiance normale sinon). `x/y/largeur/hauteur` inutilisés :
  effet plein canvas — le bloc Position de la modale est masqué pour
  ce module et Canvas.svelte ne rend PAS de carte `ModuleHud` pour
  `spots` (un rectangle plein canvas bloquerait le drag des widgets).
- **Noms dédiés** : toutes les variables sont préfixées `spots*`
  (`spotsCanvas`/`spotsCtx`/`spotsCfg`/`spotsRaf`…) — vérifié sans
  collision (bug connu : `var eqCtx` partagé avec le spectre musique).
  Ne pas réintroduire de nom générique (`spot`, `spotCtx`…).
- **Dashboard** : `Canvas.svelte` `.spots-preview` (sibling de
  `.bg-layer`, même `bgTiltStyle`, `pointer-events:none`, opacité liée
  au module) = deux cônes CSS figés (`clip-path` polygon + gradient +
  blur) — dashboard = config, pas un player.
- **JC / vierge / résumés** : couverts automatiquement — JC vide
  `modules` entier (scène dérivée `modules: {}` des deux côtés),
  `scene_vierge` teste `modules.is_none()`, `resumer()` pousse "spots"
  si le module existe.
- **i18n** : `modules.nom.spots` + `modules.hint.spots` +
  `modules.spots.{intensite,vitesse,reagir,hintReagir}` dans les 8
  locales. Pas de placeholder (pas de carte).

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | `ScenesModules.spots` + `ModuleHud.spotsVitesse`/`spotsReagir` |
| `src-tauri/src/scenes.rs` | `resumer()` → badge "spots" du tableau |
| `src/lib/tauri.ts` | `ModuleId`/`MODULE_IDS`/`ScenesModules`/`ModuleHud` |
| `src/lib/stores/scene.ts` | `moduleDefaut("spots")` (plein canvas, `opacite` 0.8) |
| `src/lib/components/Canvas.svelte` | skip carte + `.spots-preview` statique |
| `src/lib/components/ModulesModal.svelte` | icône 🔦 + options (Position/Style masqués) |
| `src-tauri/resources/diffusion.html` | `#spots-canvas` + `peindreTrous` partagé + `renderModuleSpots`/`spotsOverlayTick`/`spotsDessiner` |
| `src/lib/i18n/*.json` ×8 | clés `modules.*.spots` |
