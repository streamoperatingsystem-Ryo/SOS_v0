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
