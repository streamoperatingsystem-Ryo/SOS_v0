# Taille d'interface — base adaptative

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Taille d'interface — base adaptative à l'écran (2026-10-07, 2K/4K 2026-10-13)

> La police de base du dashboard s'adapte à la résolution de l'écran où se
> trouve la fenêtre `main` ; le select topbar (80–125 %) reste un réglage
> **relatif** à cette base — il n'est ni modifié ni borné par le calcul.

- **Formule** : `font-size` sur `<html>` = `16 px × tailleUi/100 ×
  tailleEcran/100`. `tailleEcran` = proportion de la **GRANDE DIMENSION
  LOGIQUE** du moniteur courant (`max(largeur, hauteur)` — invariante à
  la rotation : un écran portrait garde la base de sa hauteur, sa longue
  dimension ; physique ÷ `scaleFactor`, sinon le zoom Windows est
  double-compté), arrondie au pas de 5, en **2 segments** :
  - `dim ≤ 1920` : ancrée `80 % × dim/1080` (≡ 1366 px → ~100 %),
    saturée à 125 % dès ~1690 (portrait 768×1366 → ~100 %, 720p paysage
    → 95 %, 1080p → 125 %) ;
  - `dim > 1920` : proportion stricte `125 % × dim/1920` — la
    **proportion chrome/écran est IDENTIQUE sur 1080p, 2K et 4K**
    (2560 → 165 %, 3440 → 225 %, 3840 → 250 %).
  Bornes globales **55–250 %** (plafond 250 = 4K @ scaling 100 % ; un 4K
  à 150 % est logiquement un 2560 → 165 %, même proportion physique
  qu'un 2K à 100 %).
  ⚠️ Ne PAS revenir à la hauteur seule : en portrait la hauteur devient
  la longue dimension → un petit écran portrait obtiendrait une base plus
  grande qu'un paysage 1080p (régression mesurée 2026-10-07 : portrait
  768×1366 → 100 % « très bien » vs TV 1842×1036 → 75 % « tout en petit »).
- **Détection** (`stores/tailleUi.ts::initTailleUi`, App.svelte onMount,
  avant `frontend:pret`) : `currentMonitor()` (fonction module de
  `@tauri-apps/api/window` — PAS méthode de `Window`, contrairement à
  `onMoved`/`onScaleChanged`) puis recalcule sur `onMoved`/
  `onScaleChanged` debouncés 150 ms — couvre le drag de la fenêtre ET le
  saut via le sélecteur Ergonomie (les deux émettent des events de
  déplacement). Fallbacks : `window.screen` (déjà logique) →
  1920×1080. Runtime-only pour `tailleEcran`, zéro Rust.
- **Réglage utilisateur persisté PAR RÉSOLUTION** : `sos-taille-ui`
  contient un objet `{ "768x1366": 100, … }` — le select mémorise un
  niveau par taille d'écran (deux moniteurs identiques = même réglage).
  La clé `""` = ancienne valeur globale (legacy nombre seul migré à la
  1re écriture) = repli pour les écrans jamais réglés. À chaque
  changement de moniteur, `initTailleUi` ré-applique le niveau persisté
  de la résolution courante dans `tailleUi`.
- ⚠️ La BASE (`tailleEcran`) est calculée en dimensions **logiques**
  (physique ÷ `scaleFactor`) — sur un écran avec scaling Windows
  > 100 %, le grossissement OS n'est PAS re-compté. En revanche la clé de
  persistance `sos-taille-ui` reste la résolution **physique** `WxH` :
  l'identité du moniteur ne change pas quand le scaling bouge.
