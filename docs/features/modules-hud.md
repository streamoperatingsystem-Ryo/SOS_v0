# Modules HUD — horloge / musique / objectifs

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Modules HUD — horloge / musique / objectifs (2026-10-05)

> Couche d'affichage scène-persistée SÉPARÉE des widgets
> (`Scene.modules`, pas un `type` de widget). Cinq modules (clés
> `MODULE_IDS` : horloge, musique, objectifs, equaliseur, spots —
> `spots` n'a PAS de carte : effet plein canvas, aperçu statique
> seulement), chacun affichable/cachable/configuré par scène, les
> cartes positionnées en **px canvas** par drag/resize direct sur le
> canvas dashboard. Rendus au z 9000+ en diffusion (au-dessus de
> tous les widgets) + cadre optionnel réutilisant le gradient du
> cadre de scène (jamais le CadreSVG imposé — le module est une
> carte, pas un widget).

- **`ModuleHud` (scene.rs)** : struct UNIQUE pour les 3 modules —
  `actif/x/y/largeur/hauteur/police/taille/opacite/cadre` communs
  + options spécifiques optionnelles ignorées par les autres
  (`afficherDate/secondes/format12h`, `afficherTemps`,
  `titre`/`lignes` pour objectifs). Patch générique
  `Partial<ModuleHud>` côté frontend. camelCase volontaire —
  contrat JSON direct avec scene.ts.
- **`LigneObjectif`** : une ligne du bloc objectifs =
  `{ id, label?, plateforme?, metrique?, cible?, goalId? }`.
  `goalId` = Creator Goal Twitch lié (valeur lue en direct via le
  frame `goal-twitch`, comme le widget goal) ; sinon ligne libre
  plateforme+métrique+cible manuelle. Ordre persistant
  (monter/descendre dans la modale).
- **Setters** (`stores/scene.ts`) : `setModule(id, patch)` (upsert
  sur `MODULE_DEFAUTS` + commit), `ajouterLigneObjectif`,
  `setLigneObjectif`, `retirerLigneObjectif`, `deplacerLigneObjectif`.
  `moduleCourant(id)` = config effective (défauts si absent).
  `scene_vierge` compte les modules comme du contenu (test
  `scene_vierge_detecte_contenu` couvre les 3 cas).
- **`ModuleHud.svelte`** : carte drag/resize (mêmes 8 poignées et
  règles min 80×36 que l'overlay d'interactions — diviseur de
  `scale` obligatoire, même invariant que les widgets). Clic gauche
  sans déplacement = prop `onClic(id)` → carte flottante
  `.carte-module` au bord gauche de la zone canvas (Canvas.svelte,
  2026-10-30) : nom + afficher/masquer + X/Y/L/H + « Configurer… »
  → `modulesModalOpen.set(id)` — la modale reste la surface
  exhaustive (même rôle que « Configurer… » du clic droit).
  Mutuellement exclusive avec la carte d'édition widget/fond
  (`carteEditionCible`) ; fermeture ✕ / Escape / clic fond / clic
  widget. Aperçu statique réel (date/heure du
  jour, première piste de la file, lignes objectifs avec valeurs
  live `$valeursGoals`/`$goalsTwitch` + suggestion `goalSuggestion`).
  **Badge œil au survol** (`BadgeSurvol.svelte` — recette
  `.wbadge`/`.z-controls`/`.z-btn` partagée avec les widgets dans
  app.css, 2026-10-30) : masquer / réafficher via `setModuleActif`,
  épinglé (prop `visible`) quand le module est masqué — même
  sémantique que l'œil caméra ; masqué pendant drag/resize
  (`.module.actif`). **Différences assumées vs widgets** : œil SEUL
  (pas de −/+ — pas de z-order module, z fixe) et poignées resize +
  outline `--mod-accent` visibles au survol (pas de notion de
  sélection pour les modules — c'est leur seule affordance de
  resize ; les widgets n'affichent les poignées que sélectionnés,
  via `SelectionOverlay`).
- **`ModulesModal.svelte`** : master-detail 3 modules (badge
  ON/OFF). Champs X/Y/L/H = complément précis du drag/resize.
  Bloc objectifs : éditeur de lignes libres (label, plateforme,
  métrique, cible + Suggérer, réordonner, supprimer) + liaison
  Creator Goal Twitch (même liste `twitch_goals_lister` que la
  CarteEdition goal — relue, zéro persistance, `need_reauth` →
  bouton reconnexion).
- **Diffusion** (`diffusion.html`) : `renderModules` (create/update/
  remove par id dans `#modules-root`, z 9000 — DOM conservé entre
  snapshots, rebuild uniquement au changement de structure) —
  appelé dans `renderScene`. `.module` + `appliquerStyleModule`
  (position/dims/police/`--mod-fs`/`--mod-opacite`/`--mod-accent`
  depuis `bpmResoudreCouleurs`) ; `cadre: true` → `appliquerCadreOverlay`
  (même overlay SVG de cadre que les overlays existants, opt-in).
  Ticks : `assurerTickHorloge` (1 s, auto-arrêt), `assurerTickMusique`
  (250 ms partagé avec les widgets musique — même invariant) ;
  `majModuleMusique` est appelée depuis `majMusiqueWidgets`
  (transitions play/stop, jamais de rebuild DOM) ;
  `majModulesObjectifs` est appelée depuis les sinks `viewers` /
  `communaute` / `goal-twitch` (valeurs patchées in-place via
  `ligneEls`, jamais de re-render).
- **i18n** : `modules.*` + `toolbar.modules*` dans les 8 locales
  (rendu diffusion non traduit — labels objectifs viennent du
  module, `goalTwitchLabel`/`goalMetriqueLabel` restent FR comme
  les widgets goal).

| Fichier | Rôle |
|---|---|
| `src-tauri/src/scene.rs` | `ModuleHud`, `LigneObjectif`, `ScenesModules`, `Scene.modules` (serde default) |
| `src-tauri/src/scenes.rs` | `scene_vierge` compte les modules + test |
| `src/lib/tauri.ts` | `ModuleHud`/`LigneObjectif`/`ModuleId`/`MODULE_IDS`/`ScenesModules` + `Scene.modules` |
| `src/lib/stores/scene.ts` | `moduleDefaut`/`moduleCourant`/`setModule` + setters lignes |
| `src/lib/components/ModuleHud.svelte` | Carte drag/resize + aperçu statique + badge œil (`BadgeSurvol`) |
| `src/lib/components/BadgeSurvol.svelte` | Badge de survol partagé widgets/modules (ancre `.wbadge` + `.z-controls`) |
| `src/lib/components/ModulesModal.svelte` | Master-detail config (modale Modules) |
| `src/lib/components/Canvas.svelte` | Couche modules après les widgets + carte flottante `.carte-module` (clic gauche) |
| `src/lib/components/Toolbar.svelte` | Lanceur « Modules » (lanceur direct) |
| `src/lib/stores/ui.ts` | `modulesModalOpen: ModuleId \| null` |
| `src/App.svelte` | Montage `{#if $modulesModalOpen}` |
| `src-tauri/resources/diffusion.html` | CSS `.module`/`.mh-*`/`.mm-*`/`.mo-*` + `renderModules`/`majModule*`/`assurerTick*` |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `modules.*` + `toolbar.modules*` + `commun.hauteur` |
