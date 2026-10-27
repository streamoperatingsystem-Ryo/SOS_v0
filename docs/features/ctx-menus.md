# Menus contextuels — recette .ctx-* unifiée

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Menus contextuels — recette `.ctx-*` unifiée (2026-10-07)

> Une seule recette visuelle (app.css) + un seul wrapper (CtxMenu.svelte)
> pour tous les menus clic droit et les dropdowns du chrome.

- **Recette CSS** `.ctx-menu` / `.ctx-item` / `.ctx-sep` / `.ctx-label` /
  `.ctx-fit-*` / `.ctx-confirm` (app.css) : verre dépoli teinté
  `--accent-ui`, filet `--gradient-cadre`, animation `ctx-pop`. Ne jamais
  re-copier le bloc dans un composant.
- **`CtxMenu.svelte`** : wrapper des menus **fixed à la souris** — props
  `{x, y, onclose, children}` ; gère stopPropagation, preventDefault
  contextmenu, fermeture clic extérieur + Escape (svelte:window interne)
  et **clamping viewport** (marge 8 px, re-mesuré par ResizeObserver).
  Utilisé par SceneBar (menu pastille) et Canvas (menus fond + widget).
  Les dropdowns ancrés sous un bouton n'utilisent PAS le composant : ils
  posent `.ctx-menu` en `position:absolute` (pattern `.langues-dropdown`).
  Exception : `.pastilles-dropdown` (« Vos scènes ») = panneau TABLEAU
  centré dans le viewport (fixed + voile `.pastilles-backdrop` z 9990, sous
  les modales à 10000) — grille `auto-fit` à cellules bordurées, pas un
  menu ancré. Cellules enrichies (mini-map + infos) : cf.
  `docs/features/tableau-scenes.md`.
- **`.ctx-select`** (app.css) : recette pour le **popup d'un `<select>`
  natif** — `appearance: base-select` (Chromium ≥135 / WebView2 evergreen)
  rend `::picker(select)` stylable (panneau .ctx-menu + options façon
  .ctx-item, `option:checked` actif, coche `option::before` accentuée,
  `select:open`, `::picker-icon`). Repli gracieux natif si non supporté.
  Posée sur le select zoom topbar (`.taille-ui-select`) ; ne pas
  l'appliquer globalement — les selects de formulaires gardent leur look.

## Règle « 3 surfaces d'édition widget » (2026-10-26)

> Répartition des actions d'édition d'un widget entre les 3 surfaces du
> dashboard — à respecter à chaque ajout de contrôle.

- **A — CarteEdition** (panneau latéral) : réglages d'état persistants,
  configuration exhaustive, découverte.
- **B — Menu contextuel clic droit** (Canvas.svelte) : inventaire des
  VERBES / actions ponctuelles — miroir élargi de la carte.
- **C — Badges ambre au survol** (`.wbadge` / `.z-controls`,
  Widget.svelte) : ACCÉLÉRATEURS — 1 à 3 boutons max, actions
  haute-fréquence liées à l'état visuel du widget (plan −/+, œil caméra).

Règles :

1. **Toute action existe dans A ou B — jamais exclusive au badge C.** Un
   badge peut être inatteignable (widget recouvert par un module HUD
   z 8000, widget hors écran) : A/B restent le chemin garanti.
2. Un badge qui signale un état actif reste **visible en permanence**
   (pattern `.cam-vis.visible`, Widget.svelte).
3. Le clic droit doit viser juste partout où le widget est représenté :
   `data-widget-id` est porté par `.widget`, `.wbadge` (badge de survol)
   et `.sel-overlay` (poignées) — `closest("[data-widget-id]")` dans
   `onCanvasContextMenu`.
