# Moteur natif — capturer, composer et encoder sans OBS

> **Feature NON IMPLÉMENTÉE dans l'app** au 2026-10-28 — le crate
> `sos-moteur-natif` existe et est **mesuré** (§27), mais rien n'est branché à
> l'interface. Ce fichier fixe ce qui est **décidé** et ce qui est
> **interdit** ; le déroulé complet (mesures, matrices matérielles,
> algorithmes) vit dans `PLAN-MOTEUR-NATIF.md` à la racine.
> ⚠️ **Aucun chiffre de ce fichier n'est mesuré, hors §27.** Toute valeur est
> une **cible** du plan, jamais un résultat observé. Ne pas citer ces nombres
> comme acquis.
>
> **Doc de feature — contrat technique.** Les règles figées (protocole
> d'exécution, contrat vie privée, contrats caméra/cadres) restent dans
> `AGENTS.md` et priment sur ce document.

---

## 1. Objectif

Faire de StreamOS un **diffuseur autonome** : capturer, composer, encoder et
diffuser/enregistrer **sans OBS**, en rendu pixel perfect jusqu'en 4K, avec
une consommation **inférieure à OBS à fonctionnalités égales**, et un support
matériel **NVIDIA + AMD + Intel** complet sous Windows.

Le mode OBS actuel (obs-websocket + browser source `:4321`) **reste intact et
fonctionnel** : les deux modes coexistent et se basculent à la volée.

> « Plus rapide qu'OBS » n'est pas un objectif global. OBS **nu** est déjà
> proche de l'optimum GPU : on ne le bat pas là. On le bat sur l'absence de
> navigateur dans la sortie, la composition à la demande, le fan-out de
> capture et le culling d'occlusion.

### 1.1 Cibles mesurables (plan §1.2 — à confirmer par les mesures P0)

| Critère | Cible |
|---|---|
| Parité visuelle | SSIM ≥ 0,995 ; diff max ≤ 2/255 sur images statiques |
| 1080p60, 3 sources | 0 frame perdue capture/composite sur 30 min |
| 4K60 | tenu sur GPU de classe définie, sinon dégradation expliquée |
| CPU | ≤ 0,7 × OBS (mêmes fonctions, browser sources incluses) |
| RAM | ≤ 0,5 × OBS |
| Latence capture→encode | ≤ OBS − 1 frame |
| Démarrage à l'enregistrement | ≤ 5 s |
| Compatibilité | NVIDIA, AMD, Intel (iGPU et dGPU), encodeur logiciel en secours |
| Diagnostic lisible | pilote, encodeur (matériel/logiciel), barres et verdict affichés ; `N/A` si non mesurable |

---

## 2. Décisions tranchées (session du 2026-10-27)

| # | Décision | Conséquence |
|---|---|---|
| D1 | Moteur **full natif en Rust**, aucun DOM Chromium dans la sortie | pas de calque WebView2 capturé |
| D2 | Fond vert virtuel **hors périmètre** | `Capacites.fond_vert = false` en natif |
| D3 | Le système de cadres **sera refait** | le moteur n'implémente **pas** les 9 styles historiques : tuyau + 2 styles représentatifs |
| D4 | Le chat doit **fonctionner comme aujourd'hui** | fonctionnel inchangé, seul le rendu change |
| D5 | Support matériel **total** | MFT universel + fallback logiciel obligatoire |
| D6 | Diagnostic moteur dans la section **Diagnostic existante** | onglet « Moteur », lisible par un non-expert |
| D7 | Priorité : capture + enregistrement fluides, moins lourds qu'OBS | gouverneur de dégradation |

---

## 3. Architecture — trois anneaux + règle de dépendance

```
Anneau 0 : CONTRATS   sos-scene (Scene, Widget, CadreConfig, ModuleHud,
                      MorphPoint, MediaSlide, SpeedrunSettings)
Anneau 1 : DOMAINE    chat, alertes, commandes, speedrun, welcome, musique,
                      fight, télémétrie, viewers
Anneau 2 : MOTEURS    sos-moteur-api (trait Moteur) + sos-moteur-natif
                      + src/moteur/obs.rs (adaptateur OBS)
Anneau 3 : APPLI      commandes Tauri, bus d'état, WS :4321, machine de mode
```

**Règles** (plan §4.1) :

- `sos-moteur-natif` **ne dépend jamais** de `tauri`, de `crate::` de l'app,
  ni du domaine. Il ne connaît que `sos-scene` + `sos-moteur-api` + `windows`.
- L'adaptateur OBS reste **dans l'app** (`src/moteur/obs.rs`) : il délègue aux
  modules existants. `obs_trou.rs` n'est **pas déplacé** — on l'enveloppe.
- Le domaine ne connaît aucun moteur : aucune fonction de `chat`, `alertes`,
  `speedrun`… n'importe `sos_moteur_*`.
- Arbre cible : `src-tauri/crates/{sos-scene,sos-moteur-api,sos-moteur-natif}`
  + `src-tauri/src/moteur/{mod,obs}.rs`. Workspace pour que le moteur se
  compile et se teste **sans Tauri** (`cargo test -p sos-moteur-natif`).

### 3.1 Trait `Moteur` (contrat de l'anneau 2)

```rust
pub trait Moteur: Send + Sync {
    fn capacites(&self) -> Capacites;
    fn appliquer_scene(&self, scene: &Scene);        // idempotent, différentiel
    fn appliquer_etat(&self, etat: &RuntimeState);   // overlays, chat, cadres
    fn demarrer_sortie(&self, cfg: SortieCfg) -> Result<(), ErreurMoteur>;
    fn arreter_sortie(&self, kind: SortieKind) -> Result<(), ErreurMoteur>;
    fn etat(&self) -> MoteurEtat;                    // Inactif|Actif|Erreur(String)
    fn apercu(&self) -> Option<ApercuFrame>;         // basse résolution, optionnel
    fn arreter(&self);                               // teardown ordonné, idempotent
}
```

`Capacites` gère l'UI **sans** `if mode == …` (`capture_ecran`,
`capture_fenetre`, `capture_jeu`, `camera`, `fond_vert`, `masque_shader`,
`cadres_animes`, `audio_par_app`, `roi_encode`, `multi_format`, `hdr`, `srt`,
`ndi`…).

**Sémantique obligatoire :**

- `appliquer_scene` **ne bloque pas** (file bornée, drop-oldest) et est
  **différentielle** : un déplacement de widget ne recrée ni la session de
  capture, ni la texture, ni l'encodeur.
- `appliquer_scene` est **idempotente** : deux fois la même scène = aucun effet.
- `arreter` est appelable dans n'importe quel état, plusieurs fois.

### 3.2 Trait `Sink` — un seul encode, N sorties

```rust
pub trait Sink: Send {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur>;
    fn fermer(&mut self) -> Result<(), ErreurMoteur>;
}
// FileSink(fMP4/MKV) · PreviewSink · RtmpSink · SrtSink · TeeSink(Vec<Sink>)
```

**MVP = `FileSink` + `PreviewSink`.** RTMP/SRT arrivent ensuite **sans
réécriture** : ce sont des sinks supplémentaires, pas un second encodeur.

- `TeeSink` diffuse le même paquet à tous ; un sink en erreur est retiré sans
  arrêter les autres.
- L'ordre des paquets est garanti ; aucun sink ne peut ralentir l'encodeur
  au-delà de sa file (drop-oldest + compteur `drops_sink`).

### 3.3 `RuntimeState`

Instantané (pas un bus) des données éphémères nécessaires au rendu :
`just_chatting`, `alertes`, `commandes`, `welcome`, `bandeau`,
`ecran_attente`, `presentation`, `fight`, `pad`, `cadres_chat`, `chat`
(`Arc<ChatBuffer>`), `goals`, `modules`. Produit par le domaine, consommé par
le moteur. **Aucune logique métier dans le moteur.**

---

## 4. Mode dual — états et règles de sécurité

```
Inactif ──basculer(natif)──► TeardownObs ──► InitNatif ──► ActifNatif
   ▲                                                        │
   └──────────── TeardownNatif ◄── basculer(obs) ◄──────────┘
```

- `mode_switch(cible)` = drain → teardown → init → événement d'état.
- L'UI affiche une **modale bloquante avec progression** (pattern
  `ConfirmObsOutputModal`) — jamais un toggle qui ment sur l'instantanéité.
- Persistance : `%APPDATA%\com.streamos.v0\moteur.json`
  (`{ "mode": "obs"|"natif", "encodeur_choisi": …, "adaptateur_luid": … }`).

> ⚠️ **Règles figées dès le premier lot** (plan §11.2) :
>
> 1. **Ne jamais couper un live implicitement.** Basculer pendant un stream
>    OBS actif ne doit jamais appeler `StopStream`. Le moteur expose
>    `peut_basculer()` et **refuse par défaut**.
> 2. **Exclusivité caméra.** En mode OBS, `SOS-Caméra` tient le capteur
>    dshow ; en natif, le moteur l'ouvre (Media Foundation). Le switch libère
>    d'un côté, acquiert de l'autre, avec retry et état visible. Le mode OBS
>    ne doit **jamais** appeler `RemoveInput` (règle figée caméra).
> 3. **Atomicité + watchdog.** Si le moteur meurt, l'app survit ; l'état passe
>    en `Erreur`, l'enregistrement en cours est finalisé si possible (fMP4),
>    l'UI propose de revenir en mode OBS.
> 4. **Zéro régression OBS.** Tout lot laisse le mode OBS strictement
>    identique (mêmes appels, même ordre, mêmes contrats).

### 4.1 Ordre de teardown (impératif)

1. Arrêter les sorties (fermer les sinks → finaliser le fMP4).
2. Arrêter l'encodeur.
3. Arrêter le compositeur.
4. Arrêter les sessions de capture (refcount → 0).
5. Arrêter l'audio.
6. Libérer le device D3D11 et le pool.
7. Joindre tous les threads (timeout 3 s, puis détachement loggé).

### 4.2 Séquence de propriété caméra

- **OBS → natif :** `SetSceneItemEnabled false` sur l'item caméra (jamais
  `RemoveInput`), attendre la libération du capteur (retry 250 ms × 8), puis
  ouvrir en Media Foundation.
- **Natif → OBS :** fermer la session MF, puis `camera_sync` pour réactiver
  l'item.
- Échec d'acquisition → `MoteurEtat::Erreur("camera occupee")` + message UI
  explicite, **jamais** de bascule silencieuse.

---

## 5. Threads et files

| Thread | Entrée | Sortie | File | Politique |
|---|---|---|---|---|
| `moteur-controle` | commandes | état | 64 | drop-oldest |
| `moteur-capture-<id>` | source | texture partagée | 3 | drop-oldest |
| `moteur-composite` | textures | texture de sortie | — | cadencé horloge |
| `moteur-encode` | texture NV12 | `PaquetEncode` | 8 | drop-oldest + compteur |
| `moteur-audio` | WASAPI | PCM 48 kHz | 16 | drop-oldest |
| `moteur-output` | paquets | sinks | 32 | bloque le sink, jamais l'encodeur |
| `moteur-watchdog` | compteurs | actions | — | 1 Hz |

**Règles :**

- Nommage obligatoire via `conso::nommer_thread("Moteur …")` → visible dans
  le diagnostic (sous-systèmes).
- Aucun `.lock()` double dans une expression (piège Mutex non réentrant).
- Aucun appel bloquant sur le thread UI Tauri.
- Zéro thread quand le moteur est `Inactif`.

---

## 6. Horloge et cadencement

- Horloge **monotone** (`QueryPerformanceCounter`), jamais l'heure système.
- `fps_cible` = fps de sortie (CFR pour l'encodeur).
- **FPS par source** : chaque session a son `MinUpdateInterval` (WGC) et son
  consommateur peut échantillonner moins souvent.
- Si une source n'a pas de nouvelle frame : **réutiliser la dernière**
  (cache de texture), jamais bloquer.
- `pts_us`/`dts_us` calculés depuis l'horloge + `frame_index × (1e6/fps)`
  pour garantir la CFR exigée par l'encodeur.
- Audio horodaté séparément (§13) ; l'offset A/V est un réglage explicite.

---

## 7. Capture

### 7.1 Registre

```rust
struct CleSource { kind: KindSource, cible: String }
// "monitor:<id>" | "window:<hwnd>" | "camera:<device>"
```

- `HashMap<CleSource, Session>` avec **refcount** : un widget, le fond,
  l'aperçu et l'enregistrement partagent la même session.
- Refcount → 0 : session arrêtée **et** texture rendue au pool.
- Une source ne peut pas être capturée deux fois (conflit d'accès exclusif
  caméra, coût GPU inutile ailleurs).

### 7.2 Modes

| Cible | Mode principal | Fallback |
|---|---|---|
| Moniteur | DXGI Desktop Duplication | WGC moniteur |
| Fenêtre | WGC `CreateFromWindow` | DXGI + BitBlt/PrintWindow |
| Jeu (borderless) | WGC | — |
| Caméra | Media Foundation Source Reader | DirectShow |
| Média (vidéo) | MF Source Reader | FFmpeg logiciel |

### 7.3 Réglages WGC

- `IsCursorCaptureEnabled` : piloté par la scène (défaut : oui).
- `IsBorderRequired` (Win11) : `false` si supporté — jamais de bordure jaune.
- `MinUpdateInterval` : dérivé du FPS par consommateur.
- `CreateFreeThreaded` : obligatoire (pas d'exigence MTA).
- Frame pool : **4 buffers** (leçon de l'archive : 2 provoque des drops).

### 7.4 DXGI

- `AcquireNextFrame(timeout)` événementiel ; `ACCESS_LOST` → recréer la
  duplication (résolution changée, verrouillage de session).
- `GetFrameDirtyRects` / `GetFrameMoveRects` exploités par la composition à
  la demande (§9.3).
- **Jamais** de copie de frame en CPU.

### 7.5 Culling d'occlusion

Une session dont **tous** les consommateurs sont entièrement couverts par un
calque opaque supérieur est **arrêtée** (refcount logique), puis relancée au
dévoilement. Levier majeur sur PC anciens.

### 7.6 Erreurs

`ErreurMoteur::Capture { source, cause }` → l'UI affiche la source fautive et
propose un repli (autre mode, autre cible). Jamais de crash, jamais de
fenêtre noire.

---

## 8. GPU

- **Un seul adaptateur** pour capture + composition + encodage (politique par
  défaut). Deux adaptateurs = copie inter-GPU explicite, comptée et affichée.
- Sélection : adaptateur qui possède l'écran capturé ; sinon le plus capable
  (VRAM, FL) ; le logiciel seul est refusé (le moteur exige un GPU).
- Feature level **11_0** minimum ; 10_0 accepté avec dégradation documentée
  (pas de compute, masques simplifiés).
- Formats internes : **BGRA8** pour la composition, **NV12** pour l'encodeur.
  Conversion **en shader** (ou `ID3D11VideoProcessor`) — jamais en CPU.
- Espace couleur : composition en sRGB, conversion explicite en fin de
  chaîne. HDR hors périmètre v1 (tonemap SDR documenté).
- Pool de textures : 2 sorties + 1 par source active + 1 par calque
  rasterisé. Budget 4K documenté (~200 Mio à 4 sources).

---

## 9. Compositeur

### 9.1 Contrat d'ordre (figé par l'existant)

| Plage z | Contenu |
|---|---|
| 0 … N-1 | widgets (ordre `w.z` de la scène) |
| 5000 | caméra (réservée) |
| 8000 / 9000 | modules HUD |
| 9000 | aides dashboard (non diffusées) |
| 9990 | écran d'attente |
| 9993 | arène, pop de splits |
| 9995 | pad numérique |
| 9996 | commandes |
| 9997 | alertes |
| 9998 | bandeau premier message |
| 9999 | clip de bienvenue |
| 10000-10004 | présentation |

Le compositeur **consomme ce tableau**, il ne réinvente pas d'ordre.

### 9.2 Transforms

Les formules de `fit` (ajuster/remplir/étirer), `zoom`, `rotation`, `offsets`,
`miroir` H/V, `crop` et `crop ratio` sont **celles de `diffusion.html`**
(`applyMediaStyle`, `mediaTransform`, `dessinerOffscreen`). Toute divergence
est un bug de conformité (§21), pas une liberté d'implémentation.

### 9.3 Composition à la demande

1. Chaque calque porte un **rect sale** (dirty rect).
2. Un calque sale ⇒ seules ses zones sont recomposées ; les autres sont
   recopiées depuis la texture précédente (`CopySubresourceRegion`).
3. Aucun calque sale ⇒ la frame de sortie est **réutilisée telle quelle**
   (l'encodeur reçoit la même texture).
4. Les dirty rects DXGI sont propagés au calque correspondant.
5. Les animations (cadres, overlays, texte) déclarent leur rect sale à chaque
   frame où elles changent.

### 9.4 Masques

- Polygones issus de `svgPathEnPoints` / `pointsClipPathCadre` /
  `pointsCercle` (mêmes générateurs que le dashboard et `diffusion.html`).
- Rendu : **stencil** ou **SDF** selon la taille ; jamais de PNG régénéré.
- Le masque caméra et la découpe du cadre partagent **exactement** le même
  polygone (leçon « zéro dérive » 2026-09-29 / 2026-10-22).

---

## 10. Rendu natif

- **Aucun Chromium/WebView2 dans le chemin de sortie** (D1). Le rendu est
  reconstruit en natif : texte (DirectWrite), vecteurs (paths + tessellation +
  dégradés), médias (Media Foundation), masques, cadres, titres, widgets,
  modules HUD, overlays.
- **Cadres (D3)** : on construit le **tuyau** — paths → tessellation →
  dégradés → timeline d'animation → masque — validé sur **2 styles
  représentatifs** (un à biseaux, un à trait fin). Le jeu complet des 9 styles
  arrive **après** la refonte du système de cadres, sur la même interface
  `StyleCadre`. Ne pas porter les 9 styles maintenant : travail jeté.
- **Harnais de conformité obligatoire** (phase P3) : images de référence,
  SSIM ≥ 0,995 sur 10 scènes. Une régression visuelle doit être **détectée
  automatiquement**, pas à l'œil.

### 10.1 Interface `Renderer`

```rust
pub trait Renderer: Send {
    fn rendre(&mut self, ctx: &mut ContexteRendu, calque: &Calque);
    fn rects_sales(&self) -> &[Rect];
    fn detruire(&mut self);
}
```

| Type de widget / calque | Renderer |
|---|---|
| `media` | média (fit/zoom/rot/offset/miroir/morph/carrousel/PlayerBar) |
| `chat` | texte riche + images (badges, avatars, emotes) |
| `camera` | texture caméra + masque + anneau |
| `input-viewer` | vectoriel + texte (skins) |
| `speedrun` | texte + layout |
| `goal` | texte + barre |
| `musique` | texte + skin |
| `trou` | **aucun** (le concept disparaît) |
| fond | média/capture + titres |
| modules | horloge/objectifs (texte), équaliseur/spots (canvas GPU) |
| overlays | un renderer par overlay |

### 10.2 Texte

- DirectWrite : `IDWriteTextLayout` → rasterisation glyphe par glyphe →
  atlas (LRU, clé police + taille + style).
- Emojis : COLR/CPAL via DirectWrite ; fallback atlas Twemoji embarqué.
- Emotes BTTV/7TV/FFZ : images déjà résolues par le domaine (`EmoteSeg.url`)
  → textures, comme avatars et badges.
- Interdits : rendu par frame dans une texture plein écran, allocation par
  message.

### 10.3 Vectoriel

- `lyon` : tessellation CPU des paths (une fois par forme), gradients en
  shader.
- Timeline d'animation : sous-ensemble SMIL (`animateMotion` sur `path`,
  `animateTransform`, `animate` opacité), horloge propre, boucle propre.

### 10.4 Média

- MF Source Reader (hardware) → texture ; fallback FFmpeg logiciel → upload.
- PlayerBar, pause/seek, boucle, carrousel (fondu borné à la moitié de
  l'intervalle) : mêmes contrats que `carrousel.md`.
- Morphs : shader bulge/pinch, mêmes paramètres normalisés (0-1).

### 10.5 Chat — ce qui change, ce qui ne change pas (D4)

| Aspect | Sort |
|---|---|
| **Fonctionnel** — 4 plateformes (Twitch IRC, YouTube polling, Kick Pusher, TikTok Webcast), emotes, badges, avatars, filtres, viewers, commandes `!cmd`, `!cadres` | **Aucun changement.** Aucun lot du moteur ne touche `twitch_chat.rs`, `kick.rs`, `youtube_chat.rs`, `tiktok_chat.rs`, `twitch_emotes.rs` |
| **Représentation** — le rendu à l'écran | Seul point concerné : DOM Chromium en mode OBS, **texte rasterisé en natif**. Le résultat visuel doit être identique (couvert par le harnais) |

> La question du chat n'est donc **pas une question produit** : c'est un choix
> d'implémentation du renderer.

---

## 11. Encodeurs

### 11.1 Détection

1. `EnumAdapters1` → adaptateurs (`VendorId`).
2. `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE |
   MFT_ENUM_FLAG_SORTANDFILTER, …)` par couple (NV12 → H264/HEVC/AV1).
3. `MF_SA_D3D11_AWARE` vérifié → sinon copie CPU (secours seulement).
4. Choix : hardware D3D11-aware sur l'adaptateur de capture → hardware autre
   adaptateur → logiciel Microsoft.
5. Persistance dans `moteur.json` + exposition au diagnostic.

### 11.2 MFT

- `IMFTransform` + `MFCreateDXGISurfaceBuffer` sur une texture **NV12** →
  zéro copie.
- Conversion BGRA → NV12 en GPU (shader planaire ou `ID3D11VideoProcessor`).
- Attributs : `MF_MT_AVG_BITRATE`, `MF_MT_FRAME_SIZE`, `MF_MT_FRAME_RATE`,
  `MF_MT_INTERLACE_MODE = Progressive`, `MF_LOW_LATENCY = TRUE`,
  `CODECAPI_AVEncCommonRateControlMode = CBR` (ou VBR selon preset),
  `CODECAPI_AVEncMPVGOPSize` (GOP), `CODECAPI_AVEncCommonQuality`.
- Relire la configuration **effective** après création (les pilotes AMD
  anciens ignorent certains réglages) et l'afficher dans le diagnostic.

### 11.3 Politique

- **Un seul encodeur** par sortie. L'aperçu ne crée jamais de second encodeur.
- Pas de B-frames en mode faible latence ; IDR à la demande (reconnexion).
- Échec d'initialisation → repli immédiat dans l'ordre de la détection,
  journalisé et affiché (`encodeur_repli`).

---

## 12. Sorties

### 12.1 `FileSink`

- **fMP4** par défaut : `moov` vide + fragments, lisible après coupure.
- Écriture tamponnée + `sync_data` périodique (5 s) — leçon boîte noire.
- Disque plein / erreur I/O → sink retiré, état `Erreur`, les autres sinks
  continuent.

### 12.2 Sidecar FFmpeg

- Binaire existant : `src-tauri/binaries/ffmpeg-<triple>.exe`
  (`externalBin`, permission `shell:allow-execute` déjà posée).
- Protocole : bitstream sur **stdin** (`-f h264|hevc|av1 -i pipe:0`), audio
  PCM sur un **named pipe** (pattern `audioNamedPipe`), sortie RTMP/SRT.
- Args minimaux, **jamais de clé de flux dans les logs**.
- `stderr` parsé pour l'état (connexion, erreurs) ; reconnexion avec backoff.
- **Aucune frame brute dans le pipe** : uniquement le bitstream.

---

## 13. Audio

- Loopback WASAPI (sortie par défaut) + micro + **loopback par processus**
  (Windows ≥ 10 2004, `ActivateAudioInterfaceAsync` +
  `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`).
- Mixage : gains par source, mute, limiteur simple, resampling vers 48 kHz
  stéréo.
- Horodatage par paquet ; offset A/V explicite (réglage + diagnostic).
- L'équaliseur (`equaliseur.rs`) réutilise la même capture : **une seule**
  session WASAPI partagée, pas deux.
- Le routage audio OBS de l'écran d'attente (mute des items `desktop|bureau`)
  est remplacé par un **mute de source** dans le mixeur natif.

---

## 14. Gouverneur

**Entrées :** GPU busy (PDH), profondeur de file d'encodage, drops,
`QueryPerformanceCounter` vs budget frame.
**Sorties (ordre strict) :**

1. FPS de capture des sources non critiques : 60 → 30 → 15.
2. Résolution de composition : 4K → 1440p → 1080p.
3. Preset encodeur : qualité → équilibré → vitesse.
4. Résolution d'encodage (dernier recours).

**Règles :** hystérésis (pas d'oscillation), raison **toujours** exposée
(`MoteurDiag.degradation`), remontée progressive quand la marge revient,
jamais de drop silencieux.

---

## 15. Support matériel

- **Détection d'encodeur** au démarrage de sortie : énumération des MFT
  (`MFT_ENUM_FLAG_HARDWARE`), choix par constructeur, repli **logiciel**
  obligatoire — jamais d'échec silencieux faute d'encodeur.
- **Matrice de capacités** à détecter au démarrage (plan §6.1) et **matrice de
  test** à remplir par l'utilisateur, une ligne par machine (§6.4).
- Pièges matériels documentés dans le code (§6.3) : ne pas les redécouvrir.
- Vendor IDs attendus : `0x10DE` NVIDIA, `0x1002` AMD, `0x8086` Intel,
  `0x1414` Microsoft (logiciel / WARP).

---

## 16. Diagnostic « Moteur » — onglet de `ConsoModal.svelte`

> Exigence utilisateur (2026-10-27) : dans la section Diagnostic existante,
> afficher **clairement, pour un non-expert**, ce que le moteur consomme —
> et **tourne-t-on avec le pilote graphique, lequel, en matériel ou en
> logiciel ?**

### 16.1 Principe

- **Aucune valeur fabriquée.** Mesure échouée → `N/A`, **jamais `0`**.
- **Une phrase de verdict** en tête, pas seulement des chiffres.
- **Zéro polling** hors modale ouverte (invariant `conso.rs`).
- **100 % local**, conforme au CONTRAT FIGÉ vie privée.
- Le diagnostic fonctionne **même en mode OBS** : l'énumération d'adaptateur
  est sans effet de bord, l'utilisateur doit toujours voir son matériel.
- **Découplage strict** : le moteur écrit des atomics, le diagnostic lit. Le
  moteur ne dépend **jamais** du diagnostic.

### 16.2 Ce qui existe déjà (à réutiliser, pas à réécrire)

| Élément | Fichier |
|---|---|
| Échantillonneur 1 Hz + historique 300 pts | `conso.rs` |
| CPU/RAM par processus | `conso.rs` |
| CPU par sous-système (registre `nommer_thread`) | `conso.rs` — y ajouter les threads du moteur |
| GPU par PID via PDH `\GPU Engine(*)\Utilization Percentage` | `conso.rs` — étendre : agrégation par LUID |
| Boîte noire 5 s (`boite_noire.jsonl`) | `conso.rs` — y ajouter les champs moteur |
| Sparklines + lignes + onglets | `ConsoModal.svelte` |
| Polling borné à la modale | `stores/conso.ts` |

**Ce qui manque totalement** (vérifié : aucun `EnumAdapters`, `VendorId` ni
`LUID` dans tout le Rust) : identité de l'adaptateur, version de pilote,
version WDDM, identité de l'encodeur, mode de capture par source, compteurs
d'étages.

> **Détail PDH décisif** : les instances contiennent
> `pid_<PID>_luid_<LUID>_phys_<n>_eng_<n>_engtype_<type>`. On agrège donc
> **par PID** (StreamOS seul) *et* **par LUID** (charge de la carte entière)
> sans un seul appel supplémentaire → « GPU NVIDIA : 43 % » + « StreamOS :
> 12 % ».

### 16.3 Données ajoutées

| Donnée | Source | Coût |
|---|---|---|
| Adaptateur(s) : vendor, nom, VRAM, LUID | `IDXGIFactory1::EnumAdapters1` + `DXGI_ADAPTER_DESC1` | 1 fois au boot |
| Version de pilote | WMI `Win32_VideoController.DriverVersion` (one-shot) ou registre `…\Class\{4d36e968-…}\000N\DriverVersion` | 1 fois au boot |
| Version WDDM | registre `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\WddmVersion` | 1 fois, non fatal |
| Encodeur : nom MFT, matériel/logiciel, codec, preset | détection d'encodeur | au démarrage de sortie |
| Mode de capture par source (WGC/DXGI/MF) | registre de sessions | continu |
| FPS capture par source, FPS composite | compteurs atomiques | continu |
| Drops capture/composite, file d'encodage | compteurs atomiques | continu |
| Débit réel (kbps), latence capture→encode | encodeur + horloge | continu |
| Résolution effective, raison de dégradation | gouverneur | continu |
| GPU total par adaptateur | PDH, agrégation par `luid_` | 1 Hz, modale ouverte |

### 16.4 Modèle

`crates/sos-moteur-api/src/diag.rs` (partagé, **sans tauri**) :
`InfosAdaptateur { vendor_id, vendor_nom, description, vram_mio,
driver_version, wddm, luid }`, `InfosEncodeur { nom, materiel, codec, preset }`,
`EtatSource { nom, mode_capture, fps, taille, drops, active }`,
`MoteurDiag { actif, adaptateur, encodeur, sources, fps_composite,
drops_composite, queue_encode, kbps, latence_ms, resolution, degradation }`.

Les champs non mesurables sont `Option` — `None` s'affiche `N/A`, jamais `0`.
Les compteurs chauds vivent dans `CompteursMoteur` (atomics) ; `conso.rs` les
lit **sans jamais bloquer** le moteur. Commande `moteur_diag() -> MoteurDiag`
(synchrone, instantanée) ; `ConsoEtat` gagne `moteur: Option<MoteurDiag>` pour
n'avoir **qu'un seul aller-retour par seconde**.

### 16.5 UI — ordre de lecture, du verdict vers le détail

1. **Bandeau verdict**, une phrase colorée par état :
   « Encodage matériel NVIDIA (NVENC) — GPU 14 %, CPU moteur 3 %. » /
   « Encodage logiciel Microsoft — CPU 62 % : réduisez la résolution ou le
   FPS. » / « Moteur natif inactif — vous diffusez via OBS. » / « Deux GPU
   détectés : capture Intel, encodage NVIDIA — copie inter-GPU (+8 % GPU). »
2. **Carte « Pilote graphique »** : badge constructeur, nom de la carte,
   version de pilote, VRAM, WDDM, LUID (repliable).
3. **Carte « Encodage »** : nom du MFT, **matériel / logiciel**, codec, preset,
   débit, profondeur de file.
4. **Barres** : GPU total (par adaptateur), GPU StreamOS, CPU moteur, RAM
   moteur, file d'encodage. Seuils : < 70 % neutre, 70–90 % orange, > 90 %
   rouge ; **tout drop > 0 passe en orange**.
5. **Tableau « Sources »** : nom, mode de capture, FPS, taille, drops.
6. **Raison de dégradation** si le gouverneur est intervenu, valeur
   avant/après.
7. **Bouton « Copier le rapport »** — texte local pour le support, **aucune
   transmission**.

Contraintes UI : tokens `--t-*` uniquement (aucune couleur en dur), rem pour le
chrome, 8 locales (`conso.moteur.*`), libellés compris par un non-expert
(« Pilote graphique », « Encodage matériel », « Images perdues »).

### 16.6 Boîte noire — champs ajoutés

`boite_noire.jsonl` gagne, de façon **additive** (rétro-compatibilité des
lignes existantes) : `moteur_actif`, `encodeur`, `materiel`, `fps_composite`,
`drops`, `gpu_moteur_pct`, `degradation`. C'est la seule trace qui survit à un
gel : elle doit dire **quel étage** a lâché, pas seulement que l'app a gelé.

### 16.7 Règles

1. Aucune valeur inventée ; `N/A` explicite.
2. Aucun polling hors modale.
3. Énumération d'adaptateur autorisée au boot et à la demande (coût
   négligeable, aucun effet de bord).
4. Le diagnostic n'est **jamais** nécessaire au fonctionnement du moteur.
5. Toute nouvelle mesure doit être documentée dans
   `docs/features/diagnostic.md`.

---

## 17. Configuration `moteur.json`

Chemin : `%APPDATA%\com.streamos.v0\moteur.json`.

```json
{
  "mode": "obs",
  "encodeur_choisi": "NVIDIA H.264 Encoder MFT",
  "adaptateur_luid": 123456,
  "codec": "h264",
  "debit_kbps": 6000,
  "preset": "equilibre",
  "fps": 60,
  "canvas": [1920, 1080],
  "audio": { "loopback": true, "micro": true, "offset_ms": 0 }
}
```

- Lecture non fatale : fichier absent/cassé → défauts (`mode: "obs"`).
- Écriture atomique (tmp + rename), pattern `config.rs`.
- Toute clé inconnue est ignorée (rétro-compatibilité).

---

## 18. Erreurs

```rust
pub enum ErreurMoteur {
    Capture { source: String, cause: String },
    Gpu(String),
    Encodeur(String),
    Sortie(String),
    Audio(String),
    CameraOccupee,
    BasculeRefusee(String),
    Interne(String),
}
```

- Chaque variante a un message UI dédié (i18n `moteur.erreur.*`, 8 locales).
- Aucune erreur ne doit laisser un thread orphelin ni un fichier corrompu.
- Journalisation (`log::error`) **sans** clé de flux ni donnée utilisateur.

---

## 19. Invariants

1. Aucune frame brute ne traverse le CPU entre la capture et l'encodeur.
2. Aucun thread actif quand le moteur est inactif.
3. Aucun `.lock()` double dans une expression (piège Mutex non réentrant).
4. Aucun appel bloquant sur le thread UI.
5. File bornée partout ; politique de drop explicite (oldest, jamais block).
6. `appliquer_scene` idempotent et différentiel.
7. Toute dégradation est mesurée, exposée et affichée.
8. Le moteur ne connaît ni Tauri ni le domaine.

### ⚠️ Anti-patterns — NE JAMAIS RÉINTRODUIRE

1. ❌ Ajouter Chromium/WebView2 dans le chemin de sortie.
2. ❌ Copier une frame GPU→CPU « pour simplifier ».
3. ❌ Recréer les sessions de capture à chaque `commitScene`.
4. ❌ Ouvrir un second encodeur pour l'aperçu.
5. ❌ Rasteriser un cadre SVG à chaque frame en 4K.
6. ❌ Détruire `SOS-Caméra` (`RemoveInput`) au passage de mode.
7. ❌ Bloquer la bascule sur un stream actif sans confirmation.
8. ❌ Dupliquer la géométrie des cadres dans une 4ᵉ implémentation sans test de
   conformité.

---

## 20. Tests

| Niveau | Contenu | Gate |
|---|---|---|
| Unitaire | horloge, files, gouverneur, détection d'encodeur (mock), transforms | `cargo test -p sos-moteur-natif` |
| Conformité | 10 scènes de référence, horloge figée, SSIM ≥ 0,995 / diff ≤ 2/255 | obligatoire en P3+ |
| Charge | 1080p60 3 sources 30 min, 4K 10 min, drops = 0 | gate P2 |
| Robustesse | coupure disque, perte DXGI, caméra débranchée, pilote qui refuse | pas de crash |
| Matériel | matrice §6.4 du plan, une ligne par machine | gate P5 |

**Harnais de conformité :** `tests/conformance/` — scènes JSON versionnées,
rendu de référence `diffusion.html` (animations figées à t=0 et t=1 s),
comparateur SSIM + heatmap, sortie dans `target/conformance/<scene>/`.

---

## 21. Matrice de capacités OBS vs natif

| Fonction | Mode OBS (aujourd'hui) | Mode natif (v1) |
|---|---|---|
| Capture écran/fenêtre | source OBS | WGC/DXGI |
| Caméra | `SOS-Caméra` dshow | Media Foundation |
| Masque caméra | filtre PNG | stencil/SDF |
| Cadres SVG | `diffusion.html` | vectoriel (2 styles, refonte ensuite) |
| Chat | DOM | texte natif |
| Médias | `<video>` | MF Source Reader |
| Alertes / commandes / welcome / BRB / présentation / arène | DOM + CSS | renderers natifs |
| Stream / record | `StartStream`/`StartRecord` | encodeur MFT + sinks |
| Fond vert | plugin OBS | **non** (hors périmètre) |
| Aperçu | iframe `:4321` + projecteur OBS | texture partagée / aperçu natif |
| Just Chatting | dérivation de scène + OBS | layout compositeur |
| Diagnostic | CPU/RAM/GPU par PID | + pilote, encodeur, étages |

---

## 22. Lots et gates

> Convention : chaque lot = **liste blanche fermée**. Chaque fichier = un diff
> = une pause. Un lot n'est « fait » que si `cargo clippy` (exit 0),
> `npm run check` (0 erreur) et les tests du lot passent.

| Phase | Contenu | Gate |
|---|---|---|
| **P0** — Preuve | protocole + baseline OBS chiffrée, spike WGC → D3D11 → MFT → fichier | **2 cibles sur 3 tenues**, sinon STOP |
| **P1** — Socle | extraction `sos-scene`, traits, `MoteurObs`, machine de mode, UI de bascule | mode OBS **identique**, zéro changement observable |
| **P2** — Tuyau natif | capture WGC/DXGI, composition, horloge/gouverneur, MFT, `FileSink`, aperçu, enregistrement, **diagnostic (lots 2.9/2.10)** | 1080p60 3 sources 0 drop 30 min ; 4K documenté ; **diagnostic « Moteur » opérationnel et exact** |
| **P3** — Rendu | texte, vecteurs, médias, masques, tuyau cadres + 2 styles, titres, widgets, modules | harnais vert, SSIM ≥ 0,995 |
| **P4** — Overlays | alertes, commandes, welcome, bandeau, BRB, pad, fight, présentation, JC — un lot par overlay | test de conformité par overlay |
| **P5** — Différenciation | RTMP/SRT/Tee, audio par app, ROI, multi-format, reprise crash, backends SDK | matrice matérielle complète |

---

## 23. Fichiers — cible (⚠️ aucun n'existe à ce jour)

| Fichier | Rôle |
|---|---|
| `PLAN-MOTEUR-NATIF.md` | plan directeur (racine) |
| `crates/sos-scene/` | types purs serde, zéro logique d'app |
| `crates/sos-moteur-api/` | `Moteur`, `Sink`, `Capacites`, `RuntimeState`, `diag` |
| `crates/sos-moteur-natif/` | capture, gpu, compositor, render, encode, output, audio, clock |
| `src-tauri/src/moteur/mod.rs` | registre + machine à états + commandes Tauri |
| `src-tauri/src/moteur/obs.rs` | `MoteurObs` (délègue à `obs_trou`/`obs`) |
| `src-tauri/src/conso.rs` | à étendre : threads moteur, GPU par LUID, champs boîte noire |
| `src/lib/components/ConsoModal.svelte` | onglet « Moteur » |
| `src/lib/stores/{moteur,conso}.ts`, `src/lib/tauri.ts` | état + types miroir |
| `docs/features/moteur-natif/mesures-p0.md` | protocole + baseline + verdict go/no-go |
| `%APPDATA%\com.streamos.v0\moteur.json` | mode + encodeur choisi + LUID |

### 23.1 Points d'intégration dans l'app

**Commandes Tauri à ajouter** (aucune existante modifiée) :
`moteur_etat`, `moteur_basculer`, `moteur_diag`, `moteur_sortie_demarrer`,
`moteur_sortie_arreter`.
**Event :** `moteur:etat` (`{ mode, etat, capacites }`).
**Frontend :** `stores/moteur.ts`, bouton OBS / Rust dans la barre latérale,
onglet « Moteur » de `ConsoModal.svelte`.
**Cycle de vie :** les points d'appel de `scene_sync_captures` (boot,
`openScene`, duplication, preset, import ZIP, reconnexion OBS, JC ON/OFF,
hook post-commit) deviennent des **notifications au registre de moteurs** ;
le mode OBS garde exactement le comportement actuel.
**Inchangé :** `server.rs` (`:4321`, WS, médias, `/api`), `chat_tx`,
pop-outs, barre de raccourcis, télémétrie, API Stream Deck.

---

## 24. Hors périmètre v1

Fond vert virtuel, HDR, game capture par hooking, Linux/macOS, NDI,
multi-pistes audio, replay buffer, filtres (LUT, chroma key, denoise),
caméra virtuelle, plugins, SRT (phase P5), ROI (phase P5), multi-format
(phase P5).

---

## 25. Points ouverts (à trancher avant la phase concernée)

1. **Cadres — moteur de rendu** : `lyon` (procédural, 4K natif) ou `resvg`
   (fidélité immédiate, coût par frame) ? Proposition : `lyon`, le tuyau
   devant rester **agnostique du style**.
2. **Chat — cas rares** : shaping complet (RTL, ligatures) ou « équivalent
   visuel » assumé ? Proposition : shaping complet via DirectWrite, cas rares
   documentés.
3. **Tranche verticale P2/P3** : fond + 1 capture + caméra + chat + 1 cadre
   avant tout overlay ? Proposition : oui.
4. **Bascule pendant un live** : blocage ou confirmation ? Proposition :
   confirmation explicite + refus si stream natif actif.
5. **MSRV** : rester 1.77 ou monter si un SDK l'exige ? Proposition : rester
   1.77 tant que MFT + sidecar suffisent.

---

## 26. Références

- `PLAN-MOTEUR-NATIF.md` — phases, lots, mesures, matrice matérielle.
- `docs/features/moteur-natif/mesures-p0.md` — protocole P0 et verdict.
- `AGENTS.md` — protocole, contrats figés, pièges obs-websocket.
- `docs/features/camera-details.md`, `fond-capture-obs.md`,
  `selecteur-capture.md`, `obs-sorties.md`, `just-chatting.md`,
  `equaliseur.md`, `diagnostic.md`, `telemetrie.md`.

---

## 27. État mesuré et pièges vérifiés (2026-10-28)

> **Seule section avec des chiffres observés.** Le reste du fichier décrit des
> cibles. Crate : `src-tauri/crates/sos-moteur-natif` (Windows x64, MSRV 1.77,
> `windows 0.58` + `log`, aucune dépendance Tauri/app/domaine).

### 27.1 Modules implémentés

`gpu` (adaptateurs, `ContexteGpu`, `Format`, `DescTexture`, `PoolTextures`) ·
`capture` (énumération DXGI, `CaptureWgc` moniteur **ou** fenêtre) ·
`color_gpu` (BGRA→NV12 GPU, cible réutilisable) · `compositor` (`Calque`,
`Compositeur` : N flux → canvas NV12 en **un** `VideoProcessorBlt`) ·
`output` (`Sink`, `FileSink`, `FfmpegSink` fMP4/MKV, `TeeSink`) · `preview`
(NV12→BGRA réduit, 10 fps) · `clock` (échéances absolues) · `gouverneur`
(dégradation monotone + raison) · `compteurs` (atomics + `Instantane`) ·
`audio` (WASAPI loopback/micro en liaisons directes, `mixage::mixer`).
App : `moteur::mode` (machine obs/natif), `moteur_etat`/`moteur_diag`/
`moteur_basculer`, `moteur.json`.

### 27.2 Mesures P0-a — chemin zéro-copie (30 min, 1360×768@60)

| | OBS 32.2.2 | spike zéro-copie | Ratio |
|---|---|---|---|
| CPU moy (p95) | 0,75 % (1,25 %) | **0,05 % (0,20 %)** | 0,07× (0,16×) |
| RAM | 849,5 Mo | **59,6 Mo** | 0,07× |
| GPU (indicatif) | 49,2 % | 28,8 % | 0,59× |
| Frames | 0 drop | 14 en retard (0,013 %) | — |

108 000 frames en 1 799 987 ms (**60,0 fps**), 83,3 % de frames capturées,
décodage ffmpeg exit 0. L'ancien spike (readback BGRA + conversion CPU + pipe
ffmpeg) était à 2,07 % / 183,5 Mo. Détails : `mesures-p0.md`.

### 27.3 Limites constatées (ne pas promettre au-delà)

- **NVIDIA seulement** (SDK NVENC) ; AMD jamais testé, Intel échoue au MFT.
- Un moniteur par session (2 simultanées + 1 fenêtre vérifiées) ; pas de
  caméra, pas de hot-plug.
- Conteneur **vidéo seule** : le mixage audio existe mais n'est pas muxé.
- Timing du conteneur par `-framerate` (pas de PTS dans un H.264 élémentaire).
- Drops non mesurables côté spike (« en retard » seulement).
- Rien n'est branché à l'UI : le natif est refusé proprement par `moteur_basculer`.

### 27.4 Pièges vérifiés (ne pas réintroduire)

1. **`NVENC_EXTERNAL_ME_HINT_COUNTS_PER_BLOCKTYPE` = 16 octets** (mot de
   bitfields + `reserved1[3]`) : `maxMEHintCountsPerBlock[2]` = 32 o, donc
   **`tuningInfo` est à l'offset 136**. Le « calcul manuel » de 116 était
   impossible ; un test « +4 » glisse de +8 (alignement de `privData`).
   Symptôme : `nvEncInitializeEncoder` statut 8 « Presets P1-P7 … tuningInfo ».
2. **`RC_MODE_CBR = 0x2`** — `0x1` est **VBR** (enum 13.x : CONSTQP 0, VBR 1,
   CBR 2).
3. **`WAVEFORMATEXTENSIBLE` est `packed(1)`** : lire `SubFormat` via
   `addr_of!(...).read_unaligned()`.
4. **Le loopback WASAPI est post-volume** : muter la sortie coupe la capture
   (comme la « desktop audio » d'OBS). Niveau bas = volume système bas.
5. **WGC ne délivre des frames que sur changement d'écran** : compter les
   frames « sans mise à jour », ne jamais bloquer la boucle.
6. **`IMMDevice::Activate` n'a pas d'IID** (T inféré) ; `MMDeviceEnumerator`
   est une constante GUID, pas une classe.
7. **Video processor** : les flux de tailles source différentes passent dans un
   même `Blt` (5 flux max constatés sur la GTX 1070 Ti).

### 27.5 Vérification

```bash
cd src-tauri
cargo clippy -p sos-moteur-natif --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                                  # 51 tests
cargo clippy -p streamos-v0                                     # exit 0
cargo test -p streamos-v0 moteur::mode                          # 4 tests
```

Spikes : `nvenc_encode`, `capture_multi`, `compose_encode`
(`[h264|mp4|mkv] [couper|exit]`), `audio_capture`.

### 27.6 Lots faits / restants

**Faits** : 0.2g-β, 0.2g-γ, P0-a zéro-copie, 2.1, 2.2, 2.3, 2.4, 2.6, 2.7,
audio WASAPI, mixage, compteurs, app 1.4 (registre de mode), **1.1**
(`sos-scene`), **1.2** (`sos-moteur-api`), **1.3** (`MoteurObs`), **1.5** (UI
OBS/Rust), **1.6** (orchestrateur `MoteurNatif`), **1.6b** (backend NVENC).
**Restants** : 2.8 (bouton d'enregistrement natif), 2.9/2.10 (diagnostic
« Moteur »), P0-b, caméra, hot-plug, RTMP/SRT, audio muxé, **P3 renderer**
(texte/chat/cadres/médias) — prérequis produit de 2.8.

---

## 28. État mesuré du 2026-10-28 — orchestrateur et encodeurs

### 28.1 Ce qui tourne

- `MoteurNatif` (`crates/sos-moteur-natif/src/moteur.rs`) implémente
  `sos_moteur_api::Moteur` : N sources WGC (moniteur ou fenêtre, rect dans le
  canvas) → compositeur NV12 → encodeur → sinks fichier, horloge, gouverneur,
  compteurs, aperçu. Thread dédié, arrêt idempotent, `arreter()` ne coupe
  jamais un live.
- **NVENC direct** prioritaire (`encode::nvenc`, promu du spike P0-a) :
  texture NV12 du compositeur enregistrée **une fois**, zéro-copie.
- Repli **MFT** (matériels puis logiciels) si NVENC absent ou en échec.
  Variables de diagnostic : `SOS_MOTEUR_SANS_NVENC`,
  `SOS_MOTEUR_ENCODEUR_LOGICIEL`, `SOS_MOTEUR_MFT_MEMOIRE`.
- `MoteurNatif::encodeur()` expose `{ nom, materiel, zero_copie }` — le
  diagnostic ne doit jamais afficher « matériel » pour du logiciel.

### 28.2 Mesures (GTX 1070 Ti, 1360×768@60, 10 s, harnais `moteur_natif`)

| Voie | Débit | Latence | Décodage ffmpeg |
|---|---|---|---|
| NVENC direct | 5975 kb/s (cible 6000) | **1–2 ms** | exit 0, aucun avertissement |
| MFT logiciel | 300–4500 kb/s | 230 ms | exit 0, DTS non monotone |

### 28.3 Défauts vérifiés — ne pas les redécouvrir

1. **MFT NVIDIA** : `ProcessOutput` rend `E_FAIL` (0x8000FFFF) sur cette
   machine, quel que soit le chemin (surface GPU, mémoire, sans gestionnaire
   D3D11). Constat reproductible par `cargo run --example mft_diag`. Le MFT
   Intel refuse `SetD3DManager` avec le device NVIDIA.
2. **Readback NV12** : `Map` de la sous-ressource 1 renvoie `E_INVALIDARG` ;
   NV12 est **une** sous-ressource, la UV se lit à
   `pData + RowPitch × hauteur` dans le même mapping.
3. **B-frames** : elles produisent un DTS non monotone au remux d'un flux
   élémentaire. `CODECAPI_AVEncMPVDefaultBPictureCount = 0` exigé.
4. **Spike NVENC** : forçait `PIC_TYPE_IDR` à chaque frame. Corrigé : IDR à
   la première frame et chaque GOP, P sinon.
5. **WGC** ne délivre que sur changement d'écran : `composites` dépasse
   `capturees` sans que ce soit une perte. Ne jamais compter cela comme drop.

### 28.4 Blocage produit assumé

Le rendu natif (texte, chat, alertes, cadres, titres, médias) n'existe pas
encore (phases P3/P4). L'orchestrateur enregistre **des captures**, pas la
scène StreamOS. Les lots 2.8/2.9 sont techniquement débloqués (moteur réel,
diagnostic réel) mais un bouton « enregistrer le stream » mentirait tant que
P3 n'est pas livré.

Le pont scène → sources existe côté moteur :
`scene_sources::sources_depuis_scene(&Scene, &HashMap<widget_id, CibleCapture>)`
et `MoteurNatif::configurer_scene`. Seuls les widgets `trou` avec une cible
connue et un rect non nul produisent une source. La cible est **persistée
dans la scène** (`Widget.captureCible`, optionnel, `skip_serializing_if`) :
`{ "kind": "moniteur", "name": "\\\\.\\DISPLAY1", "index": 0 }` ou
`{ "kind": "fenetre", "hwnd": 42, "title": "…", "exe": "…" }`. Le JSON des
scènes existantes est **inchangé** (champ absent) et le mode OBS l'ignore.
Le sélecteur de capture remplit ce champ ; `setWidgetTrou(false)` le purge.
Reste à faire : P3 renderer (texte/chat/alertes/cadres/médias) avant qu'un
enregistrement « de scène » soit complet.


---

## 29. P3 — tranche 1 : médias statiques (2026-10-28)

### 29.1 Livré

- `media.rs` : décodage **WIC** (PNG/JPG/BMP) → texture BGRA, téléversée une
  fois (`D3D11_USAGE_DEFAULT`, bind render target, compatible video
  processor). COM MTA initialisé sur le fil moteur.
- `scene_sources::medias_depuis_scene(scene, base)` : fond
  (`bgSlides` prioritaire, sinon `bgMedia`, ignoré si `bgObsSource`) +
  widgets média (hors trou/caméra), avec leur `z`. `base` = dossier de
  données, les chemins sont relatifs (`medias/<uuid>.<ext>`).
- `MoteurNatif::configurer_medias` / `configurer_scene(scene, cibles, base)`.
- Composition : médias et captures triés par `z` dans un plan unique ;
  `SourceCfg.z` ajouté. Une scène **sans capture** (fond seul) démarre.
- Harnais `examples/media_scene.rs`.

### 29.2 Validation (GTX 1070 Ti, 1360×768@60, 3 s)

| Contrôle | Résultat |
|---|---|
| Décodage ffmpeg du MP4 | exit 0 |
| Frame 60, `signalstats` | YAVG **126,2** / YMIN 22 / YMAX 211 — image visible |
| Encodeur | NVENC H.264, zéro-copie |

### 29.3 Limites de la tranche (assumées)

- **Fit** : `etirer` / `ajuster` (letterbox centré) / `remplir` (crop centré)
  sont appliqués (`mediaFit`, `bgFit`). Rotation, zoom, miroir et effets ne
  sont pas encore appliqués.
- **Vidéos** : MP4/MOV/AVI/MKV/WEBM via Media Foundation Source Reader
  (RGB32, téléversement CPU par frame, cadencé par l'horloge). Pas de
  bouclage (dernière image figée), audio non lu. `mediaSlides` : seule la
  première slide est chargée.
- **Texte** : aucun rendu (DirectWrite à venir) — donc pas de titres, chat,
  alertes ni cadres.
- Le média est chargé **une fois** au démarrage : pas de rechargement à chaud.

### 29.4 Fit réel (ajout 2026-10-28)

`media::Fit` + `geometrie(taille, rect, canvas, fit)` : `etirer` garde tout
et le rect ; `ajuster` réduit la destination au ratio du média (centrée,
arrondi pour éviter 1 px d'asymétrie) ; `remplir` garde le rect et croppe la
source au centre. `mediaFit` des widgets et `bgFit` du fond sont transmis.

Validation : carré 400×400 dans un canvas 1360×768 avec `ajuster` → bande
gauche `YAVG = 16` (noir), centre `YAVG = 125` (image). Décodage exit 0.

### 29.5 Vidéos (ajout 2026-10-28)

`media::est_video` (extensions) + `charger_video` : Source Reader MF, sortie
RGB32, texture BGRA réutilisée et mise à jour par `UpdateSubresource`.
`MediaChargee::avancer(temps_us)` lit les échantillons jusqu'à l'horodatage
courant ; `boucle` l'appelle avant chaque composition. Fin de flux = dernière
image figée (pas de bouclage). Audio vidéo non lu.

Validation : clip `testsrc2` 2 s / 30 fps, 3 s d'enregistrement NVENC ;
frames 10 et 70 extraites → **md5 différents** (la vidéo avance), décodage
ffmpeg exit 0.

⚠️ Cette validation ne vérifiait que « ça avance », pas **à quelle
cadence** : la vidéo était jouée à la fréquence du moteur. Corrigé le
2026-10-28, voir §34.

### 29.6 Suite P3

1. Vidéos (Source Reader) + fit réel (`ajuster`/`remplir`).
2. Texte DirectWrite → texture (titres, chat).
3. Cadres SVG (tuyau de rendu vectoriel, 2 styles représentatifs).
4. Overlays (alertes, commandes, welcome, BRB, pad, fight, présentation).

---

## 30. P3 — tranche 2 : texte (2026-10-28)

### 30.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Blending | **Voie A** : canvas **BGRA** + Direct2D, puis conversion NV12 | `VideoProcessorBlt` n'a pas d'alpha par pixel (alpha constant par flux) → un texte passé en calque ressort opaque sur un rectangle noir |
| Polices | **WOFF2 décodés en mémoire** (`wuff`) → `IDWriteInMemoryFontFileLoader` | les 20 `.woff2` sont déjà versionnés (`src-tauri/resources/fonts/`), aucun téléchargement, aucun fichier écrit, fidélité exacte au dashboard |
| Contour | **passes décalées** (16 directions) | approximation assumée de `-webkit-text-stroke` ; le trait exact (`GetGlyphRunOutline`) est la suite |
| Périmètre | titres de **fond** + titres de **widget** | 4 positions chacun |

### 30.2 Livré

- `texte.rs` : `TexteCfg` / `StyleTexte` / `PositionTitre` / `CibleTitre`,
  `placement()` (géométrie de `applyTitreFond` / `applyTitreWidget`),
  `DessinateurTexte` (Direct2D + DirectWrite).
- `compositor::Compositeur::nouveau_sur(contexte, texture, fps)` : compose
  vers une texture **existante** (canvas BGRA). `nouveau()` inchangé.
- `scene_sources::textes_depuis_scene(scene)` : 4 titres de fond
  (`bgTitreHaut/Bas/Gauche/Droite`, `z = i32::MIN + 1`) + titres de chaque
  widget (`titreHaut/Bas/Gauche/Droite`, `z = w.z`, **hors** du rect).
- `moteur.rs` : `configurer_textes`, `configurer_scene` remplit aussi les
  textes ; `VoieTexte` (canvas BGRA + `ConvertisseurNv12` + dessinateur).
  Sans texte, le chemin NV12 direct reste **inchangé** (aucune passe ajoutée).
- Harnais `examples/texte_scene.rs` et `examples/passe_nv12.rs` (mesure).

### 30.3 Mesures (GTX 1070 Ti)

| Passe | 1360×768 | 3840×2160 |
|---|---|---|
| Composition 2 calques → NV12 (référence) | 0,178 ms | 0,316 ms |
| Conversion BGRA → NV12 (passe ajoutée) | **0,145 ms** | **0,468 ms** |
| Voie A complète | 0,269 ms | 1,065 ms |

Budget 16,7 ms @60 fps : la passe ajoutée reste marginale (≤ 3 % en 4K).

### 30.4 Validation (3 s, 1360×768@60, NVENC zéro-copie)

| Contrôle | Résultat |
|---|---|
| Décodage ffmpeg du MP4 | exit 0 |
| Bande du titre haut, frame 40 | YAVG **16,5** / YMAX **249** |
| Bande vide (témoin) | YAVG 0 / YMAX 0 |
| Bande du titre bas | YAVG 20,2 / YMAX 251 |
| Rendu | 4 positions (dont latérales tournées 90°), dégradé 135°, contour, titre de widget |

### 30.5 Limites de la tranche (assumées)

- **Contour approximé** par dilation (16 passes) : plus épais qu'un trait
  centré, les fûts fins se referment à petite taille. Suite : géométrie de
  glyphes (`IDWriteFontFace::GetGlyphRunOutline`).
- **Tous les titres sont peints après la composition** : un titre de widget
  passe donc au-dessus de calques de `z` supérieur. Le rendu CSS les imbrique
  dans le widget ; le correctif (découpe de la composition en deux passes
  autour du texte) n'est pas fait.
- **`line-height`** : DWrite utilise son interligne natif, le CSS force
  `1.1` → écart vertical de quelques pixels.
- Pas de `crop`/`rotation` du texte, pas d'emoji couleur, pas de retour à la
  ligne (les titres sont mono-ligne, comme `white-space: nowrap`).
- Le cache est indexé par (contenu, style) : un texte dynamique (chat) devra
  fournir sa propre invalidation.

### 30.6 Pièges vérifiés — ne pas les redécouvrir

1. **`RegisterFontFileLoader` obligatoire** : sans enregistrement du loader
   mémoire auprès de la factory, `CreateInMemoryFontFileReference` rend
   `E_INVALIDARG` (0x80070057), **sans message utile**. Vérifié 2026-10-28.
2. **`CreateBitmapFromDxgiSurface` + `D2D1_BITMAP_OPTIONS_TARGET`** exige une
   texture `D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE` ; avec
   `RENDER_TARGET` seul il faut `TARGET | CANNOT_DRAW`, sinon `E_INVALIDARG`.
   Le canvas (jamais échantillonné par D2D) utilise `TARGET|CANNOT_DRAW` ;
   les textures de texte (dessinées ensuite) ajoutent `SHADER_RESOURCE`.
3. **`Matrix3x2::rotation` attend des DEGRÉS** (`D2D1MakeRotateMatrix`) : avec
   `FRAC_PI_2` le titre latéral reste quasi horizontal (1,57°).
4. **`D3D11_QUERY_TIMESTAMP` ne voit pas les `VideoProcessorBlt`** (moteur
   vidéo distinct du pipeline 3D) : deux timestamps encadrant N blts rendent
   la **même valeur** (delta 0). Mesurer à l'horloge murale avec
   synchronisation (`examples/passe_nv12.rs`).
5. **D2D et `ID2D1RenderTarget`** : `ID2D1DeviceContext` ne `Deref` pas vers
   `ID2D1RenderTarget` dans `windows` 0.58 → cast explicite pour
   `BeginDraw`/`DrawBitmap`/`SetTransform`/`CreateSolidColorBrush`.
6. `Clear` sur une cible **premultipliée** avec `(0,0,0,0)` = transparent ;
   ne jamais effacer le canvas (il porte les calques composés).


---

## 31. Audio dans la sortie (2026-10-28)

### 31.1 Décisions

| Sujet | Décision | Motif |
|---|---|---|
| Muxage | **deux temps** : PCM capturé pendant l'enregistrement, FFmpeg muxe à l'arrêt | une 2ᵉ entrée FFmpeg en direct exige un pipe nommé Windows ; le live (RTMP) est déjà refusé et demandera un muxeur portant les PTS |
| Sources | **loopback + micro** | un streamer a besoin des deux |
| Gains | **millièmes entiers** (1000 = 1.0) dans `AudioCfg` | `SortieCfg` dérive `Eq` — un `f32` casserait le contrat partagé |
| Resampling / mixage | **faits par FFmpeg** (`volume` + `amix`) | zéro code de rééchantillonnage côté Rust, une seule source de vérité |

### 31.2 Livré

- `sos-moteur-api` : `AudioCfg { loopback, micro, gain_loopback, gain_micro }`,
  `SortieCfg.audio: Option<AudioCfg>`. `SortieCfg::fichier()` a du son par
  défaut ; les harnais vidéo passent `None`. Gains bornés à 4000 ‰ dans
  `valider()`.
- `audio::piste` : **un thread par source** (WASAPI → PCM `s16le` dans
  `%TEMP%`). `CapturePiste::attendre_demarrage` / `arreter() -> Piste`
  (chemin, taux, canaux, gain, durée).
- `output::ffmpeg::muxer_audio(chemin, pistes)` : `-c:v copy -c:a aac 192k`,
  filtre `volume` par source → `amix` → `apad`, `-shortest`. Bascule
  atomique (temporaire → sauvegarde → remplacement → suppression), avec
  restauration si la bascule échoue.
- `moteur.rs` : démarrage des captures avant la boucle, arrêt + muxage après
  fermeture des sinks, suppression des PCM temporaires dans tous les cas.
- Harnais `examples/audio_scene.rs` (journal `log` minimal intégré).

### 31.3 Alignement A/V — piège corrigé

Première mesure : **2,83 s d'audio pour 3 s de vidéo**. Deux causes :

1. le buffer WASAPI partagé fait **200 ms** — le premier paquet n'arrive
   qu'une fois rempli, donc le son démarrait 170 ms après la première frame ;
2. `-shortest` sans `apad` **tronquait le fichier** à la longueur de l'audio.

Correctifs : la boucle vidéo **ne démarre qu'après le premier paquet audio**
(`attendre_demarrage`, 400 ms max), et `apad` complète une piste courte en
silence avant `-shortest`. Mesure après correctif (3 s demandées) :

| Flux | Durée |
|---|---|
| Vidéo | 2,81 s |
| Loopback | 2,80 s |
| Micro | 2,77 s |

L'écart résiduel (< 40 ms) est la granularité d'un paquet. Le démarrage est
retardé d'environ 200 ms (le temps de remplir le buffer) : c'est le prix de
l'alignement, pas une régression.

### 31.4 Validation (GTX 1070 Ti, 1360×768@60, 3 s)

| Contrôle | Résultat |
|---|---|
| Flux du MP4 | **2** : H.264 1360×768 + **AAC 48 kHz stéréo** |
| Décodage ffmpeg complet | exit 0 |
| Gain 1000 ‰ (micro, bruit de pièce) | mean **−59,6 dB**, max −46,5 dB |
| Gain 0 ‰ | mean **−91,0 dB** (silence numérique) → le filtre `volume` est bien appliqué |
| PCM temporaires | supprimés après muxage |

### 31.5 Limites (assumées)

- **Fichier uniquement** : pas de muxage en direct (voir 31.1). RTMP/SRT
  refusés comme avant.
- **Loopback de l'appareil par défaut** seulement — pas de loopback par
  processus (`Capacites.audio_par_app` reste `false`, phase P5).
- Pas de mute/solo par source dans l'UI (gains fixes par `SortieCfg`).
- Mode partagé WASAPI uniquement (pas d'exclusif), format du mixage système.
- **Dérive A/V longue non mesurée** : l'alignement est fait au démarrage ;
  une dérive sur 30 min n'est pas instrumentée (l'horloge audio et l'horloge
  vidéo sont indépendantes).
- `Capacites.audio` existe depuis le lot 1.4 (voir §32) : l'UI affiche
  « Audio (loopback + micro) » quand le moteur actif le déclare.

### 31.6 Pièges vérifiés

1. **`Capteur` n'est pas `Send`** (`IUnknown` → `NonNull<c_void>` dans
   `windows-core` 0.58) : l'ouverture WASAPI doit rester **dans** le thread
   audio. Ne pas ajouter d'`unsafe impl Send` sur un objet d'appartement —
   l'alignement se fait par `attendre_demarrage`, pas en déplaçant le capteur.
2. **`-shortest` sans `apad` coupe la vidéo** à la durée de l'audio.
3. Le buffer WASAPI partagé (200 ms) décale le son si la vidéo démarre avant
   le premier paquet.
4. Les harnais n'installent pas de logger : les traces `log::info!` du moteur
   sont invisibles (`examples/audio_scene.rs` embarque un journal minimal).

---

## 32. Intégration app — lot 1.4 (2026-10-28)

Le moteur natif est **branché dans l'application** : le crate est une
dépendance de `src-tauri`, le registre porte les deux moteurs, et une sortie
native se pilote depuis la barre latérale. Le mode OBS est inchangé
(`obs.rs`, `obs_trou.rs`, `diffusion.html` non modifiés).

### 32.1 Décisions tranchées

| Question | Décision | Conséquence |
|---|---|---|
| Destination du fichier | **Dialogue « Enregistrer sous »** | Défaut proposé : `<app_data>/enregistrements/StreamOS-<horodatage>.mp4` (tout local) ; l'utilisateur choisit. `normaliser_chemin` ajoute `.mp4` et crée le dossier parent. |
| Bouton | **Le bouton « Enregistrer » existant route selon le mode** | En mode `natif`, il ouvre le dialogue puis démarre/arrête la sortie native ; en mode `obs`, il ouvre la confirmation OBS comme avant. |
| Paramètres | **Figés, affichés en lecture seule** | 60 fps, H.264, 6000 kb/s, preset `equilibre`, `AudioCfg::default()` (loopback + micro, gains 1000 ‰). |
| `Capacites.audio` | **Ajouté** | Contrat `sos-moteur-api`, miroir `moteur.ts`, id `audio` dans `capacites_actives`, clé `moteur.cap.audio` dans les 8 locales. |

### 32.2 Livré

- `src-tauri/Cargo.toml` : dépendance `sos-moteur-natif`.
- `moteur/mod.rs` : `RegistreMoteur { mode, natif_disponible: true, obs,
  natif: MoteurNatif, session }` ; `moteur_etat` renvoie les capacités du
  moteur **actif** ; `moteur_diag` renvoie encodeur/résolution/fps réels
  (`None` avant démarrage) ; trois commandes de sortie.
- `moteur/mode.rs` : `verifier_bascule(actuel, cible, live_actif,
  natif_actif)` — refus symétrique (`LiveEnCours` / `SortieNatifEnCours`).
- `lib.rs` : commandes enregistrées + teardown `RunEvent::Exit` →
  `arreter_natif()`.
- Frontend : `tauri.ts` (3 wrappers), `stores/moteur.ts`
  (`sortieNatif`, `sortieNatifBusy`, `sortieNatifFichier`,
  `demarrerEnregistrementNatif`, `arreterEnregistrementNatif`,
  `rafraichirSortieNatif`), `Toolbar.svelte` (routage + état réel).

### 32.3 Sémantique

- **Démarrer** (`moteur_sortie_demarrer(chemin)`) : exige le mode `natif` et
  aucune sortie en cours ; lit `ScenesState.scene`, `configurer_scene(&scene,
  &HashMap::new(), data_dir)` (les cibles viennent de `Widget.captureCible`
  persisté), puis `SortieCfg::fichier(chemin, Mp4)` avec `canvas =
  (scene.canvasW, scene.canvasH)`. Retourne le chemin.
- **État** (`moteur_sortie_etat`) : `{ actif, chemin, kbps, frames_perdues,
  encodeur, degradation, erreur }` — `chemin`/`encodeur` valent `null` hors
  enregistrement, `kbps` vaut 0 à l'arrêt (aucune valeur inventée).
  `erreur` = erreur **terminale** du moteur (`MoteurEtat::Erreur`), distincte
  de `degradation` (gouverneur) : une dégradation n'arrête pas la sortie.
- **Arrêt** (`moteur_sortie_arreter`) : `arreter()` (joint le fil, finalise
  le conteneur, muxe l'audio), retourne le chemin finalisé ou `null`.
- **Bascule** : refusée si `en_cours()` (symétrique de `LiveEnCours`).
- **Jamais de démarrage implicite** : aucun enregistrement au boot ni à la
  connexion OBS.
- L'UI suit l'état à 1 s **pendant** l'enregistrement seulement (aucun timer
  au repos).

### 32.4 Pièges vérifiés

1. **Commandes `async` + `spawn_blocking`** : `demarrer_sortie` initialise
   GPU + encodeur (~100–500 ms) et `arreter` attend le muxage audio (~1 s) —
   en synchrone, la fenêtre gèle. Les commandes ne prennent que `AppHandle` :
   les états (`RegistreMoteur`, `ScenesState`) sont relus **dans** le thread
   bloquant, ce qui évite tout `State` non `'static` capturé.
2. **Teardown obligatoire** : sans `RunEvent::Exit` → `arreter_natif()`, le
   fil moteur et NVENC survivent à la fermeture et le MP4 reste non finalisé.
3. **Scène sans média ni titre** : `demarrer_sortie` refuse
   (`aucune source, média ni texte configuré`). L'erreur remonte telle quelle
   dans le bloc moteur — pas de faux « enregistrement lancé ».
4. **`moteur_basculer` relit l'état après bascule** (`chargerEtatMoteur`) :
   sans cela l'UI garderait les capacités d'OBS en mode natif.
5. **Le natif ne rend pas la scène complète** : ni chat, ni cadres SVG, ni
   overlays, ni caméra, ni capture de jeu, ni RTMP/SRT. Une sortie native =
   captures (trous avec cible) + médias + titres + audio. L'UI doit le dire.
6. **Plafond matériel de calques — levé au §33 (2026-10-28).** Le
   compositeur compose en un seul `VideoProcessorBlt`, borné par
   `D3D11_VIDEO_PROCESSOR_CAPS.MaxInputStreams` (5 sur la GTX 1070 Ti).
   Avant le §33, une scène de 11 calques (fond + 10 médias) échouait avec
   `composition : trop de calques : 11 (maximum 5)` et le sidecar FFmpeg
   mourait sur `Output file does not contain any stream`. La composition
   multi-passes supprime la limite.
7. **Une erreur terminale doit être visible** : `moteur_sortie_etat.erreur`
   porte `MoteurEtat::Erreur`. Sans ce champ (première version du lot), un
   échec de composition laissait l'UI muette et seule la sortie stderr
   d'FFmpeg parlait — l'utilisateur croyait à un bouton inerte.

### 32.5 Vérification

```
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif      # 66
cargo test -p sos-scene             # 8
cargo test -p sos-moteur-api        # 12
cargo test -p streamos-v0           # 200 passed, 1 ignoré
cd .. && npm run check              # 0 erreur
npm run build                       # build production
```

---

## 33. Composition multi-passes — plafond `MaxInputStreams` levé (2026-10-28)

Le compositeur ne refuse plus les scènes riches. **Aucun réglage** n'a été
ajouté : c'est le chemin de composition qui change.

### 33.1 Décision

`D3D11_VIDEO_PROCESSOR_CAPS.MaxInputStreams` est un **plafond matériel**
(5 sur la GTX 1070 Ti) : un `VideoProcessorBlt` ne peut pas recevoir plus de
flux. Trois voies ont été écartées :

- **texture intermédiaire + fusion** : exigerait un mélange alpha que le
  processeur vidéo ne fait pas entre deux surfaces (il écrit, il ne compose
  pas) ;
- **lots avec fenêtre = union des destinations** : les trous de l'union
  seraient repeints en noir et effaceraient les passes précédentes ;
- **plafonner la scène** : refuse le produit, pas la technique.

Voie retenue : **un blt par calque au-delà du premier lot**, la fenêtre de
sortie étant restreinte à la destination du calque
(`VideoProcessorSetOutputTargetRect`). Le fond noir n'est peint que dans
cette fenêtre, que le calque recouvre entièrement (la destination est
exactement sa boîte) → les passes précédentes restent intactes.

### 33.2 Sémantique exacte

1. `calques.len() <= MaxInputStreams` → **un seul blt** (chemin historique,
   aucune régression : même nombre d'appels, même coût).
2. Au-delà : premier lot de `MaxInputStreams` calques en un blt (il écrit
   **toute** la surface — fond noir compris — donc aucun pixel résiduel de
   la frame précédente), puis un blt par calque restant, dans l'ordre z.
3. La fenêtre de sortie est un **état** du processeur : elle est relâchée
   (`enable=false`) à la fin de chaque frame multi-passes, sinon la frame
   suivante serait rognée.
4. Un calque dont la destination tombe entièrement hors canvas est ignoré
   (`fenetre_destination` → `None`), sans erreur.
5. `ErreurComposition::TropDeCalques` est **supprimée** : la variante ne
   pouvait plus se produire (une erreur inatteignable ment sur le contrat).

### 33.3 Mesures (GTX 1070 Ti, 1360×768@60, NVENC zéro-copie)

| Scène | Calques | blts/frame | fps effectif | drops | latence |
|---|---|---|---|---|---|
| scène de test | 4 | 1 | ~53 | 0 | — |
| scène réelle `af09059c` | 11 | 1 + 6 = 7 | ~56,6 | 0 | 39 ms |

Le coût des passes supplémentaires n'est pas mesurable ici : le facteur
limitant reste l'encodeur (~56 fps sur cette machine, y compris à 4 calques).

### 33.4 Validation

1. **Scène réelle à 11 calques** (fond + 10 médias) : enregistrement complet,
   `ffmpeg -f null -` → `exit 0`, 2 flux (h264 1360×768@60 + AAC 48 kHz
   stéréo), 4,73 s, 1,4 Mo.
2. **Preuve que les passes n'effacent rien** — scène synthétique de 7 calques
   de couleurs pures, positions disjointes, décodage d'une frame et mesure
   `signalstats` par région :

   | calque | passe | YAVG mesuré |
   |---|---|---|
   | rouge | lot 1 | 54 |
   | vert | lot 1 | 91 |
   | bleu | lot 1 | 18 |
   | jaune | passe 6 | 235 |
   | magenta | passe 7 | 72 |
   | cyan | passe 8 | 201 |
   | blanc | passe 9 | 255 |

   Les sept régions gardent leur luminance propre : si la fenêtre de sortie
   était ignorée, chaque passe aurait repeint tout le canvas en noir et seuls
   les derniers calques survivraient.
3. Tests unitaires ajoutés (`fenetre_destination`) : bornage au canvas,
   fenêtre intérieure intacte, calque hors canvas ou de surface nulle ignoré.

### 33.5 Limites (assumées)

- Une passe par calque au-delà du plafond : coût proportionnel au nombre de
  calques, pas de regroupement intelligent (une scène à 40 calques fera
  36 blts/frame). À optimiser si un usage réel le demande.
- Les calques restent **opaques** (alpha 1.0) : la composition ne fait pas de
  mélange alpha entre calques, comme avant.
- Le gain réel n'est mesurable que sur une machine dont l'encodeur n'est pas
  le facteur limitant.

---

## 34. Cadence des vidéos médias — piège corrigé (2026-10-28)

**Symptôme rapporté** : une vidéo média jouée dans un enregistrement natif
paraît **×2 ou ×3** trop rapide.

### 34.1 Cause

`MediaChargee::avancer(temps_us)` téléversait la texture **avant** de tester
l'horodatage :

```rust
loop {
    ReadSample(...);                 // une frame
    UpdateSubresource(...);          // affichée immédiatement
    if pts_us >= temps_us { return } // puis sortie
}
```

Chaque appel du moteur consommait donc exactement **une** frame, et le test
final ne pouvait jamais retarder quoi que ce soit (la première frame lue est
toujours celle qui vient d'être affichée). Le média était joué à la cadence
du moteur (60 Hz) au lieu de la sienne : ×2 pour une source 30 fps, ×2,5
pour du 24 fps.

### 34.2 Correctif — une frame d'avance

`VideoLecteur` porte `en_attente: Option<(IMFSample, i64)>` (le Source Reader
MF **ne sait pas remettre** un échantillon : on le garde). `avancer` :

1. si une frame est en attente et que `pts <= temps_us` → téléverser, boucler ;
2. si `pts > temps_us` → la remettre en attente et sortir (rien à afficher) ;
3. sinon lire l'échantillon suivant dans `en_attente`.

Fin de flux inchangée (dernière image figée, pas de bouclage). Coût : au plus
une période de la source de latence d'affichage (41,7 ms pour du 24 fps).

### 34.3 Mesures (vidéo compteur 24 fps, moteur 60 Hz, 4 s)

| instant du fichier | avant | après | attendu |
|---|---|---|---|
| t = 1 s | frame **61** (×2,54) | frame **20** | 24 |
| t = 3 s | ~180 | frame **68** | 72 |

Débit mesuré après correctif : (68 − 20) / 2 s = **24 fps**. L'écart constant
de 4 frames (0,17 s) est la latence de pipeline (file encodeur + arrondi du
seek), pas une dérive : l'écart ne grandit pas entre t = 1 et t = 3.

### 34.4 Pièges vérifiés

1. **`IMFSample` ne se rembobine pas** : tout correctif de cadence doit
   conserver l'échantillon lu trop tôt, pas le relire.
2. **Un test « ça avance » ne prouve pas la cadence** (§29.5) : il faut un
   compteur d'image lisible dans la source et une mesure à deux instants.
3. Les images (`video: None`) ne sont pas concernées : `avancer` sort
   immédiatement.

---

## 35. P3 — tranche 3 : cadres SVG (2026-10-28)

### 35.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Moteur vectoriel | **Direct2D** (écart assumé au plan §25.1 qui proposait `lyon`) | `lyon` a été décidé **avant** l'existence de la voie texte ; Direct2D + `VoieRendu` savent remplir/dessiner un path avec dégradé, jointures et masque géométrique, **sans dépendance ni shader**. Vérifié : `windows 0.58` expose déjà `CreatePathGeometry`, `ID2D1GeometrySink`, `PushLayer(geometricMask)`, `DrawBitmap(sourceRect)`. |
| Styles | **2** : `mgstyle` + `thin-line` | D3 du plan ; les 7 autres attendent la refonte du système de cadres. |
| Découpe du média | **Direct2D sur le canvas BGRA** (option a) | `VideoProcessorBlt` n'a **pas d'alpha par pixel** : une texture pré-découpée ressort en rectangle noir. Un masque PNG (option b) réintroduirait la dérive que le dépôt combat. |
| Ordre z avec cadres | **Composition par segments** : `Clear` noir, puis un `VideoProcessorBlt` **borné à la destination** de chaque calque, les dessins D2D s'intercalant dans l'ordre z | le premier lot de `Compositeur::composer` peint **tout** le canvas en noir : il effacerait les cadres et les médias découpés déjà dessinés. |
| Fidélité au parent | **Clip au polygone** (comme `overflow:hidden` + `clip-path`) | mesuré : le parent rogne l'overlay → la moitié externe du trait et **tous** les ornements de marge sont invisibles dans la référence. Les porter aurait produit du code sans effet visible. |

### 35.2 Livré

- `cadres.rs` : `StyleCadre` (2 styles), `CadreCfg`, `points_polygone` (miroir
  exact de `pointsClipPathCadre`), `path_cercle` / `points_cercle` (caméra,
  non rendue ici), `stops_liseres`, `angle_vers_coords`, `eclaircir` /
  `assombrir` / `couleur_hex`, `geometrie`, `dessiner`, `style_trait`.
- `scene_sources.rs` : `cadre_de_widget` (exclusions `sansCadre` / caméra /
  chat `bulles`), `cadres_depuis_scene` (bord `z = i32::MAX` + un par widget),
  `chat_depuis_scene` ; `MediaCfg.cadre` / `SourceCfg.cadre` remplis.
- `compositor/mod.rs` : `composer_borne` (une passe bornée par calque).
- `gpu.rs` : `creer_texture_echantillonnable` (`RENDER_TARGET | SHADER_RESOURCE`).
- `media.rs` : textures média échantillonnables + champ `cadre`.
- `texte.rs` : `debut` / `fin` / `effacer`, `geometrie_cadre`,
  `dessiner_cadre`, `bitmap_media`, `dessiner_media`.
- `moteur.rs` : plan `(z, genre, index)` à 4 genres (media < capture < chat <
  cadre), voie de rendu `VoieRendu`, `flush_run`.
- Harnais `examples/cadres_scene.rs`.

### 35.3 Le polygone — un seul, vérifié contre le TS

`points_polygone` reproduit **au bit près** (`1e-9`) ce que produit
`src/lib/cadres/points.ts` (exécuté via Node 26 pour la référence) :

| Cas | Points | Remarque |
|---|---|---|
| `mgstyle` 692,548×88,625 sw20 | 9 | coupe bornée à `H/4` = 22,156 |
| `mgstyle` 215,008×704,572 sw20 | 9 | coupe bornée à `W/4` = 53,752 |
| `thin-line` 100×40 sw4 | **54** | le `Z` rejoue le point de départ → doublon final **conservé** |
| `points_cercle` 129,176×88,552 | 50 | miroir de `pathCercle` |

Deux pièges d'arithmétique JS, traités et testés : `Math.round` arrondit les
demis vers **+∞** (pas hors de zéro), et `Math.abs(-2147483648)` reste positif
→ `unsigned_abs()` côté Rust.

### 35.4 Ce qui est réellement visible (et donc dessiné)

| Style | Dessiné | Non porté (invisible dans la référence) |
|---|---|---|
| `mgstyle` | 3 traits (`2ep` @18 %, dégradé `ep`, accent `0.25ep` @55 %) + 4 arcs d'angle | triangles de coin, brackets, circuits, nœuds, diode, « ! », ticks |
| `thin-line` | trait principal `ep` + fermoir (moitié basse) | filet externe, 4 accents L |

### 35.5 Mesures (GTX 1070 Ti, 1360×768@60, NVENC zéro-copie)

`cadres_scene` — 3 widgets (2 à cadre + 1 témoin `sansCadre`), fond noir,
mire `testsrc2` (YAVG 125) comme média :

| Contrôle | Résultat |
|---|---|
| Cadres configurés | **3** (bord + 2 widgets) |
| Encodage | 169 frames / 2,81 s ≈ **60 fps**, NVENC H.264 zéro-copie |
| `mgstyle` coin de w1 (coupé) | YAVG **0,49** / YMAX 41 |
| `mgstyle` centre de w1 | YAVG **235** |
| `mgstyle` coin de w2 | YAVG **0,01** |
| `thin-line` coin de w1 | YAVG **0,66** / YMAX 12 |
| `thin-line` centre | YAVG **235** |
| Témoin `sansCadre`, même coin | YAVG **54** (média, **pas** coupé) |
| Bord du canvas (bande 6 px) | YAVG 10,7, **U 153 / V 141** (chroma du violet #8c00ff) |

Le contraste coin/centre est la preuve de la découpe : le coin coupé est noir
alors que le centre porte la mire, et le témoin garde son coin plein.

Scène de référence `af09059c` (11 calques + 3 cadres de widget + bord + chat) :
**5 cadres**, 222 composites / 4 s (~55 fps), `ffmpeg -f null -` **exit 0,
2 flux** (h264 1360×768 + AAC 48 kHz stéréo), chat `YMAX 255`, bord du trou
`YAVG 2,75` (trait sur le fond).

### 35.6 Limites (assumées)

- **`animer` ignoré** : le cadre est rendu **statique**. `Capacites.cadres_animes`
  reste `false` — jamais un rendu faux silencieux.
- **7 styles non portés** : `arrondis`, `carre`, `cyberpunk`, `story`,
  `hexagon`, `arena`, `tribal` ne produisent **aucun** cadre (le moteur
  dégrade l'état au démarrage, il ne dessine pas un tracé faux).
- **Caméra exclue** : `widget_type == "camera"` → aucun cadre (forme dédiée
  `cameraForme` hors tranche).
- **Titres dessinés après le bord du canvas** : dans `diffusion.html`,
  `#cadre-app-overlay` (`z-index: 100`) passe au-dessus des `.bg-titre`. Écart
  de quelques pixels au bord uniquement.
- **Une passe par calque** dès qu'un cadre existe : 11 blts/frame sur la
  scène de référence (l'encodeur reste le facteur limitant, ~56 fps).
- **Captures à cadre** : le chemin existe (bitmap + polygone) mais n'a été
  mesuré sur aucune scène réelle (le trou de la référence n'a pas de cible).
- `thin-line` : le trait suit le **polygone aplati** (contrat « un seul
  polygone »), alors que le SVG d'origine trace un `rect rx` (arcs) — écart
  < 0,5 px, assumé.

### 35.7 Pièges vérifiés — ne pas les redécouvrir

1. **`ID2D1Factory1::CreatePathGeometry` rend un `ID2D1PathGeometry1`** (et
   `CreateStrokeStyle` un `…Style1` avec `…PROPERTIES1`) : cast explicite vers
   l'interface de base, sinon erreur de type à l'usage.
2. **`D2D1_LAYER_PARAMETERS1.geometricMask` est en `ManuallyDrop`** : sans
   `drop` explicite après `PopLayer`, une référence COM fuit **par frame**.
3. **`PushLayer` / `PopLayer` / `PushAxisAlignedClip` / `FillRectangle` /
   `DrawGeometry` rendent `()`** sur `ID2D1DeviceContext` / `ID2D1RenderTarget`
   (pas de `Result`) — ne pas chaîner `map_err`.
4. **Direct2D ne peut pas échantillonner une texture sans
   `D3D11_BIND_SHADER_RESOURCE`** (même piège que les cibles de texte, §30.6).
5. **Ne jamais laisser une passe Direct2D ouverte pendant un
   `VideoProcessorBlt`** : `fin()` avant le blt, `debut()` après.
6. **Le premier lot du compositeur peint tout le canvas en noir** : en
   composition segmentée, c'est `Clear` qui remet le fond à zéro, une seule
   fois par frame.

---

## 36. P3 — tranche 4 : chat (2026-10-28)

### 36.1 Décisions

| Sujet | Décision | Motif |
|---|---|---|
| Alimentation | **L'app s'abonne à `chat_tx`** et pousse dans le moteur | le crate reste **sans tokio** ; un seul abonné pour les 4 plateformes ; aucun accès réseau ajouté. |
| Contrat | `MoteurNatif::pousser_chat` **jamais refusé** pendant une sortie | les messages arrivent après le démarrage : aucune reconfiguration de scène, donc aucun `DejaActif`. |
| Buffer | **200**, drop-oldest | `MAX_CHAT` de `diffusion.html`. |
| Contenu | **Texte + émojis Unicode** | les emotes/badges/avatars en images exigent des téléchargements : décision produit séparée (CONTRAT FIGÉ vie privée). |
| Style | `bulles` = fond uniforme clair ; `compact` = zébrage noir/blanc | miroir du CSS ; **aucun cadre de widget** en `bulles` (comme `diffusion.html`), cadres par bulle hors tranche. |

### 36.2 Livré

- `chat.rs` : `MessageChat`, `FileChat` (buffer borné + `version`), `ChatCfg`,
  `teinte_pseudo` / `couleur_depuis_nom` (miroirs de `couleurDepuisNom`).
- `scene_sources::chat_depuis_scene` (rect, filtre, taille, style, z).
- `texte.rs` : `dessiner_chat` (DirectWrite, wrap, couleur par plage),
  `geometrie_chat` (métriques CSS), cache de blocs par version.
- `moteur.rs` : `configurer_chats`, `pousser_chat`, `chat_len`, buffer partagé.
- App : `moteur::demarrer_ecoute_chat` (abonnement `chat_tx`, filtre
  `{"type":"chat"}`), commande `moteur_chat_message`.
- Harnais `examples/chat_scene.rs`.

### 36.3 Mesures

`chat_scene` (widget 692×420, `bulles`, 18 px, 4 messages initiaux + 1/s) :

| Contrôle | Résultat |
|---|---|
| Encodage | 351 frames / 5,85 s, NVENC zéro-copie |
| Zone du widget à t = 0,5 s | YAVG **12,14**, YMAX **255** |
| Zone du widget à t = 5,5 s | YAVG **22,40**, YMAX **255** |
| md5 des deux frames | **différents** → les messages reçus **pendant** la sortie s'affichent sans redémarrage |
| `composites` | **0** (aucun calque vidéo : la frame n'est faite que de Direct2D) |

Scène de référence : zone chat `YMAX 255` sur un widget de 88 px de haut
(~3 bulles visibles, défilement automatique vers le bas).

### 36.4 Limites (assumées)

- **Emotes / badges / avatars en images non rendus** : seuls le pseudo coloré
  et le texte (émojis Unicode compris) le sont. Aucun téléchargement.
- **Pas d'horodatage affiché** (la référence n'en affiche pas) ; l'ordre est
  chronologique, l'insertion horodatée par la `version` du buffer.
- **Reconstruction complète du bloc à chaque message** : jusqu'à 200 mises en
  page DirectWrite par message. Mesuré non bloquant ici ; à rendre incrémental
  si un chat à forte cadence le demande.
- **Pas de défilement manuel** (widget de diffusion en lecture seule).
- **Cadres par bulle non portés** : en `bulles`, le widget n'a **aucun**
  cadre (miroir d'OBS).
- Le style `compact` suit le même chemin que `bulles` (zébrage) : les autres
  `chatStyle` se dégradent en bulles, comme annoncé.

### 36.5 Pièges vérifiés

1. **`SetDrawingEffect` attend un `Param<IUnknown>`** : caster la brosse
   (`brosse.cast::<IUnknown>()`) — `&ID2D1SolidColorBrush` ne se convertit pas
   implicitement dans cette signature.
2. **Le layout DirectWrite ne possède ni le format ni la brosse** : les deux
   sont conservés dans le bloc (`_format`, `_brosse`), sinon libération pendant
   le dessin.
3. **`chat_tx` transporte aussi les spectres de l'égaliseur** (plusieurs
   dizaines de frames/s) : filtrer sur le préfixe `{"type":"chat"` **avant**
   `serde_json`, sinon on désérialise tout le trafic de l'overlay.
4. **`FileChat::is_empty`** (et pas `est_vide`) : `clippy::len_without_is_empty`
   est une erreur dans ce dépôt (`-D warnings`).
5. **`r#"…"#` et une couleur `"#ff0000"`** : la séquence `"#` ferme la chaîne
   brute — utiliser `r##"…"##` dans les tests.

---

## 37. P3 — tranche 5 : parité de rendu, composition 100 % Direct2D (2026-10-28)

Le rendu natif est devenu **celui de `diffusion.html`** sur les formes et les
couleurs : alpha par pixel, découpe polygonale, voile et bordure de widget. Le
video processor ne compose plus rien — il ne fait que la conversion NV12.

### 37.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Composition | **100 % Direct2D** sur un canvas BGRA, une passe par frame | `VideoProcessorBlt` ne sait faire ni alpha par pixel, ni clip polygonnal, ni rotation ; deux chemins = deux jeux de bugs |
| VP | **conversion NV12 uniquement** (`ConvertisseurNv12`) | 0,145 ms en 1360×768, 0,468 ms en 4K (§30.3) |
| Alpha | décodage **WIC `32bppPBGRA`** (prémultiplié) | D2D prémultiplié attend PBGRA ; `32bppBGRA` = alpha droit |
| Bitmap média | `PREMULTIPLIED` si `MediaChargee.alpha`, `IGNORE` sinon | l'octet alpha d'un `RGB32` (MF) n'est pas garanti : jamais un média qui disparaît |
| **Trou** | **supprimé côté natif** | le trou n'existe que pour laisser voir une source OBS *sous* la browser source ; en natif la capture est le **contenu** du widget, à son `z` |
| Captures | **`Fit::Remplir`** (cover + crop) | même sémantique que le mode OBS (`OBS_BOUNDS_SCALE_OUTER` + `crop_source`, contrat 6 d'`AGENTS.md`) |
| Voile / bordure | parité CSS exacte | `.widget` 0,15 + bordure 1 px ; **bordure retirée sous un cadre** (`applyCadreWidget`) ; rien pour `sansCadre` / trou / caméra |
| Fit | calculé sur la **boîte entière**, rognage canvas **proportionnel** après | `object-fit` ignore le rognage ; borner avant écrasait le média (D11) |
| `composites` | **une par frame composée** | plus de `VideoProcessorBlt` à compter |

### 37.2 Livré

- `media.rs` : `decoder_pbgra` (WIC `PBGRA`), `MediaChargee.alpha` (inspection
  de l'octet alpha au chargement), `geometrie` = fit sur la boîte + `rogner_au_canvas`
  (source recadrée dans la même proportion, jamais de déformation).
- `texte.rs` : `bitmap_media(texture, alpha)` (cache clé `(pointeur, alpha)`),
  `dessiner_media(…, clip: Option<&PathGeometry>)` (aucune couche sans cadre),
  `dessiner_fond` (voile, `FillGeometry` si cadre sinon `FillRectangle` élargi
  1 px), `dessiner_bordure` (bande 1 px hors de la boîte de contenu).
- `scene_sources.rs` : `BoiteCfg` + `boites_depuis_scene` (exclusions
  `sansCadre` / trou / caméra, rect nul).
- `moteur.rs` : plan unique `(z, genre, index)` à 6 genres (`Boite < Media <
  Capture < Chat < Bordure < Cadre`), `VoieRendu` **toujours** active,
  `ContenuRendu` (regroupement des 4 listes), suppression de `segments`,
  `flush_run`, `calque_de`, `destination`, `rendre_textes`.
- `compositor/mod.rs` : `composer_borne` supprimée (plus aucun appelant ; le
  compositeur VP reste pour les harnais).
- Harnais `examples/parite_scene.rs` : scène JSON réelle → MP4.

### 37.3 Mesures (GTX 1070 Ti, NVENC zéro-copie)

| Scène | Frames / durée | fps | drops | latence |
|---|---|---|---|---|
| `af09059c` (13 widgets, 11 couches, 5 cadres, 1 chat, 4 boîtes) | 282 / 5 s | **56,4** | **0** | 19 ms |
| `cadres_scene` (3 widgets, 2 cadres) | 161 / 3 s | ~54 | 0 | — |

Le coût des couches Direct2D n'est pas mesurable ici : l'encodeur reste le
facteur limitant (~56 fps, comme à 4 calques).

**D1 — alpha, avant/après** (même région, 1070,0 → 1360,380) : avant, le PNG
`rgba` sortait sur un **rectangle noir plein** ; après, le fond de scène passe
à travers (dégradé, texte du fond, objet cyan).

**D5 — voile et bordure**, sondes sur la frame de `af09059c` :

| Sonde | Mesure | Attendu |
|---|---|---|
| Voile du chat (400,690) | YMIN 37 / YAVG 37,9 / YMAX 39 — **uniforme** | `rgba(224,224,224,0.15)` |
| Bordure haute (y = 668) | YAVG 93,8 / YMAX 151 | trait 1 px à 0,6 |
| Bordure gauche (x = 333) | YAVG 92,5 | idem |
| Hors boîte (x = 326) | YAVG 16 | rien |
| Bande `ajuster` du speedrun | YAVG 43,3 uniforme | voile dans la bande |
| Trou sans cible | YAVG 18,5 / YMAX 127 | **fond visible** (plus de trou) |

### 37.4 Pièges vérifiés — ne pas les redécouvrir

1. **`ID2D1RenderTarget::DrawRectangle` / `FillRectangle` / `FillGeometry`
   rendent `()`** (pas de `Result`) — reconfirmé (§35.7.3).
2. **`background-clip: border-box`** : le voile couvre la boîte de **bordure**,
   donc `rect + 1 px` quand le widget a une bordure — et **pas** d'élargissement
   quand un cadre est actif (le CSS met `border: none`).
3. **`object-fit` se calcule sur la boîte entière**, pas sur sa partie visible :
   borner le rect au canvas avant le fit écrasait le média (`06de1ec7` :
   311 px au lieu de 452). Rogner la destination **et** recadrer la source dans
   la même proportion donne les pixels exacts du CSS.
4. **`VideoProcessorSetStreamAlpha` est constant par flux** : aucune
   transparence par pixel, jamais (§30.1) — c'est la raison d'être du §37.
5. **`too_many_arguments` (clippy 8/7)** : `VoieRendu::nouveau` prend un
   `ContenuRendu` — regroupement, pas un `allow`.
6. **La bordure CSS est hors de la boîte de contenu** (`content-box`) : le trait
   se centre sur le bord extérieur (`left - 0,5`, `right + 0,5`, épaisseur 1).

### 37.5 Limites (assumées)

- **Le trou n'est plus percé** : une scène OBS à widget `trou` sans
  `captureCible` rend le **fond de scène**, pas du noir. C'est le comportement
  natif voulu (aucune source OBS derrière), et la comparaison région par région
  avec le projecteur OBS ne vaut plus pour cette zone.
- **Un widget de capture sans cible ne rend rien** — l'avertissement « source
  possible sur ce widget » vit dans le dashboard, jamais brûlé dans la sortie.
- **Caméra, speedrun, goal, musique, overlays** : toujours absents (lots 1.6d
  à 1.6f). L'input viewer est porté par le §38 ; le voile/bordure des autres
  widgets est en revanche correct.
- **Titres peints après le bord du canvas** (D10) : écart de quelques pixels au
  bord uniquement.
- **Transformations média** : `zoom`/`rot`/`offset`/`miroir` (widget et
  fond) + `bgZoom` (zoom écran), les fits `centrer`/`vignette`/`etendre`
  (§40) et `rotateX/Y` + `bgRotX/Y` en perspective (§41) sont portés.
  Restent : effets (lum/contraste/teinte/flou/pixel), morphs, lecture
  vidéo, carrousels (lots T3–T5).
- **Chat** : texte + pseudos colorés, sans emotes/badges/avatars en images.
- **SSIM non instrumenté** : `ffmpeg` (sidecar du dépôt) sait le faire, aucun
  rasteriseur SVG n'est embarqué. Les sondes `signalstats` restent la preuve
  utilisée ici.
- **Une couche Direct2D par média à cadre** (`PushLayer`) : coût non mesurable
  sur cette machine, à surveiller si l'encodeur cesse d'être le facteur limitant.

### 37.6 Vérification

```bash
cd src-tauri
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 103
# scène réelle, sans capture :
cargo run --release --example parite_scene -- \
  "%APPDATA%/com.streamos.v0/scenes/af09059c.json" "%APPDATA%/com.streamos.v0" sortie.mp4 5
# frame + sondes :
ffmpeg -ss 2.5 -i sortie.mp4 -frames:v 1 frame.png
ffmpeg -i frame.png -vf "crop=200:40:400:690,signalstats,metadata=print:file=-" -f null -
```

Sortie attendue du harnais : `13 widgets, 0 source(s) de capture, 0 texte(s),
5 cadre(s), 1 chat(s), 4 boîte(s)` puis `drops=0`, ~56 fps, `ffmpeg -f null -`
exit 0.

---

## 38. Lot 1.6c — input viewer natif : clavier, numpad, souris, manettes (2026-10-28)

Le widget `input-viewer` est rendu par le moteur natif pour ses 4 modes :
clavier AZERTY/QWERTY + numpad (c1), souris (c2), manettes 3 skins (c3).
Même gabarit partout : **état → géométrie pure (`input.rs`) → dessin D2D
(`texte.rs`) → pont (`src/moteur/mod.rs`) → harnais (`parite_scene`)**.

### 38.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| État | frame `input-viewer-etat` (chat_tx, 30 Hz) → `EtatInput {touches, souris, manette}` | canal déjà publié par `input_viewer.rs` — zéro nouvelle capture |
| Géométrie | viewBox `xMidYMid meet`, formes mises à l'échelle en px canvas | même calcul que `souris()` ; un seul tracé partout |
| Manette | `Vec<GroupeManette>` en **ordre de document SVG** (`lien` Statique/Bouton/Dpad/Stick × primitives Forme/Texte) | ordre = peinture ; transcription littérale des 3 `ivGenererManette*SVG` |
| Presse | couleur unie `inputCouleur` + liseré-halo α 0,3 sous la forme | approximation du `drop-shadow` (rayon ÷ 2 : 12 px→6e, 10→5e, 8→4e) |
| `connectee` | porté par l'état, **aucun rendu** | `majInputViewerEtat` ne le lit pas non plus (vérifié) |
| `inputMapping` | remap `boutons[affiché]=physique` (garde `idx<len` → 16/17 identité) ; D-pad **jamais remappé** ; `dpadInvertX/Y` uniquement dans le fallback 8bitdo | parité exacte `majInputViewerEtat` |

### 38.2 Pièges vérifiés — ne pas les redécouvrir

1. **Fallback 8bitdo** : si le skin est 8bitdo ET le D-pad (boutons 12-15)
   inactif → `axes[0]`/`axes[1]` (stick gauche) servent de D-pad, seuil 0,5,
   `axes[1] < 0` = haut. Inexistant sur les autres skins.
2. **Flèches D-pad** : hors des `<g data-dpad>` sur 8bitdo/xbox → elles
   restent grises quand le bras est pressé ; la flèche PS est **dans** le
   `<g>` → blanche quand pressée.
3. **Sticks** : translate `axes × 26` en **px viewBox** (× `echelle`) ;
   `.actif` (|ax|>0,15) éclaire le cap comme `.presse` ; le cap porte aussi
   `data-bouton` 10/11 (L3/R3).
4. **`.iv-fig-home`** garde son filet blanc 0,10 quand pressé (fill seul
   passe à la couleur) ; `iv-fig-pad` pareil.
5. **Dégradés `iv*-dish` sont radiaux** : centre (50 %, 45 %) de la boîte,
   rayons 0,62 × boîte → `CreateRadialGradientBrush` (premier usage du
   dépôt).
6. **`EtatManette` = tableaux fixes `[bool;20]`/`[f32;6]`** : le payload
   `Vec` est paddé/tronqué dans le pont (une frame peut porter `[]`).

### 38.3 Vérification

```bash
cargo clippy -p sos-moteur-natif --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                                 # 119
cargo test -p streamos-v0 decoder_input
cargo run --release -p sos-moteur-natif --example parite_scene -- \
  "$APPDATA/com.streamos.v0/scenes/af09059c.json" "$APPDATA/com.streamos.v0" out.mp4 5 \
  "manette=connectee,btn=0,btn=12,axe0=0.8"
```

Sondes sur le widget manette (1124, 328, 213×197, skin 8bitdo) : zone
YAVG 82,1 / YMAX 250 ; bouton A pressé YAVG 88,8 > bouton X relâché 72,5 ;
crop visuel : fightpad complète, A allumé `#214bf2`, média de fond
transparaissant (corps 0,62), cadre mgstyle.

---

## 39. Projecteur natif — fenêtre d'aperçu du moteur (2026-10-28)

Équivalent natif du projecteur OBS de la section Prévisualisation : **le
même bouton** « Ouvrir le projecteur » ouvre, en mode Rust, une fenêtre
Tauri détachée (déplaçable, redimensionnable, maximisable) qui affiche le
rendu réel du moteur — très légère, **aucun encodeur ni sink** n'est créé.

### 39.1 Chaînage

```
Toolbar.onOuvrirProjecteur (natifMode)
  → tauri.apercuFenetreToggle()
  → moteur::apercu_fenetre_toggle  (fenêtre "apercu-natif",
      streamos-apercu://localhost/ → resources/apercu.html)
  → page : invoke("moteur_apercu") toutes les 100 ms
  → moteur::moteur_apercu        (réponse binaire, PAS de JSON)
      └─ si rien ne tourne : configurer_scene + demarrer_apercu
  → boucle (SinkCfg::Apercu seul, 30 fps, audio None)
      └─ composition D2D + NV12 → Apercu (BGRA réduit, 10 fps)
  → page : ImageData BGRA→RGBA → canvas letterboxé
```

### 39.2 Session « aperçu seul » (moteur.rs)

- Champs dédiés `fil_apercu` / `arret_apercu` / `etat_apercu` :
  **invisible pour `en_cours()`/`etat()`/`natif_actif()`** — elle ne bloque
  ni la bascule de mode ni l'état « enregistrement » affiché dans l'UI.
- `demarrer_apercu(canvas, fps)` : idempotent ; pendant un enregistrement,
  retour immédiat (l'enregistrement alimente déjà `apercu`). Une erreur
  terminale persiste dans `etat_apercu` tant que `arreter_apercu` n'a pas
  acquitté — jamais de respawn en boucle sur la même panne.
- `encode_requis` dans `boucle` : `sinks = [Apercu]` seul → pas de
  NVENC/MFT, pas de sinks, pas d'audio ; la composition et l'aperçu sont
  inchangés.
- **La commande `moteur_apercu` démarre la session** : fenêtre ouverte =
  demande ; rien ne compose tant que personne ne regarde.
- Cycle de vie : fermeture fenêtre (`Destroyed`) → `arreter_apercu` ;
  mutation de scène (`snapshot_tx`, veille `demarrer_veille_apercu`) →
  `arreter_apercu` → le poll relance sur la scène fraîche ; bascule OBS →
  `arreter_apercu` ; démarrage d'enregistrement → `arreter_apercu` (accès
  exclusif captures) puis l'enregistrement sert l'aperçu.
- Réponse brute `tauri::ipc::Response` `[w u32 LE][h u32 LE][BGRA]` :
  **aucune route HTTP ajoutée** — les pixels ne sont pas exposés sur
  :4321 (contrat vie privée : une page arbitraire ne peut pas lire le
  flux du stream).

---

## 40. Lot T1 — transforms 2D, fits manquants, clip sans cadre (2026-10-28)

Premier lot du portage « transformations média + modules HUD ». Le média
natif applique désormais la chaîne CSS d'`applyMediaStyle` (widget) et de
`dessinerContenuFond`/`renderBg` (fond) : zoom, rotation, offset, miroirs —
plus les fits `centrer`/`vignette`/`etendre` qui tombaient sur `Etirer`.

### 40.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Chaîne | `T(−c)·S(zoom·miroir)·R(rot)·T(offset)·T(c)` autour du centre de la boîte | miroir exact `transform: translate(o) rotate(r) scale(z·m)` + `transform-origin: center` ; convention `Matrix3x2` ligne `p·M` (le facteur de gauche s'applique au point en premier) |
| `bgZoom` | porté ici (décision utilisateur) en `zoom_ecran` — `T(−c)·S(z)·T(c)` après toute la chaîne | `scale(z)` de `#bg` est un scale 2D en espace écran, trivial sans tilt ; les deux scènes MGS l'utilisent (1,5×) — restera en position externe pour la perspective (T2) |
| `bgFit` ≠ `mediaFit` | `Fit::depuis_nom_fond` : `centrer` fond → `Vignette` | `fitRects` n'est PAS `object-fit: none` : naturel si le média tient, contain sinon — sortie identique à `scale-down` |
| Fits ajoutés | `Centrer` (`none`, déborde) + `Vignette` (`scale-down`, jamais agrandi) + `etendre` → `Remplir` | `FIT_CSS` : ces valeurs sont réelles dans les scènes |
| Clip sans cadre | `PushAxisAlignedClip(boite)` sur TOUT média sans cadre | `overflow: hidden` du `.widget` s'applique aussi sans transform — `Centrer` déborde la boîte |
| Clip + transform | le masque (couche polygone ou clip axe-aligné) est posé **avant** `SetTransform` | il reste dans l'espace de la boîte comme le clip-path CSS ; sous transform `contentBounds` = la boîte |
| Rognage canvas | ignoré sous transform (`fit_rects` pur, le clip rogne) | recadrer src/dst avant rotation/zoom retire des pixels qui peuvent tourner dans le champ |
| Transform neutre | `Transform2D::NEUTRE` → chemin historique `geometrie` byte-identique | aucune régression sur les scènes déjà vérifiées |

### 40.2 Livré

- `media.rs` : `Transform2D` (+`NEUTRE`/`est_neutre`/`matrice`), `Fit` +=
  `Centrer`/`Vignette`, `depuis_nom` étendu (`etendre`→`Remplir`),
  `depuis_nom_fond`, `fit_rects` extrait de `geometrie`, champ `transform`
  sur `MediaCfg`/`MediaChargee`.
- `scene_sources.rs` : fond → `depuis_nom_fond` + transform (`bgRot`,
  offsets, miroirs, `bgZoom`→`zoom_ecran`) ; widgets → transform
  (`mediaZoom`/`mediaRot`/offsets/miroirs). Le transform couvre aussi le
  média de fond des widgets à contenu (même `w.*`, comme `syncMediaFond`).
- `texte.rs` : `dessiner_media(…, boite, transforme, clip)` — clip
  axe-aligné `PER_PRIMITIVE` sans cadre, couche masquée sous cadre, puis
  `tracer_bitmap` (`GetTransform`/`SetTransform`/`avant`).
- `moteur.rs` : branche `Media` choisit `fit_rects` sous transform,
  `geometrie` sinon ; accesseur `medias()`.
- `parite_scene` : compte des médias transformés dans le résumé.

### 40.3 Vérification

```bash
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 131 (8 nouveaux)
npm run check                                          # 0 erreur
cargo run --release -p sos-moteur-natif --example parite_scene -- \
  "$APPDATA/com.streamos.v0/scenes/f35daefd.json" "$APPDATA/com.streamos.v0" out.mp4 5
# → « 3 média(s) transformé(s) », ~56 fps, 0 drop
# af09059c : zoom 4,3 du speedrun rogné net à la boîte (crop visuel)
```

### 40.4 Limites (assumées)

- `rotateX`/`rotateY` (perspective 800 px widget, 2·canvasW fond) : lot T2,
  porté au §41.
- Effets `mediaLum/Contraste/Teinte/Flou/Pixel` et `bg*` : lot T3.
- Morphs bulge/pinch : lot T4. Lecture vidéo (`paused`/`time`/seek/A-B),
  `mediaSlides`/`bgSlides` : lot T5.
- Sous rotation, un `Centrer` peut déborder la boîte **et** être rogné à
  la boîte (parité `overflow: hidden`) — pas de rognage canvas préalable.

---

## 41. Lot T2 — perspective widget + fond (`rotateX/Y`, `bgRotX/Y`, `bgZoom`) (2026-10-28)

Deuxième lot du portage « transformations média + modules HUD ». Le moteur
projette désormais les plans `perspective(d) rotateX rotateY`
(`applyWidgetStyle` sur `.widget`, `renderBg` sur `#bg`) via l'effet
`CLSID_D2D13DPerspectiveTransform` : chaque plan est d'abord rasterisé dans
une texture intermédiaire, puis projeté sur le canvas.

### 41.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Rasterisation | une texture offscreen **taille canvas** par plan (`BITMAP_OPTIONS_TARGET`, PBGRA) | `DrawBitmap` ne sait pas la perspective ; les calques y dessinent en coords canvas, comme le DOM |
| Regroupement | clé `(rect, z)` : voile/média/capture/bordure/cadre d'un widget partagent l'offscreen | tous portés par le même `.widget` DOM ; `Chat`/`Input` exclus (limite §41.4) |
| Effet | `3DPerspectiveTransform` : LINEAR + BORDER_SOFT, `depth` = `perspective(d)`, origins perspective **et** rotation = centre de la boîte | miroir `perspective(d) rotateX() rotateY()` + `transform-origin: center` |
| `bgZoom` | `PerspCfg.zoom_ecran`, appliqué **après** projection en espace écran autour du centre ; retiré de la matrice 2D pour les entrées groupées | `scale(z)` est posé en premier sur `#bg`, hors de la perspective — même lecture qu'en T1 (§40.1) |
| Projection | `flusher_persps(z)` quand le plan sort de son z (persps triés par z à la config) | le plan projeté se pose à son niveau de z, comme le `.widget` DOM dans son stacking context |
| `dirty` | un offscreen sans contenu n'est pas projeté | les plans ne recoivent du contenu que si leurs calques dessinent |
| Rognage canvas | `fit_rects` sous perspective (pas de rognage préalable), comme sous transform 2D | la projection peut ramener des pixels hors canvas dans le champ |

### 41.2 Livré

- `media.rs` : `PerspCfg {rect, z, rot_x, rot_y, profondeur, zoom_ecran}`.
- `scene_sources.rs` : `persps_depuis_scene` — widget `rotateX/Y` ≠ 0 →
  `profondeur` 800 ; fond `bgRotX/Y` ≠ 0 → `profondeur` 2·canvasW +
  `zoom_ecran` = `bgZoom`, exclu si `bgObsSource` ; tri par `z`.
- `texte.rs` : `PlanPersp` (texture + bitmap + effet + `dirty`),
  `configurer_persps`, `effacer_persps`, `groupe_de`/`cible_pour`
  (`SetTarget` en cours de passe), `flusher_persps`/`_fin`,
  `peindre_persp` (effet + `zoom_ecran` post-projection).
- `moteur.rs` : `persps` dans `ContenuRendu`, accesseur `persps()`, boucle
  : `flusher_persps(z)` par entrée, `groupe_de`/`cible_pour` par calque ;
  `zoom_ecran` retiré du transform 2D d'une entrée groupée.
- `parite_scene` : compte des plans perspective dans le résumé.

### 41.3 Pièges vérifiés — ne pas les redécouvrir

1. **`ID2D1Properties::SetValue` et les propriétés déclarées** : le champ
   `type` doit être `D2D1_PROPERTY_TYPE_UNKNOWN` (« type déjà enregistré »)
   ou le type exact de la propriété. Les enums
   (`INTERPOLATION_MODE`, `BORDER_MODE`) sont enregistrés `ENUM`, pas
   `UINT32` → `SetValue(idx, UINT32, …)` = `E_INVALIDARG`
   (« Paramètre incorrect »). `UNKNOWN` convient pour les six propriétés.
2. **`octets(&temporaire)`** : le `&[u8]` d'un `[f32; N]` temporaire meurt
   en fin de statement (E0716) — chaque valeur est nommée avant le
   tableau de propriétés.
3. **`GetOutput` relu au moment du flush** (`peindre_persp`), jamais mis
   en cache : l'effet recalcule sa sortie depuis la bitmap d'entrée.
4. **Un plan = une texture canvas + une passe d'effet** : mesuré 52–56 fps
   sur les deux scènes MGS (1 plan chacune) — l'encodeur reste le facteur
   limitant.

### 41.4 Limites (assumées)

- **Contenu chat / input-viewer d'un widget 3D reste plat** : les genres
  `Chat`/`Input` ne sont pas regroupés dans l'offscreen (leur rendu
  DirectWrite devrait y être re-routé). En OBS tout le `.widget` est
  tourné — écart visible uniquement sur un widget 3D de ces types.
- Deux widgets au même `(rect, z)` fusionneraient en un seul plan — cas
  dégénéré accepté.
- `mediaLum/Contraste/Teinte/Flou/Pixel` et `bg*` : lot T3 ; morphs : T4 ;
  lecture vidéo + carrousels : T5.

### 41.5 Vérification

```bash
cd src-tauri
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 133 (2 nouveaux)
npm run check                                          # 0 erreur
cargo run --release -p sos-moteur-natif --example parite_scene -- \
  "$APPDATA/com.streamos.v0/scenes/f35daefd.json" "$APPDATA/com.streamos.v0" out.mp4 5
# → « 1 plan(s) perspective », ~52 fps, 0 drop : jacquette rotateY 38,25°
#   projetée (crop visuel — bord droit reculant, cadre projeté avec).
# af09059c → « 1 plan(s) perspective », 56 fps, latence 11 ms.
```

---

## 42. Lot T3 — effets visuels (`lum`/`contraste`/`teinte`/`flou`/`pixel`) (2026-10-28)

Troisième lot du portage « transformations média + modules HUD ». Le média
natif applique désormais `chaineFilter` (diffusion.html) : une seule
`ColorMatrix` pour brightness/contrast/hue-rotate, puis `GaussianBlur`,
plus la pixelisation `bgPixel` du chemin canvas du fond.

### 42.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Couleur | **une** `D2D1ColorMatrix` composant `brightness·contrast` (diagonale `k = b·c` + offset `o`) puis `hue-rotate` — `M = k·H`, offset inchangé | les trois filtres CSS sont linéaires et commutent dans l'ordre brightness→contrast→hue-rotate |
| Alpha | `D2D1_COLORMATRIX_ALPHA_MODE_STRAIGHT` | les filtres CSS opèrent sur l'alpha **non prémultiplié** ; nos bitmaps sont PBGRA |
| Flou | `GaussianBlur` chaîné après la matrice, `σ_src = flou / s_fit` | `blur(px)` CSS vaut dans l'espace de l'élément (post-fit) ; le flou d'une image mise à l'échelle `s` par σ vaut `σ·s` |
| `marge` | `imageRectangle` élargi de `⌈3σ_src⌉` | le halo du flou déborde la source ; sans marge il est rogné |
| Pixel | buffer réduit `boite/pf` + upscale `NEAREST` | miroir du petit canvas DOM (`imageSmoothingEnabled = false`) |
| `mediaPixel` | **non porté ici** | parité DOM : aucun `filter` pixel côté widget — il n'agit qu'en chemin morph (T4) |
| `zoom_ecran` + pixel | appliqué à l'**upscale** (espace écran), pas dans le petit buffer | `scale(bgZ)` est posé sur `#bg` entier (§40/§41) |

### 42.2 Livré

- `media.rs` : `EffetsCfg` (`lum`/`contraste`/`teinte`/`flou`/`pixel`,
  `NEUTRE`, `couleur_active`, `pixel_active`, `matrice_couleur` — la H de
  hue-rotate transposée en convention ligne D2D, luminance préservée,
  gris teinté comme le CSS).
- `scene_sources.rs` : `effets` du fond (`bg*`) et des widgets (`media*`,
  `pixel: 0` — morph uniquement en T4).
- `texte.rs` : `TraceMedia`/`TracePixel` (regroupement clippy), cache
  `effets` clé (bitmap, matrice, σ), `petits` (buffers `boite/pf`),
  `image_effets` (chaîne bitmap→`ColorMatrix`→`GaussianBlur`),
  `tracer_image` (la paire src→dst devient un transform, `DrawImage` ne
  scale pas), `dessiner_media_pixel`.
- `moteur.rs` : `TraceCouche`/`PixelCouche`, branche `pixel_active()`.

### 42.3 Pièges vérifiés

1. **`hue-rotate` ne préserve PAS le gris** : il conserve la luminance
   (pondération 0.213/0.715/0.072) — un gris pur est teinté, comme en CSS.
2. **`blur` est en px de destination**, pas de source : `σ_src = flou /
   s_fit` (isotrope ; moyenne géométrique des deux axes sous `Etirer`).
3. **Dans le petit buffer, le flou est en px réduits** : `σ` calculé sur
   l'échelle vers `boite/pf` → halo ×pf à l'upscale (quirck DOM reproduit).
4. **`SetInput` chaîne les effets** via `GetOutput` de l'amont (pas de
   `SetInputEffect` en windows 0.58).

### 42.4 Limites (assumées)

- `mediaPixel` : ignoré hors chemin morph (T4).
- Flou non uniforme (`Etirer` anisotrope) : approché par σ isotrope.
- Morphs : T4 ; lecture vidéo + carrousels : lot T5 (§44).

### 42.5 Vérification

```bash
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 137 (6 nouveaux)
# scène synthétique (f35daefd + bgLum −40/bgPixel 8/bgTeinte 90 +
# mediaLum/Contraste/Flou) → fond pixelisé-assombri-teinté, média filtré,
# ~55 fps, 0 drop (crop visuel).
```

---

## 43. Lot T4 — morphs bulge/pinch (warp `MORPH_FRAG`) (2026-10-28)

Quatrième lot du portage. Les points de morph `w.morphs` / `scene.bgMorphs`
sont honorés : rasterisation offscreen **taille boîte** puis warp par
pixel shader D3D11, réplique exacte de `MORPH_FRAG` (diffusion.html
~5400).

### 43.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Warp | pixel shader D3D11 dédié (`morph.rs`), pas de mesh | `MORPH_FRAG` est un warp par échantillonnage — le mesh D2D ne sait pas le faire |
| Offscreen | texture `RT|SRV` taille boîte + bitmap `TARGET` ; sortie `RT|SRV` + bitmap `NONE` | même motif que les offscreens de perspective ; D2D peint, D3D lit puis écrit, D2D relit |
| Uniforms | `float4 u_morphs[20]` `(x, y, rayon, k)` + `u_count` (bits int !) + `u_aspect = w/h` | miroir exact du WebGL ; `k = intensite/100` en `retrecir`, `−intensite/200` sinon |
| `mediaPixel` | pixelisation dessinée **dans** l'offscreen (`peindre_media_local`) | le DOM applique `mPixel` dans `morphOffscreen` ; hors morph il n'est rien (parité) |
| `zoom_ecran` | retiré du transform interne (appliqué au plan par T2) | `scale(bgZ)` agit hors de l'offscreen, comme le `transform` de `#bg` |
| Clip | posé **après** le warp sur la sortie (cadre ou axe-aligné) | le warp déplace les pixels ; le clip doit rogner le résultat, pas la source |

### 43.2 Livré

- `morph.rs` (nouveau) : `WarpMorph` — VS fullscreen triangle (aucun
  buffer vertex), PS = `MORPH_FRAG` traduite en HLSL, constant buffer
  20×vec4 + count + aspect, sampler linéaire clamp. Compilation runtime
  via `D3DCompile` (feature `Win32_Graphics_Direct3D_Fxc` ajoutée à
  `Cargo.toml` — nécessaire, hors liste blanche des sources mais requis
  pour compiler).
- `media.rs` : `MediaCfg`/`MediaChargee` += `morphs` + `pixel_morph` ;
  `PartialEq` retiré (`MorphPoint` ne le porte pas et `sos-scene` est
  hors périmètre).
- `scene_sources.rs` : fond ← `bgMorphs` + `pixel_morph = bgPixel` ;
  widget ← `w.morphs` + `pixel_morph = w.mediaPixel`.
- `texte.rs` : `dessiner_media_morphe` (offscreen → contenu → `Flush` →
  warp → `DrawBitmap` sous clip), `peindre_media_local`,
  `buffers_morphe` (caches `morphes`/`sorties_morph` par rect).
- `moteur.rs` : branche `!media.morphs.is_empty()` → `MorpheCouche`.

### 43.3 Pièges vérifiés

1. **`u_count` est un `int` HLSL** — écrire ses *bits* (`f32::from_bits`),
   pas `n as f32`.
2. **Les factories D3D11 de windows 0.58 sont en out-param** —
   `CreateVertexShader/CreatePixelShader/CreateBuffer/CreateSamplerState/
   CreateShaderResourceView/CreateRenderTargetView` retournent
   `Result<()>` + `Some(&mut out)`.
3. **L'écriture `u_count` en float faisait boucler 0 fois** (bits de
   `1.0f32` ≠ int 1) — le warp sortait identique.
4. **`D3DCompile` (d3dcompiler_47.dll)** : `PCSTR` pour les noms
   d'entrée (`s!("VSMain")`), blob d'erreurs en UTF-8 dans `pperrormsgs`.

### 43.4 Limites (assumées)

- ≤ 20 points de morph par média (comme le DOM).
- Le warp échantillonne en `CLAMP` linéaire — les pixels en dehors du
  rayon n'ont aucune déformation (identique à `MORPH_FRAG`).
- Lecture vidéo + carrousels : livrés, lot T5 (§44).

### 43.5 Vérification

```bash
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 137
# scène synthétique (f35daefd + morphs agrandir 100 @0.5,0.5 + retrecir
# 80 @0.15,0.15 sur le 1er média) → bulge+pinch nets au crop, ~54 fps,
# 0 drop, NVENC actif.
```

## 44. Lot T5 — lecture vidéo (`paused`/boucle/A–B) + carrousels (2026-10-28)

### 44.1 Décisions tranchées

- **`mediaTime`/`bgTime` NON portés** — le DOM seede le tracker à la
  création (`entry.mediaTime = w.mediaTime` puis `applyMediaState` compare
  à cette valeur) → aucun seek initial n'existe. Une session native = un
  rechargement de page : la vignette d'une vidéo en pause est sa **frame 0**,
  jamais `mediaTime`.
- **Boucle A–B par position affichée** (`position_us ≥ B → seek A`) — le
  `timeupdate` ~4 Hz du DOM est approximé à la frame près ; **un seul seek
  par appel `avancer`** : un fichier dont le seek retomberait ≥ B bouclerait
  à l'infini.
- **Boucle simple** (`v.loop = !paused && !ab`) = reseek à 0 en fin de flux ;
  vidéo en pause = première frame chargée puis plus aucune avance ; fin de
  flux sans boucle = dernière image figée.
- **`decalage_us` = horloge vidéo** : un seek à `p` au temps moteur `t` pose
  `decalage = t − p` — les frames post-seek redeviennent dues en rythme
  (miroir du `v.currentTime = …` du DOM ; sans lui, tout le segment A→B se
  viderait d'un coup après un reseek).
- **Carrousel** : `sec = max(1, sec||5)`, `fondu = min(0,8 s, sec/2)`
  (contrat `carrousel.md`) ; < 2 slides = statique (le DOM n'arme aucun
  timer). Widget = fondu `ease-in-out` (transition CSS `.slide`, courbe
  Bézier évaluée par Newton) ; fond = alpha linéaire (`globalAlpha` de
  `dessinerContenuFond`).
- **Morph + slides = swap sec** : la slide courante seule, sans fondu (le
  DOM remplace `src`, pas de fondu dans `morphRendreWidget`).
- **Slide illisible ignorée, jamais fatale** (miroir d'un `<img>` en échec) ;
  pile entièrement vide = rien dessiné.
- **Le fit se recalcule par slide** (`taille_slide`) — tailles possibles
  différentes, comme les `<img>` du DOM.

### 44.2 Livré

- `media.rs` : `LectureCfg`/`CarrouselCfg` (config), `Carrousel`/
  `SlideChargee` (chargé), `Carrousel::fenetre` (+ `fenetre_carrousel` pur,
  testable), `ease_in_out` (Bézier CSS exacte), `MediaChargee::taille_slide`,
  `charger_carrousel` (slides → textures), `VideoLecteur` += `paused`/
  `boucle`/`position_us`/`decalage_us`/`taille`, `seeker`
  (`SetCurrentPosition`, hectonanosecondes) + `afficher` ; `avancer`
  réécrit (pause/A–B/EOS/horloge vidéo).
- `scene_sources.rs` : `lecture(paused, a, b)` (b > a ou rien) + `carrousel`
  (liste non vide → `CarrouselCfg`, `ease` vrai widget/faux fond) câblés
  sur le fond (`bgPaused`/`bgLoopA/B`/`bgSlides`/`bgSlidesSec`) et les
  widgets (`mediaPaused`/`mediaLoopA/B`/`mediaSlides`/`mediaSlidesSec`).
- `texte.rs` : `TraceMedia`/`TracePixel` += `opacite` ; `tracer_bitmap` +=
  opacité (`DrawBitmap`) ; `image_opacite` (`ColorMatrix` diag k en
  **prémultiplié** — diag(k,k,k,k) ; en STRAIGHT il faudrait (1,1,1,k) et
  diag k produirait k² sur RVB) ; `dessiner_media_pixel` upscale à
  `t.opacite`.
- `moteur.rs` : `bitmaps_medias` → `Vec<Vec<ID2D1Bitmap1>>` (une bitmap par
  slide) ; la branche `Media` calcule `carrousel.fenetre(t)` puis trace
  `(précédente, 1−k)` puis `(courante, k)` ; `TraceCouche`/`PixelCouche` +=
  `opacite` (capture : 1,0).

### 44.3 Pièges vérifiés

1. **`globalAlpha` s'applique UNE fois** — au `drawImage` final du canvas,
   pas au remplissage du petit buffer (chemin pixel : l'upscale porte
   l'opacité, les passes internes restent à 1,0).
2. **`SetCurrentPosition(GUID_NULL, VT_I8 hns)`** : `PROPVARIANT::from(i64)`
   + `&GUID::zeroed()` ; la frame post-seek peut précéder A (keyframe) — la
   position remonte seule, pas de reseek côté bas (comme le DOM).
3. **Première frame hors fenêtre temporelle** : `position_us = −1` tant
   qu'aucune frame n'est affichée — la première est téléversée même si son
   PTS > temps (vignette DOM d'une vidéo en pause, et démarrage plus
   fidèle qu'un écran noir jusqu'au PTS).
4. **`ease-in-out` CSS ≠ smoothstep** : la courbe exacte est
   `cubic-bezier(0,42 ; 0 ; 0,58 ; 1)` — inversée en x par Newton (4 it.),
   y évalué ensuite ; bornes exactes 0/1, milieu = 0,5.

### 44.4 Limites (assumées)

- Carrousels **images uniquement** (contrat `carrousel.md` — un `.mp4` en
  slide est ignoré comme un `<img>` cassé du DOM).
- `mediaPaused`/`bgPaused`/`bgLoop*` **ignorés sur les carrousels** (le DOM
  de même : ce sont des images).
- Pas de seek live (`mediaTime` interactif = PlayerBar/dashboard —
  l'événement n'emprunte pas `chat_tx` ; hors périmètre T5).
- Modules HUD (horloge, objectifs), équaliseur, spots : lots M1/M2.

### 44.5 Vérification

```bash
cargo clippy --workspace --all-targets -- -D warnings   # exit 0
cargo test -p sos-moteur-natif                          # 140 (+ 3 tests
#   fenetre_carrousel statique/avance/bouclage + ease_in_out bornes/monotone)
# media_scene sur un .mp4 réel : 159 composites en 3 s, NVENC zéro-copie,
# 0 drop — le nouveau `avancer` ne casse pas la lecture.
# parite_scene af09059c : composition OK (la capture WGC échoue ici — hwnd
#   périmé de session, sans rapport avec le lot).
```
