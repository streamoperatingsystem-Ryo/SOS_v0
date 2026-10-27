# Variables CSS UI — thème & recettes

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Variables CSS UI — messages & états (NE PAS CODER LES COULEURS EN DUR)

> ⚠️ **RÈGLE — à lire avant d'écrire TOUT style de message ou d'indicateur d'état.**
> Cette section existe parce que les couleurs d'état étaient fragmentées
> (`--ok`, `--connexion-ok`, `#3c3`, `#5c5`, `#c0392b`, `#e0a060` en dur dans
> 5 composants). Unification du 2026-09-12. **NE PAS RECRÉER ce qui existe.**

### Les 3 variables (source unique : `src/theme/glace.css` → tokens `--t-*`)

| Variable | Couleur | Sémantique | Usages |
|---|---|---|---|
| `--message-user-action-color` | `#f59e0b` (orange) | Action requise de l'utilisateur | Toolbar `.obs-status.erreur` (mot de passe OBS invalide, OBS injoignable), ObsSourceDialog `.hint.warn` |
| `--message-ok-color` | `#22c55e` (vert) | Succès / état connecté | Toolbar `.obs-status.ok` + `.header.etat-ok` (en-tête section OBS connecté : texte vert), `.dot.on` partagée (bandeau top App.svelte + lignes Connexions) |
| `--message-error-color` | `#c0392b` (rouge) | Erreur / danger | Toolbar `.header.etat-erreur` (en-tête section OBS non connecté : texte + liseré gauche rouges), boutons destructifs au survol (ConfirmDeleteWidgetModal `.action.danger:hover`, SceneBar `.ctx-item.danger:hover`), `.ctrl-btn.ctrl-arreter:hover` (topbar) |

### Système de thème `src/theme/` — 3 couches (2026-09-20, spec IDEE/IDEA02.md)

> Le thème du chrome dashboard vit dans `src/theme/`, importé par `main.ts`
> dans cet ordre : `tokens.css` → `roles.css` → `glace.css` → `app.css`.
> **UN SEUL THÈME (décision 2026-09-20) : `glace` est la base définitive —
> la « variante » est la couleur utilisateur `--accent-ui` (couleur du
> cadre SVG), qui re-thème tout le chrome à chaud. Pas de sélecteur de
> thème. Si un jour un 2e thème est nécessaire : nouveau fichier
> `theme-*.css` + changer l'import dans `main.ts`, zéro touch aux
> composants.**

- **`tokens.css`** : primitives brutes `--t-*` (hex + composites : dégradés,
  glows). Seule source de `#hex` — un composant ne contient JAMAIS de hex.
- **`roles.css`** : le contrat — recettes dérivées `--btn-surface`,
  `--btn-surface-hover`, `--btn-surface-active` + reliefs `--btn-inset`,
  `--btn-inset-hover`, `--btn-inset-active` + arrondis `--rayon-*`.
  Calculées en fonction des rôles assignés par le thème ; un thème
  n'assigne QUE des couleurs.
- **`glace.css`** : LE THÈME — assigne chaque rôle (`--fond`, `--btn-tint`,
  `--accent-*`, `--dash-*`, `--message-*`…) à un token.
- **`app.css`** : structure uniquement (reset, shine/gloss `button::before`/
  `::after`, scrollbars, `.modal-*`, `.msg-pill`, couche décor `.fond-decor`
  + classes partagées `.ico` et `.dot`). Les composants consomment les
  rôles — JAMAIS un `--t-*` directement.

**Règles du thème « glace » :**
- **Pas de fill de bouton coloré au repos** : `--btn-tint: transparent`,
  `--btn-base` = verre sombre translucide (`--fond-controle` à 80%). La
  couleur vit dans les bordures, glows, textes et états actifs.
- **Exceptions sémantiques** (reposent leur `--btn-tint` coloré) :
  `.action.danger`, `.modal-action.danger`, `.speedrun-start`/`-stop` ;
  `.ctrl-arreter`/`-redemarrer`/`-refresh` (topbar) le reposent **au
  survol uniquement** — icônes neutres au repos, couleur `--ctrl-*` au
  hover (icône + teinte + bordure + glow).
- **Bordures en dégradé** : technique `background: <gradient> border-box
  border-area` — `border-area` clippe la couche à l'anneau de bordure
  (requis depuis que les fills sont translucides ; sans ça le dégradé
  transparaît sous tout le bouton). Chrome/WebView2 ≥ 128.
- **Fond d'ambiance** : `--fond-ambiance` (aurora en radial-gradients) est
  le fond de `.fond-decor` — PAS sur `body` (un fond body, bloc in-flow,
  se peindrait par-dessus le calque z-index -1 et le masquerait). `body`
  n'a aucun fond ; le fond plat `--fond` reste sur `:root`. `.fond-decor`
  (`position: fixed; z-index: -1; pointer-events: none`, définie dans
  `app.css`, montée dans `App.svelte`)
  contenant, dans l'ordre de peinture : `.fond-image` (image utilisateur
  pleine fenêtre + voile assombrissant/lavé `--cadre-couleur`), les orbs
  (arcs orbitaux — cercles géants hors-champ), `.ligne` (diagonale fine) et
  les `.boule` (points variés 3–17px remontant le bord gauche). Tout est
  SOUS le contenu : rien ne masque le texte. Les panneaux fixes (header,
  `.sidebar-col`, `.scene-bar`, `.drop-bar`) sont `transparent`.
  `--fond-panneau` reste pour les panneaux flottants (modales, dropdowns,
  carte d'édition).
- **Accent utilisateur (couleur du cadre)** : `App.svelte` pousse la
  couleur EFFECTIVE du cadre unifié (`cadreWidget` — source unique)
  dans `--cadre-couleur` / `--cadre-couleur-fin` sur `:root`, résolue via
  `gradientCadre()` de `lib/cadres/registre` — le même resolver que le
  rendu : TOUS les styles → `couleur`/`couleurFin` (les tons internes des
  presets sont dérivés de ces couleurs ; `variante` est legacy, désérialisé
  mais ignoré).
  `--accent-ui` = `var(--cadre-couleur, var(--accent-violet))` + dérivés
  `--glow-ui`, `--bordure-ui`, `--bordure-ui-active`, `--gradient-cadre`
  (glace.css).
  Tout le chrome les consomme (bordures de boutons, hovers, focus,
  anneaux, modales). `--bordure`/`--bordure-active` RESTENT neutres —
  consommés aussi par le canvas (outline de sélection widget), hors
  périmètre. Les signaux sémantiques (`--accent-carte`, `--dash-*`,
  `--ctrl-*`, `--plat-*`, `--message-*`) ne suivent PAS `--accent-ui`.
  `--accent-violet` reste la marque (logo, `--gradient-sig`).
- **Image de fond dashboard** : `stores/fondSidebar.ts` (localStorage
  `sidebarFond` + `tauri.importAlerteMedia("image")` — pipeline média
  commun, zéro Rust ajouté) ; boutons dans Personnalisation.
- **Sidebar** = liste plate : icône SVG `.ico` + label + chevron `›` ;
  lignes **transparentes au repos**, pilule verre au survol ; section
  ouverte = pilule neutre discrète `color-mix(--texte 7%)` ; chaque
  `.section` porte un encadré `--bordure-ui` (accent cadre), renforcé
  `--bordure-ui-active` quand ouverte (le contenu déplié reste dans le
  cadre) ; section OBS non connectée = texte `--message-error-color` +
  liseré rouge `inset 3px 0 0` à gauche. Chaque `.section` porte aussi une
  classe permanente `.sec-*` (couleurs → crochet, cf. « Crochets colorés »).
- **Accent primaire** : `--accent-rose` (`#d4888f`) — bouton « Vos scènes »
  (pilule pleine : dégradé vertical léger + liseré sombre, glow au survol).
  Quand le cadre SVG de l'app est actif, la pilule suit `--cadre-couleur`
  (via `var(--cadre-couleur, var(--accent-rose))`, comme le décor).

| Variable | Valeur | Rôle |
|---|---|---|
| `--fond` / `--texte` | `#08080f` / `#e5e7eb` | Fond app (noir bleuté) + texte principal |
| `--gris` | `#8b8fa3` | Texte secondaire : placeholders, hints, outline canvas (Canvas.svelte) |
| `--gris-fonce` | `#12121e` | Fonds secondaires : hover subtil, inputs |
| `--bordure` | `#1e1e30` | Bordures NEUTRES — canvas (outline sélection widget) + base des variantes teintées |
| `--bordure-active` | `#2a2a40` | Bordure neutre renforcée — idem, canvas uniquement |
| `--bordure-ui` / `--bordure-ui-active` | `color-mix(--accent-ui 35%/65%, --bordure*)` | Bordures du CHROME teintées couleur cadre (boutons, sections, séparateurs) |
| `--fond-panneau` | `#0d0d16` | Fond des panneaux FLOTTANTS (modales, dropdowns, carte d'édition) — les panneaux fixes sont transparents |
| `--fond-controle` | `#15151f` | Substrat du verre boutons (`--btn-base`) + fonds inputs |
| `--fond-ambiance` | composite radial-gradients | Fond aurora — fond de `.fond-decor` (PAS sur `body`) |
| `--accent-ui` | `var(--cadre-couleur, --accent-violet)` | Accent du chrome entier : bordures, focus, glows, anneaux — suit la couleur du cadre |
| `--glow-ui` | `0 0 8px color-mix(--accent-ui 40%)` | Glow externe teinté cadre (hovers, dropdowns) |
| `--gradient-cadre` | `linear-gradient(135deg, --cadre-couleur, --cadre-couleur-fin)` | Anneaux de boutons SceneBar — dégradé exact du cadre |
| `--accent-violet` | `#8b5cf6` | Marque (logo) + fallback de `--accent-ui` sans cadre |
| `--accent-orange` | `#f97316` | Accent néon secondaire (scène active, glows SceneBar) + fallback décor |
| `--accent-magenta` | `#ec4899` | Milieu du dégradé signature |
| `--accent-rose` | `#d4888f` | Fallback de « Vos scènes » quand aucun cadre n'est actif |
| `--gradient-sig` | `linear-gradient(135deg, #f97316, #ec4899, #8b5cf6)` | Dégradé signature — marque uniquement (logo, titre) |
| `--glow-orange` | `0 0 8px rgba(249,115,22,0.4)` | Glow pastille scène active |
| `--rayon-petit` | `5px` | Boutons, inputs, selects (appliqué globalement dans app.css) |
| `--rayon` | `8px` | Blocs, barres, conteneurs |
| `--rayon-grand` | `14px` | Modales (`.modal` / `.dialog` des 5 modales) |

### Carte d'édition contextuelle — 3 cartes colorées (2026-09-15)

> La carte d'édition (`CarteEdition.svelte`, panneau latéral gauche) adopte une
> couleur selon la cible sélectionnée. La couleur vit dans la bordure des
> boutons `.action` (`--btn-border` = `--accent-carte` plein) + le glow
> hover/active — le fill reste le verre sombre neutre commun (IDEA02).

| Cible | Classe CSS | Couleur | Variable | Condition |
|---|---|---|---|---|
| Widget classique | `.cible-widget` | Rouge bordeaux | `--dash-danger` (`#7f1d1d`) | widget sélectionné (sauf input-viewer) |
| Fond de l'application | `.cible-fond` | Orange | `--dash-orange` (`#9a3412`) | fond sélectionné (clic canvas vide) |
| Widget input-viewer | `.cible-input-viewer` | Jaune | `--dash-jaune` (`#a16207`) | widget input-viewer sélectionné |

- La classe CSS est **dérivée** dans `CarteEdition.svelte` (`carteClasse`) :
  `cible === "widget" && isInputViewerWidget` → `cible-input-viewer`, sinon
  `cible-${cible}`. La cible du store reste `"widget"` (pas de nouveau type) —
  seul le rendu CSS change.
- Variables `--dash-orange` / `--dash-jaune` définies dans `theme/glace.css`
  (palette `--dash-*`, couleurs froides assombries — cohérent avec
  `--dash-danger`).

### Boutons — recette « verre sombre » + reflet animé (thème glace, 2026-09-20)

> Recette partagée par toute l'UI — un contexte = surcharger les rôles
> (`--btn-tint`, `--btn-border` local), jamais copier le CSS.
> Recettes dérivées : `src/theme/roles.css` ; valeurs : `src/theme/glace.css` ;
> keyframes + `button::before` global : `src/app.css`.

| Variable | Valeur (glace) | Rôle |
|---|---|---|
| `--btn-tint` | `transparent` | Teinte du verre + du reflet. `transparent` par défaut — PAS de fill coloré au repos ; surchargé UNIQUEMENT par les états sémantiques |
| `--btn-base` | `--fond-controle` à 80% | Substrat translucide du verre (le fond ambiant transparaît) |
| `--btn-border` | pas de défaut global | Anneau de bordure — assigné localement par contexte (porte la couleur d'identité) |
| `--btn-surface` | dégradé 180° tint→base | Surface verre — repos |
| `--btn-surface-hover` | dégradé + intense | Surface — survol |
| `--btn-surface-active` | dégradé inversé (substrat plein en haut) | Surface — pression (`:active`) |
| `--btn-inset` / `--btn-inset-hover` | inset haut lumineux + bas sombre + halo `color-mix(--accent-ui 16%/30%)` | Relief verre bombé + lueur cadre — repos / survol |
| `--btn-inset-active` | `inset 0 2px 4px rgba(0,0,0,.4)` | Relief creusé — pression (`:active`) |

**Qui surcharge `--btn-tint` (sémantique, exhaustif) :**
`.speedrun-start`/`.speedrun-stop` (CarteEdition), `.action.danger`
(DropBar), `.modal-action.danger:hover` (app.css) ; `.ctrl-arreter`/
`.ctrl-redemarrer`/`.ctrl-refresh` (topbar) **au `:hover` seulement**. Tout
autre usage = bug à corriger.

**Couleur de contexte sans fill** : `--btn-border` porte l'identité
(`--accent-carte` sur CarteEdition `.action`, `--dash-danger` sur DropBar
danger, `--gradient-cadre` sur les anneaux SceneBar). Technique anneau :
`<gradient> border-box border-area` (clip à la bordure — voir règle
`border-area` ci-dessus).

**Verre dépoli** : `button { backdrop-filter: blur(6px) }` (global dans
`app.css`) — le décor (aurora, image, boules) transparaît flouté derrière
chaque bouton. La lueur cadre au repos vit dans `--btn-inset` (halo
`--accent-ui`, cf. table ci-dessus).

**Reflet animé** : `button::before` (global dans `app.css`) en `z-index: -1`
(au-dessus du fond, sous le texte), `isolation: isolate` sur le bouton pour
contenir le z-index négatif. `@keyframes btn-shine` = sweep `translateX` +
`skewX(-18deg)`, 0.7s ease-out, one-shot au `:hover:not(:disabled)`.
Invisible par défaut (teinté `--btn-tint` = transparent) ; réapparaît sur
les boutons sémantiques qui reposent un `--btn-tint` coloré.

### Crochets colorés — sections + modales (2026-10-04)

> Le « crochet » = liseré vertical de 3px `inset 3px 0 0` sur le bord gauche.
> Présent EN PERMANENCE sur chaque section de la sidebar et sur le flanc
> gauche de chaque modale — couleur = celle du bouton/section d'origine.

- **Sections** (`Toolbar.svelte`) : chaque `.section` porte une classe
  PERMANENTE `.sec-*` (`sec-widgets`, `sec-connexions`, `sec-interactions`,
  `sec-attente`, `sec-jc`, `sec-moderation`, `sec-diagnostic`, `sec-obs`,
  `sec-perso`, `sec-ergo`) qui pose DEUX variables
  indépendamment de l'ouverture :
  - `--accent-carte` : teinte carte SOMBRE (`--carte-*` 950/700 — fond +
    bordure pointillée de la carte ouverte, `.cible-*` reste conditionnel) ;
  - `--accent-crochet` : teinte VIVE (`--crochet-*` niveaux 400-500,
    glace.css) — la barre `.header` l'utilise pleine teinte
    (`box-shadow: inset 3px 0 0 var(--accent-crochet, var(--accent-ui))`).
  - Hiérarchie au survol : `.header:hover` repasse la barre en `--accent-ui`
    + glow ; `.etat-erreur` (OBS non connecté) la repasse en
    `--message-error-color` — le rouge sémantique prime toujours.
- **Modales** (`Modal.svelte`) : prop `crochet` (couleur CSS, défaut
  `var(--accent-ui)`) → `--crochet-modal` sur `.modal-shell`, empilé dans le
  box-shadow après le glow existant (`inset 3px 0 0`, suit le
  border-radius). Chaque modale passe la couleur de son bouton lanceur :
  Interactions `rose`, Écran d'attente `cyan`, Modération `indigo`,
  Diagnostic `teal`, Cadres `violet`, ObsSource `dore`, Device `vert`,
  widgets (suppr/titre/speedrun) `--message-error-color` (carte widget
  rouge), Titre du fond `orange`, InputBinding `jaune`, scènes (presets/
  dupliquer) `--accent-rose` saumon.
- **Tokens** (`tokens.css`) : les teintes vives dédiées sont
  `--t-bleu-500/emeraude-500/jaune-500/ardoise-400/rose-500/cyan-500/
  indigo-500/teal-500` ; violet/ambre/orange réutilisent la palette. Ne
  JAMAIS utiliser les `--carte-*` (950/700) pour une barre de 3px —
  invisibles sur `--fond-panneau`.

### Pastille de message centré — recette `.msg-pill` (2026-09-19)

> Badge centré sur un widget ou le fond du canvas (lecture OBS, input
> viewer). Source unique : `src/app.css`. **Ne jamais re-copier le bloc**
> dans un composant — la triple duplication (`.lecture-obs` /
> `.input-viewer-obs` / `.lecture-obs-fond`) a été unifiée.

- `.msg-pill` : conteneur (centrage `translate(-50%,-50%)`, fond `--fond` 82%, `--rayon-petit`, `font-size: clamp(0.5rem, 9cqw, 1.25rem)`).
- `.msg-pill-text` : sur le `<span>` texte — 4 lignes max + ellipsis (`-webkit-line-clamp:4` ; `min-width:0` requis pour le shrink flex).
- `.msg-pill-icon` : icône ▶ à `1.08em` (suit le clamp cqw).
- **Prérequis cqw** : un ancêtre `container-type: inline-size` (`.widget-3d`, `.bg-layer`), sinon fallback % viewport.
- Couleur de texte : `--texte` par défaut ; les badges « rendu dans OBS » (`.lecture-obs` widget vidéo, `.lecture-obs-fond` fond canvas, `.input-viewer-obs`) partagent `--message-user-action-color` (orange) via une règle globale app.css — un seul message = une seule couleur, lisible quel que soit le cadre (2026-10-04 : remplace l'ancien `color:` inline = couleur du cadre, illisible quand l'utilisateur choisissait une couleur sombre — NE PAS réintroduire).

### Couleurs de marque des plateformes (décoratif, 2026-09-15)

> Exception documentée à la règle Variables CSS UI : `--plat-*` ne doit
> JAMAIS servir pour un message/état ok/erreur — uniquement décoratif
> (pastille plateforme connectée dans le bandeau + modales). Connecté =
> couleur de marque + glow ; déconnecté = gris (`--gris-fonce`). OBS et
> Diffusion gardent le vert sémantique (`--message-ok-color`).

| Variable | Valeur | Plateforme |
|---|---|---|
| `--plat-twitch` | `#a855f7` | Twitch (violet) |
| `--plat-kick` | `#22c55e` | Kick (vert) |
| `--plat-youtube` | `#f43f5e` | YouTube (rouge) |
| `--plat-tiktok` | `#ec4899` | TikTok (magenta) |
| `--plat-obs` | `#d1d5db` | OBS (gris clair) |
| `--plat-diffusion` | `#22c55e` | Diffusion (vert) |

### Couleurs contrôles & snap (centralisées, 2026-09-15)

> Source unique pour les boutons topbar (Arrêter/Redémarrer/Refresh OBS dans
> `App.svelte`), le gizmo 3D (`Gizmo3D.svelte`, `var()` dans les attributs SVG)
> et le guide magnétique (`AlignmentGuides.svelte`, lu via `getComputedStyle`
> car rendu canvas). Aliases des variables sémantiques — changer la couleur de
> base propage partout.

| Variable | Valeur | Usages |
|---|---|---|
| `--ctrl-arreter` | `var(--message-error-color)` (rouge `#c0392b`) | Bouton Arrêter ■, traits bords du guide, cadre du gizmo |
| `--ctrl-redemarrer` | `var(--accent-orange)` (orange `#f97316`) | Bouton Redémarrer ↻, axes/croix du gizmo |
| `--ctrl-refresh` | `var(--message-ok-color)` (vert `#22c55e`) | Bouton Refresh ⟳, viseur du guide, boule du gizmo |
| `--ctrl-centre` | `#ffee00` (jaune) | État centré/snap : viseur du guide proche du centre, boule du gizmo à (0,0) |

**Exclusions volontaires (WYSIWYG + règle figée cadres) :**
- `Widget.svelte` : `.widget-3d` garde `border: 1px solid var(--texte)` + outline
  de sélection blancs — le widget canvas représente ce qu'OBS affiche
  (diffusion.html `.widget` a sa propre bordure rgba blanche).
- `diffusion.html` : rendu OBS, JAMAIS touché par le style dashboard.
- `src/lib/cadres/*.ts` + `registre.ts` : palettes de rendu des cadres SVG,
  exception volontaire aux variables CSS (couleurs de données, pas de style UI).
- `AlignmentGuides.svelte` : guides snap dessinés en canvas via JS, couleurs
  lues depuis les variables `--ctrl-*` via `getComputedStyle` (cf. section
  ci-dessous) — pas de `#hex` en dur.
- Base globale `button/input/select/textarea` (radius + transition) + **accent-color
  violet néon sur tous les sliders/checkbox/radios** (`app.css` — jamais le bleu
  navigateur) — ne pas la dupliquer dans les composants.

### Règles

1. **Tout nouveau message, hint d'avertissement, indicateur d'état ou bouton
   destructif DOIT utiliser ces variables** — jamais de `#hex` dans un composant.
2. **Ajuster la teinte = modifier UNE ligne dans `app.css`** — jamais dans les composants.
3. Une nouvelle sémantique de couleur (ex. info bleu) = ajouter une variable
   `--message-<sémantique>-color` dans `app.css` + documenter dans le tableau ci-dessus.
4. Les messages d'erreur destinés à l'utilisateur doivent être **clairs et
   actionnables** (traduits depuis l'erreur technique, cf. `messageClair()` dans
   `src/lib/stores/obs.ts` qui ouvre en plus la section concernée).

### Exceptions volontaires (ne PAS migrer)

- **Palettes des cadres SVG** (`src/lib/cadres/cadre-*.ts`, `CadresModal.svelte`) :
  couleurs de rendu des cadres, pas des messages UI.
- **Guide de snap canvas** (`AlignmentGuides.svelte`) : dessiné en canvas via
  JS, couleurs `--ctrl-arreter` (traits) / `--ctrl-refresh` (viseur) /
  `--ctrl-centre` (viseur centré) lues via `getComputedStyle`.
- **`color: #fff` sur fond coloré** (texte des boutons ON/danger) : constante de
  contraste sur une couleur variable, pas une couleur d'état.
- **Valeurs de DONNÉES** (couleur par défaut des cadres/alertes stockée en
  config : `scene.ts`, `alertes.ts`) : contenu persisté, pas du style UI.

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `color: #e0a060` (ou tout hex de message/état) dans un composant Svelte.
2. ❌ Recréer une variable doublon (`--ok`, `--connexion-ok`, `--warning`, …) —
   elles ont été SUPPRIMÉES au profit des 3 variables `--message-*-color`.
3. ❌ Afficher une erreur technique brute à l'utilisateur (ex. "OBS: connexion
   fermée par le serveur") sans traduction actionnable.
4. ❌ Ajouter un état visuel sans vérifier si une variable existe déjà (grep
   `--message-` dans `app.css` AVANT d'écrire du style).
5. ❌ `border: … solid var(--texte)` sur le chrome dashboard (boutons, inputs,
   modales, sections) — utiliser `var(--bordure)`. Seule exception : les
   widgets canvas de `Widget.svelte` (WYSIWYG diffusion).
6. ❌ Arrondir les widgets canvas ou toucher `diffusion.html` pour le style —
   règle figée cadres (voir section dédiée).
