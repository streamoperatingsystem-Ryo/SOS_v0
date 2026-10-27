# PLAN — Moteur natif de capture / diffusion (Rust)

> **Statut :** proposition à valider — aucune ligne de moteur écrite à ce jour.
> **Date :** 2026-10-27
> **Périmètre :** Windows uniquement (x64), Tauri v2 + Svelte 5 + Rust.
> **Document jumeau à créer après validation :** `docs/features/moteur-natif.md`
> (contrat technique détaillé, référencé depuis l'index `AGENTS.md`).

---

## 0. Comment reprendre (prochaine fenêtre de contexte)

1. Lire `AGENTS.md` en entier, puis ce document en entier. En cas de
   contradiction, `AGENTS.md` fait foi (protocole, contrats figés).
2. Vérifier l'état du dépôt **avant toute action** :
   ```bash
   git status --short
   git log --oneline -5
   ```
   ⚠️ Au 2026-10-27 l'arbre est **sale** (20+ fichiers modifiés, presets
   `gaming.json`/`minimal.json` supprimés). **Aucun lot ne commence avant un
   commit de base propre ou un stash explicite validé par l'utilisateur.**
3. Lire la section « Phases et lots » (§12) et reprendre au premier lot non
   coché. Un lot = une liste blanche de fichiers = un diff par fichier =
   une pause de validation (protocole `AGENTS.md`).
4. Vérifications de référence, à garder vertes **à chaque diff** :
   ```bash
   cd src-tauri && cargo check && cargo clippy     # exit 0 exigé
   npm run check                                    # 0 erreur exigée
   ```
5. Ne jamais modifier une section marquée « RÈGLE FIGÉE » / « CONTRAT FIGÉ »
   dans `AGENTS.md` sans accord explicite.

---

## 1. Objectif et définition du succès

### 1.1 Objectif produit

Faire de StreamOS un **diffuseur autonome** : capturer, composer, encoder et
diffuser/enregistrer **sans OBS**, avec un rendu **pixel perfect jusqu'en 4K**,
une consommation **inférieure à OBS à fonctionnalités égales**, et un support
matériel **NVIDIA + AMD + Intel** complet sous Windows.

Le mode OBS actuel (obs-websocket + browser source `:4321`) **reste intact et
fonctionnel**. Les deux modes coexistent et se basculent à la volée.

### 1.2 Définition du succès (mesurable)

| Critère | Cible | Méthode |
|---|---|---|
| Parité visuelle | SSIM ≥ 0,995 ; diff max ≤ 2/255 sur images statiques | harnais §8 |
| 1080p60, 3 sources | 0 frame perdue côté capture/composite sur 30 min | compteurs moteur §9 |
| 4K60 | tenu sur GPU de classe définie, sinon dégradation expliquée | matrice §6 |
| CPU | ≤ 0,7 × OBS (mêmes fonctions, browser sources incluses) | PresentMon + PDH |
| RAM | ≤ 0,5 × OBS | RSS |
| Latence capture→encode | ≤ OBS − 1 frame | PresentMon |
| Démarrage à l'enregistrement | ≤ 5 s | chrono |
| Compatibilité | NVIDIA, AMD, Intel (iGPU et dGPU), encodeur logiciel en secours | matrice §6 |
| Diagnostic lisible | pilote, encodeur (matériel/logiciel), barres et verdict affichés ; `N/A` si non mesurable | onglet « Moteur » §10 |
| Non-régression OBS | toutes les features OBS inchangées | tests manuels + clippy/check |

> « Plus rapide qu'OBS » n'est **pas** un objectif global. L'objectif est :
> à fonctionnalités égales (chat, alertes, cadres, caméra), consommer moins.
> OBS **nu** (3 sources natives) est déjà proche de l'optimum GPU : on ne le
> bat pas sur ce terrain-là. On le bat sur l'absence de navigateur dans la
> sortie, la composition à la demande, le fan-out de capture et le culling
> d'occlusion.

---

## 2. Contraintes non négociables

### 2.1 Protocole du dépôt (`AGENTS.md`)

1. Diagnostic avant écriture, systématiquement.
2. Un fichier = un diff = une pause.
3. Le périmètre annoncé est une liste blanche fermée.
4. Les sections figées priment.
5. Le doute s'exprime, il ne se résout pas seul.
6. La doc d'une feature vit dans `docs/features/`.

### 2.2 Contrat vie privée (figé)

Le moteur natif est **100 % local** : aucune donnée ne sort, aucun beacon,
aucun crash-reporting. Les seules sorties réseau restent les APIs officielles
des plateformes, déclenchées par une action utilisateur.

### 2.3 Contraintes techniques du dépôt

- `rust-version = "1.77"` (`src-tauri/Cargo.toml`) — **contrainte forte** :
  tout crate ajouté doit compiler avec cette MSRV, ou faire l'objet d'un lot
  explicite de montée de version validé par l'utilisateur.
- `cargo clippy` exit 0 et `npm run check` 0 erreur = niveau de référence.
- Champs `camelCase` de `Scene`/`Widget` = contrat JSON direct avec le
  frontend. **Ne jamais les renommer.**
- Aucune couleur en dur dans l'UI : tokens `--t-*` (`src/theme/glace.css`).
- `diffusion.html`, `chat-popout.html`, `raccourcis.html` = pages autonomes
  vanilla, sans module partagé ; elles restent en px.

### 2.4 Contraintes utilisateur (session du 2026-10-27)

- **Full natif en Rust** (pas de calque DOM Chromium dans la sortie).
- Pixel perfect, 4K, zéro perte.
- Support **total** des pilotes/GPU : NVIDIA, AMD, Intel, encodeur logiciel.
- Fond vert virtuel : **hors périmètre** (reporté).
- Politique de bascule pendant un live : **à trancher plus tard**.
- Priorité : capture + enregistrement fluides, moins lourds qu'OBS, utilisables
  sur des PC anciens.

---

## 3. Diagnostic de l'existant (mesuré le 2026-10-27)

### 3.1 Chiffres

| Élément | Valeur |
|---|---|
| Modules Rust (`lib.rs`) | 45 |
| Commandes Tauri | 223 |
| `lib.rs` | 4 758 lignes |
| `scene.rs` | 1 013 lignes (couplage unique : `crate::config::SpeedrunSettings`) |
| `scenes.rs` | 1 623 lignes |
| `obs_trou.rs` | 2 590 lignes, 12 fonctions publiques |
| `obs.rs` | 879 lignes |
| `server.rs` | 332 lignes (axum : routes `/`, `/ws`, `/fonts/{name}`, `/brand/{name}` + `api_deck`) |
| `diffusion.html` | **13 324 lignes** (renderer de référence) |
| `chat-popout.html` / `raccourcis.html` | 1 323 / 216 lignes |
| Champs `Widget` | **106** |
| Docs de features | 55 fichiers |

### 3.2 Pipeline actuel (mode OBS)

```
Dashboard WebView2 (config)                    OBS (compositeur)
  │ commitScene ──► snapshot WS :4321             ▲
  ▼                                               │
diffusion.html (CEF) ── page 1920×1080 TRANSPARENTE, 60 fps
  │  dessine widgets DOM, cadres SVG, trous destination-out
  └────────────► Browser source « SOS-Diffusion » ─┘
Pile OBS (bas→haut) :
  SOS-Fond-*  <  SOS-Trou-*  <  SOS-Caméra-Fond  <  SOS-Caméra  <  SOS-Diffusion
```

Coûts structurels : **deux compositeurs en série** (Chromium + OBS), une
surface plein écran transparente rendue à 60 fps même quand rien ne change,
et un one-shot obs-websocket à chaque `commitScene` (donc à chaque pointerup
pendant un drag, cf. `just-chatting.md`).

### 3.3 Points de couplage OBS à découper (liste fermée)

Fonctions de `obs_trou.rs` :
`enumerate_targets`, `create_trou_source`, `enumerate_inputs_by_kind`,
`link_existing_source`, `sync_trous`, `camera_sync`, `camera_hide`,
`camera_fond_vert`, `camera_mask_sync`, `sync_scene_captures`,
`delete_trou_source`, `create_trou_from_pc`.

Fonctions de `obs.rs` :
`connect_and_setup`, `refresh_diffusion`, `set_stream`, `set_record`,
`outputs_status`, `moniteurs_liste`, `ouvrir_projecteur_preview`,
`fermer_projecteurs`.

Cycle de vie appelant `scene_sync_captures` : boot, `openScene`,
`duplicateScene`, preset, import ZIP, reconnexion OBS, Just Chatting ON/OFF,
hook `registerApresSyncCaptures` (fin de `commitScene`).

Routage audio OBS de l'écran d'attente : source `wasapi_output_capture` +
`SetSceneItemEnabled false` sur les items `desktop|bureau`.

### 3.4 Ce qui est déjà indépendant du moteur (réutilisable tel quel)

chat (4 plateformes), alertes, commandes, cadres `!cadres`, musique, fight,
speedrun (ASL/LSS + Boa), input viewer, pad numérique, télémétrie, viewers,
communauté, modération, planning, API Stream Deck, raccourcis, pop-outs,
`equaliseur.rs` (WASAPI loopback + micro + FFT), `mask_png.rs`,
`zip_store.rs`, sidecar ffmpeg.

---

## 4. Décisions d'architecture

### 4.1 Trois anneaux + règle de dépendance

```
┌─ Anneau 0 : CONTRATS ──────────────────────────────┐
│ sos-scene : Scene, Widget, CadreConfig, ModuleHud, │
│             MorphPoint, MediaSlide, SpeedrunSettings│
└────────────────────────────────────────────────────┘
          ▲                              ▲
┌─ Anneau 1 : DOMAINE ──────────┐  ┌─ Anneau 2 : MOTEURS ───────────┐
│ chat, alertes, commandes,     │  │ sos-moteur-api : trait Moteur  │
│ speedrun, welcome, musique,   │  │ sos-moteur-natif : moteur      │
│ fight, télémétrie, viewers…   │  │ (app) src/moteur/obs.rs        │
└───────────────────────────────┘  └────────────────────────────────┘
          ▲                              ▲
┌─ Anneau 3 : APPLICATION ─────────────────────────────┐
│ Tauri commands, bus d'état, WS :4321, machine de mode│
└──────────────────────────────────────────────────────┘
```

**Règles :**
- `sos-moteur-natif` **ne dépend jamais** de `tauri`, de `crate::` de l'app,
  ni du domaine. Il ne connaît que `sos-scene` + `sos-moteur-api` + `windows`.
- L'adaptateur OBS reste **dans l'app** (`src/moteur/obs.rs`) : il ne fait que
  déléguer aux modules existants. On ne déplace pas `obs_trou.rs` (risque
  inutile), on l'enveloppe.
- Le domaine ne connaît aucun moteur : aucune fonction de `chat`, `alertes`,
  `speedrun`… ne doit importer `sos_moteur_*`.

### 4.2 Arbre de crates cible

```
src-tauri/
├─ Cargo.toml                  # [workspace] members = ["crates/*"] + package
├─ crates/
│  ├─ sos-scene/               # types purs (serde), zéro logique d'app
│  ├─ sos-moteur-api/          # trait Moteur, Sink, Capacites, RuntimeState
│  └─ sos-moteur-natif/        # moteur : capture, gpu, compositor, render,
│                              #           encode, output, audio, clock, mode
└─ src/
   ├─ moteur/
   │  ├─ mod.rs                # registre + machine à états + commandes Tauri
   │  └─ obs.rs                # adaptateur MoteurObs (délègue à obs_trou/obs)
   └─ … (existant inchangé)
```

> **Pourquoi un workspace :** isolation de compilation (le moteur se compile et
> se teste sans Tauri), impossibilité structurelle de créer un cycle, et
> `cargo test -p sos-moteur-natif` utilisable en CI.

### 4.3 Trait `Moteur` (contrat de l'anneau 2)

```rust
// crates/sos-moteur-api/src/lib.rs
pub trait Moteur: Send + Sync {
    fn capacites(&self) -> Capacites;
    fn appliquer_scene(&self, scene: &Scene);            // idempotent, différentiel
    fn appliquer_etat(&self, etat: &RuntimeState);       // overlays, chat, cadres
    fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur>;
    fn arreter_sortie(&self, kind: SortieKind) -> Result<(), ErreurMoteur>;
    fn etat(&self) -> MoteurEtat;                        // Inactif|Actif|Erreur(String)
    fn apercu(&self) -> Option<ApercuFrame>;             // basse résolution, optionnel
    fn arreter(&self);                                   // teardown ordonné, idempotent
}
```

`Capacites` (bitflags ou struct de booléens) sert à **grer l'UI** sans
`if mode == …` : `capture_ecran`, `capture_fenetre`, `capture_jeu`,
`camera`, `fond_vert`, `masque_shader`, `cadres_animes`, `audio_par_app`,
`roi_encode`, `multi_format`, `hdr`, `srt`, `ndi`…

`SortieCfg` : `{ sinks: Vec<SinkCfg>, canvas: (u32,u32), fps: u32, codec:
H264|Hevc|Av1, bitrate, preset, cle: Option<String> }`.
`SinkCfg` : `Fichier { chemin, conteneur }` | `Rtmp { url }` | `Srt { url }` |
`Apercu`.

### 4.4 Trait `Sink` — un seul encode, N sorties

```rust
pub trait Sink: Send {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur>;
    fn fermer(&mut self) -> Result<(), ErreurMoteur>;
}
// FileSink(fMP4/MKV) · PreviewSink · RtmpSink · SrtSink · TeeSink(Vec<Sink>)
```

**MVP = `FileSink` + `PreviewSink`.** RTMP/SRT arrivent ensuite **sans
réécriture** : ce sont des sinks supplémentaires.

### 4.5 Flux d'état

```
Tauri command / store
   │  update_scene, commitScene
   ▼
ScenesState (existant) ──► watch::Sender<Arc<Scene>> ──► MoteurNatif
   │                                   └──────────────► MoteurObs (traduction)
   └─► chat_tx (WS :4321) ──► diffusion.html (mode OBS uniquement)
```

- Un seul producteur d'état : `ScenesState`. Le moteur **s'abonne**, il ne
  tire jamais.
- `appliquer_scene` est **différentiel** : le moteur compare avec sa scène
  précédente et ne recrée que ce qui a changé (sessions de capture, textures,
  layouts). Aucun rebuild complet par snapshot.
- `RuntimeState` transporte les données éphémères (alertes en cours, messages
  chat, cadres `!cadres`, JC actif, modules).

### 4.6 Modèle de threads

| Thread | Rôle | Règle |
|---|---|---|
| `capture-<source>` | WGC/DXGI → textures partagées | jamais bloquant sur l'encodeur |
| `compositor` | graphe de calques → texture de sortie | cadencé par l'horloge |
| `encoder` | MFT → bitstream | file bornée, drop-oldest |
| `audio` | WASAPI → mix → PCM | horodaté, jamais bloquant |
| `output` | FFmpeg sidecar (mux/RTMP) | backpressure sur stdin |
| `watchdog` | surveille les étages | redémarre ou dégrade |

**Invariants :** aucun `.lock()` double dans une expression (piège documenté
`AGENTS.md`) ; aucun appel bloquant sur le thread UI Tauri ; zéro thread
lorsque le moteur est inactif (même invariant que `input_viewer`/`equaliseur`).

---

## 5. Pipeline natif

```
Source (écran/fenêtre/caméra/média)
   │ WGC / DXGI Duplication / Media Foundation Source Reader
   ▼
Texture D3D11 (pool, refcountée, N consommateurs)
   ▼
Compositeur D3D11 + HLSL
   │  calques = scène (z), viewports, crop, masques SDF, cadres, texte, overlays
   │  dirty regions + culling d'occlusion
   ▼
Texture de sortie (canvas W×H)
   ▼
Encodeur MFT (NVENC / AMF / QSV / logiciel Microsoft)
   │  entrée = texture D3D11 (zéro-copie)
   ▼
Bitstream H.264 / HEVC / AV1
   ▼
FFmpeg sidecar (mux fMP4/MKV, RTMP, SRT, reconnexion)
   ▼
Disque / plateforme / aperçu
```

### 5.1 Choix structurants (et pourquoi)

1. **D3D11 direct, pas wgpu.** WGC et DXGI produisent des textures D3D11 ;
   les MFT les consomment directement. wgpu (DX12/Vulkan) imposerait des
   copies inter-API et une MSRV plus élevée. D3D11 feature level 11_0 couvre
   tout GPU Windows moderne (y compris iGPU anciens).
2. **Encodage par Media Foundation (MFT), pas par SDK constructeur.**
   Une seule API pour NVIDIA/AMD/Intel + encodeur logiciel Microsoft, aucune
   dépendance C, aucun SDK à télécharger, pas de licence GPL. Les SDK
   constructeurs (NVENC direct, AMF, oneVPL) deviennent des **backends
   optionnels d'amélioration** en phase ultérieure, pas un prérequis.
3. **Mux/streaming par FFmpeg sidecar.** Le sidecar existe déjà
   (`src-tauri/binaries/ffmpeg`, remux MKV). On lui passe le bitstream encodé
   (petit volume) : mux fMP4, RTMP, SRT, reconnexion, tee. **Aucune donnée
   brute ne transite par un pipe** : la vidéo reste sur le GPU jusqu'à
   l'encodeur.
4. **Décodage média par Media Foundation Source Reader**, fallback FFmpeg
   (webm/VP9 absent de certains Windows). Les médias importés sont déjà
   `mp4/webm` (cf. `import-media-mkv.md`).
5. **Audio par WASAPI** — le module `equaliseur.rs` fait déjà loopback + micro ;
   on factorise au lieu de réécrire.

---

## 6. Support matériel Windows (NVIDIA / AMD / Intel / logiciel)

### 6.1 Matrice de capacités à détecter au démarrage

| Étage | Détection | Fallback |
|---|---|---|
| Capture écran | `IDXGIOutputDuplication` (Win8+) | WGC moniteur |
| Capture fenêtre | WGC `CreateFromWindow` (Win10 1809+) | DXGI + BitBlt/PrintWindow |
| Capture caméra | Media Foundation Source Reader | DirectShow (dshow) |
| Composition | D3D11 FL 11_0 | FL 10_0 (sans compute, masques simplifiés) |
| Encode H.264 | MFT hardware (`MFT_ENUM_FLAG_HARDWARE`) | MFT Microsoft logiciel |
| Encode HEVC | MFT hardware si exposé | H.264 |
| Encode AV1 | MFT hardware si exposé (RTX 40, RX 7000, Arc) | HEVC → H.264 |
| Décodage média | MF Source Reader hardware | FFmpeg logiciel |
| Audio par application | `ActivateAudioInterfaceAsync` + process loopback (Win10 2004+) | loopback global |
| Audio micro | WASAPI capture | — |

### 6.2 Détection d'encodeur (algorithme)

1. `IDXGIFactory1::EnumAdapters1` → liste des adaptateurs
   (`VendorId` : `0x10DE` NVIDIA, `0x1002`/`0x1022` AMD, `0x8086` Intel,
   `0x1414` Microsoft).
2. Créer un device D3D11 par adaptateur candidat (BGRA support).
3. `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE |
   MFT_ENUM_FLAG_SORTANDFILTER, …)` pour chaque couple
   (entrée `NV12`, sortie `H264`/`HEVC`/`AV1`).
4. Vérifier `MF_SA_D3D11_AWARE` (attribut de l'activatе) → sinon l'encodeur
   impose une copie CPU (acceptable en secours, jamais par défaut).
5. Choisir : (a) hardware D3D11-aware sur le **même adaptateur que la
   capture**, (b) hardware sur un autre adaptateur, (c) logiciel Microsoft.
6. Persister le résultat dans `moteur.json` (`encodeur_choisi`,
   `adaptateur_luid`) et l'exposer au diagnostic.

### 6.3 Pièges matériels à documenter dans le code

- **Portables à double GPU** : si la capture est sur l'iGPU (écran) et
  l'encodeur sur le dGPU, une copie inter-adaptateurs est nécessaire
  (texture partagée NT handle). Politique : privilégier un **adaptateur
  unique** ; si impossible, copie explicite + avertissement UI + mesure.
- **Limite de sessions encodeur** : une seule session par sortie ; ne jamais
  ouvrir un second encodeur pour l'aperçu (utiliser le même bitstream).
- **Pilotes AMD anciens** : certains MFT AMF n'exposent que H.264 et ignorent
  les presets ; toujours relire la configuration effective.
- **Intel iGPU anciens** : QSV MFT peut manquer ; basculer logiciel.
- **Windows N / LTSC** : Media Feature Pack absent → décodage média limité ;
  fallback FFmpeg obligatoire.
- **Plein écran exclusif** : WGC ne capture pas certains jeux en flip
  exclusif ; documenter la limite, proposer borderless. **Pas de hooking
  DLL/DX/VK** (anti-cheat — règle déjà posée pour la barre de raccourcis).
- **HDR** : hors périmètre v1 ; si la source est HDR, tonemapper en SDR
  (sinon couleurs délavées). À documenter comme dégradation explicite.

### 6.4 Matrice de test (à remplir par l'utilisateur, une ligne par machine)

| Machine | GPU | Pilote | Capture | H.264 | HEVC | AV1 | 1080p60 | 4K30 | 4K60 |
|---|---|---|---|---|---|---|---|---|---|
| PC principal | à remplir | | | | | | | | |
| PC ancien | à remplir | | | | | | | | |

> Sans cette matrice, « support parfait » n'est pas vérifiable. Le plan impose
> de la remplir au fil des phases, pas à la fin.

---

## 7. Rendu natif — inventaire et stratégie

### 7.1 Sous-systèmes à reproduire (issus de `diffusion.html`)

| # | Sous-système | Source de référence | Complexité |
|---|---|---|---|
| 1 | Fond : image / vidéo / carrousel / capture | `peindreFond`, `renderBg` | moyenne |
| 2 | Fond : morphs bulge/pinch | shader `gl-morph` (2 exemplaires) | faible (shader) |
| 3 | Fond : titres 4 positions (vertical, dégradé, contour) | `applyTitreFond` | moyenne |
| 4 | Widget média + PlayerBar + carrousel + morph + miroir | `renderScene` branche média | moyenne |
| 5 | Chat : messages, badges, avatars, emotes, bulles, cadre par message, compteur viewers | `appendChatMessage`, `fillChatBody` | **élevée** |
| 6 | Caméra : forme cercle/carré, anneau, masque, cachée | `peindreTrous`, `#sos-cam-clip` | moyenne |
| 7 | Input viewer : 4 modes, skins SVG (Xbox/PS/8BitDo/souris/clavier) | `ivGenerer*SVG` | **élevée** |
| 8 | Speedrun : timer, splits, 3 modes de layout, pop de validation | `sr*` | élevée |
| 9 | Goal, musique (skin Winamp), modules HUD | `majGoal*`, `majModule*` | moyenne |
| 10 | Équaliseur (24 bandes) | `eqDessiner` + `equaliseur.rs` | faible (Rust existant) |
| 11 | Spots (2 faisceaux) | `spotsDessiner` | faible-moyenne |
| 12 | Cadres SVG 9 styles + animations SMIL + anneau caméra | `src/lib/cadres/`, `diffusion.html` | **élevée** |
| 13 | Alertes / commandes (média + texte + son + cadre) | `playAlerte`, `playCommande` | moyenne |
| 14 | Clip de bienvenue (squelette + spotlight + rideaux + favoris) | `playWelcomeClip` | **élevée** |
| 15 | Bandeau premier message | `playBandeauPM` | faible |
| 16 | Écran d'attente signature (stries, comète, mascotte, morse) | `majEcranAttente` | moyenne |
| 17 | Présentation théâtrale (rideaux, public, clips, audio) | `presentation-*` | **très élevée** |
| 18 | Pad numérique (plein écran, transparence) | `pad-play` | faible |
| 19 | Emoji Arena (animations CSS) | `#fight-root` | élevée |
| 20 | Just Chatting (layout dérivé) | `jcDeriverScene` | faible en natif |

### 7.2 Stratégie de rendu

- **Texte** : shaping + rasterisation de glyphes via DirectWrite
  (`IDWriteTextLayout` → bitmap) → atlas de glyphes → quads GPU.
  Emojis : COLR/CPAL via DirectWrite ; fallback atlas Twemoji embarqué.
  Emotes BTTV/7TV/FFZ = images distantes déjà résolues par le domaine
  (`EmoteSeg.url`) → textures, comme les avatars/badges.
- **Vectoriel** : `lyon` (tessellation pure Rust) pour les paths de cadres,
  dégradés en shader. Les animations SMIL sont **rejouées par une timeline**
  (`animateMotion` sur `path`, `animateTransform`, `animate` opacité) —
  sous-ensemble fermé, documenté, testé.
  *Alternative évaluée* : `resvg` (fidélité immédiate, mais rasterisation à
  chaque frame en 4K = trop coûteux). Décision proposée : **lyon**.
  ⚠️ **Dépendance à la refonte des cadres** (décision utilisateur du
  2026-10-27) : le système de cadres sera **refait**, pas seulement porté.
  Le moteur natif ne doit donc **pas figer les 9 styles historiques**. Il
  implémente d'abord le **tuyau** (paths → tessellation → dégradés →
  timeline → masque), validé sur **2 styles représentatifs** (un à biseaux
  type cyberpunk, un à trait fin type mgstyle). Le jeu complet de styles
  arrive **après** la refonte, sur la même interface `StyleCadre`.
- **Média** : MF Source Reader → texture → composition (fit/zoom/rot/offset/
  miroir identiques aux formules existantes).
- **Masques** : SDF ou stencil shader à partir des mêmes polygones
  (`svgPathEnPoints` est déjà le générateur commun TS/vanilla) → **une
  troisième implémentation du même polygone** doit être testée par le harnais.
- **Overlays** : chaque overlay = un « renderer » autonome branché sur
  `RuntimeState`, avec sa propre durée/animation, sans dépendance DOM.

### 7.2.1 Chat — ce qui change, ce qui ne change pas (clarification)

> Remarque utilisateur du 2026-10-27 : « le chat, il faudra qu'il fonctionne
> aussi comme ça fonctionne maintenant ; je ne vois pas le rapport avec le
> module C++/Rust ». La remarque est juste : **le chat n'a aucun rapport avec
> le moteur**. Il faut distinguer deux choses.

| Couche | Impact du mode natif |
|---|---|
| **Fonctionnel** (4 plateformes, IRC/WS/HTTP, emotes BTTV/7TV/FFZ, badges, avatars, filtres, compteur viewers, commandes, cadres `!cadres`, suppression de message) | **Aucun changement.** Tout vit dans le domaine Rust existant et continue d'émettre `chat:message` / `chat_tx`. |
| **Représentation** (le rendu à l'écran) | Seul point concerné : en mode OBS c'est du DOM Chromium (`appendChatMessage`) ; en mode natif c'est du texte rasterisé (DirectWrite) + textures d'images. Le **résultat visuel doit être identique** (contrat §8). |

Conséquence pratique : aucun lot du plan ne touche `twitch_chat.rs`,
`kick.rs`, `youtube_chat.rs`, `tiktok_chat.rs`, `twitch_emotes.rs` ni
`commandes.rs`. Le seul lot concerné est `3.7` (renderer chat), et il est
validé par le harnais de conformité, pas par des tests fonctionnels.

### 7.3 Résolution et 4K

- Renderer **vectoriel et indépendant de la résolution** dès le premier jour.
- **Interdit** : reproduire les optimisations « backing store à 40 % »
  (`spots.md`) ou les plafonds 1× du fond — elles sont incompatibles avec
  « pixel perfect 4K ». Si un budget GPU l'exige, la dégradation doit être
  **explicite, mesurée et affichée** par le gouverneur (§9).
- Budget mémoire textures à 4K : BGRA 3840×2160 = 31,6 Mio par texture.
  Pool cible : 2 textures de sortie + 1 par source active + 1 par calque
  rasterisé. À 4 sources : ~200 Mio. À documenter et mesurer.

---

## 8. Fidélité « pixel perfect » — contrat et harnais

### 8.1 Contrat d'acceptation

| Aspect | Exigence | Vérification |
|---|---|---|
| Géométrie, z-order, crop | exacts | diff de boîtes, tolérance 0 px |
| Couleurs / dégradés | exacts (espace linéaire) | ΔE ≤ 1 |
| Typographie | même police, même shaping | SSIM ≥ 0,995 |
| SVG / cadres | mêmes paths, même timeline | comparaison image à image |
| Raster | net à 1×, 2×, 4× | variantes ou scaling Lanczos GPU |
| Statique global | — | SSIM ≥ 0,995, diff max ≤ 2/255 |

### 8.2 Harnais de conformité (obligatoire, phase P3)

- Jeu de **scènes de référence** versionnées : `tests/scenes/*.json`
  (fond, média, chat, caméra cercle, cadre cyberpunk animé, speedrun, goal,
  modules, JC).
- Rendu de référence : `diffusion.html` capturé à taille canvas fixe, avec
  une horloge figée (animations en pause à t=0 et t=1 s).
- Rendu candidat : moteur natif, mêmes scènes, même horloge.
- Comparateur : SSIM + diff max + heatmap de différence, sortie dans
  `target/conformance/<scene>/`.
- **Gate** : aucune phase P3+ validée sans harnais vert sur les scènes de
  référence.

> Risque n°1 du projet : la **dérive** entre `src/lib/cadres/`,
> `diffusion.html`, `chat-popout.html` et le moteur natif. Le harnais est la
> seule mitigation crédible.

---

## 9. Performance — leviers, cibles, gouverneur

### 9.1 Leviers réels (classés par gain)

| Levier | Gain | Détail |
|---|---|---|
| Zéro navigateur dans la sortie | CPU + RAM : très fort | supprime CEF et la surface transparente 60 fps |
| Composition à la demande (dirty regions) | GPU : fort sur scènes statiques | ne recompose que les zones modifiées |
| Culling d'occlusion | GPU + capture : fort | un widget entièrement couvert → session arrêtée |
| FPS par source | GPU : fort | un widget de 320 px n'a pas besoin de 60 fps |
| Dirty rects DXGI | GPU : moyen-fort | bureau statique quasi gratuit |
| Réveil événementiel (waitable) | énergie : fort | pas de spin, pas de polling |
| Encode unique → tee | GPU : moyen | stream + record sans double encodage |
| Gouverneur adaptatif | tenue sur vieux PC | dégradation explicite |

**Non-leviers (ne pas promettre) :** la capture WGC/DXGI elle-même et
l'encodage matériel — OBS est déjà optimal là.

### 9.2 Gouverneur (ordre de dégradation)

1. FPS de capture des sources non critiques : 60 → 30 → 15
2. Résolution de composition : 4K → 1440p → 1080p
3. Preset encodeur : qualité → équilibré → vitesse
4. Résolution d'encodage (dernier recours)

Le gouverneur **affiche toujours** la raison de la dégradation (UI + boîte
noire). Jamais de perte silencieuse.

### 9.3 Compteurs à exposer (alimentent `conso.rs` / boîte noire)

`fps_capture[source]`, `fps_composite`, `drops_capture`, `drops_composite`,
`queue_encode`, `kbps_encode`, `latence_capture_ms`, `adaptateur`,
`encodeur`, `resolution_effective`, `raison_degradation`.

> La boîte noire (`boite_noire.jsonl`, 1 point / 5 s) doit gagner ces champs
> en mode natif. C'est la preuve continue des gains, pas seulement en P0.

### 9.4 Protocole P0 (go / no-go)

1. Baseline OBS : même scène (1 jeu + 1 webcam + 1 chat + 1 alerte), OBS
   **avec** ses browser sources, 1080p60, 30 min.
2. Spike Rust : WGC → composite → MFT → fichier, même scène simplifiée.
3. Mesures : CPU, GPU, RSS, drops, latence (PresentMon + PDH).
4. **Go si 2 cibles sur 3** (CPU ≤ 0,7×, RAM ≤ 0,5×, drops = 0) sont tenues.
   Sinon, arrêt et réévaluation — pas de plan de six mois sur une intuition.

---

## 10. Diagnostic et observabilité du moteur (UI utilisateur)

> **Exigence utilisateur (2026-10-27)** : dans la section Diagnostic existante,
> calculer et afficher **de façon claire et lisible par un non-expert** les
> ressources consommées par le moteur, avec des barres, un pourcentage, et
> l'indication explicite : **tourne-t-on avec le pilote graphique, lequel,
> et en encodage matériel ou logiciel ?**

### 10.1 Principe

- **Aucune valeur fabriquée.** Si une mesure échoue → `N/A`, jamais `0`.
- **Une phrase de verdict en langage clair** en tête, pas seulement des
  chiffres.
- **Zéro polling** hors modale ouverte (invariant existant de `conso.rs`).
- **100 % local**, conforme au CONTRAT FIGÉ vie privée.
- Le diagnostic doit fonctionner **même en mode OBS** : l'énumération des
  adaptateurs est sans effet de bord, l'utilisateur doit toujours voir son
  matériel.

### 10.2 Ce qui existe déjà (à réutiliser, pas à réécrire)

| Élément | Fichier | Réutilisation |
|---|---|---|
| Échantillonneur 1 Hz + historique 300 pts | `conso.rs` | tel quel |
| CPU/RAM par processus | `conso.rs` | tel quel |
| CPU par sous-système (registre `nommer_thread`) | `conso.rs` | ajouter les threads du moteur |
| GPU **par PID** via PDH `\GPU Engine(*)\Utilization Percentage` | `conso.rs` | étendre : agrégation par LUID |
| Boîte noire 5 s (`boite_noire.jsonl`) | `conso.rs` | ajouter les champs moteur |
| Sparklines + lignes + onglets | `ConsoModal.svelte` | réutiliser la recette |
| Polling borné à la modale | `stores/conso.ts` | tel quel |

**Ce qui manque totalement aujourd'hui** (vérifié : aucun `EnumAdapters`,
`VendorId` ni `LUID` dans tout le Rust) : identité de l'adaptateur, version
de pilote, version WDDM, identité de l'encodeur, mode de capture par source,
compteurs d'étages.

### 10.3 Données à ajouter

| Donnée | Source technique | Coût |
|---|---|---|
| Adaptateur(s) : vendor, nom, VRAM, LUID | `IDXGIFactory1::EnumAdapters1` + `DXGI_ADAPTER_DESC1` | 1 fois au boot |
| Version de pilote | WMI `Win32_VideoController.DriverVersion` (PowerShell one-shot, pattern déjà utilisé pour les caméras) ou registre `...\Class\{4d36e968-...}\000N\DriverVersion` | 1 fois au boot |
| Version WDDM | registre `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\WddmVersion` | 1 fois, non fatal |
| Encodeur choisi : nom MFT, matériel/logiciel, codec, preset | résultat de la détection §6.2 | au démarrage de sortie |
| Mode de capture par source (WGC / DXGI / MF) | registre de sessions | continu |
| FPS capture par source / FPS composite | compteurs atomiques | continu |
| Drops capture / composite, file encode | compteurs atomiques | continu |
| Débit réel (kbps), latence capture→encode | encodeur + horloge | continu |
| Résolution effective + raison de dégradation | gouverneur §9.2 | continu |
| GPU total par adaptateur | PDH, agrégation des instances par `luid_` | 1 Hz (modale ouverte) |

> **Détail PDH utile** : les instances du compteur contiennent
> `pid_<PID>_luid_<LUID>_phys_<n>_eng_<n>_engtype_<type>`. On peut donc
> agréger **par PID** (StreamOS seul) *et* **par LUID** (charge de la carte
> entière) — c'est ce qui permet d'afficher « GPU NVIDIA : 43 % » et
> « StreamOS : 12 % » sans aucun appel supplémentaire.

### 10.4 Modèle de données

```rust
// crates/sos-moteur-api/src/diag.rs — partagé, sans tauri
pub struct InfosAdaptateur {
    pub vendor_id: u32,          // 0x10DE / 0x1002 / 0x8086 / 0x1414
    pub vendor_nom: String,      // "NVIDIA" | "AMD" | "Intel" | "Microsoft"
    pub description: String,     // nom commercial de la carte
    pub vram_mio: u64,
    pub driver_version: Option<String>,
    pub wddm: Option<String>,
    pub luid: u64,
}

pub struct InfosEncodeur {
    pub nom: String,             // nom convivial du MFT
    pub materiel: bool,          // MFT_ENUM_FLAG_HARDWARE
    pub codec: String,           // "H.264" | "HEVC" | "AV1"
    pub preset: String,
}

pub struct EtatSource {
    pub nom: String,
    pub mode_capture: String,    // "WGC" | "DXGI" | "MF"
    pub fps: u32,
    pub taille: (u32, u32),
    pub drops: u64,
    pub active: bool,
}

pub struct MoteurDiag {
    pub actif: bool,
    pub adaptateur: Option<InfosAdaptateur>,   // None = N/A (jamais 0)
    pub encodeur: Option<InfosEncodeur>,
    pub sources: Vec<EtatSource>,
    pub fps_composite: u32,
    pub drops_composite: u64,
    pub queue_encode: u32,
    pub kbps: u32,
    pub latence_ms: u32,
    pub resolution: (u32, u32),
    pub degradation: Option<String>,
}
```

- Les compteurs chauds vivent dans `CompteursMoteur` (atomics + `Mutex` sur
  les champs figés) ; `conso.rs` les lit **sans jamais bloquer** le moteur.
- Commande Tauri `moteur_diag() -> MoteurDiag` (synchrone, instantanée) ;
  `ConsoEtat` gagne `moteur: Option<MoteurDiag>` pour n'avoir qu'un seul
  aller-retour par seconde.

### 10.5 UI — nouvel onglet « Moteur » de `ConsoModal.svelte`

Structure (dans l'ordre de lecture, du verdict vers le détail) :

1. **Bandeau verdict** — une phrase, colorée par état :
   - « Encodage matériel NVIDIA (NVENC) — GPU 14 %, CPU moteur 3 %. »
   - « Encodage logiciel Microsoft — CPU 62 % : réduisez la résolution ou le FPS. »
   - « Moteur natif inactif — vous diffusez via OBS. »
   - « Deux GPU détectés : capture Intel, encodage NVIDIA — copie inter-GPU (+8 % GPU). »
2. **Carte « Pilote graphique »** : badge constructeur, nom de la carte,
   version de pilote, VRAM, WDDM, LUID (repliable). `N/A` si non mesurable.
3. **Carte « Encodage »** : nom du MFT, **matériel / logiciel**, codec, preset,
   débit, profondeur de file.
4. **Barres** (réutiliser la recette sparkline/ligne existante) :
   GPU total (par adaptateur), GPU StreamOS, CPU moteur, RAM moteur,
   file d'encodage (%). Seuils : < 70 % neutre/vert, 70–90 % orange,
   > 90 % rouge ; `drops > 0` → orange.
5. **Tableau « Sources »** : nom, mode de capture, FPS, taille, drops.
6. **Raison de dégradation** si le gouverneur est intervenu, avec la valeur
   avant/après.
7. **Bouton « Copier le rapport »** (texte local, pour le support) —
   aucune transmission.

Contraintes UI : tokens `--t-*` uniquement (aucune couleur en dur), rem pour
le chrome, 8 locales (`conso.moteur.*`), libellés compris par un non-expert
(« Pilote graphique », « Encodage matériel », « Images perdues »).

### 10.6 Boîte noire — champs ajoutés

`boite_noire.jsonl` (1 point / 5 s) gagne, de façon **additive**
(rétro-compat des lignes existantes) :
`moteur_actif`, `encodeur`, `materiel`, `fps_composite`, `drops`,
`gpu_moteur_pct`, `degradation`.
C'est la seule trace qui survit à un gel — elle doit dire **quel étage** a
lâché, pas seulement que l'app a gelé.

### 10.7 Règles

1. Aucune valeur inventée ; `N/A` explicite.
2. Aucun polling hors modale.
3. L'énumération d'adaptateur est autorisée au boot et à la demande (coût
   négligeable, aucun effet de bord).
4. Le diagnostic ne doit **jamais** être nécessaire au fonctionnement du
   moteur (découplage strict : le moteur écrit des atomics, le diagnostic lit).
5. Toute nouvelle mesure ajoutée ici doit être documentée dans
   `docs/features/diagnostic.md`.

---

## 11. Mode dual — machine à états et règles

### 11.1 États

```
Inactif ──basculer(natif)──► TeardownObs ──► InitNatif ──► ActifNatif
   ▲                                                        │
   └──────────── TeardownNatif ◄── basculer(obs) ◄──────────┘
```

- `mode_switch(cible)` = drain → teardown → init → événement d'état.
- L'UI affiche une **modale bloquante avec progression** (pattern
  `ConfirmObsOutputModal`), jamais un toggle qui ment sur l'instantanéité.
- Persistance : `%APPDATA%\com.streamos.v0\moteur.json`
  (`{ "mode": "obs" | "natif", "encodeur_choisi": …, "adaptateur_luid": … }`).

### 11.2 Règles de sécurité (figées dès le premier lot)

1. **Ne jamais couper un live implicitement.** Basculer pendant un stream
   OBS actif ne doit jamais appeler `StopStream`. Décision UI reportée, mais
   le moteur expose `peut_basculer()` et refuse par défaut.
2. **Exclusivité caméra.** En mode OBS, `SOS-Caméra` tient le capteur dshow.
   En natif, le moteur doit l'ouvrir (Media Foundation). Le switch libère
   d'un côté, acquiert de l'autre, avec retry et état visible.
   Le mode OBS ne doit **jamais** appeler `RemoveInput` (règle figée).
3. **Atomicité + watchdog.** Si le moteur natif meurt, l'app survit ; l'état
   passe en `Erreur`, l'enregistrement en cours est finalisé si possible
   (fMP4), et l'UI propose de revenir en mode OBS.
4. **Zéro régression OBS.** Tout lot doit laisser le mode OBS strictement
   identique (mêmes appels, même ordre, mêmes contrats).

---

## 12. Phases et lots

> Convention : chaque lot = liste blanche fermée. Chaque fichier = un diff =
> une pause. Un lot n'est « fait » que si `cargo clippy` (exit 0),
> `npm run check` (0 erreur) et les tests du lot passent.

### Phase P0 — Preuve (2 semaines max, aucun code livré)

| Lot | Fichiers | Contenu | Critère de sortie |
|---|---|---|---|
| 0.1 | `docs/features/moteur-natif/mesures-p0.md` (nouveau) | protocole + baseline OBS chiffrée | tableau rempli |
| 0.2 | `spikes/moteur-spike/{Cargo.toml,src/main.rs,src/gpu.rs,src/capture.rs,src/encode.rs}` | WGC → D3D11 → MFT → fichier | vidéo lisible + mesures |
| 0.3 | `docs/features/moteur-natif/mesures-p0.md` | comparaison + verdict go/no-go | verdict explicite |

**Gate P0 :** 2 cibles sur 3 tenues. Sinon STOP et réévaluation.

### Phase P1 — Socle (extraction, zéro feature)

| Lot | Fichiers | Contenu | Critère |
|---|---|---|---|
| 1.1 | `src-tauri/Cargo.toml`, `crates/sos-scene/{Cargo.toml,src/lib.rs}`, `src-tauri/src/scene.rs`, `src-tauri/src/config.rs`, `src-tauri/src/lib.rs` | extraire `Scene`/`Widget`/`CadreConfig`/`ModuleHud`/`MorphPoint`/`MediaSlide`/`SpeedrunSettings` dans `sos-scene` ; app ré-exporte | JSON sur disque **identique** ; tests existants verts |
| 1.2 | `crates/sos-moteur-api/{Cargo.toml,src/lib.rs}` | trait `Moteur`, `Sink`, `Capacites`, `RuntimeState`, erreurs | `cargo test -p sos-moteur-api` |
| 1.3 | `src-tauri/src/moteur/mod.rs`, `src-tauri/src/moteur/obs.rs`, `src-tauri/src/lib.rs` | `MoteurObs` qui délègue à `obs_trou`/`obs` ; registre | mode OBS inchangé (test manuel scène + caméra + masque) |
| 1.4 | `src-tauri/src/moteur/mode.rs`, `src-tauri/src/config.rs`, `src-tauri/src/lib.rs` | machine à états + `moteur.json` + commandes `moteur_etat`/`moteur_basculer` | bascule OBS→OBS sans effet ; natif indisponible → erreur propre |
| 1.5 | `src/lib/stores/moteur.ts`, `src/lib/components/Toolbar.svelte`, `src/lib/i18n/*.json` | UI : bouton OBS / Rust + état + capacités | 8 locales, tokens `--t-*` |

**Gate P1 :** mode OBS identique, aucun changement de comportement observable.

### Phase P2 — Tuyau natif (capture + composition + fichier)

| Lot | Fichiers | Contenu | Critère |
|---|---|---|---|
| 2.1 | `crates/sos-moteur-natif/{Cargo.toml,src/lib.rs,src/gpu.rs}` | device D3D11, pool de textures, formats | tests unitaires |
| 2.2 | `…/src/capture/mod.rs`, `…/src/capture/wgc.rs`, `…/src/capture/dxgi.rs` | sessions, refcount, hot-plug | 2 moniteurs + 1 fenêtre simultanés |
| 2.3 | `…/src/compositor/mod.rs`, `…/src/compositor/layers.rs` | calques, z, viewports, crop, dirty regions | scène 3 sources 60 fps |
| 2.4 | `…/src/clock.rs`, `…/src/gouverneur.rs` | horloge, FPS par consommateur, dégradation | compteurs exposés |
| 2.5 | `…/src/encode/mft.rs`, `…/src/encode/detect.rs` | détection + encodeur MFT zéro-copie | H.264 sur NVIDIA/AMD/Intel |
| 2.6 | `…/src/output/mod.rs`, `…/src/output/file.rs`, `…/src/output/ffmpeg.rs` | `Sink`, `FileSink` fMP4, sidecar | fichier lisible après coupure simulée |
| 2.7 | `…/src/preview.rs`, `src-tauri/src/moteur/mod.rs` | aperçu basse résolution → dashboard | 10 fps, < 2 % CPU |
| 2.8 | `src-tauri/src/moteur/mod.rs`, `src/lib/components/Toolbar.svelte` | boutons Démarrer/Arrêter l'enregistrement natif | confirmation explicite |
| 2.9 | `crates/sos-moteur-api/src/diag.rs`, `crates/sos-moteur-natif/src/gpu.rs`, `crates/sos-moteur-natif/src/encode/detect.rs`, `src-tauri/src/conso.rs`, `src-tauri/src/moteur/mod.rs` | compteurs d'étages + `moteur_diag()` + champs boîte noire | valeurs réelles, `N/A` si non mesurable, moteur jamais bloqué par la lecture |
| 2.10 | `src/lib/components/ConsoModal.svelte`, `src/lib/stores/conso.ts`, `src/lib/tauri.ts`, `src/lib/i18n/*.json` | onglet « Moteur » : verdict, pilote, encodeur, barres, sources | lisible par un non-expert, 8 locales, tokens `--t-*` |

**Gate P2 :** 1080p60, 3 sources, 0 drop, 30 min ; 4K testé et documenté ;
**diagnostic « Moteur » opérationnel et exact** (pilote, encodeur, barres).

### Phase P3 — Rendu (parité visuelle)

| Lot | Fichiers | Contenu |
|---|---|---|
| 3.1 | `…/src/render/text/*` | shaping DirectWrite, atlas, emojis |
| 3.2 | `…/src/render/vector/*` | lyon + dégradés + timeline SMIL |
| 3.3 | `…/src/render/media/*` | MF Source Reader, fit/zoom/rot/miroir/carrousel |
| 3.4 | `…/src/render/mask.rs` | masques SDF/stencil (caméra + cadres) |
| 3.5 | `…/src/render/cadres/*` | **tuyau** cadres (paths → tessellation → dégradés → timeline → masque) + **2 styles représentatifs** seulement ; jeu complet après la refonte |
| 3.6 | `…/src/render/titres.rs` | 4 positions, vertical, contour |
| 3.7 | `…/src/render/widgets/{chat,camera,inputviewer,speedrun,goal,musique}.rs` | widgets |
| 3.8 | `…/src/render/modules/*` | horloge, musique, objectifs, équaliseur, spots |
| 3.9 | `tests/conformance/*` | harnais golden images |

**Gate P3 :** harnais vert sur les 10 scènes de référence (SSIM ≥ 0,995).

### Phase P4 — Overlays

`alertes`, `commandes`, `welcome`, `bandeau`, `BRB`, `pad`, `fight`,
`presentation`, `JC` — un lot par overlay, dans cet ordre (du plus simple au
plus complexe). Chaque lot = un fichier de renderer + son test de conformité.

**Gate P4 :** les 20 sous-systèmes du §7.1 sont couverts ou explicitement
dégradés (documentés).

### Phase P5 — Différenciation et robustesse

| Lot | Contenu | Bénéfice |
|---|---|---|
| 5.1 | `Sink` RTMP + SRT + `TeeSink` | stream + record, une seule passe |
| 5.2 | Audio par application (process loopback) | OBS ne le fait pas bien |
| 5.3 | ROI `qpDeltaMap` par widget | qualité à débit égal |
| 5.4 | Deux sorties simultanées 16:9 + 9:16 | un format vertical sans dupliquer la scène |
| 5.5 | Reprise après crash (fMP4 + journal) | « zéro enregistrement perdu » |
| 5.6 | Backends SDK constructeurs (NVENC/AMF/oneVPL) | qualité/contrôle supérieurs |

---

## 13. Invariants et anti-patterns du moteur natif

### 13.1 Invariants

1. Aucune frame brute ne traverse le CPU entre la capture et l'encodeur.
2. Aucun thread actif quand le moteur est inactif.
3. Aucun `.lock()` double dans une expression.
4. Aucun appel bloquant sur le thread UI.
5. File bornée partout ; politique explicite de drop (oldest, jamais block).
6. `appliquer_scene` idempotent et différentiel.
7. Toute dégradation est mesurée, exposée et affichée.
8. Le moteur ne connaît ni Tauri ni le domaine.

### 13.2 Anti-patterns (à ne jamais réintroduire)

1. ❌ Ajouter Chromium/WebView2 dans le chemin de sortie.
2. ❌ Copier une frame GPU→CPU « pour simplifier ».
3. ❌ Recréer les sessions de capture à chaque `commitScene`.
4. ❌ Ouvrir un second encodeur pour l'aperçu.
5. ❌ Rasteriser un cadre SVG à chaque frame en 4K.
6. ❌ Détruire `SOS-Caméra` (`RemoveInput`) au passage de mode.
7. ❌ Bloquer la bascule sur un stream actif sans confirmation.
8. ❌ Dupliquer la géométrie des cadres dans une 4ᵉ implémentation sans test
   de conformité.

---

## 14. Risques

| Risque | Impact | Mitigation |
|---|---|---|
| Dérive entre renderers | majeur | harnais golden images (P3), source unique de géométrie |
| Rendu des 5 sous-systèmes « très élevés» | majeur | phases P4 séparées, dégradation documentée possible |
| MSRV 1.77 bloquante | moyen | D3D11 direct + MFT + sidecar = pas de C, pas de crate récent |
| Pilotes constructeurs hétérogènes | moyen | MFT universel + fallback logiciel + matrice §6.4 |
| Double GPU (portable) | moyen | politique adaptateur unique + avertissement |
| Exclusivité caméra | moyen | machine à états + retry + état visible |
| Double QA OBS/natif | moyen | mode OBS gelé fonctionnellement ; tests de non-régression |
| Arbre de travail sale au départ | élevé | commit/stash de base obligatoire avant LOT 0 |
| Périmètre qui s'élargit (fond vert, HDR, jeu) | élevé | hors périmètre explicite §2.4 et §6.3 |

---

## 15. Décisions — tranchées et ouvertes

### 15.1 Tranchées (session du 2026-10-27)

| # | Décision | Conséquence sur le plan |
|---|---|---|
| D1 | **Moteur full natif en Rust**, aucun DOM Chromium dans la sortie | §4, §5, §7 ; pas de calque WebView2 capturé |
| D2 | **Fond vert virtuel hors périmètre** | aucun lot ; `Capacites.fond_vert = false` en natif |
| D3 | **Le système de cadres sera refait** (pas seulement porté) | le moteur natif n'implémente **pas** les 9 styles historiques : tuyau + 2 styles représentatifs (§7.2, lot 3.5) |
| D4 | **Le chat doit fonctionner comme aujourd'hui** | fonctionnel inchangé ; seul le rendu change (§7.2.1) ; aucun lot ne touche le domaine chat |
| D5 | **Support matériel total** : NVIDIA, AMD, Intel, logiciel | MFT universel + matrice §6 + fallback logiciel obligatoire |
| D6 | **Diagnostic moteur dans la section existante**, lisible par un non-expert | §10 + lots 2.9 / 2.10 |
| D7 | Priorité : capture + enregistrement fluides, moins lourds qu'OBS, PC anciens | gouverneur §9.2, cibles §1.2 |

### 15.2 Ouvertes (à trancher avant la phase concernée)

1. **Cadres — moteur de rendu** : `lyon` (procédural, 4K natif) ou `resvg`
   (fidélité immédiate, coût par frame) ? *Proposition : `lyon`* — mais la
   question devient secondaire puisque les styles seront refaits : le tuyau
   doit surtout être **agnostique du style**.
2. **Chat — cas rares** : shaping complet (RTL, ligatures) ou « équivalent
   visuel » assumé ? *Proposition : shaping complet via DirectWrite, cas
   rares documentés.* Ce n'est plus une question produit, seulement un choix
   d'implémentation du renderer 3.7.
3. **Ordre de la tranche verticale P2/P3** : `fond + 1 capture + caméra +
   chat + 1 cadre` avant tout overlay ? *Proposition : oui.*
4. **Politique de bascule pendant un live** : blocage ou confirmation ?
   *Proposition : confirmation explicite + refus si stream natif actif.*
5. **MSRV** : rester 1.77 ou monter (si un backend SDK l'exige) ?
   *Proposition : rester 1.77 tant que MFT + sidecar suffisent.*

---

## 16. Annexes

### 16.1 Checklist de démarrage d'un lot

- [ ] `git status` propre (ou stash validé)
- [ ] lot annoncé avec sa liste blanche de fichiers
- [ ] diagnostic du fichier concerné affiché avant écriture
- [ ] un seul fichier modifié
- [ ] `cargo clippy` + `npm run check` verts
- [ ] diff montré, pause, validation utilisateur

### 16.2 Glossaire

- **MFT** : Media Foundation Transform — encodeur/décodeur Windows.
- **WGC** : Windows Graphics Capture.
- **DXGI duplication** : capture d'écran GPU (Desktop Duplication API).
- **fMP4** : MP4 fragmenté, lisible après coupure.
- **ROI** : Region Of Interest — allocation de bits par zone.
- **Sink** : destination d'un flux encodé (fichier, RTMP, SRT, aperçu).
- **Gouverneur** : régulateur de qualité/performance.

### 16.3 Références internes

- `AGENTS.md` — protocole, contrats figés, pièges.
- `docs/features/camera-details.md` — caméra, masque, formes, fond vert.
- `docs/features/fond-capture-obs.md` — `SOS-Fond-*`.
- `docs/features/selecteur-capture.md` — WGC/énumération (Rust existant).
- `docs/features/obs-sorties.md` — Start/Stop explicites, confirmations.
- `docs/features/just-chatting.md` — dérivation de scène.
- `docs/features/equaliseur.md` — WASAPI + FFT réutilisables.
- `docs/features/input-viewer.md` — polling global, hooks.
- `docs/features/telemetrie.md` / `diagnostic.md` — compteurs et boîte noire.
- `E:\SteamOs - All Version\stream-operating-system-react\DOCS-REACT-NODE-VITE\`
  — moteur dual OBS/C++ historique (référence conceptuelle, **pas** du code
  à porter : il est Electron/C++ et sous licence MIT d'un autre projet).

---

*Fin du plan. Prochain document après validation : `docs/features/moteur-natif.md`
(contrat technique : traits, structures, invariants, machine à états, API).*
