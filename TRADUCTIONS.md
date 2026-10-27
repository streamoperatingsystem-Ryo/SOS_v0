# TRADUCTIONS.md — Feuille de route i18n (FR → EN / DE / ES / IT / RU / ZH / JA)

> Système d'internationalisation du dashboard StreamOS. **Ce document est la
> feuille de route officielle** : chaque lot ci-dessous est une tâche
> autonome, prévue pour être exécutée par un agent (SWE 2.0) en suivant le
> protocole AGENTS.md (diagnostic avant écriture, un fichier = un diff =
> une pause, périmètre = liste blanche fermée).
>
> **Statut (2026-10-13) : TOUTES LES PHASES SONT FAITES.** Les 8 locales
> sont en quasi-parité (~1195 clés chacune, fr = source de vérité). Le
> document est conservé comme RECETTE pour toute future extraction de
> chaînes ou nouvelle langue (§7).

## État d'avancement

| Phase | Contenu | État |
|---|---|---|
| 0 — Infrastructure | store + Rust + sélecteur topbar | ✅ FAITE (2026-09-28) |
| 1 — Extraction FR | 12 lots, un fichier = une tâche | ✅ FAITE (2026-10) |
| 2 — Traduction | 7 dictionnaires (en / de / es / it / ru / zh / ja) | ✅ FAITE (v0.1.7, 2026-10-04) |
| 3 — Vérification | checks + smoke test | ✅ FAITE — svelte-check 0/0, parité 8 locales |

---

## 1. Architecture (mise en place en phase 0)

### Fichiers

| Fichier | Rôle |
|---|---|
| `src/lib/i18n/fr.json` | **Dictionnaire de référence** — toute clé existe d'abord ici |
| `src/lib/i18n/{en,de,es,it,ru,zh,ja}.json` | Traductions (mêmes clés, valeurs traduites) |
| `src/lib/stores/langue.ts` | Store : `langue`, `t`, `localeTag`, `initLangue`, `setLangue`, `LANGUES` |
| `src-tauri/src/config.rs` | `lire_langue` / `sauver_langue` (fichier `langue.json` dans `%APPDATA%\com.streamos.v0\`) |
| `src-tauri/src/lib.rs` | Commandes `langue_etat` / `langue_sauver` |
| `src/lib/tauri.ts` | Wrappers `langueEtat()` / `langueSauver()` |
| `src/App.svelte` | `initLangue()` au boot (onMount étape 0b) + bouton « Langue » topbar (à côté du bouton Dev) avec dropdown |

### Utilisation

```svelte
<!-- Template Svelte : -->
<script lang="ts">
  import { t, localeTag } from "../stores/langue";
</script>

<button>{$t("toolbar.widgets.creer")}</button>
<input placeholder={$t("toolbar.obs.motDePasse")} />
<button title={$t("toolbar.connexions.twitch.hint")} />
{nombre.toLocaleString($localeTag)}   <!-- au lieu de toLocaleString("fr-FR") -->
```

```ts
// Code TS (stores, fonctions) :
import { get } from "svelte/store";
import { t } from "./langue";
alert(get(t)("scenes.creationRefusee", { erreur: String(e) }));
```

- **Fallback** : langue choisie → `fr` → la clé brute (une clé manquante
  s'affiche telle quelle — visible, jamais de crash).
- **Interpolation** : `t("cle", { nom })` remplace `{nom}` dans la valeur.
  Ne JAMAIS concaténer des morceaux traduits — une phrase = une clé.
- **Réactivité** : `$t(...)` en template, `get(t)(...)` en TS hors composant.

### Choix de langue

Bouton « Langue » dans la topbar (à côté du bouton Dev) → dropdown des
langues (noms natifs : Français / English / Español / Italiano / 中文 —
**jamais traduits**). Sélection = `setLangue()` → bascule immédiate de
l'UI + persistance `langue.json`. Au boot : `langue.json` →
`navigator.language` (préfixe 2 lettres) → `"fr"`.

---

## 2. Règles pour TOUS les lots (à relire avant chaque tâche)

1. **Protocole AGENTS.md prioritaire** : diagnostic avant écriture, un
   fichier = un diff = une pause (montrer le diff, attendre la validation),
   périmètre = la liste blanche du lot. Rien hors périmètre.
2. **fr.json d'abord** : ajouter les nouvelles clés dans `fr.json`
   (zone par zone), PUIS remplacer les chaînes en dur dans le fichier du
   lot. Les autres dictionnaires restent inchangés (fallback fr).
3. **Conventions de clés** :
   - Format `zone.element[.variante]`, tout en **camelCase**, minuscules.
   - Une zone par fichier/composant (table de correspondance §3).
   - Libellés partagés → `commun.*` : avant de créer une clé, **grepper
     `fr.json`** pour vérifier qu'elle n'existe pas déjà
     (`connecter`, `deconnecter`, `annuler`, `valider`, `supprimer`,
     `retirer`, `fermer`, `importer`, `exporter`, `oui`, `non`, `erreur`…).
   - Les clés décrivent la **fonction**, pas le texte
     (`toolbar.obs.connecter`, pas `toolbar.obs.boutonConnecter`).
4. **NE PAS TRADUIRE / NE PAS TOUCHER** (liste fermée) :
   - Noms de marques/plateformes : Twitch, YouTube, Kick, TikTok, OBS.
   - Noms de sources OBS : `SOS-Diffusion`, `SOS-Caméra`, `SOS-Trou-*`…
   - **Contenu utilisateur** : noms de scènes, textes des titres/widgets,
     templates d'alertes (`{pseudo}`), noms de fichiers/médias.
   - **Valeurs techniques** : `"azerty"`/`"qwerty"` (valeurs `inputLayout`),
     clés de `GOAL_METRIQUES` (`"followers"`, `"subs"`…), types de widgets,
     `"left"`/`"center"`…, couleurs hex.
   - Placeholders d'identifiants plateforme : `"slug"` (Kick),
     `"username"` (TikTok) — ce sont des identifiants, pas de l'UI.
   - `src-tauri/` en entier : messages Rust, `eprintln!`, logs, presets
     JSON embarqués (noms/descriptions des presets = lot ultérieur).
   - Pages OBS autonomes : `src-tauri/resources/diffusion.html`,
     `raccourcis.html`, `chat-popout.html` (hors périmètre v1).
   - `console.log` / `console.error` (logs internes).
   - `src/lib/dev-log.ts` (journal Dev interne).
   - **Commentaires de code** — jamais modifiés, jamais traduits.
5. **Diff minimal** : ne pas réindenter, ne pas renommer de variables, ne
   pas réorganiser le fichier. Seule la chaîne en dur → `{$t("...")}`.
6. **JSON** : UTF-8 (accents et CJK directement en clair), pas de
   commentaires, pas de virgule finale, échapper `"` dans les valeurs.
   Ajouter les clés dans le bloc de la zone (créer le bloc si absent).
7. **Vérification après CHAQUE fichier** :
   ```bash
   npm run check        # 0 erreur, 0 warning EXIGÉS (base zéro-warning)
   ```
   Rust non touché en phases 1-2 → pas de cargo. En cas d'échec : corriger
   AVANT de passer au fichier suivant (souvent : import manquant `t`, ou
   `$t` utilisé dans un fichier `.ts` au lieu de `get(t)`).

### Exemples de transformation (le motif mécanique)

```svelte
<!-- AVANT -->
<button class="action" onclick={createWidget}>Créer un widget</button>
<input placeholder="Mot de passe" />
{streaming ? "Arrêter le stream" : "Démarrer le stream"}

<!-- APRÈS -->
<button class="action" onclick={createWidget}>{$t("toolbar.widgets.creer")}</button>
<input placeholder={$t("toolbar.obs.motDePasse")} />
{streaming ? $t("toolbar.obs.arreterStream") : $t("toolbar.obs.demarrerStream")}
```

```json
// fr.json (blocs imbriqués — une zone par fichier) :
{
  "commun": { "connecter": "Connecter", "deconnecter": "Déconnecter" },
  "toolbar": {
    "widgets": { "creer": "Créer un widget" },
    "obs": {
      "motDePasse": "Mot de passe",
      "arreterStream": "Arrêter le stream",
      "demarrerStream": "Démarrer le stream"
    }
  }
}
```

```ts
// AVANT (store TS) :
alert("Création scène refusée : " + e);
// APRÈS :
alert(get(t)("scenes.creationRefusee", { erreur: String(e) }));
// fr.json → "scenes": { "creationRefusee": "Création scène refusée : {erreur}" }
```

```svelte
<!-- Nombres formatés (widgets objectif) : -->
<!-- AVANT --> {goalActuel.toLocaleString("fr-FR")}
<!-- APRÈS --> {goalActuel.toLocaleString($localeTag)}
```

```ts
// Libellés de métriques goal (tauri.ts GOAL_METRIQUE_LABELS) :
// AVANT → GOAL_METRIQUE_LABELS[m]
// APRÈS → $t("goal.metrique." + m)   // "goal.metrique.followers" etc.
```

---

## 3. Table de correspondance zones ↔ fichiers

| Zone (préfixe de clés) | Fichier(s) |
|---|---|
| `app` | `src/App.svelte` (topbar, bandeau, boutons contrôle) |
| `toolbar` | `src/lib/components/Toolbar.svelte` |
| `carteEdition` | `src/lib/components/CarteEdition.svelte` |
| `interactModal` | `src/lib/components/InteractionViewerModal.svelte` |
| `moderModal` | `src/lib/components/ModerationModal.svelte` |
| `consoModal` | `src/lib/components/ConsoModal.svelte` |
| `ecranAttenteModal` | `src/lib/components/EcranAttenteModal.svelte` |
| `cadresModal` | `src/lib/components/CadresModal.svelte` |
| `sceneBar` | `src/lib/components/SceneBar.svelte` |
| `canvas` | `src/lib/components/Canvas.svelte` |
| `dropBar` | `src/lib/components/DropBar.svelte` |
| `widget` | `src/lib/components/Widget.svelte` |
| `speedrunWidget` | `src/lib/components/WidgetSpeedrun.svelte` |
| `speedrunConfig` | `src/lib/components/SpeedrunConfigModal.svelte` |
| `presetsModal` | `src/lib/components/PresetsModal.svelte` |
| `obsSourceDialog` | `src/lib/components/ObsSourceDialog.svelte` |
| `inputBinding` | `src/lib/components/InputBindingModal.svelte` |
| `titreModal` / `titreForm` | `TitreModal.svelte` / `TitreFondModal.svelte` / `TitreEditForm.svelte` |
| `twitchDevice` | `src/lib/components/TwitchDeviceModal.svelte` |
| `dupliquerModal` | `src/lib/components/DupliquerSceneModal.svelte` |
| `confirmSupprWidget` | `src/lib/components/ConfirmDeleteWidgetModal.svelte` |
| `confirmOuvrirScene` | `src/lib/components/ConfirmOuvrirSceneModal.svelte` |
| `confirmObsOutput` | `src/lib/components/ConfirmObsOutputModal.svelte` |
| `gizmo` | `src/lib/components/Gizmo3D.svelte` (labels « Miroir H/V ») |
| `guides` | `src/lib/components/AlignmentGuides.svelte` (label « CENTRE » — via `get(t)` au dessin) |
| `playerBar` | `src/lib/components/PlayerBar.svelte` (tooltips le cas échéant) |
| `scenes` / `scene` / `obs` / `chat` / `speedrun` / `pad` / `welcome` / `bandeau` / `alertes` / `commandes` / `positionOverlay` / `ecranAttente` / `communaute` / `moderation` / `raccourcis` / `youtube` / `tiktok` / `kick` / `twitch` / `objectifs` / `conso` | stores TS homonymes dans `src/lib/stores/` |
| `goal` | usages de `GOAL_METRIQUE_LABELS` (composants + stores) |
| `commun` | libellés partagés (créé au fil des lots) |

---

## 4. Phase 1 — Extraction FR (un lot = une tâche SWE 2.0)

> Chaque lot : pour CHAQUE fichier listé, extraire les chaînes
> utilisateur-françaises → clés dans `fr.json` → remplacer par `$t(...)` →
> `npm run check` → montrer le diff → pause validation. Les compteurs
> ci-dessous sont des estimations (textes template + attributs +
> `alert()` + libellés de records).

- [x] **Lot 1** — stores légers (~15 chaînes) : `stores/ui.ts` (aucune
      chaîne UI — vérifier), `video.ts`, `welcome.ts`, `fondSidebar.ts`,
      `commandes.ts`, `positionOverlay.ts`, `presets.ts`, `youtube.ts`,
      `communaute.ts`, `raccourcis.ts`.
- [x] **Lot 2** — stores moyens (~30) : `chat.ts`, `speedrun.ts`,
      `obs.ts` (fonction `messageClair` — messages actionnaires),
      `padNumerique.ts`, `scenes.ts` (les `alert(...)` de refus).
- [x] **Lot 3** — `stores/scene.ts` (~17) + `lib/tauri.ts`
      (`GOAL_METRIQUE_LABELS` : remplacer les USAGES par
      `$t("goal.metrique." + m)` ; la constante reste en données de
      fallback non affichées ou est retirée si plus référencée).
- [x] **Lot 4** — petits composants (~20) : `DropBar.svelte`,
      `AlignmentGuides.svelte`, `TwitchDeviceModal.svelte`,
      `TitreModal.svelte`, `TitreFondModal.svelte`,
      `DupliquerSceneModal.svelte`, `ConfirmDeleteWidgetModal.svelte`,
      `Gizmo3D.svelte`, `DevPanel.svelte` (non — journal interne, SKIP),
      `Modal.svelte`, `PlayerBar.svelte`.
- [x] **Lot 5** — composants moyens (~40) : `PresetsModal.svelte`,
      `ObsSourceDialog.svelte`, `InputBindingModal.svelte`,
      `SpeedrunConfigModal.svelte`.
- [x] **Lot 6** — `SceneBar.svelte` + `Canvas.svelte` + `ConsoModal.svelte`
      + `EcranAttenteModal.svelte` (~50).
- [x] **Lot 7** — `CadresModal.svelte` + `WidgetSpeedrun.svelte` +
      `TitreEditForm.svelte` (~50).
- [x] **Lot 8** — `App.svelte` + `Toolbar.svelte` (~50 ; le bouton
      « Langue » est déjà traduit via `langue.bouton`).
- [x] **Lot 9** — `Widget.svelte` (~24) + remplacer les
      `toLocaleString("fr-FR")` par `toLocaleString($localeTag)`
      (`Widget.svelte` + `CarteEdition.svelte`).
- [x] **Lot 10** — `CarteEdition.svelte` (~119 — le plus gros composant).
- [x] **Lot 11** — `ModerationModal.svelte` (~50).
- [x] **Lot 12** — `InteractionViewerModal.svelte` (~102 — 5 onglets).

## 5. Phase 2 — Traduction des dictionnaires (un fichier = une tâche)

> Traduire UNIQUEMENT les valeurs, clés identiques à fr.json. Ne pas
> toucher aux composants. Traduction naturelle d'outil de streaming
> (garder les anglicismes admis : « stream », « chat », « widget »,
     « follower »… selon usage de chaque langue).

- [x] **Lot 13** — `src/lib/i18n/en.json`
- [x] **Lot 14** — `src/lib/i18n/es.json`
- [x] **Lot 15** — `src/lib/i18n/it.json`
- [x] **Lot 16** — `src/lib/i18n/zh.json`
- [x] **de / ru / ja** — ajoutés directement complets avec la refonte
      i18n de v0.1.7 (2026-10-04) : dictionnaires + `LANGUES` +
      `LOCALE_TAGS` + `DICTS` en une passe (la recette §7, appliquée 3 fois).

## 6. Phase 3 — Vérification finale

- [x] `npm run check` → 0 erreur, 0 warning.
- [x] Grep résiduel de texte français en dur dans `src/`
      (hors commentaires, noms propres, contenu utilisateur).
- [x] `npm run tauri dev` : basculer les 8 langues dans le dropdown →
      vérifier topbar, Toolbar, modales principales (Interactions chat,
      Modération, CarteEdition) ; vérifier la persistance après
      redémarrage (`%APPDATA%\com.streamos.v0\langue.json`).
- [x] Vérifier les nombres des widgets objectif (`$localeTag`).

## 7. Ajouter une langue plus tard (exemple : coréen `ko`)

1. Créer `src/lib/i18n/ko.json` (copie de fr.json, valeurs traduites).
2. `src/lib/stores/langue.ts` : ajouter `"ko"` au type `Langue`, une
   entrée dans `LANGUES` (`{ id: "ko", nom: "한국어" }`) et
   `LOCALE_TAGS` (`ko: "ko-KR"`), l'import JSON + l'entrée `DICTS`.
3. Terminé — le dropdown topbar liste la nouvelle langue automatiquement,
   `langue_sauver` (Rust) n'a pas de whitelist. (Recette appliquée telle
   quelle pour de, ru et ja en 2026-10.)

## 8. Hors périmètre v1 (lots ultérieurs éventuels)

- `diffusion.html` (page OBS :4321) : rares textes fixes (« Followers »,
  « en attente… ») — nécessiterait de pousser la langue via le WS :4321.
- `raccourcis.html` (title « Raccourcis »), `chat-popout.html`.
- Presets embarqués (`src-tauri/resources/presets/*.json` : nom +
  description affichés dans la galerie) — i18n côté Rust.
- Messages d'erreur techniques Rust (resteront en français dans les logs).
