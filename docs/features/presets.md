# Presets de scènes embarqués

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Presets de scènes embarqués — parcours « tout prêt » (2026-09-20)

> Scènes modèles prêtes à l'emploi pour le user lambda : widgets positionnés
> + cadre actif, AUCUN média à importer (chat fonctionne dès qu'une
> plateforme est connectée, caméra via SOS-Caméra). Proposées au PREMIER
> lancement (onboarding) et à chaque création de scène (bouton « + Scène »).

### Données

- 4 presets JSON dans `src-tauri/resources/presets/` — `gaming-plein-ecran`,
  `gaming-4-3`, `just-chatting`, `speedrun` — embarqués via `include_str!`
  (pattern diffusion.html / chat-popout.html : pas de ressource à bundler,
  recompile si changement).
- Format : `{ id, nom, description, scene: Scene complète }`. Widgets
  fonctionnels sans import média (chat / camera / input-viewer / speedrun /
  goal / media troué sans fichier) + `cadreWidget` actif (règle figée
  cadres : s'applique à tous). Conçus 1920×1080 ; `appliquer_canvas_obs`
  ajuste le canvas sans rescaler les widgets (comportement des scènes
  importées).

### Flux

```
resources/presets/*.json ──include_str!──→ presets.rs
  ├─ presets_lister() → Vec<PresetInfo {id, nom, description}>  (galerie)
  │    parse NON-FATAL : JSON cassé → log::error + absent de la liste
  ├─ charger(id) + instancier(app) → Scene (uuid régénérés par widget
  │    + appliquer_canvas_obs)
  ├─ scene_creer_depuis_preset(nom, preset_id) → scenes::creer_avec →
  │    NOUVELLE scène (l'existante jamais touchée)
  └─ scene_initialiser_preset(preset_id) → ONBOARDING : si la scène courante
       est VIERGE (garde scene_vierge — le « Défaut » auto-créé au 1er boot),
       remplace son contenu + renomme l'entrée d'index ; SINON crée une
       NOUVELLE scène (rien n'est jamais écrasé)
```

Commandes Tauri (lib.rs) : `presets_lister`, `scene_creer_depuis_preset`,
`scene_initialiser_preset`, `onboarding_etat`, `onboarding_marquer_fait`.

### Onboarding (1er lancement)

- Flag `%APPDATA%\com.streamos.v0\onboarding.json` `{ "fait": true }`
  (pattern raccourcis.json — `lire_onboarding_fait` / `marquer_onboarding_fait`
  dans config.rs). Sémantique « sûr » : erreur de lecture = fait (on ne
  ré-affiche pas la modale à chaque boot si le fichier est illisible).
- `App.svelte` onMount (step 5b, APRÈS loadScene/loadScenesIndex/
  loadCurrentScene) : `onboarding_etat()` false → `chargerPresets()` +
  `presetsModalOpen.set("onboarding")`. Non-bloquant (try/catch).
- Choix preset → `initialiserSceneDepuisPreset` (store) : invoke +
  rechargement complet (index + courante + scène) + `sceneSyncCaptures`
  (caméra — même chaîne que openScene) + onboarding marqué fait.
- Fermer sans choisir (✕ / Escape / overlay / « Partir d'une page vierge »)
  → onboarding marqué fait, scène « Défaut » vide conservée.

### UI

- `PresetsModal.svelte` (montée dans App.svelte, pattern CadresModal) :
  mode porté par le store `presetsModalOpen` (`"creation" | "onboarding"`,
  ui.ts). Cards = icône SVG + nom + description (PAS de preview image en v1).
  - mode creation : champ nom (vide → nom du preset, ou « Scène N » pour la
    vierge) + card « Page vierge » (comportement historique `createScene`)
    en première position + cards presets.
  - mode onboarding : cards presets + footer « Partir d'une page vierge ».
- SceneBar bouton « + Scène » : ouvre la modale (`presetsModalOpen.set
  ("creation")`) — plus de création directe dans `onNouvelle`.

### Contrats

- ⚠️ `scene_initialiser_preset` (onboarding) ne remplace la scène courante
  QUE si elle est **vierge** (garde `scene_vierge` : 0 widget, pas de fond,
  cadre, titre ni morph — test `scene_vierge_detecte_contenu`). Un
  utilisateur EXISTANT (onboarding.json absent à la mise à jour → la modale
  s'affiche une fois) qui clique un preset obtient une NOUVELLE scène, son
  travail n'est JAMAIS touché. Pour toute création utilisateur :
  TOUJOURS `scene_creer_depuis_preset` (nouvelle scène).
- Widget caméra dans les presets : `obsSource: "SOS-Caméra"` (constante
  singleton), 1 max par preset, z=5000 (plage réservée). La source est créée
  par `ensure_camera` au `scene_sync_captures` (boot/openScene/connexion OBS)
  — le store presets appelle sceneSyncCaptures après application.
- Ids des widgets régénérés à chaque instanciation : 2 scènes issues du
  même preset ne partagent jamais leurs ids (propre pour duplication etc.).
- `appliquer_canvas_obs` est `pub(crate)` (utilisée par scenes.rs et
  presets.rs).
