# Tableau « Vos scènes » enrichi

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Tableau « Vos scènes » enrichi — résumés + mini-map (2026-10-19)

> Chaque cellule du tableau affiche une **mini-map procédurale** (aperçu de
> layout façon OBS — PAS de capture d'écran) + le titre + une ligne d'infos
> (`Widgets : n` + points colorés par type + chips modules HUD + badge
> « À l'antenne » sur la scène courante). Données : commande
> `scenes_resumer` → `scenes::resumer` (scenes.rs) relit chaque `<id>.json`
> de l'index à l'ouverture du tableau (petits fichiers, zéro cache) et rend
> `SceneResume { id, ok, nbWidgets, types, nbTrous, modules, aFond,
> canvasW, canvasH, widgets: Vec<WidgetResume{type,x,y,largeur,hauteur,z,
> trou}> }` — `ok:false` = fichier absent/illisible (pastille affichée
> quand même). Front : wrapper `tauri.scenesResumer` (tauri.ts), store
> `scenesResumeStore` + `loadScenesResume()` (stores/scenes.ts, appelé dans
> `toggleScenes`). Mini-map : `<span>` au `aspect-ratio` canvasW/canvasH,
> rects positionnés en % triés par z croissant, couleur par type via
> custom-prop `--tc` (`[data-type]` : media violet, chat bleu, camera rose,
> input-viewer jaune, speedrun vert, goal orange, musique teal) ; `trou` =
> contour pointillé ; `aFond` (bgMedia/bgSlides/bgObsSource) teinte le fond
> de la mini-map. Libellés types réutilisés de `carteEdition.type.*`,
> modules de `modules.nom.*` ; nouvelles clés `sceneBar.nbWidgets|antenne|
> illisible|groupeMasquees|groupeVisibles` (8 locales). Les scènes
> masquées sont regroupées EN FIN de tableau, distinctes : dérivé
> `indexTri` (visibles puis masquées, ordre relatif conservé) + bandeaux
> labelisés `.pastilles-sep` `grid-column: 1/-1` — « Scènes visibles » en
> tête, « Scènes masquées » avant la 1re masquée, MÊME recette orange pour
> les deux ; chaque bandeau n'apparaît que si son groupe est non vide. Le drag & drop travaille toujours sur l'ordre réel
> de l'index — une masquée déplacée parmi les visibles revient
> visuellement dans son groupe.
