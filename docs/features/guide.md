# Guide du streamer — livre feuilletable

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Guide du streamer — livre feuilletable intégré (2026-10-03)

> Guide utilisateur en langage simple, rendu comme un livre ouvert dont
> les feuilles basculent en 3D autour de la reliure (référence : carnet
> Tomb Raider). **Dashboard uniquement** — jamais dans `diffusion.html`
> ni aucune page OBS. Plan complet : `PLAN-GUIDE-STREAMER.md`.

### Lanceur & montage

- **Lanceur du guide** = section LANCEUR DIRECT `.section.sec-guide`
  (`Toolbar.svelte`, icône livre + `toolbar.guide` + chevron — même
  mécanique que Modération / Diagnostic, pas un accordéon). Pied fixe de
  la sidebar **juste au-dessus d'Ergonomie** (sibling de l'aside dans
  `.sidebar-col`, hors zone scrollable). Crochet `--crochet-ardoise` —
  même famille qu'Ergonomie, les deux entrées du bas forment un bloc.
  (Sorti d'Ergonomie le 2026-10-06 : avant, c'était un `.action` dans le
  contenu de l'accordéon.)
- **Section « Ergonomie »** = pied fixe de la sidebar (`Toolbar.svelte`,
  `.section.dash-fond` — sibling de l'aside, `flex-shrink:0`, hors zone
  scrollable, sous le lanceur du guide). Accordéon standard
  (`toggleSection("ergonomie")`, union `Section` de `ui.ts`) regroupant
  le fond du chrome dashboard (ex-« Fond du dashboard », déplacé là) +
  fenêtre de démarrage + barre de raccourcis. Carte **ardoise** à
  l'ouverture : `--carte-ardoise` (glace.css ← `--t-ardoise-700` — niveau
  700 exigé : un 950 serait invisible en bordure pointillée sur
  `--fond-panneau`), variante `.cible-ergo` au même mécanisme que les
  autres `.cible-*`. La `.dot` marque le fond dashboard posé ; surcharge
  `.dash-fond .chevron` (margin-left réduit — sinon trou entre la `.dot`
  poussée à droite par `margin-left:auto` et le chevron).
- Flag `guideLivreOpen` (`ui.ts`) → `{#if $guideLivreOpen}<GuideLivre/>`
  dans `App.svelte`, à la suite des autres modales.

### Mécanique du livre (`GuideLivre.svelte`)

- **Modèle** : `N = ceil(PAGES.length / 2)` feuilles physiques ; feuille
  `i` : recto = `PAGES[2i]`, verso = `PAGES[2i+1]`. État `tournees` =
  feuilles tournées (0..N) ; à t=0 la base gauche = page de garde, à t=N
  la base droite = 4ᵉ de couverture.
- **3D CSS, pas SVG** : `perspective:1600px` sur `.livre-scene`,
  `preserve-3d` sur `.livre` ET `.feuille`, `transform-origin: left
  center` (reliure), `backface-visibility:hidden` (avec `-webkit-`) sur
  les deux faces, flip `rotateY(-180deg)` en 0.65 s. Le SVG ne sert qu'au
  décor (ornements dorés) — les éléments SVG ne supportent pas les
  transforms 3D.
- **Z-index** : pile droite `calc(--n-feuilles - --i)`, pile gauche
  `calc(--i + 1)` — les vars sont posées en style inline par feuille.
- **État** : verrou `enTransition` 680 ms ; `feuilleEnMouvement` reçoit
  seul `will-change`. **Reprise de lecture** :
  `localStorage["sos-guide-page"]` (bornée [0,N], lecture/écriture
  non-fatales).
- **Navigation** : zones tranche gauche/droite (72px, à cheval `-8px` sur
  le bord du livre) — flèche SVG `opacity:0→0.9` au hover + soulèvement
  du coin (`rotateY(-7deg)` recto / `-173deg` verso, classes `survol-*`
  sur `.livre`) ; masquées aux extrémités. Clavier `←`/`→` + `Escape` ;
  clic-overlay pointerdown+pointerup (pattern `Modal.svelte`) ; ✕
  recette `.modal-close`.
- **Sommaire cliquable (2026-10-06)** : bloc `{ type: "sommaire" }` rendu
  depuis `chapitres()` (contenu.ts). `allerA(folio)` → cible
  `ceil((folio-1)/2)` feuilles ; les feuilles intermédiaires tournent **en
  cascade** (`basculer(dir)` toutes les `ECART_CASCADE_MS = 120` ms), verrou
  `enTransition` tenu jusqu'à `(n-1)×120 + DUREE_FLIP_MS` ; `timersCascade`
  annulés dans `onDestroy`. `tourner()` = `basculer` + `armerFinTransition`.
  Entrées = `<button class="sommaire-entree">` (titre + pointillés de
  conduite + folio) → même neutralisation du chrome global que
  `.zone-tourne`. Étiquette `.chapitre-label` (dorée, capitales) au-dessus
  du titre quand la page porte `chapitreCle`.
- **Retour au sommaire (2026-10-06)** : `<button class="retour-sommaire">`
  UNIQUE, posé dans `.livre-scene` (**sibling de `.livre`, HORS du
  contexte 3D des feuilles** — ni transform ni miroir : toujours au
  même endroit, z-index 70), aligné sur le coin haut-gauche de la page
  de gauche. Affiché via `pageGauche = PAGES[2*tournees-1]` : caché à
  t=0 (garde), sur verso orphelin et quand la page de gauche EST le
  sommaire. `allerA(FOLIO_SOMMAIRE)` avec `FOLIO_SOMMAIRE` calculé
  depuis `PAGES.findIndex(id === "sommaire")` — jamais de folio en
  dur. Label = `guide.retourSommaire` (8 locales). Même neutralisation
  du chrome global que `.sommaire-entree`.
- ⚠️ **Piège chrome global des boutons** : `app.css` applique
  `backdrop-filter:blur(6px)` + pseudo-éléments shine/reflet à TOUS les
  `<button>` → les `.zone-tourne` (72px pleine hauteur sur les tranches)
  floutaient le texte des bords de page. Correctif : `backdrop-filter:
  none` + `::before/::after{content:none}` sur `.zone-tourne` (même
  neutralisation que `.header` de Toolbar — réflexe à avoir pour tout
  bouton « zone invisible »).
- **Contenu** : `{#snippet face(pg, folio, cote)}` — couverture
  (`id:"couverture"` = mise en page dédiée), verso orphelin (PAGES
  impair) et bases = pages de garde décoratives. Blocs : `para` /
  `liste` / `etapes` / `note` / `sep`. Titres et lettrine
  (`::first-letter`) en **Caveat** (déjà injectée par `fonts.ts` —
  zéro ajout de font) **sauf ru/zh/ja** : Caveat ne couvre pas le
  cyrillique ni le CJK → `--fonte-titre` sur `.livre` bascule sur une
  police système du pays (Segoe UI / Microsoft YaHei / Yu Gothic —
  map `FONTES_TITRES`). Signature « Ryo Shinkawa » et marqueurs
  d'étapes (chiffres) restent en Caveat. Folios bas-droite (recto) /
  bas-gauche (verso).
- **Palette** : `var(--livre-*)` (glace.css ← tokens `--t-parchemin`,
  `--t-encre`, `--t-cuir`, `--t-dore`, `--t-ruban`…) — artwork interne
  du composant ; le chrome de l'overlay reste sur les rôles existants.
  Zéro hex dans le composant (règle figée Variables CSS UI).

### Contenu (`src/lib/guide/contenu.ts`)

- `BlocGuide` (`para|liste|etapes|note|sep|sommaire`) + `PageGuide { id,
  titreCle, chapitreCle?, blocs }` ; `PAGES[]` = ordre des faces.
  **Textes = clés i18n uniquement** (`guide.pages.<id>.*` +
  `guide.chapitres.*` dans les 8 JSON — fr.json source de vérité).
  Ajouter une page = 1 entrée `PAGES` + les clés ×8.
- **Chapitres** : `chapitreCle` posée UNIQUEMENT sur la première page du
  chapitre → crée l'entrée de sommaire ; `chapitres()` calcule le folio
  (`index + 1`) — **jamais de numéro de page saisi à la main**. Insérer
  une page décale les folios automatiquement.
- **Parcours « principal » (24 faces, 9 chapitres, 2026-10-06)** :
  couverture → sommaire → Comprendre (principe) → Trois réglages
  (premierPas, obs, modeles) → Se repérer (cartes, barreLaterale, scenes)
  → Chat (plateformes, chat) → Widgets & médias (widgets, medias,
  carteEdition) → Cadres (cadres, cadresModale) → Faire vivre le live
  (alertes, bandeau, attente, justChatting) → Passer en direct
  (previsualisation, demarrer) → Pour aller plus loin (plusLoin) → fin.
  L'ordre suit ce qu'un nouveau streamer FAIT, pas l'ordre de la sidebar.
  Garder un nombre PAIR de faces (sinon verso orphelin décoratif).
- **Capacité d'une face** : titre + 1 para + 4-6 puces courtes + 1 note
  (`overflow:hidden`, pas de scroll). Page qui déborde → raccourcir le
  TEXTE de la locale, jamais la mise en page.
- **Marqueurs `**…**`** : rendus `<strong class="mot-cle">` bordeaux par
  `segments()` (split sur `**`). Les traductions doivent conserver le
  même nombre de marqueurs, autour des mots équivalents (jamais autour
  de la ponctuation).
- **Registre figé : jamais de tutoiement**, aucune langue — infinitif /
  impersonnel privilégié pour l'UI, sinon pronom formel (vous / Sie /
  usted / Lei / вы-impératif pluriel / 您 / です・ます調). S'applique aux
  fichiers i18n entiers, pas seulement au guide.
- **Règle de rédaction** : seules des fonctionnalités **vérifiées dans
  le code** sont documentées (pas de sommaire exhaustif, pas de features
  dormantes) — re-grep la feature avant d'écrire ses textes.

### Fichiers

| Fichier | Rôle |
|---|---|
| `src/lib/components/GuideLivre.svelte` | Overlay + mécanique du livre (flip 3D, zones tranche, clavier, reprise localStorage) |
| `src/lib/guide/contenu.ts` | Modèle de données `PAGES` / `BlocGuide` / `PageGuide` / `chapitres()` (sommaire calculé) |
| `src/lib/stores/ui.ts` | `"ergonomie"` dans `Section` + flag `guideLivreOpen` |
| `src/lib/components/Toolbar.svelte` | Lanceur `.sec-guide` (pied fixe) + section Ergonomie + variante `.cible-ergo` |
| `src/App.svelte` | Import + montage `{#if $guideLivreOpen}` |
| `src/theme/tokens.css` | Tokens `--t-*` palette livre + `--t-ardoise-700` |
| `src/theme/glace.css` | Rôles `--livre-*` + `--carte-ardoise` |
| `src/lib/i18n/{fr,en,de,es,it,ru,zh,ja}.json` | `toolbar.ergonomie`/`guide`/`guideTitle` + section `guide` complète |
