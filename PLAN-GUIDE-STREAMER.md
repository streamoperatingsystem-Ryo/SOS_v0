# PLAN — i18n : ajout de l'allemand (`de`) + synchronisation du Guide du streamer

**Date :** 3 octobre 2026
**Rédigé par :** Devin (session de travail guide — diagnostic i18n vérifié dans le code)
**Statut :** prêt pour exécution dans une nouvelle fenêtre de contexte
**Contexte :** le « Guide du streamer » (livre feuilletable dashboard-only) est
terminé et validé **en français uniquement**. La section `guide.*` de
`fr.json` a été entièrement réécrite pendant la relecture de Rio
(formulations user-friendly + marqueurs `**…**` + nouvelle page « cartes »)
— les autres locales sont donc **obsolètes**, pas seulement incomplètes.

---

## 1. Objectif

1. **Ajouter l'allemand** comme 6ᵉ langue du dashboard (`de` / « Deutsch »).
2. **Synchroniser la section `guide.*`** dans les 5 locales existantes +
   créer la version allemande — la seule référence est `fr.json`.

Périmètre : dashboard uniquement (diffusion.html & pages OBS ne sont PAS
traduits — contrat i18n existant, `langue.ts` l.7-8).

## 2. État vérifié (diagnostic — ne pas re-chercher)

### 2.1 Mécanisme i18n — `src/lib/stores/langue.ts`

- `fr.json` = **source de vérité** ; fallback : langue → fr → clé brute.
- Type `Langue` (l.20), tableau `LANGUES` noms natifs (l.24-30),
  `LOCALE_TAGS` (l.34-40), `DICTS` imports statiques (l.11-15, 44).
- **Ajouter une langue = UNIQUEMENT `langue.ts` + `<code>.json`** : le
  commentaire du fichier (l.17-19) confirme « aucun changement Rust —
  `langue_sauver` n'a pas de whitelist ».
- Le sélecteur de langue (`App.svelte` l.711) itère `LANGUES` → entrée
  automatique. `navigator.language` → détection automatique (l.84-87).

### 2.2 Décalage actuel des locales (mesuré par script, §6)

en / es / it / zh ont **12 clés manquantes** dans `guide.*` :
`principe.l1|l2|l3`, `chat.p2`, `cartes.titre|p1|intro|l1|l2|l3|p2|note`
+ **1 clé orpheline** : `guide.pages.widgets.note` (supprimée du fr —
à retirer).
⚠️ Ce n'est PAS qu'un problème de clés manquantes : **tous les textes
`guide.*` ont été réécrits en fr** — les traductions existantes sont
périmées (ancienne formulation « canvas », pas de page cartes, pas de
marqueurs `**`). → **retraduire la section `guide.*` complète** par
locale, pas juste boucher les trous.

### 2.3 Contraintes de contenu (règles du guide)

- **Marqueurs `**…**`** → `<strong class="mot-cle">` bordeaux
  (`GuideLivre.segments()`, split sur `**`). Les traductions DOIVENT
  conserver les marqueurs **autour des mots équivalents** (verbe/action,
  nom de bouton, élément d'UI) — jamais autour de la ponctuation.
- **Libellés d'UI cités dans le guide** : doivent reprendre la traduction
  EXISTANTE de la clé UI dans chaque locale — ne pas réinventer :
  - « Créer un widget chat » → `toolbar.creerChat`
  - « Lier une source OBS » → `carteEdition.sourceObsLier`
  - section « Widgets » → `toolbar.widgets` ; « OBS » → section OBS ;
    « Personnalisation » → `toolbar.personnalisation`
  - « + Scène » → libellé du bouton scène (toolbar)
  - « fenêtre de capture » → `carteEdition.trouOn`
  - « Compact » / « Bulles » → `carteEdition.chatStyle.compact|bulles`
- **Noms propres jamais traduits** : StreamOS, OBS Studio, Stream Deck,
  Twitch/YouTube/Kick/TikTok, WebSocket Server Settings, « Ryo Shinkawa »
  (signature en dur, hors i18n — rien à faire).
- **Ponctuation par locale** : suivre la convention déjà en place dans
  chaque fichier (en : quotes droites ; es/it : « » ; zh :「」/《》 selon
  l'existant ; **de : „…“ ou »…« — choisir et rester cohérent**).
- **Ton — RÈGLE FIGÉE RIO : JAMAIS de forme tutoyante**, dans aucune
  langue (pas de « du », « tú », « tu », « 你 »). Deux registres
  autorisés, au choix phrase par phrase :
  1. **Infinitif / impersonnel** — privilégié pour l'UI (boutons,
     instructions courtes) : « Seleccionar… », « Selezionare… »,
     « 选择… », « Auswählen… » ;
  2. **Pronom formel** quand l'adresse directe est nécessaire :
     es → « usted », it → « Lei », zh → « 您 », de → « Sie », fr → « vous ».
  ⚠️ **Ampleur réelle** : es/it/zh utilisent le tutoiement dans TOUT le
  fichier (~840 clés chacun), pas seulement dans `guide.*`. La règle
  s'applique donc aux fichiers entiers — la resync `guide.*` de l'étape 4
  devient une **re-traduction au bon registre des locales complètes**,
  pas juste de la section guide.
- Le guide ne contient **que du vérifié** : ne rien ajouter qui n'existe
  pas dans le code, même en traduisant « au mieux ».

### 2.4 Points de vigilance

- `i18n/` est **non tracké dans git** — pas de `git diff` dispo : montrer
  les diffs par extrait de fichier.
- `de.json` partiel = fallback fr visible à l'écran (mieux que cassé,
  mais l'objectif est la **parité complète** — vérification §6).
- Police **Caveat** (titres/signature du guide) : vérifier le rendu des
  **umlauts ä/ö/ü + ß** en allemand (latin-ext attendu, à confirmer
  visuellement sur une page de titre).
- `LOCALE_TAGS` → `de: "de-DE"` (compteurs viewers : 1.234).
- Volume `de.json` : **~855 lignes / ~400 clés** — traduction intégrale
  de `fr.json`, pas seulement `guide.*`. C'est l'étape la plus longue.

## 3. Ordre des pages du guide (référence — 24 faces, révisé 2026-10-06)

> ⚠️ **Décision 2026-10-06 (Rio)** : la règle « pas de sommaire exhaustif,
> ne pas toucher contenu.ts » du 3 octobre est **annulée**. Le guide est
> passé à un **parcours principal en 9 chapitres** avec **sommaire
> cliquable** (saut en cascade). La structure vit dans `contenu.ts`
> (`chapitreCle` + `chapitres()`), les textes dans `fr.json`
> (`guide.chapitres.*` + `guide.pages.*`). Détail : AGENTS.md § « Guide du
> streamer » → « Contenu ».

couverture(1) → sommaire(2) → principe(3) → premierPas(4) → obs(5) →
modeles(6) → cartes(7) → barreLaterale(8) → scenes(9) → plateformes(10) →
chat(11) → widgets(12) → medias(13) → carteEdition(14) → cadres(15) →
cadresModale(16) → alertes(17) → bandeau(18) → attente(19) →
justChatting(20) → previsualisation(21) → demarrer(22) → plusLoin(23) →
fin(24).

Conséquence pour les étapes 1 et 4 ci-dessous : la section `guide.*` à
traduire compte désormais **9 clés `chapitres` + 24 pages** (~130 clés),
toutes à reprendre depuis le fr — les anciennes traductions `guide.*`
(8 pages) restent périmées.

## 4. Étapes d'exécution (protocole AGENTS : 1 fichier = 1 diff = 1 pause)

### Étape 0 — Gel de fr.json
`fr.json` ne bouge plus pendant la traduction. Toute correction fr
découverte en route → signaler à Rio, ne pas corriger en silence.

### Étape 1 — `src/lib/i18n/de.json` (nouveau, ~855 lignes)
Copier `fr.json`, traduire **toutes** les sections (pas seulement
`guide.*`), clés et structure identiques. Marqueurs `**` préservés.
Vérifier immédiatement : script §6 → `de missing: 0, extra: 0`.
*Le fichier est gros : traduire par grosses sections (toolbar, carteEdition,
modales…) en relisant — la JSON validity (`node -e "require('./de.json')"`)
est le filet de sécurité minimum.*

### Étape 2 — `src/lib/stores/langue.ts` (5 retouches, un seul fichier)
- `import de from "../i18n/de.json";`
- union : `| "de"`
- `LANGUES` : `{ id: "de", nom: "Deutsch" }` — position validée par Rio :
  **après en** (ordre final : fr, en, de, es, it, zh)
- `LOCALE_TAGS` : `de: "de-DE"`
- `DICTS` : `de`
Puis `npm run check` → 0 erreur exigé.

### Étape 3 — Test visuel allemand
`npm run tauri dev` → sélecteur topbar affiche « Deutsch » → basculer :
UI en allemand, guide feuilleté en allemand, umlauts/ß corrects en
Caveat, flèches-cuir et folios intacts, reprise de page localStorage OK.

### Étape 4 — Re-traduction au registre formel : en / es / it / zh
Pour chaque locale (1 locale = 1 diff) :
- supprimer `widgets.note`, ajouter les 12 clés manquantes dans `guide.*` ;
- retraduire `guide.*` complet sur la base du fr (marqueurs `**`,
  libellés d'UI repris des clés existantes §2.3) ;
- **ET surtout : réécrire le fichier entier au registre non-tutoyant**
  (règle figée §2.3 : infinitif/impersonnel privilégié, sinon usted /
  Lei / 您) — les fichiers actuels tutoient partout ;
- en : neutre par nature, seule la section `guide.*` demande la resync.
Script §6 après chaque locale → 0 manquante / 0 orpheline.

### Étape 5 — Vérification finale
- Script §6 sur les 6 fichiers : parité parfaite avec fr.
- `npm run check` → 0 erreur / 0 warning (base zéro-warning du dépôt).
- Passage visuel rapide dans ≥2 locales non-fr (longueurs de texte :
  l'allemand/enfonce ~+20 % — vérifier qu'aucune page ne déborde en bas ;
  si une page déborde → raccourcir la formulation LOCALE, ne pas toucher
  la mise en page).
- `AGENTS.md` : compléter la section « Guide du streamer » (ligne i18n :
  6 langues, marqueurs `**`, page cartes).

## 5. Hors périmètre (à signaler, pas à faire)

- Traduire diffusion.html / raccourcis.html / chat-popout.html (pages OBS).
- Retoucher `contenu.ts`, `GuideLivre.svelte`, les textes fr (structure et
  fr validés le 2026-10-06 — toute correction fr se signale à Rio).
- Commit git (le répertoire i18n est non tracké — laisser comme trouvé).

## 6. Script de vérification de parité (à réutiliser tel quel)

```bash
node -e "const fr=require('./src/lib/i18n/fr.json');
for(const n of ['en','es','it','zh','de']){
  const d=require('./src/lib/i18n/'+n+'.json');
  const flat=(o,p='')=>Object.entries(o).flatMap(([k,v])=>typeof v==='object'&&v!==null?flat(v,p+k+'.'):[p+k]);
  const f=new Set(flat(fr)),s=new Set(flat(d));
  console.log(n,'missing:',[...f].filter(k=>!s.has(k)).length,'extra:',[...s].filter(k=>!f.has(k)).length);
}"
```

Attendu en fin de chantier : `missing: 0 extra: 0` pour les 5 locales.

## 7. Décisions tranchées par Rio (figées)

- **JAMAIS de tutoiement**, aucune langue : infinitif/impersonnel en
  priorité, sinon pronom formel (usted / Lei / 您 / Sie / vous).
  Pourquoi : respect de l'utilisateur — ce ne sont pas des copains.
  Le vouvoiement est la norme de politesse attendue, en Chine (您) plus
  encore qu'ailleurs.
- **Position du sélecteur** : fr, en, **de**, es, it, zh.
