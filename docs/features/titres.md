# Titres multi-positions — widget + fond

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Titres multi-positions — widget + fond (2026-09-19, refondu 2026-09-28)

> Jusqu'à 4 titres SIMULTANÉS par cible (widget / fond), style partagé.
> Latéraux (gauche/droite) : texte VERTICAL (`writing-mode: vertical-rl`).
> Vocabulaire unifié « haut/bas/gauche/droite » pour les deux cibles :
> widget = HORS du widget (au-dessus/dessous/à côté), fond = INTÉRIEUR
> le long du bord correspondant.

### Champs (tauri.ts + scene.rs — contrat JSON direct, camelCase)

- Widget : `titreHaut` / `titreBas` / `titreGauche` / `titreDroite`
  (`Option<String>`, vide/None = aucun titre à cette position) + style
  partagé `titrePolice` (id Google Font, fonts.ts — woff2 embarqués via
  `server.rs`), `titreTaille` (12–72 px, défaut 24),
  `titreGras`/`titreItalique`/`titreSouligne`, `titreEspacement`
  (−2 à 20 px), `titreContourCouleur` (hex, défaut `"#fff"`),
  `titreContourEpaisseur` (0–6 px, 0 = aucun contour, défaut 2),
  `titreCouleur`/`titreCouleurFin` (hex — dégradé du texte personnalisé ;
  absents = auto, dégradé repris du cadre ; `titreCouleur` seule = couleur
  unie).
- Fond : miroirs `bgTitreHaut/Bas/Gauche/Droite` + `bgTitre*` style
  (`bgTitreCouleur`/`bgTitreCouleurFin` idem).
- **LEGACY** : `titre`/`titrePosition` (« dessus »/« dessous ») et
  `bgTitre`/`bgTitrePosition` (« haut »/« bas ») — désérialisés puis migrés
  par `Scene::migrer_titres()` (pattern `migrer_cadres`) aux mêmes 4 sites
  que la migration cadres (load_scene_file, migrer_ancien_config, importer,
  presets::instancier) : « dessus »→titreHaut, « dessous »/« bas »→Bas, un
  champ nouveau déjà présent gagne, legacy vidé dans tous les cas (le JSON
  se nettoie à la prochaine sauvegarde). Jamais écrits par l'UI.

### Édition (dashboard)

Double-clic sur un widget : bord le plus proche du clic (relX/relY, min des
4 distances haut/bas/gauche/droite) → `titreModalOpen` (ui.ts — porte
`{ id, position }`) → `TitreModal.svelte` → `TitreEditForm.svelte` (corps
partagé widget + fond : 4 champs texte + style + 2 pickers de dégradé
(perso/auto-cadre) + aperçu mini-canvas avec les textes ancrés à leur
position) → `setWidgetTitre` (scene.ts). Bandes
`.titre-zone` : fines bandes ambre `--message-user-action-color` de 7 %
le long des 4 bords intérieurs (haut/bas pleine largeur, gauche/droite
entre les deux), libellées « Titre (double-clic) » (writing-mode vertical
sur les côtés) — affichées au `:hover` de TOUT widget, avec ou sans titre
existant (pointer-events:none, le dblclick est géré par le parent).
Fond : bouton « Titre du fond » (CarteEdition) →
`TitreFondModal.svelte` (même TitreEditForm) → `setBgTitre`.

### Rendu

- Dashboard : `.widget-titre.haut/.bas/.gauche/.droite` hors `.widget-3d`
  (non clippés par le cadre SVG) — haut/bas `bottom/top:100%` centrés
  horizontalement, gauche/droite `right/left:100%` centrés verticalement +
  vertical-rl. `Canvas.svelte` `.bg-titre.*` (intérieur, 8 px des bords).
  Gradient de texte = `titreCouleur`/`bgTitreCouleur` définis → dégradé
  personnalisé (`*CouleurFin` absente → couleur unie) ; sinon couleurs du
  cadre actif (`gradientCadre`, fallback blanc→gris ; `sansCadre` →
  fallback). Priorité : perso > cadre > fallback.
- Diffusion : `applyTitreWidget` (entry.titreEls par position —
  create/update/remove selon les 4 champs) / `applyTitreFond` (bgTitreEls,
  `.bg-titre` classe). Mêmes formules inline (top/left/transform/marges)
  + writing-mode vertical-rl pour les latéraux.
- **Contour des lettres** : `-webkit-text-stroke: <N>px <couleur>` réglable
  (partagé par les 4 titres) — inline dans les templates Svelte / via
  `el.style.webkitTextStroke` dans les apply* de diffusion.html.
  `paint-order: stroke fill` reste fixé dans les règles CSS (le contour est
  peint SOUS le gradient — sans paint-order il mange les glyphes).
- `scene_vierge` (scenes.rs) : couvre les 4 `bgTitre*` + legacy `bgTitre`.
