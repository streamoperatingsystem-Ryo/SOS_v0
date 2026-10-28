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

