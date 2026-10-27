# Système CSS unifié + scale rem

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Système CSS unifié + scale rem (2026-10-13) — convention et annulation

> Chantier « chrome responsive » : tout le chrome dashboard (topbar, sidebar,
> modales, onglets) est dimensionné en **rem** ; le canvas WYSIWYG (px/cqw)
> et les pages OBS (`diffusion.html`, `raccourcis.html`, `chat-popout.html`)
> restent en px — ce sont des contrats figés avec OBS/CEF, ne pas convertir.

### Échelle d'UI — `tailleUi × tailleEcran`

- `src/lib/stores/tailleUi.ts` : `tailleUi` = choix utilisateur (80–125 %,
  persistant **par moniteur physique**, clé `"WxH"` en px physiques —
  l'identité du moniteur, ne pas passer en logique) ; `tailleEcran` =
  pourcentage calculé sur les dimensions **logiques**
  (physique ÷ `scaleFactor` — sinon le zoom Windows est double-compté),
  arrondi par pas de 5, borné 55–250 % (2 segments : ≤ 1920 saturé à
  125, > 1920 proportion stricte 125 × dim/1920 — cf. section « Taille
  d'interface »).
- `App.svelte` pose `html { font-size } = 16 × tailleUi × tailleEcran / 10000`
  → **1 rem = taille logique du chrome**. Tout `px` structurel du chrome est
  une erreur ; les filets (1–2 px), ombres/glows et breakpoints `@media`
  (le rem y vaut 16 px, jamais le font-size root — garder px) restent en px.

### Recette `.action` — bouton canonique unique (app.css)

Un seul bouton pour tout le chrome : `class="action"`. Recette anneau
dégradé `--btn-border` + surface verre `--btn-surface` (décision IDEA02 :
l'identité vit en bordure/glow, jamais en fill teinté). **Ne jamais
re-copier le bloc dans un composant** — poser la classe + variantes :

| Variante | Rôle |
|---|---|
| `.small` | padding/font réduits (boutons inline, « Tester »…) |
| `.primary` | action principale (ring accent faible + fond légèrement teinté) |
| `.danger` | action destructive (ring/texte `--message-error-color`) |
| `.ghost` | bouton invisible (Annuler) — hover = apparition |
| `.centre` | texte centré (grid de choix de type) |
| `.actif` | état « toggle on » (ring + texte accent) — `.active` alias accepté |

Variantes locales autorisées **par-dessus** `.action` (taille/couleur sans
re-copier la recette) : `.btn-mod` (ModerationModal), `.alerte-toggle`,
`.alerte-test`, `.tab` (barres d'onglets), `.chip`, `.fit-btn`,
`.setting-toggle`, `.etat-chip`, `.ligne-btn`, `.mini-btn`, `.wbtn`/`.pl-btn`
(skin Winamp), `.ea-*` (Écran d'attente), `.mode-btn`/`.pos-btn`/`.pad-*`
(sélecteurs IVM). Leurs états restent `.selected` / `.on` / `.active` locaux.

### Autres recettes globales (app.css — ne pas re-copier)

- `.modal-bloc` / `.modal-bloc-titre` : carte bordurée + titre de section.
- `.modal-actions` : footer de boutons de modale.
- `.hint` (+`.msg-ok`/`.msg-err`/`.msg-warn`) : texte d'aide discret.
- `.field` / `.field-label` : conteneur label+contrôle / label secondaire.
- `.modal-list` + `.action.list-row` : liste scrollable de lignes cliquables.
- `.md-layout` / `.md-liste` / `.md-select` / `.md-detail` : master-detail
  (unifie les ex-`.alertes-*` d'IVM et `.mod-*` de ModerationModal —
  `.md-select.selected` = entrée active).
- `.modal-range`, `.modal-field-label`, `select` : recettes existantes.

### Procédure d'annulation du chantier (test utilisateur NOK)

L'arbre contient des modifications **non commitées pré-existantes** (module
équaliseur + correctifs) — `git checkout -- .` les détruirait aussi.
États pré-chantier sauvegardés **hors dépôt** dans
`..\annulation-chantier-rem\` (`C:\Users\Ryo\Desktop\Rust-SOS\annulation-chantier-rem` :
`AGENTS.md`, `lib.rs`, `ModulesModal.svelte` — seuls fichiers déjà
modifiés avant ce chantier).

1. `git status` → repérer les fichiers touchés par le chantier (liste :
   `src/app.css`, `src/App.svelte`, `src/lib/stores/tailleUi.ts`,
   `src-tauri/src/lib.rs`, `src/lib/components/*.svelte` modifiés ci-dessus).
2. Fichiers **sans modif pré-existante** : `git checkout -- <fichier>`.
3. `src-tauri/src/lib.rs`, `ModulesModal.svelte`, `AGENTS.md` : **restaurer
   depuis `..\annulation-chantier-rem\`** (pas git checkout — tuerait
   l'équaliseur).
4. Re-vérifier : `npm run check` + `cd src-tauri && cargo check`.
