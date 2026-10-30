# HANDOFF — StreamOS (réécrit à chaque fin de session)

## État
Lot « clic gauche module → carte à gauche » livré (2026-10-30) : carte
flottante `.carte-module` rendue par Canvas.svelte (nom + afficher/masquer
+ X/Y/L/H + « Configurer… » → modale). Zéro Rust.

## Fait [EXÉCUTÉ]
- `ModuleHud.svelte` : prop `onClic(id)` (requis — convention `onclose`
  de CtxMenu) ; `onUp` etaitClic → `onClic` au lieu de
  `modulesModalOpen.set` ; import `modulesModalOpen` retiré (la modale
  reste ouvrable via Toolbar + « Configurer… » carte/clic droit).
- `Canvas.svelte` : `carteModuleId` + `carteModule`/`carteModuleMasque` ;
  `ouvrirCarteModule` (id + `selectWidget(null)` +
  `carteEditionCible.set(null)` — exclusivité carte module ↔ carte
  widget/fond) ; fermetures ✕ / Escape (`<svelte:window onkeydown>`) /
  `$effect` `carteEditionCible` non null (clic fond, clic widget, morph) ;
  markup overlay `.carte-edition.carte-module` `absolute` dans
  `.canvas-wrap` (hors du `.canvas` scalé, après lui → au-dessus du
  canvas, sous ctx-menu 9999 et modales 10000) : nom (`modules.nom.*`) +
  toggle afficher/masquer (`setModuleActif`, carte ouverte) + X/Y/L/H
  (`setModule`, `posNum` min 0/round comme la modale) + « Configurer… »
  → `modulesModalOpen` + ferme la carte.
- `src/app.css` : recette `.carte-module-*` après `.carte-edition`
  (réutilise pointillés+glow, accent `--accent-ui`/`--glow-ui`, 14 rem,
  grille pos 2×2, inputs nombre recette `.mod-detail` de ModulesModal).
- Docs : modules-hud.md (clic gauche + « Trois modules » → cinq),
  ctx-menus.md (surface A module), problemes-resolus n°8.
- Vérif : `npm run check` 0 erreur à chaque étape.

## Fait [EXÉCUTÉ — lot précédent]
- `src/lib/components/BadgeSurvol.svelte` (nouveau) : ancre `.wbadge`
  frère de `.widget`/`.module` + `.z-controls` {haut|bas|droite|gauche|
  dedans} contre-scalé + boutons `.z-btn` via prop `boutons`
  (`BoutonSurvol`, exporté en `<script module>`) ; `badgePos`
  internalisé (`sceneStore` canvasW/H, nb = boutons.length) ; props
  `widgetId`/`moduleId` → `data-*-id` (clic droit Canvas `closest`) ;
  `visible` → `.z-controls.visible` (œil épinglé caméra/module masqué).
- `src/app.css` : recette globale `.wbadge`/`.z-controls`/`.z-btn` +
  règles d'affichage unifiées (`.widget:hover`/`.module:hover` +
  `.wbadge`, `.wbadge:hover` ; `.widget.dragging`/`.module.actif` +
  `.wbadge` masque). ⚠️ Adjacence DOM obligatoire (sélecteur `+`) :
  `.wbadge` doit suivre immédiatement `.widget`/`.module`.
- `Widget.svelte` : bloc `.wbadge` → `<BadgeSurvol boutons={badgeBoutons}
  visible={cameraCachee} widgetId>` ; `badgePos` → `badgeBoutons`
  (−/+ `mettreWidgetPlan` disabled `estDerriere`/`estDevant`, œil
  `setCameraCachee`) ; −119 lignes CSS déplacées dans app.css.
- `ModuleHud.svelte` : bloc `.mod-*` → `<BadgeSurvol boutons` (œil
  `setModuleActif`) `visible={masque} moduleId x={lx} y={ly}>` ; −134
  lignes (copie renommée supprimée). `.cam-vis` → prop `visible`.
- Docs : z-order.md, ctx-menus.md (« 3 surfaces »), camera-details.md,
  modules-hud.md (différences assumées), problemes-resolus (n°7).
- Vérif : `npm run check` 0 erreur à chaque étape.

## Décisions actées (utilisateur)
- Module = œil seul (pas de −/+ : pas de z-order module, z fixe).
- Poignées + outline module au survol conservées (seule affordance de
  resize — pas de sélection module) ; recolorisation ambre : hors lot.
- Classes `.wbadge`/`.z-controls`/`.z-btn` conservées (pas de renommage).
- Carte module : flottante au bord gauche de la zone canvas (périmètre
  fermé — pas de cible dans CarteEdition/ui.ts) ; contenu léger +
  position (pas de config exhaustive — la modale la garde).

## Constat (d) — push à chaud : aperçu OUI, enregistrement NON
commitScene → update_scene → save_current → push_snapshot → snapshot_tx
(scenes.rs:135). Deux consommateurs : WS :4321 (page diffusion) ET la
veille aperçu (moteur/mod.rs:524, boot lib.rs:4196).
- APERÇU : chaque broadcast → arreter_apercu() ; le poll suivant (~10
  fps) relance configurer_scene + demarrer_apercu sur la scène fraîche
  (moteur/mod.rs:566-581) → projecteur rechargé à chaque commit
  (kill+respawn, pas reconfig ; captures WGC recréées, flash bref).
- ENREGISTREMENT : scène figée à scene_chargee() du démarrage
  (moteur.rs:615) ; configurer_scene refuse si en_cours (:351).
  Passent quand même à chaud : buffers chat/input (Arc partagés) et
  l'équaliseur (eq.sync dans push_snapshot, scenes.rs:141). Le
  projecteur sert alors les frames de la boucle → scène figée.
→ Une capture choisie pendant une sortie native : visible au poll
  suivant dans le projecteur, jamais dans un enregistrement en cours.

## Test manuel (humain)
Mode natif, OBS fermé : « trou » on → libellés « Lier une capture »
attendus dans le clic droit, la carte d'édition ET le titre du dialogue
→ Écran puis Fenêtre (caméra absente) ; ouvrir l'aperçu natif → la
capture s'affiche. Repasser en OBS : libellés « Lier une source OBS »
comme avant ; le widget reste un trou SANS source.

## Limites
- obsSource non resynchronisé au changement de mode → lot N5.
- linkExistingObsSource et bgObsSource sans cible native ; le chemin
  « fond » du dialogue reste OBS (échoue en natif, erreur affichée).
- Caméra native absente (pas de variant CaptureCible).
- Scène non repoussée à chaud (constat d).
- Labels « OBS » restants en natif, écartés volontairement (N3b) :
  sourceObsSupprimer (obsSource persistant = littéralement vrai), clés
  fond (canvas.captureFond/videoFondObs, carteEdition.fondCaptureHint —
  bgObsSource n'existe que sur scène construite sous OBS), musiqueHint
  (« source navigateur OBS »), guide/onboarding citant « Lier une
  source OBS » (textes statiques non mode-aware).

## Écart connu (harnais)
parite_scene.rs : cible `cibles` sans captureCible → capturée + média
au même rect. Absent en prod (HashMap::new() :269/:577).

## Pièges
- trou:true SANS cible = widget ordinaire en natif (média dessiné) ;
  presets gaming-4-3.json:33/:40 à vérifier via parite_scene.
- sos-moteur-natif jamais rustfmt → lot cargo fmt isolé à faire seul.
- docs/PRINCIPES.md absent → substitut PLAN-MOTEUR-NATIF.md (créé par
  l'utilisateur — ne pas le créer).

## Questions ouvertes
- moteur-natif.md « trou » :968, :1671, :1673, :1687 — session de
  restructuration de la doc, pas un lot isolé.

## Décisions actives
- D8 : capture = porteur de captureCible (ou cibles[id] harnais), sans
  regarder trou — flag réservé au mode OBS.
- D9 : natif → trou:true + captureCible, sans obsSource ni appel OBS ;
  OBS inchangé ; caméra masquée ; obsSource existant jamais touché.

## Prochaine action unique
Diagnostic « push à chaud » : qualifier si/quand les modifs du
dashboard atteignent la sortie native en cours (constat d — a priori
non, à confirmer chemins par chemins). PUIS lot N5 (resync
obsSource au changement de mode). Test manuel humain en parallèle.
