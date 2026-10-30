# Problèmes résolus par Ryozaki — 2026-10-30

## Problème n°1 — Bordure top manquante sur le splash screen

La fenêtre de splash (sans décorations) reçoit du DWM une bordure accent
sur trois côtés — gauche, droite, bas — mais jamais sur le haut. Le
correctif dessine un liseré CSS `border-top: 1px solid __BORDER__`
(`src-tauri/resources/splash.html`) coloré par `couleur_bordure()`
(`src-tauri/src/splash.rs`) : `AccentColor` du registre quand « accent
sur les bordures » est actif (`ColorPrevalence=1`), sinon
`GetSysColor(COLOR_ACTIVEBORDER)`, gris `#767676` en ultime repli.

Pourquoi ça a été laborieux : le Coder a cherché la couleur du liseré
dans la mauvaise source (palette de l'app / couleurs en dur) au lieu de
matcher les bordures que le DWM dessinait déjà sur les trois autres
côtés. Une fois la bonne source identifiée — registre DWM puis couleur
système — la solution tenait en ~20 lignes Rust et une ligne CSS.

## Modification n°2 — Liseré pointillé sans le côté gauche

Le liseré pointillé autour de la fenêtre surchargeait le panneau latéral
gauche. On a retiré le milieu du flanc gauche tout en conservant les
angles haut-gauche et bas-gauche. Technique : pseudo-élément `::after`
+ `clip-path`.

## Modification n°3 — Phase 0 widgets/modules

Renommage « Créer un widget » → « Créer un widget média » pour lever l'ambiguïté.
Correction de commentaires obsolètes (nombre de modules, liste des types avec média de fond).

## Modification n°4 — Phase 1 widgets/modules

Menu contextuel dédié pour les modules (Configurer / Masquer).
Le clic droit sur un module n'ouvre plus le menu du fond.

## Modification n°5 — Bouton œil modules

Ajout d'un bouton œil au survol des modules sur le dashboard pour masquer / réafficher sans passer par la modale.
La configuration est conservée.

## Modification n°6 — Phase 2 widgets/modules

Alignement de la branche média du menu contextuel.
Le widget musique n'est plus proposé dans la branche trou.
« Importer un média » est cohérent entre le clic droit et la carte d'édition.

## Modification n°7 — Uniformisation des boutons au survol (badges widgets/modules)

Diagnostic : les badges au survol (boutons −/+ sur les widgets, œil sur la
caméra et les modules) étaient implémentés deux fois — `.wbadge`/`.z-controls`/
`.z-btn` dans Widget.svelte et une copie renommée `.mod-badge`/`.mod-eye`/
`.mod-eye-btn` dans ModuleHud.svelte (≈ 90 lignes CSS + la logique `badgePos`
dupliquées). Comportement visuel déjà quasi identique, mais la duplication
faisait dériver le code.

Unification : un seul composant `BadgeSurvol.svelte` (ancre `.wbadge` frère de
`.widget`/`.module`, badge `.z-controls` contre-scalé, placement adaptatif
haut/bas/droite/gauche/dedans, boutons passés en props) + une recette CSS
globale `.wbadge`/`.z-controls`/`.z-btn` dans app.css — même pattern que
CtxMenu + `.ctx-*`. `.cam-vis` est fusionné dans l'état générique `visible`.

Différences assumées documentées : les modules affichent l'œil seul (pas de
−/+ — pas de z-order module, z fixe) et gardent leurs poignées resize +
outline au survol (seule affordance de resize, pas de notion de sélection).
Zéro changement de modèle de données, zéro régression clic droit / édition /
caméra / musique.
