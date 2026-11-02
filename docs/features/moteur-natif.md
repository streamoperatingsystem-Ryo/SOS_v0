# Moteur natif — capturer, composer et encoder sans OBS

> **Feature IMPLÉMENTÉE et branchée dans l'app** (mode Rust, §32+) — état au
> 2026-11-01 : la cible est la **parité complète avec le mode OBS**. Le plan
> de suivi lot par lot (état, prochain lot, pièges déjà payés) vit dans
> `AGENTS.md`, section « Plan de suivi — parité moteur natif ». Ce fichier
> fixe les contrats et le détail d'implémentation ; le déroulé complet
> (mesures, matrices matérielles, algorithmes) vit dans
> `PLAN-MOTEUR-NATIF.md` à la racine.
> ⚠️ **Aucun chiffre de ce fichier n'est mesuré, hors §27/§28.** Toute autre
> valeur est une **cible** du plan, jamais un résultat observé. Ne pas citer
> ces nombres comme acquis.
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
- ⚠️ **Orthographe du pipe** : `CreateNamedPipeW` exige `\\.\pipe\…`
  (`chemin_pipe_audio`) mais FFmpeg refuse cette forme en `-i` (ENOENT —
  son protocole `file` n'accepte pas le backslash) ; lui passer
  `//./pipe/…` (`chemin_pipe_audio_ffmpeg`). `ecrire` tolère
  `ERROR_PIPE_LISTENING` (client pas encore connecté → bloc sauté).
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
  "audio": { "loopback": true, "micro": true, "offset_ms": 0 },
  "apercu": { "largeur": 480, "fps": 10 }
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

> ⚠️ **Historique de la tranche 1** — dépassé par les lots T1/T5 (§40-§44) :
> transforms, fits, lecture vidéo (pause/boucle A–B), carrousels et
> **toutes les slides** sont désormais portés. Ne pas lire cette liste comme
> l'état actuel.

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

- **`animer`** : porté depuis le lot C1 pour les 2 styles rendus — segment
  comète (§48) ; `Capacites.cadres_animes = true`. Les styles non portés ne
  produisent toujours aucun cadre (dégradé au démarrage).
- **Les 9 styles du registre sont portés** (§49 à §53) : `mgstyle`,
  `thin-line`, `carre`, `arrondis`, `tribal`, `cyberpunk`, `story`,
  `hexagon`, `arena`. Un style **inconnu** (scène corrompue/importée) ne
  produit aucun cadre et est signalé au démarrage.
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
  → page : invoke("moteur_apercu_config") → poll à 1000/fps
  → page : invoke("moteur_apercu") au rythme du réglage
  → moteur::moteur_apercu        (réponse binaire, PAS de JSON)
      └─ si rien ne tourne : configurer_scene + demarrer_apercu
  → boucle (SinkCfg::Apercu seul, fps = `apercu.fps`, audio None)
      └─ composition D2D + NV12 → Apercu (RGBA réduit, cadence réglée)
  → page : ImageData RGBA direct → canvas letterboxé
```

### 39.2 Session « aperçu seul » (moteur.rs)

- Champs dédiés `fil_apercu` / `arret_apercu` / `etat_apercu` :
  **invisible pour `en_cours()`/`etat()`/`natif_actif()`** — elle ne bloque
  ni la bascule de mode ni l'état « enregistrement » affiché dans l'UI.
- `demarrer_apercu(canvas, fps, apercu)` : idempotent ; pendant un
  enregistrement, retour immédiat (l'enregistrement alimente déjà
  `apercu`). Une erreur terminale persiste dans `etat_apercu` tant que
  `arreter_apercu` n'a pas acquitté — jamais de respawn en boucle sur la
  même panne.
- `encode_requis` dans `boucle` : `sinks = [Apercu]` seul → pas de
  NVENC/MFT, pas de sinks, pas d'audio ; la composition et l'aperçu sont
  inchangés.
- **La commande `moteur_apercu` démarre la session** : fenêtre ouverte =
  demande ; rien ne compose tant que personne ne regarde.
- Cycle de vie : fermeture fenêtre (`Destroyed`) → `arreter_apercu` ;
  mutation de scène (`snapshot_tx`, veille `demarrer_veille_apercu`) →
  `arreter_apercu` → le poll relance sur la scène fraîche ; bascule OBS →
  `arreter_apercu` ; démarrage d'enregistrement → `arreter_apercu` (accès
  exclusif captures) puis l'enregistrement sert l'aperçu ;
  sauvegarde des réglages aperçu → `arreter_apercu` hors enregistrement
  (application quasi immédiate).

### 39.3 Réglages de qualité (2026-10-30)

Le projecteur n'est plus figé à 480×270 @ 10 fps :

- **`ApercuCfg { largeur, fps }`** (sos-moteur-api) : champ `SortieCfg.apercu`
  — consommé par la session « aperçu seul » **et** par la boucle
  d'enregistrement/stream (le projecteur y sert les frames de la sortie).
  `largeur` = borne de largeur du flux réduit (`0` = résolution canvas,
  hauteur au ratio, dimensions paires — `taille_apercu`) ; `fps` = cadence
  d'émission (`pas = fps_composition.div_ceil(apercu.fps)`, jamais au-delà).
- **Sortie RGBA8 directe** : le tampon `Apercu` est swappé B↔R pendant la
  copie staging (le VideoProcessor ne garantit pas R8G8B8A8 en sortie) —
  `ImageData` le consomme sans conversion, la boucle de swap JS est morte.
- **Persistance** : bloc `apercu` de `moteur.json` (`ApercuNatifCfg`,
  défaut **480 px / 10 fps**), commandes `moteur_apercu_config` /
  `moteur_apercu_config_sauver` — UI : bloc « Projecteur » de la modale
  Sortie (480 / 960 / 1280 / canvas × 10 / 15 / 30 fps).
- **Défaut léger (2026-11-01)** : le projecteur doit rester quasi gratuit —
  480 px à 10 fps (480×270 en 16:9), l'utilisateur peut remonter.
  `moteur_apercu` passe **`apercu.fps` comme cadence de composition** de la
  session « aperçu seul » (avant : `30` codés en dur, seule l'émission était
  throttlée par `pas_apercu`) — la passe Direct2D plein canvas et les copies
  WGC tombent donc à 10/s. Conséquence assumée : animations à 10 pas/s, ce
  que l'émission à 10 fps rend invisible. Le **gouverneur n'est pas évalué**
  en session aperçu seule (`encode_requis == false`) : une « dégradation »
  y serait inventée et écrite dans les compteurs partagés avec une future
  sortie.
- **Dédup backend** : la commande prend `depuis: Option<i64>` (dernier
  `pts_us` affiché par la page) et renvoie une réponse **vide sans recopier
  le tampon** quand la frame n'a pas changé — fin des ~110 Mo/s de copies
  IPC sur une scène figée (1280×720@30 avant ce lot).
- **Dedupe côté page** : l'en-tête porte `pts_us` ; la page saute les
  frames déjà affichées et poll exactement au rythme réglé.
- Réponse brute `tauri::ipc::Response` `[w u32 LE][h u32 LE][pts_us i64 LE]
  [RGBA]` : **aucune route HTTP ajoutée** — les pixels ne sont pas exposés sur
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
| Regroupement | clé `(rect, z)` : voile/média/capture/contenu/titre/bordure/cadre d'un widget partagent l'offscreen | tous portés par le même `.widget` DOM ; contenus et titres ajoutés en H2 (§71) |
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

- **Contenu d'un widget 3D** : **porté en H2/H2b/H2c** (§71) — les cinq
  genres de contenu (`Chat`, `Input`, `Goal`, `Speedrun`, `Musique`) et les
  titres de widget sont regroupés dans l'offscreen du plan comme le média.
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
- Modules HUD : les 5 rendus sont portés (2026-10-31) — objectifs,
  musique, équaliseur, spots + horloge existante. eq/spots = calques
  plein écran sous les widgets (`Z_SPOTS` −2 / `Z_EQ` −1), spectre
  poussé via `pousser_eq` (frames `eq-spectrum` du canal `chat_tx` →
  `EtatEq`, lissage attaque/retombée du DOM).

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

---

## 45. Modale « Sortie » — réglages persistés + stream RTMP/SRT (2026-10-30)

La modale « Sortie » (équivalent du panneau Sortie d'OBS) rend réglables
les paramètres figés du lot 1.4 (§32.1 « affichés en lecture seule ») et
expose enfin le flux réseau natif côté UI.

### 45.1 Persistance — bloc `sortie` de `moteur.json`

```json
{
  "mode": "natif",
  "sortie": {
    "codec": "h264", "debit_kbps": 6000, "fps": 60, "preset": "equilibre",
    "audio_loopback": true, "audio_micro": true,
    "gain_loopback": 1000, "gain_micro": 1000
  }
}
```

- Lecture/écriture en **objet JSON générique** (`config.rs`) : les clés
  inconnues sont préservées à la réécriture, conforme §17. `sauver_mode_moteur`
  relit le fichier avant d'écrire — l'écriture directe historique écrasait
  le bloc `sortie`.
- Bloc absent → défauts produit (= valeurs figées historiques, zéro
  régression). Bloc invalide → erreur honnête à la lecture par la modale ;
  non-fatal aux démarrages de sortie (`unwrap_or_default`).
- Les gains sont en **millièmes** (1000 = 100 %, max 4000 — bornes
  `SortieCfg::valider`), l'UI affiche des %.

### 45.2 Commandes

- `moteur_sortie_config` / `moteur_sortie_config_sauver` : lecture et
  persistance des réglages. La sauvegarde **valide avant d'écrire**
  (`valider_sortie_cfg` : codec `h264|hevc|av1`, fps 1-240, débit
  1-200000 kbps, preset `rapide|equilibre|qualite`, gains ≤ 4000) — un
  JSON inapplicable n'atteint jamais le disque.
- `moteur_sortie_demarrer` (enregistrement) applique désormais les
  réglages persistés (`appliquer_sortie_cfg` : codec, débit, fps, preset,
  `AudioCfg`) ; `SessionNatif.fps` reflète le vrai fps au lieu du 60
  hardcodé.
- `moteur_stream_demarrer` : les paramètres absents retombent sur la
  config persistée (au lieu des défauts en dur) ; l'`AudioCfg` suit les
  réglages (loopback/micro/gains). Les paramètres explicites priment
  toujours. La config qui a réellement démarré reste persistée dans le
  keyring (`StreamCfgPersistee`) pour le pré-remplissage — clé et
  passphrase SRT incluses, jamais en clair dans un fichier.

### 45.3 UI

- Lanceur : bouton « Réglages de sortie » dans le bloc moteur de la
  section OBS (mode natif) ; le résumé `moteur.sortieParametres`
  affiche les valeurs réelles (`{fps} fps · {codec} · {debit} kb/s`).
- `SortieNatifModal.svelte` — trois blocs : **Vidéo** (résolution =
  canvas de la scène, en lecture seule ce lot — pas de rescale GPU ;
  FPS 24/30/50/60/120 ; codec ; débit ; preset), **Audio** (toggles
  loopback/micro + sliders de gain 0-200 %), **Stream** (protocole
  RTMP/SRT, URL, clé, passphrase/latence SRT, démarrer/arrêter — même
  session et même arrêt qu'un enregistrement).
- Persistance au `change` de chaque champ (pas de bouton « Appliquer ») ;
  erreurs de validation remontées dans la modale.
- i18n : `sortieModal.*` dans les 8 locales (`fr.json` source de vérité).

### 45.4 Limites (assumées)

- **Résolution de sortie = canvas de la scène** : aucun rescale GPU dans
  ce lot (la passe `VideoProcessorBlt` peut sortir en NV12 à une autre
  taille — chantier futur, livré au §46). Le champ est affiché en lecture
  seule avec un hint explicite.
- Une seule sortie à la fois (enregistrement OU stream) — règle
  `preparer_sortie` inchangée.

---

## 46. Résolution de sortie + conteneur MKV (2026-10-30)

La limite §45.4 est levée : la vidéo encodée peut être réduite sous la
résolution du canvas, et le fichier n'est plus figé en fMP4.

### 46.1 Décisions tranchées

| Sujet | Décision | Motif |
|---|---|---|
| Forme du réglage | **Hauteurs standards** (720/540/480/360), `null` = canvas | l'utilisateur choisit une qualité, pas une géométrie |
| Largeur | **Dérivée du ratio du canvas** au démarrage (`resoudre_resolution`, moteur/mod.rs), arrondie au pair supérieur | jamais persistée : elle suivrait mal un changement de canvas |
| Chemin du rescale | Passe `VideoProcessorBlt` de la conversion NV12 (`ConvertisseurNv12` à dims entrée/sortie séparées) | zéro passe ajoutée, composition inchangée à la taille du canvas |
| Conteneur | `"mp4"` (fMP4, défaut historique) \| `"mkv"` (matroska) — `FfmpegSink` gérait déjà les deux | fichier seul : le protocole RTMP/SRT fixe le format réseau |

### 46.2 Livré

- `sos-moteur-api` : `SortieCfg.resolution: Option<(u32,u32)>` — `None` =
  canvas (aucune régression) ; `valider` exige des dims paires non nulles.
- `color_gpu.rs` : `ConvertisseurNv12::nouveau(ctx, entree, sortie, fps)` —
  le `Blt` fait l'échelle dans la même passe (rects par défaut, comme
  `Apercu`).
- `moteur.rs` : `(ow, oh)` câblé — `VoieRendu` produit le NV12 à la
  résolution encodée ; encodeurs NVENC/MFT, `Apercu` (source = la texture
  encodée : le projecteur montre ce qui part), compteurs et gouverneur la
  voient.
- `config.rs` : `SortieNatifCfg.conteneur` + `resolution` (hauteur,
  `None` = canvas), `#[serde(default)]` → rétro-compat.
- `moteur/mod.rs` : `resoudre_conteneur` / `resoudre_resolution`,
  `normaliser_chemin` pose l'extension du conteneur, bornes dans
  `valider_sortie_cfg` (conteneur mp4|mkv, hauteur paire 2-4320).
  Portée : enregistrement **et** stream (`demarrer_stream_bloquant` lit
  aussi `resolution`).
- UI : selects résolution + conteneur dans `SortieNatifModal` ; le
  dialogue « Enregistrer sous » (`Toolbar`) suit le conteneur persisté
  (extension + filtre MP4/Matroska). i18n `sortieModal.resCanvas` /
  `conteneur`, `resolutionHint` réécrit dans les 8 locales.

### 46.3 Limites (assumées)

- Hauteurs uniquement — pas de résolution libre (la borne backend accepte
  toute hauteur paire 2-4320 pour une config écrite à la main).
- Une résolution **supérieure** au canvas est acceptée (upscale par le
  même `Blt`) — choix utilisateur honoré, jamais de clamp muet.
- `appliquer_scene` à chaud inchangé : la résolution se fixe au démarrage
  de la sortie, comme le codec et le débit.

---

## 47. Parité OBS — fond = capture native (`bgCaptureCible`, lot B, 2026-11-01)

Le fond d'une scène peut être une capture **sans OBS** : `Scene.bgCaptureCible`
(même enum `CaptureCible` que `Widget.captureCible`, `serde(default,
skip_serializing_if)` — absent des scènes existantes, ignoré par le mode OBS).

- **Priorité** : `bgCaptureCible` > `bgObsSource` (OBS) > `bgSlides` >
  `bgMedia`. `bgObsSource` et `bgCaptureCible` peuvent coexister : chaque mode
  lit le sien, une même scène reste valide dans les deux modes.
- **Rendu** : `scene_sources::sources_depuis_scene` produit une source
  `FOND_CAPTURE` (`"__fond"`), rect `(0, 0, canvasW, canvasH)`,
  `z = i32::MIN`, sans cadre ni forme caméra. `medias_depuis_scene` cesse de
  peindre le fond média/slides quand la capture est liée.
- **UI** : en mode Rust, le dialogue de fond écrit `bgCaptureCible`
  (`setFondCaptureCible`, **aucun appel OBS**) ; en mode OBS,
  `lierFondCapture` est inchangé. « Retirer la capture » purge les deux
  champs (et la source OBS si elle existe).
- **Dégradation** : `bgObsSource` seul (sans `bgCaptureCible`) reste non rendu
  en mode Rust et est **signalé** — jamais un noir muet.
- **Vérification** : `cargo test -p sos-scene` (round-trip du champ),
  `cargo test -p sos-moteur-natif`
  (`fond_capture_produit_une_source_plein_canvas_prioritaire`),
  `cargo clippy --workspace --all-targets -- -D warnings`, `npm run check`.

---

## 48. Cadres animés — comète `animer` (lot C1, 2026-11-01)

`CadreConfig.animer` n'est plus ignoré : les deux styles rendus (`mgstyle`,
`thin-line`) animent leur bord comme `diffusion.html`.

- **Recette mirée** : `<path pathLength="100" stroke-dasharray="7 93">` +
  `<animate attributeName="stroke-dashoffset" from="0" to="-100" dur="4s">`.
  Conséquence : un segment de **7 % du périmètre**, un tour toutes les **4 s**,
  dans `couleurFin`, épaisseur `max(1.2, 0.7 × strokeWidth)`, bouts ronds.
- **Tracé** (`cadres::trace_comete`) : le chemin du **SVG du style**, pas le
  polygone de découpe — rectangle plein cadre pour `mgstyle` (`rectPath`
  rx=0), rectangle arrondi pour `thin-line`. Aux coins coupés de `mgstyle`,
  la comète sort du polygone et y est rognée par la couche, comme le
  `clip-path` du parent dans le DOM.
- **Horloge** : `pts_us` de la boucle moteur (`temps_s = pts_us / 1e6`), jamais
  l'heure système. Le cadre de la **carte alerte** anime aussi (même gabarit).
- **Implémentation** : `comete_points` (pur, testé : périmètre, bouclage,
  périodicité), `geometrie_ouverte` (figure `END_OPEN`), `style_comete`
  (bouts ronds, créé une fois dans `DessinateurTexte`).
- **Coût** : une géométrie créée par frame et par cadre animé — non mesuré ;
  à optimiser (cache par phase quantifiée) si l'encodeur cesse d'être le
  facteur limitant.
- **Limites** : les 7 styles non portés n'ont toujours pas de cadre (lot C2) ;
  l'animation de `carre`/`arrondis` arrivera avec leur rendu.

---

## 49. Cadres `carre` et `arrondis` (lot C2a, 2026-11-01)

Deux styles de plus rendus, miroirs de `renderCadreInline` et
`genererClipPathCadre` (`diffusion.html` / `registre.ts`).

- **Rendu** : un seul trait au dégradé `stopsLiseres` sur la boîte
  (`rect x=0 y=0 W×H`), épaisseur `ep`, centré sur le bord (moitié externe
  rognée par la couche). `arrondis` = rectangle arrondi `rx = 8` (borné à
  W/2 · H/2, comme le SVG) ; `carre` = rectangle.
- **Découpe** : `arrondis` → rectangle arrondi `r = min(8, W/4, H/4)` ;
  `carre` → **aucun clip** (le polygone est le rectangle, neutre).
- **Comète** : `trace_comete` suit `rectPath` du SVG — `carre` = rectangle,
  `arrondis` = rectangle arrondi **rx = 8 non borné** (la rognure vient du
  clip `min(8, W/4, H/4)`, exactement comme le DOM).
- **Vérification** : `cargo test -p sos-moteur-natif`
  (`carre_et_arrondis_sont_portes`, `trace_comete_carre_et_arrondis`) —
  points du clip identiques au TS (54 points, premier point `(8, 0)` pour
  100×40 ; `(5, 0)` pour 20×20). 209 tests, clippy 0.
- **Reste (C2b)** : aucun — les 9 styles sont portés (§49 à §53).

---

## 50. Cadre `tribal` (lot C2b-1, 2026-11-01)

Miroir de `renderTribal` + `tribalPath` + `motifBord` (`diffusion.html`
8727-8785, 8649-8725). Scène réelle de référence : `BAE - Feet01`
(`93885a83`, `tribal`, trait 13, animation activée).

- **Bord** : octogone à coins adoucis `tribalPath` — coins `cx = max(12,
  round(3·ep))`, `cy = max(9, round(2.4·ep))`, bornés à `W/4`/`H/4`.
  C'est aussi le **clip** du style.
- **4 traits** : halo `rgba(6,6,6,0.5)` à `3.2·ep`, principal à 22 % `2.4·ep`,
  dégradé `ep`, filet clair `0.2·ep` à 45 % — miter, clippés au bord.
- **Motif** (`motifBord`) : par bord, une pointe centrale + 1 ou 2 griffes
  par côté + balayures (si le bord est assez long), **remplis** au dégradé.
  Bords trop courts : rien (condition du DOM).
- **Équerres** de visée aux 4 coins (trait clair, bouts ronds), seulement si
  `W > 130` et `H > 90`.
- **Animations** : respiration du groupe `opacity 0.85 → 1 → 0.85` sur 4 s
  (alpha du dégradé), et **nœud voyageur** (halo clair + point blanc) le long
  du bord sur 5,5 s — dessiné dans la couche, donc rogné au bord comme le
  `clip-path` du parent (seule la moitié intérieure du nœud est visible).
- **Approximations assumées** : coins du bord aplatis (12 pas/quadratique,
  comme le clip) ; le dégradé est exprimé sur le rect du cadre, pas sur le
  viewBox élargi de la marge (écart de quelques pour cent sur les extrémités).
- **Vérification** : `tribal_porte_et_suit_le_path`,
  `motif_tribal_suit_la_longueur_du_bord`, `equerre_tribal_est_un_trait_ouvert`
  — 212 tests, clippy 0.

---

## 51. Cadre `cyberpunk` (lot C2b-2, 2026-11-01)

Miroir de `renderCyberpunk` + `getCyberpunkClipPath` (`diffusion.html`
8024-8120). Scène réelle : `BAE - Grok01` (`9ac9d50a`, trait 20).

- **Bord** : octogone à biseaux droits, `bevel = max(8, round(3.5·ep))` borné
  à `W/4`/`H/4` — c'est le clip.
- **4 traits** : néon `couleurFin` à 22 % (`2.6·ep`), dégradé
  `stopsLiseres(couleur, couleurFin)` (`ep`), **liseré intérieur** à trois
  stops (blanc 0.9 → néon 0.6 → blanc 0.9, opacité 0.85, `0.35·ep`), filet
  blanc 40 % (`0.2·ep`).
- **Ornements de marge non portés** : équerres de coin, circuits, code-barres
  et pastilles sont dessinés **hors** du bord dans le DOM → rognés par le
  `clip-path` du parent, donc **invisibles dans la référence** (même leçon
  que mgstyle, §35.4). Les porter aurait produit du code sans effet visible.
- **Animation** : runner lumineux (`animateMotion dur="5s"` sur le bord) —
  halo néon + point blanc, groupe `opacity 0.95`, dans la couche (rogné au
  bord comme le parent).
- **Vérification** : `cyberpunk_porte_et_suit_le_biseau` (biseau 70 pour
  trait 20 en 400×300, borné à 10 en 100×40) — 213 tests, clippy 0.

---

## 52. Cadre `story` (lot C2b-3, 2026-11-01)

Miroir de `renderStory` + `getStoryClipPath` (`diffusion.html` 8125-8265).

- **Bord festonné** en **cubiques** : `s = max(12, round(4·ep))` borné à
  `W/4`/`H/4`. L'aplatissement cubique (12 pas, coefficients
  `svgPathEnPoints`) est partagé par le clip et le tracé.
- **3 traits** (jointure ronde) : néon 20 % (`2·ep`), dégradé
  `stopsLiseres(profond, vif)` (`ep`), liseré intérieur blanc/pâle/blanc
  (`0.3·ep`, opacité 0.7).
- **Cœurs** : 4 cœurs de coin (`max(6, 1.5·ep)`, blanc 0.92) + petits cœurs
  répartis sur les bords (`max(3, 0.7·ep)`, groupe 0.75, un tous les ~70 px).
- **Volutes de coin** : double cubique dans la marge + point final — rognées
  au bord comme le `clip-path` du parent (visibles seulement pour la partie
  intérieure).
- **Animation** : étincelle 4-pointes voyageuse (`animateMotion dur="7s"` sur
  le bord, groupe 0.95). Les 3 étincelles de marge du DOM sont **hors
  polygone** → invisibles dans la référence, non portées.
- **Vérification** : `story_porte_et_suit_le_feston` (102 points, `s=75` en
  400×300, `s=10` en 100×40), `formes_story_ont_le_bon_nombre_de_points` —
  215 tests, clippy 0.

---

## 53. Cadres `hexagon` et `arena` (lot C2b-4, 2026-11-01)

Derniers styles du registre — **parité cadres complète (9/9)**.

### `hexagon`

- **Bord** octogonal : `s = max(12, round(4·ep))` borné à `W/4`/`H/4`
  (`getHexagonClipPath`).
- **3 traits** (miter) : principal 20 % (`2.2·ep`), dégradé (`ep`), filet
  clair 60 % (`0.3·ep`).
- **Nœuds** aux 8 sommets (`max(1.2, 0.3·ep)`, groupe 0.85) — sur le bord,
  donc rognés à moitié comme le parent.
- **Cadre intérieur** inséré : `translate(0.6·ep)` +
  `scale(1 − 1.2·ep/W, 1 − 1.2·ep/H)`, filet clair 40 %.
- **Animation** : nœud voyageur (`animateMotion dur="6s"`).
- **Non porté** : grappes d'alvéoles rotatives et glyphes de marge — **hors
  polygone**, invisibles dans la référence.

### `arena`

- **Bord** octogonal allongé : `bx = max(16, round(4.2·ep))`,
  `by = max(10, round(2.4·ep))`, bornés à `W/4`/`H/4`.
- **4 traits** : canal sombre `rgba(6,10,4,0.55)` (`3.4·ep`), principal 28 %
  (`2.6·ep`), dégradé (`ep`), filet clair 50 % (`0.2·ep`).
- **Écho intérieur** : octogone inséré de `d = max(2.5, 1.1·ep)`, filet clair
  75 %.
- **Nœuds** aux 8 sommets (`max(1.3, 0.35·ep)`).
- **Cartouches** (plaque hexagonale + barre + 2 points) à `W·0.72` en haut et
  `W/2` en bas ; **ailerons** cerf-volant à gauche/droite — seulement si
  `W > 4·cw` et `H > 4·wingT`.
- **4 orbs** aux biseaux : anneau sombre + contour clair, disque en
  **dégradé radial** (`clair → profond`, centre décalé 35 %/30 %), croix X
  blanche (2 quadratiques). Groupe animé `opacity 1 → 0.55 → 1` (3,5 s).
- **Animation** : nœud voyageur (`animateMotion dur="5s"`).
- **Vérification** : `hexagon_porte_et_suit_les_pans` (`s=75` en 400×300,
  `s=10` en 100×40), `arena_porte_et_suit_les_biseaux` (`bx=75`, `by=48` en
  300×300) — 217 tests, clippy 0.

---

## 54. Correctif — carrousel d'un widget sans `media` (2026-11-01)

**Symptôme** : un widget qui n'a **que** `mediaSlides` (aucun champ `media`)
n'affichait aucun carrousel en mode Rust, alors que le DOM l'affiche.

**Cause** : `medias_depuis_scene` n'incluait un widget que si `w.media` était
`Some` — la liste `mediaSlides` (pourtant prioritaire, contrat
`carrousel.md`) n'était jamais lue pour un widget sans média unique.

**Correctif** : la source du widget est
`mediaSlides.first()` **sinon** `media` non vide — même priorité que le fond
(`bgSlides` > `bgMedia`). Un widget sans média mais avec des slides produit
donc un `MediaCfg` dont le `carrousel` porte toutes les slides ; le `chemin`
d'identité reste la première slide (le diff à chaud ne recharge pas quand la
slide courante change).

**Vérification** : `widget_avec_slides_sans_media_produit_un_carrousel` —
218 tests, clippy 0.

---

## 55. Anneau caméra `cameraBord` (lot D2, 2026-11-01)

Miroir de `svgAnneauCamera` + `gradientCadre` (`diffusion.html` 8885,
`registre.ts` 151). Uniquement pour une caméra à **forme dédiée**
(`cercle`/`carre`) : sans forme dédiée, c'est le cadre de scène qui
s'applique (D1), il n'y a pas d'anneau.

- **Géométrie** : cercle inscrit (`min(W,H)/2`) ou rectangle du widget.
- **Trait** : `clamp(strokeWidth, 1.5, 12)` — un cadre de scène épais ne doit
  pas engloutir un cercle caméra.
- **Dégradé** : `stopsLiseres(couleur, couleurFin)` du cadre de scène,
  **diagonal TL→BR** (le SVG fixe `x1 0% y1 0% x2 100% y2 100%`, il n'utilise
  PAS `gradientAngle`). Aucun cadre actif → `#e0e0e0 → #9a9a9a`, épaisseur 4.
- **Rognure** : la moitié extérieure du trait est invisible — couche à masque
  géométrique (cercle) ou clip axe-aligné (rect), comme le `clip-path` du
  parent.
- **Données** : `SourceCfg.anneau: Option<AnneauCamera>` (`cadres.rs`), posé
  par `anneau_camera_de` (`scene_sources.rs`) ; dessiné après la capture dans
  la même entrée de plan (`VoieRendu::dessiner_anneau_camera`).
- **Vérification** : `camera_bord_produit_un_anneau_sur_forme_dediee`
  (couleurs du cadre, fallback, pas d'anneau sans bord ni sans forme dédiée) —
  219 tests, clippy 0.

---

## 56. Caméra — exclusivité dshow et hot-plug (lot D3, 2026-11-01)

### Bascule OBS ↔ Rust (frontend)

En mode OBS, `SOS-Caméra` (dshow) tient le capteur en exclusif. Après une
bascule **acceptée** vers le Rust, `basculerMoteur` appelle
`camera_sync(visible = false)` : les items sont désactivés (jamais
`RemoveInput`, contrat figé), le capteur est libéré, la source et ses
réglages restent. Le retour en mode OBS appelle `syncCapturesOBS()` qui
réactive caméra + trous de la scène courante. La libération n'a lieu
**qu'après** une bascule acceptée — un refus (live en cours) ne coupe jamais
la caméra OBS.

### Hot-plug natif

- `CaptureCamera::est_vivante()` : le fil MF tourne encore ? Un
  débranchement laisse la dernière frame figée.
- `SceneVivante::appliquer_captures` ne réutilise plus une session morte
  (même nom + même cible) : elle est **recréée** à la prochaine application
  de scène (mutation, changement de layout, réouverture) — auto-guérison.
- La boucle signale une caméra débranchée **une fois par seconde**
  (`caméra « … » débranchée : dernière image figée`), jamais un gel muet.

**Reste assumé** : la reconnexion **sans** mutation de scène n'est pas
instrumentée (le rebranchement seul ne relance pas la session) ; le prochain
lot « robustesse » (I) peut ajouter une relance périodique si le besoin est
réel.

**Vérification** : `cargo test -p sos-moteur-natif` (219), clippy 0,
`npm run check` 0 erreur.

---

## 57. Overlay clip de bienvenue (lot E1, 2026-11-01)

Miroir de `playWelcomeClip` / `stopWelcomeClip` / `applyWelcomeOverlayConfig`
(`diffusion.html` 10389-10610) : carte transitoire `z = 9999` (§9.1).

**Canal** : frames `chat_tx` — `welcome-clip-play`, `welcome-clip-stop`,
`welcome-clip-config` (+ `position-overlay-config`, **partagé** avec les
alertes : le pont le pousse aux deux états, comme `appliquerSnapshot`).
⚠️ `welcome:etat` (event Tauri) est l'état **dashboard** (`QueueEtat`) : il ne
porte ni `favoris` ni l'URL MP4 résolue au bon moment — ce n'est jamais le
canal du rendu.

| Élément | Squelette (`utiliser_squelette: true`) | Spotlight |
|---|---|---|
| Carte | x/y/largeur du squelette, hauteur auto | 85 % de la largeur canvas, centrée |
| Fond | `rgba(10,10,10,0.92)`, radius 12 | `rgba(12,14,22,0.55)`, bord 1 px `rgba(255,255,255,0.14)`, radius 18 |
| Padding / gaps | 1,5 / 0,5 cqw | 3 / 1,2 / 2,5 cqw |
| Pseudo | `clamp(10,4 ; 3,5cqw ; 17,6)`, gras | `clamp(16 ; 3cqw ; 35,2)` |
| CTA « Checkout and follow » | `clamp(8 ; 2,2cqw ; 11,52)`, italique, α 0,6 | `clamp(11,2 ; 1,6cqw ; 16)` |
| Bio | 2 lignes, `clamp(8,8 ; 2,5cqw ; 13,12)`, α 0,85 | 4 lignes, `clamp(12,8 ; 1,8cqw ; 19,2)` |
| Message | `display:none` | `clamp(12,8 ; 2,2cqw ; 22,4)`, italique, α 0,9 |
| Titre « Streamer Twitch » | `display:none` | `clamp(14,4 ; 2,6cqw ; 27,2)`, `letter-spacing .38em`, dégradé 3 stops + barre 2 px |
| Cadre de scène | appliqué à la carte mesurée (`appliquerCadreOverlay`) | **non** (`nettoyerCadreOverlay`) |

**Phases spotlight** (horloge du play) : voile noir plein canvas `ease` 350 ms →
carte `opacity 550 ms ease` + `translateY(26px) scale(0.94) → 1` en
`cubic-bezier(.16,1,.3,1)`, retardées de 120 ms → **intro 3 s** (textes et
colonne droite masqués, avatar seul) → split `flex-basis 100 % → 42 %` en
600 ms. `welcome-clip-stop` coupe tout sans fondu (`display:none` du DOM).

**Limites assumées** (parité partielle — le lot G est requis pour les lever) :

- **aucun média** : `clip.mp4_url`, `viewer.avatar`, `viewer.banniere` et les
  avatars des `favoris` sont des **URLs distantes** ; le moteur ne charge que
  des fichiers locaux (`media::decoder_pbgra`,
  `MFCreateSourceReaderFromURL`) → avatar absent, colonne droite (clip 16:9)
  jamais affichée (elle attend `canplay`), fond spotlight noir, rangée des
  favoris vide — exactement ce que le DOM montre **sans** média ;
- **aucun son** (l'`<video>` vivait dans le navigateur OBS) ;
- `backdrop-filter`, `box-shadow`, `drop-shadow` et `line-height` CSS non
  portés.

**Vérification** : `cargo test -p sos-moteur-natif` (228 tests) +
`cargo test -p streamos-v0 --lib` (223 tests), `cargo clippy` 0,
`npm run check` 0 erreur.

---

## 58. Reste à faire — parité (état au 2026-11-01)

**Acquis** : 9/9 styles de cadres (§49-§53), animations (§48), fond capture
natif (§47), carrousels (§54), caméra forme/anneau/exclusivité/hot-plug
(§55-§56), toutes les scènes acceptées (lot A), overlay clip de bienvenue
(§57, lot E1), images et composition du chat (§59, lot G), bandeau premier
message (§60, lot E2), écran d'attente BRB (§61, lot E3), pad numérique
(§62, lot E4), Emoji Arena (§63, lot E5), présentation streamer — décor et
rideaux (§64, lot E6a), carte (§65, lot E6b), clips (§66, lot E6c) et audio
(§67, lot E6d) — **lot E6 complet** — et pop de validation des splits
(§68, lot E7), Just Chatting (§69, lot F), puis **lot H complet** : titres
dans l'ordre z (§70), contenus de widget + titres sous perspective (§71),
`line-height` (§72) et contour exact (§73).

### Lot E — overlays (en cours), **un overlay = un diff**

| # | Overlay | Canal d'état | z (§9.1) |
|---|---|---|---|
| E1 | Clip de bienvenue | `chat_tx` `welcome-clip-*` + `position-overlay-config` | 9999 |
| E2 | Bandeau 1er message | `chat_tx` `bandeau-premier-message-*` | 9998 |
| E3 | Écran d'attente BRB | `chat_tx` **`ecran-attente-etat`** (+ `muter_bureau` ajouté à la frame) | 9990 |
| E4 | Pad numérique | `chat_tx` **`pad-play`/`pad-stop`** + `position-overlay-config` (⚠️ `pad:touche*` = dashboard) | 9995 |
| E5 | Emoji Arena | `chat_tx` **`fight-state`** (⚠️ `fight:update` = dashboard) | 9993 |
| E6a | Présentation — décor + rideaux (**fait**) | `chat_tx` **`presentation-show`/`-hide`/`-mode`** | 10000, 10001, 10003 |
| E6b | Présentation — carte (**fait**) | idem (même frame, données déjà décodées) | 10002 |
| E6c | Présentation — clips (**fait**) | `chat_tx` `-clip-play`/`-clip-stop` | 10004 |
| E6d | Présentation — audio (**fait**) | `chat_tx` `-audio-play`/`-audio-stop` | — |
| E7 | Pop de validation des splits (**fait**) | dérivé de `chat_tx` **`speedrun-etat`** + `splitpop-pos` (squelette) | 9993 |

Méthode : décodeur + abonnement dans `src/moteur/mod.rs` (pattern
`decoder_frame_alerte`), `pousser_<overlay>` dans `moteur.rs`, renderer
`texte/<overlay>.rs`, entrée de plan au z ci-dessus. Vérifier le canal réel
avant de coder — jamais supposer (E1 a montré que `welcome:etat` n'est PAS le
canal du rendu).

**Diagnostic E6 (fait le 2026-11-01, canal vérifié dans `presentation.rs`)** :
le rendu passe par les frames `chat_tx` `presentation-show` (payload `mode` +
`donnees` = `PresentationDonnees`), `presentation-hide`, `presentation-mode`,
`presentation-clip-play`/`-clip-stop`, `presentation-audio-play`/`-audio-stop`.
⚠️ `presentation:etat` est l'event **dashboard**. **Lot E6 COMPLET** :
**E6a** (§64), **E6b** (§65), **E6c** (§66) et **E6d** (§67).

**Décision utilisateur (2026-11-01), appliquée en E6c** : `mp4_url` est une
URL **signée distante** → **préchargement disque côté app**
(`medias/clips/<fnv16(url)>.mp4`), le moteur ne lit qu'un fichier local
(§13.8). L'hôte `clips-media-assets2.twitch.tv` est documenté dans le CONTRAT
FIGÉ vie privée d'`AGENTS.md`.

### Lots suivants

- **F — Just Chatting (fait, §69)** : la scène dérivée (`sceneDerivee`) part
  au moteur natif comme à OBS, par `moteur_appliquer_scene`.
- **G — Chat images (fait, §59)** : badges emoji, logos plateforme, avatars et
  emotes inline, bordure par message. Images = cache disque côté app.
- **H — Finitions** : **lot H complet** — H1 (§70), H2/H2b/H2c (§71), H3
  (§72) et H4 (§73).
- **I — Robustesse** : canvas différent en cours de sortie, `hwnd` périmé
  (re-résolution titre/exe), PlayerBar/seek, `capture_jeu`, relance caméra
  sans mutation de scène.
- **J — Fond vert sans OBS** (plus tard) : solution maison, remplace le plugin
  OBS `background_removal`.

---

## 59. Chat — badges, logos, avatars et emotes (lot G, 2026-11-01)

Miroir de `renderChatMsg` / `renderTexte` / `renderBadges` / `creerNoeudChat`
(`diffusion.html` 4180-4279). Composition d'un message, dans l'ordre du DOM :
**badges emoji** → **logo plateforme** → **pseudo** (gras + couleur IRCv3) →
`:` (opacité 0,7) → **texte** (emotes en images inline), plus l'**avatar** rond
à gauche.

| Élément | Rendu natif |
|---|---|
| Badges | Emoji (mod, vip, sub, broadcaster) à `0,9em` — **aucune image** |
| Logo | Twitch/Kick : glyphe vectoriel inline 12×12 (`goal.rs::logo_plateforme`, couleurs du DOM) ; YouTube/TikTok : `YT`/`TT` en gras coloré (sans la pastille pleine) |
| Avatar | Bitmap 1,8 em, découpe circulaire, anneau 1 px de la couleur du cadre |
| Emotes | Bitmap inline `height:1.5em`, largeur au ratio, `vertical-align:middle` |
| Bordure | 1 px, **teinte médiane** du dégradé `gradientCadre` (le `border-image` 135° exact coûterait une brosse par message et par frame pour un écart invisible à 1 px) |
| Zebra / fond | inchangés (`rgba(255,255,255,0.07)` / noir, bulles = fond uniforme) |

**Objets inline DirectWrite** — `ID2D1RenderTarget::DrawTextLayout` n'appelle
**jamais** `IDWriteInlineObject::Draw` (le `clientDrawingContext` transmis
n'est pas la cible : vérifié par test GPU). Le chat dessine donc ses layouts
avec un `IDWriteTextRenderer` maison (`RenduChat`), qui relaie les
`DrawGlyphRun` (brosse = `SetDrawingEffect`, sinon blanc) et **propage sa
cible** aux objets inline.

**Images : cache disque côté app** (décision utilisateur 2026-11-01)

- L'app télécharge (`cache_images.rs`, reqwest) vers
  `<app_data>\medias\cache\<fnv16(url)>` — **sans extension** (WIC détecte
  le format au contenu : PNG/GIF/WebP vérifiés).
- Le nom est **dérivé de l'URL** (FNV-1a 64 bits) : aucune modification de la
  frame `chat` n'a été nécessaire. Miroir verrouillé par le même vecteur de
  test des deux côtés (`cache_images::empreinte` ↔ `chat::empreinte_url`).
- **À la volée** : au premier affichage l'image manque (l'emote reste en
  texte, l'avatar réserve sa boîte), le GET part en tâche de fond ; les
  occurrences suivantes sont locales. Aucun blocage du chat, aucune rafale au
  démarrage.
- Le moteur ne connaît **pas le réseau** (invariant §13.8) : il ne lit qu'un
  fichier local, et retente à chaque reconstruction de bloc (un fichier
  arrivé entre-temps apparaît au message suivant).
- Cache borné : au-delà de 1 024 fichiers, les plus anciens sont purgés au
  démarrage.

**Limites assumées** : `backdrop-filter` (verre) et `box-shadow` non portés ;
cadre SVG **par bulle** (`decorerCadreMsg`) non porté (la bulle garde sa
bordure unie) ; pastille pleine YouTube/TikTok simplifiée en texte coloré ;
`border-image` remplacé par une teinte médiane.

**Vérification** : `cargo test -p sos-moteur-natif` (233, dont le test GPU
`objet_inline_est_dessine_par_draw_text_layout`), `cargo test -p streamos-v0
--lib` (228), clippy 0, `npm run check` 0.

---

## 60. Bandeau premier message (lot E2, 2026-11-01)

Miroir de `playBandeauPM` / `stopBandeauPM` / `applyBandeauPMConfig` /
`bpmResoudreCouleurs` (`diffusion.html` 11189-11420) : barre pleine largeur
`z = 9998`, en bas (défaut) ou en haut.

**Canal** : frames `chat_tx` `bandeau-premier-message-play` / `-stop` /
`-config`. ⚠️ `bandeau:etat` (event Tauri) est l'état **dashboard** — jamais
le canal du rendu (même leçon qu'E1).

| Élément | Valeur (DOM → natif) |
|---|---|
| Fond | dégradé 90° `rgba(10,10,10,.97)` → `assombrir(primaire, 55 %)` → noir |
| Filet | 2 px du bord intérieur, **teinte médiane** `primaire → secondaire` |
| Ligne | 2 px, `transparent → prim 85 % → sec 95 % → prim 85 % → transparent` |
| Pastille | `padding 5×14`, bord 1 px, `✦` 16 px + « PREMIER MESSAGE » 11 px gras, `letter-spacing .14em` |
| Avatar | 44 px rond, bord 2 px à 60 % de la primaire (cache disque, lot G) |
| Textes | pseudo 15 px gras (primaire), message 20 px blanc, une ligne tronquée |
| Padding | `16px 300px 16px 40px`, `gap 18px` — la réserve de 300 px est celle de la mascotte |
| Entrée | `opacity 0 → 1` (0,45 s `ease`) + `translateY(±100 %) → 0` (0,5 s `cubic-bezier(.16,1.08,.38,.98)`) |
| Sortie | transitions inverses, masquage après 550 ms |

**Couleurs** (`resoudre_couleurs`) : cadre actif → l'extrémité la plus
lumineuse (rec.601) devient la primaire, l'autre la secondaire ; `couleur`
seule → secondaire assombrie de 25 % ; aucun cadre → `#e0e0e0 / #a0a0a0`.

**Limites assumées** : la **mascotte Streamy** n'est pas portée (asset de
marque servi par `/brand/streamy-welcome.png`, hors du crate) — sa réserve de
300 px est conservée pour que le texte se coupe au même endroit ;
`box-shadow` et `text-shadow` non portés ; `border-image` 135° remplacé par
une teinte médiane.

**Vérification** : `cargo test -p sos-moteur-natif` (238),
`cargo test -p streamos-v0 --lib` (230), clippy 0, `npm run check` 0.

---

## 61. Écran d'attente BRB (lot E3, 2026-11-01)

Miroir de `initEcranAttenteOverlay` / `majEcranAttente` / `eaCacher`
(`diffusion.html` 13009-13318) : overlay **plein écran** `z = 9990`, au-dessus
de la scène mais **sous** pad (9995), alertes (9997), bandeau (9998) et
bienvenue (9999) — comportement validé du DOM.

**Canal** : frames `chat_tx` de type **`ecran-attente-etat`** (payload `etat`),
ré-émises à chaque tick 1 Hz du chrono Rust (source de vérité). La frame est
**auto-réparatrice** : le moteur remplace son état complet à chaque frame, il
ne tient aucune horloge métier. ⚠️ `ecran-attente:etat` (event Tauri) est
l'état **dashboard** (modale + pillule) — jamais le canal du rendu (leçon E1).
`muter_bureau` a été **ajouté à la frame** (1 ligne dans `ecran_attente.rs`,
champ ignoré par le DOM) : sans lui, le moteur ne pouvait pas savoir s'il doit
muter sa source loopback.

| Élément | Valeur (DOM → natif) |
|---|---|
| Racine | `#05050a` opaque, `overflow: hidden`, `opacity 0 → 1` (0,45 s `ease`) |
| Aurore | `linear-gradient(160deg, #06060c, #0b0b16 55 %, #06060c)` + 2 radiaux (`prim` 20 % à 20 %/18 %, `sec` 16 % à 80 %/78 %) + halo `::before` dérivant (22 s `alternate`, inset −18 %) |
| Média | `medias/…` local, plein cadre `cover`, vidéo **en boucle** ; présent = aurore masquée |
| Voile | `linear-gradient(180deg, noir 45 %, transparent 30 %, transparent 68 %, noir 50 %)` |
| Titre | 84 px poids 800, `letter-spacing .04em`, `line-height 1.1`, centré, `top 7 %`, padding 0 5 %, dégradé 135° `prim → sec` sur la **boîte de l'élément** |
| Carte | `padding 24×64`, `gap 8`, `border 2 px`, `radius 14`, `rgba(8,8,14,.88)` ; centrée sur `minuteur_x/y`, `scale(minuteur_echelle)` (0,4-3,0) |
| Label | 20 px poids 600, `#bdbdbd`, `letter-spacing .22em`, `text-transform: uppercase` (« DÉBUT DANS » / « EN PAUSE ») |
| Chrono | 84 px poids 800, `line-height 1`, `M:SS` ou `H:MM:SS`, dégradé 135° `color-mix(prim 82 %, blanc) → color-mix(sec 60 %, blanc)` ; pause = α 0,6 |
| Cadre de scène | appliqué à la **carte mesurée** (`appliquerCadreOverlay`), trait à la taille non mise à l'échelle, **masque** au polygone à la taille écran |
| Stries | 4 barres 2 px × 34 vmin aux coins (`rotate ±45°`), dégradé cyan→violet, ligne sœur 1 px, comète 4 px × 18 % sur 6 s (retards 0/1,5/3/4,5 s) |
| Streamy | PNG embarqué, `contain` dans 300×430, flottement ±9 px / 5,5 s ; pause → `streamy-offline` |
| Orbites | anneau 2 px (`118 %` de la largeur) + 2 arcs rotatifs (9 s / 13 s inversé) |
| Wordmark | PNG embarqué, 480 px de large, α 0,9, centré sur `logo_x/y`, `scale(logo_echelle)` |
| Morse | 9 segments (6 px / 22 px, haut 6, gap 9) à `bottom 1.7 %`, flashs 9,5 s, retards 0/0,7/1,4/2,4/4/5,6/7,2/7,9/8,6 s |

**Phases** : `actif: true` → fondu d'entrée 0,45 s ; `actif: false` → fondu de
sortie 0,45 s puis libération du média et du bloc (le DOM retire le `<video>`
après 500 ms). Une frame identique ne change pas la version : la carte n'est
re-mise en page qu'au tick du chrono (1 Hz) ou au changement de scène.

**Média et audio**

- Chargé par `media::charger` sur le **même device** que la scène
  (`ContexteGpu::depuis_device`), rechargé seulement si `media|kind` change
  (`eaMediaSig` du DOM). Vidéo : `LectureCfg { paused: false, boucle: None }`
  = `v.loop`, et **recalage** (`MediaChargee::recaler`) : un média chargé en
  cours de session démarrerait sinon à `temps_s` (fin de flux).
- Le média est avancé par la **composition** (une frame d'avance), pas par la
  boucle : il n'est pas dans le plan des médias de scène.
- **Son** : l'anneau de la vidéo rejoint le mixeur en **queue** d'entrées
  (`nb_entrees_scene`), gain = `son ? volume × 1000 : 0` (miroir `muted = !son`
  + `volume` du DOM), reconstruit à chaque frame.
- **Mute « bureau »** : `muter_bureau` met à **0 le gain de l'entrée loopback**
  du mixeur natif (`index_loopback`, mémorisé à la création des sources) —
  c'est l'équivalent exact du `SetSceneItemEnabled false` posé par le mode OBS
  sur les sources `desktop|bureau`. Rétabli dès `actif: false` (comme
  `restaurer_audio`), jamais pendant le fondu de sortie.

**Assets de marque** : `src/assets/streamy/streamy-{loading,offline}.png` et
`src/assets/logo/LogoSOSDashboardSmallSize.png` sont **embarqués**
(`include_bytes!`, mêmes fichiers que `GET /brand/{name}` de l'app) et décodés
depuis la mémoire (`media::decoder_pbgra_memoire`) — zéro écriture disque,
zéro dérive d'asset.

**Limites assumées**

- `box-shadow` / `drop-shadow` / `text-shadow` non portés (halo des stries,
  glow des orbites, ombre de la carte, lueur du titre et des chiffres, halo de
  la mascotte) ;
- arcs orbitaux : `conic-gradient` + `mask` annulaire rendus en **trait d'arc
  uni** (une brosse Direct2D n'a pas de dégradé angulaire) ;
- wordmark : `mix-blend-mode: screen` approché par une **alpha dérivée de la
  luminance** (noir → transparent, pic lumineux → opaque) — additif sur fond
  sombre, sans effet de bord ;
- `font-variant-numeric: tabular-nums` non appliqué (chiffres proportionnels) ;
- média illisible → aurore seule (une tentative par signature, journalisée en
  `debug`), comme un `<img>` cassé du DOM.

**Écart de parité constaté (hors périmètre E3, à arbitrer)** : le **bord du
canvas** de la scène porte `z = i32::MAX` dans le crate (`cadres_depuis_scene`)
alors que `#cadre-app-overlay` du DOM est à `z-index: 100` — il est donc peint
**au-dessus** de l'écran d'attente (et de tout overlay ≥ 9990), là où le DOM le
laisse dessous. Invisible avec les cartes E1/E2 (elles ne touchent pas le bord),
visible ici en plein écran. Deux options : ramener le bord sous `Z_ATTENTE`
(côté crate) ou aligner le z du DOM — **non tranché**, signalé.

**Vérification** : `cargo test -p sos-moteur-natif` (248),
`cargo test -p streamos-v0 --lib` (233), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle : rendu de l'overlay seul
(harnais temporaire, supprimé) en 1280×720 — layout `signature` et `minimal`
conformes (aurore, titre dégradé, carte minuteur + cadre de scène, stries,
Streamy + orbites, wordmark, morse).

---

## 62. Pad numérique (lot E4, 2026-11-01)

Miroir de `initPadOverlay` / `applyPadOverlayConfig` / `playPad` /
`stopPad` (`diffusion.html` 12203-12411) : carte média transitoire
`z = 9995` — sous commandes (9996), alertes (9997), bandeau (9998) et
bienvenue (9999), au-dessus de l'écran d'attente (9990).

**Canal** : frames `chat_tx` **`pad-play`** (payload `pad`) et **`pad-stop`**,
plus la frame **partagée** `position-overlay-config` (squelette unifié
welcome + alertes + commandes + pad). ⚠️ Les events Tauri `pad:touche` /
`pad:touche-stop` sont l'état **dashboard** (audio local + retour UI) : ce
n'est jamais le canal du rendu (le plan les annonçait — faux, comme E1/E2/E3).

| Élément | Valeur (DOM → natif) |
|---|---|
| Racine | `(x, y)` + `largeur` du squelette, hauteur déduite du média ; `opacity 0 → 1` (0,3 s `ease`) et `scale(0.6) → 1` (0,3 s `cubic-bezier(.16,1.08,.38,.98)`) autour du centre |
| Carte | largeur du squelette, hauteur = `largeur × ratio`, bornée à **80 %** de la hauteur canvas (`max-height: 80vh`) |
| Média | `object-fit: contain`, `border-radius: 8px` (géométrie arrondie en masque de couche) ; image **ou** vidéo jouée **une fois** |
| Effets | `bounce` `translateY(0 → −20 px)` 0,6 s `ease-in-out` ; `zoom` `scale(1 → 1,15)` 1,2 s — sur le média, jamais en plein écran |
| Plein écran | racine = canvas, média `contain` 100 %×100 %, sans radius, **sans cadre**, `transform: none` ; `transparence` → opacité `transparence_opacite` sur le média seul |
| Cadre de scène | appliqué à la carte mesurée (`appliquerCadreOverlay`), jamais en plein écran ; le polygone sert aussi de `clip-path` |
| Sortie | `pad-stop` → transitions inverses 0,3 s, masquage à 500 ms (le DOM retire le `<video>`) |

**Son** (miroir de `playPad`) : vidéo → son porté par la vidéo
(`MediaChargee::anneau_audio`) ; image → fichier `son` séparé ; touche
`audio` → le **média est le son**, aucun visuel. Volume = `volume` de la
touche, **coupé dès le `pad-stop`** (le DOM met l'audio en pause).

**Nouveau : lecture d'un fichier audio seul** (`media::AudioFichier`) — même
Source Reader que les vidéos (flux audio uniquement, f32 48 kHz stéréo par le
resampler MF), horloge identique (`avancer(temps_us)` + `decalage_us`). Le
pompage des échantillons est factorisé (`media::pomper_audio`) et partagé avec
`MediaChargee::avancer_audio`.

**`LectureCfg::boucle_infinie`** (nouveau) : `true` = `v.loop` implicite des
médias de scène (comportement historique) ; `false` = **une seule lecture**,
dernière image figée en fin de flux — c'est ce que fait un `<video>` du pad
(aucun attribut `loop`). Sans ce champ, la vidéo d'une touche bouclait
silencieusement.

**Limites assumées** : `max-height: 80vh` mesuré sur la hauteur du **canvas**
(le viewport du DOM) ; pas de `box-shadow` (aucun texte dans cet overlay) ;
une image illisible → rien n'est affiché (une tentative par signature,
journalisée en `debug`), comme un `<img>` cassé.

**Reste hors lot (à planifier)** : le **chrono 2** du même module
(`chrono2-etat` + `chrono2-pos`, squelette dédié) n'est pas porté — c'est un
overlay distinct, à traiter comme un lot à part entière.

**Vérification** : `cargo test -p sos-moteur-natif` (253),
`cargo test -p streamos-v0 --lib` (236), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé) :
carte image avec cadre de scène + mode plein écran transparent.

---

## 63. Emoji Arena `!fight` (lot E5, 2026-11-01)

Miroir de `initFightOverlay` / `majFightState` / `fightTick` /
`fightAnimerCoup` (`diffusion.html` 2133-2550 + 12415-12845) : arène 1v1
`z = 9993` — sous chrono 2 (9994), pad (9995), commandes (9996), alertes
(9997), bandeau (9998) et welcome (9999), au-dessus de l'écran d'attente
(9990).

**Canal** : frames `chat_tx` **`fight-state`** (payload `combat`), snapshot
COMPLET à chaque transition + resync (`fight-resync`). ⚠️ L'event Tauri
`fight:update` est l'état **dashboard** (modale + pillule) — jamais le canal
du rendu (le plan l'annonçait : faux, comme E1→E4).

| Élément | Valeur (DOM → natif) |
|---|---|
| Zones | `bottom 4%`, largeur 300, `left/right 4%`, colonne centrée |
| PV | en-tête `700 15px` (pseudo / `pv/max`) + barre 220×13, `radius 7`, fond `rgba(0,0,0,.55)`, bord 1 px `rgba(255,255,255,.28)` ; remplissage dégradé 90° vert (> 55 %), ambre (≤ 55 %), rouge (≤ 25 %) |
| Emoji | 128 px, `line-height 1`, zone B **miroir** (`scaleX(-1)`), respiration ±5 px / 2,6 s |
| Nom | `700 20px`, puis plateforme `11px` α 0,7 (masquée pour `bot`/`local`) |
| Chips | zone active seulement : `⚡ !a rapide`, `💥 !b lourd`, `🩸 !c spécial` (spécial barré α 0,35 après usage), pastilles `padding 6×10`, `radius 999` |
| Badge | « À TOI » (`800 12px`, noir sur `#ffd24b`) au-dessus de la zone active |
| Timers | zone active 220×5 + colonne centrale 210×8, dégradé `#8b5cf6 → #d946ef`, fraction `reste_tour / duree_tour` |
| Centre | pastille « ⚔ Au tour de X · N s » (`700 17px`, `radius 999`, fond `rgba(10,12,20,.6)`) + temps de match `M:SS` (`600 13px`, α 0,8) |
| Bannière | lobby (emoji 72 px + titre + `!accept … ou !refuse · 3 coups · N s/tour · M min — N s`) et fin (🛑 arrêté / ⏹ inactivité / 🏆 gagnant / 🤝 égalité), `radius 16`, `max-width 70 %` |
| Toast | racine séparée, `top 3%`, centré, `600 18px/1.35`, bord coloré `warn`/`ok`/`aide`, fondu 0,18 s, dedup par `seq` + âge vérifié |

**Échéances locales** : la frame ne revient qu'aux transitions — `reste_tour_ms`,
`reste_lobby_ms` et `reste_match_s` sont décomptés depuis l'instant de
réception (miroir du `setInterval(fightTick, 250)`), et le bloc est re-mis en
page quand la seconde affichée change.

**Animations** (miroir de `fightAnimerCoup`) : lunge 0,5 s (40 % → ±90 px,
×1,15), secousse du défenseur 0,4 s à +200 ms, esquive ±46 px si le coup rate,
popup `-N` / `+N` / « raté » / « ⏳ passe… » (1,1 s, `translateY` −16 → −70 px,
×0,6 → ×1,15), impact `.fight-fx` (💥 / 💢 / 🩸 / ✨, 0,5 s), flash d'impact
(voile blanc 0,26 s) et secousse d'écran 0,3 s (coup lourd ou critique), KO du
perdant (`rotate 75°`, `+28 px`, α 0,35). `anims_reduites` coupe les FX, la
secousse d'écran et la respiration — jamais les popups.

**Emojis en couleur — piège vérifié** : `ID2D1RenderTarget::DrawTextLayout`
n'active pas les glyphes COLR/CPAL (les emojis sortaient en blanc). Il faut
`ID2D1DeviceContext::DrawTextLayout` avec
`D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT` — c'est ce que fait
`dessiner_ligne_f`. ⚠️ Et une ligne `centre: true` est centrée dans **sa boîte
de mise en page** : l'origine de dessin doit être `centre − largeur_layout / 2`
(même piège que la carte minuteur d'E3).

**Limites assumées** : `text-shadow` / `drop-shadow` non portés ; le
`grayscale` du KO n'est pas appliqué (rotation + α conservés) ; le flash
d'impact remplace `brightness(2.1)` par un voile blanc ; `tabular-nums` non
appliqué.

**Vérification** : `cargo test -p sos-moteur-natif` (261),
`cargo test -p streamos-v0 --lib` (238), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé) :
combat (PV, chips, badge, timers, pastille de tour), lobby (bannière + compte à
rebours), fin (bannière + KO du perdant) — emojis en couleur.

---

## 64. Présentation streamer — décor et rideaux (lot E6a, 2026-11-01)

Miroir de `initPresentationOverlay` / `playPresentationShow` /
`appliquerModePresentation` / `stopPresentation` (`diffusion.html`
10615-11118) et du bloc CSS `.pres-*` (2600-3142). Overlay **plein écran
autonome** (pas un widget de scène), au-dessus de toute la pile : écran
d'attente 9990 → welcome 9999.

Cette tranche porte le **décor** ; la carte (E6b), le lecteur clip (E6c) et
les 2 pistes audio (E6d) suivent.

**Canal** : frames `chat_tx` **`presentation-show`** (payload `mode` +
`donnees`), **`presentation-hide`** et **`presentation-mode`** (bascule à
chaud). ⚠️ L'event Tauri `presentation:etat` est l'état **dashboard**
(favoris + contrôles live) — jamais le canal du rendu (leçon E1→E5).
`presentation-show` n'arrive **qu'une fois** : toute la chorégraphie est une
horloge **locale** (`debut` / `debut_chore` / `fin`), le moteur ne reçoit
aucun tick.

**Trois couches, trois entrées de plan** — `GenreCalque::Presentation` dont
l'index porte la couche (`COUCHE_VOILE` / `COUCHE_RIDEAUX` / `COUCHE_PUBLIC`),
ce qui laisse la carte `10002` (E6b) et le clip `10004` (E6c) s'intercaler
sans renuméroter :

| Couche | z | Contenu |
|---|---|---|
| Voile | 10000 | `radial-gradient` de fond + bannière `cover` + scrim, 3 orbes, sol, 3 faisceaux, 20 poussières |
| Rideaux | 10001 | rideau gauche/droit, cantonné festonné, 2 embrasses |
| Public | 10003 | 3 rangées de fauteuils + silhouettes — **devant** la carte (10002) |

| Élément | Valeur (DOM → natif) |
|---|---|
| Voile | `radial-gradient(ellipse at 50% 42%, #1b1b38, #05050a 74%)` — taille `farthest-corner` (ellipse au ratio de la boîte passant par le coin le plus loin) ; `opacity 0 → 1` en 0,5 s `ease` **portée par une couche** (tous les enfants fondent ensemble) |
| Bannière | `offline_image_url` Helix, `cover` plein cadre + scrim `linear-gradient(180deg, rgba(5,5,10,.55), .32 45%, .72)` ; absente = dégradé seul |
| Orbes | `o1` 620 px `left -140 / top -160` violet, `o2` 560 px `right -120 / top 12%` rose, `o3` 520 px `left 34% / bottom -240` bleu ; `opacity .45`, cycle 14 s `ease-in-out` alterné (`translate(60,-40) scale(1.12)`), retards −5 s / −9 s |
| Sol | bas 24 % : dégradé vertical `transparent → rgba(20,11,7,.78) 38% → #0b0503`, lueur `ellipse 62% 95% at 50% 112%`, liseré 2 px à 8 % des bords |
| Faisceaux | `s1` 420 px `left 6%` `rotate(17deg)`, `s2` `right 6%` `rotate(-17deg)`, `s3` 520 px centré ; `top -60px`, hauteur 126 %, trapèze `47% 0, 53% 0, 100% 100%, 0 100%`, dégradé 180°, balancement ±1,4° sur 11 s (retards −4 s / −7 s), origine `50% 0` |
| Poussière | 20 grains 2-5 px, `left 22-78 %`, `top 8-86 %`, dérive `(26,-42)` sur 8-17 s, opacité 0,12 → 0,5 → 0,22 |
| Rideaux | 50 % de large chacun, plis `repeating-linear-gradient(90deg)` période **88 px** + ombre verticale, ourlet doré 14 px, bord intérieur 6 px `#c9a13b` ; ouverture `translateX(∓27 %) scaleX(0,5)` en 1,2 s `cubic-bezier(.4,0,.2,1)` |
| Cantonné | `height 110px`, mêmes plis + ombre, festons `::after` : par tuile de **192 px**, cercle doré (r 104, +4 px) sous un cercle rouge (r 100), chacun clippé à sa tuile |
| Embrasses | 290 × 30, `top 57%`, `left/right -55px`, `rotate(∓24deg)`, rayon 15 ; `translateY(-16px) → 0` et `opacity 0 → 1` en 0,4 s **après 0,75 s** |
| Public | conteneur 240 px (`overflow: hidden`), 3 rangées `bottom 118 / 16 / -26`, fauteuils 88×74 / 116×94 / aucun (gros têtes floues), ~15 % de fauteuils vides, tête = ellipse + épaules (demi-ellipse 1,64 × largeur), balancement ±1,5° + `translateY(-2px)` sur 4-7,5 s |
| Fauteuil | `border-radius: 46% 46% 8% 8% / 32% 32% 5% 5%` → **géométrie** à deux rayons par coin (un `D2D1_ROUNDED_RECT` n'a qu'un rayon) |

**Chorégraphie** (miroir du CSS) : voile 0,5 s ; théâtral = valance et public
0,6 s, rideaux 1,2 s **après le double `requestAnimationFrame`** du DOM
(~33 ms), embrasses après 0,75 s ; sortie = tout se referme, masquage à
**1 250 ms** (théâtral) / **500 ms** (card) — le DOM retire les nœuds à ce
moment-là. Un `show` reçu pendant le fondu de sortie repart des phases
**courantes** (`depart`), jamais du noir. Une bascule `presentation-mode`
rejoue la chorégraphie (`debut_chore` remis à zéro) **sans** re-fondre le
voile (`debut` conservé) ; un `show` de même mode ne rejoue rien (le DOM
ajoute et retire les classes dans la même frame).

**Mode `card`** : **le voile et tout son décor restent affichés** (le code du
DOM fait `presVoileEl.style.display = "block"` + `.visible`) ; seuls rideaux,
cantonné, embrasses et public disparaissent. Le commentaire CSS « sans voile
complet » est faux — c'est le **code** qui fait foi.

**Tirage déterministe** : `Math.random()` (20 poussières + places du public)
est remplacé par un xorshift à graine fixe (`GRAINE_DECOR`) : le décor est
stable d'une session à l'autre, mais pas identique au tirage du DOM
(irreproductible par nature). Le public suit le **1920 codé en dur** du DOM
(`for (x = offset; x < 1920; …)`) : sur un canvas plus large, les rangées
s'arrêtent à 1 920 px comme dans la diffusion.

**Bannière et cache disque** : le crate ne fait aucun réseau (§13.8) — l'app
précharge l'URL dans `medias/cache/` (`cache_images.rs`, lot G) et le moteur
lit `image_cache(url)`. Tant que le fichier manque, le décor retombe sur le
dégradé seul, puis se répare seul : **re-tentative toutes les 500 ms**
(`pres_banniere_tentative`), l'équivalent natif du `<img>` qui se charge en
asynchrone.

**Limites assumées** : `filter: blur()` non porté (orbes = disque à dégradé
radial, opaque jusqu'à `r − 90 px` et transparent à `r + 90 px` ; faisceaux à
bord net ; poussière non adoucie) ; `box-shadow` / `inset` des rideaux, du
cantonné et des fauteuils non portés (même limite qu'E3) ; les cuspides entre
festons du cantonné sont **conformes au CSS** (le dégradé radial atteint le
bord de tuile à ~60 % d'alpha, il ne faut pas les « corriger »).

**Piège payé** : le fauteuil sortait en « M » (creux au milieu) —
`D2D1_ARC_SEGMENT` avec `D2D1_SWEEP_DIRECTION_CLOCKWISE` + `ARC_SIZE_SMALL`
sur des extrémités distantes de 90° **anti-horaires** force D2D à tracer les
270° restants (vers l'intérieur). Un contour parcouru bas → droite → haut →
gauche (y vers le bas) est anti-horaire : les quatre coins sont
`D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE`.

**Vérification** : `cargo test -p sos-moteur-natif` (273),
`cargo test -p streamos-v0 --lib` (241), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé,
1280×720) : théâtral à mi-ouverture (rideaux froncés + bord doré + ourlet +
embrasses), théâtral ouvert (ouverture ~11,5 % par côté, public et cantonné en
place) et mode `card` (voile + 3 faisceaux + orbes + poussière + sol, sans
rideaux ni public).

---

## 65. Présentation streamer — carte (lot E6b, 2026-11-01)

Miroir de `.pres-root` / `.pres-carte` / `.pres-col-gauche` /
`.pres-col-droite` et de leurs enfants (`diffusion.html` 2907-3070) + du
remplissage `playPresentationShow` (10891-10950). Couche `z 10002` — **sous**
le public (10003), **au-dessus** des rideaux (10001).

**Canal** : aucune frame nouvelle. La carte lit `PresentationDonnees` déjà
décodé par E6a (`presentation-show`) ; le canal est vérifié depuis E6a.

| Élément | Valeur (DOM → natif) |
|---|---|
| Racine | `inset 0`, `display: flex`, `align-items: center`, `justify-content: center` — la carte est centrée dans le canvas |
| Carte | `display: flex`, `gap 56px`, `max-width 73%` ; entrée `opacity 0 → 1` (0,55 s `ease`) et `translateY(26px) scale(.94) → none` (0,55 s `cubic-bezier(.2,.9,.3,1.15)`), **retard 0,65 s** — la carte apparaît pendant l'ouverture des rideaux |
| Carte (card) | `flex-direction: column`, `gap 22px`, `max-width 760px`, textes centrés, **retard 0 s** |
| Colonne gauche | `gap 14px`, `max-width 460px`, centrée : avatar → nom → `@login` → badges → bio → jeu → lien |
| Avatar | 190 px (150 en card), cercle : image `cover` clippée, `border 5px` blanc 90 %, anneau `box-shadow: 0 0 0 6px` violet 55 % (**sans flou → porté comme trait**) |
| Nom | `800 54px/1.05` (42 px en card), blanc, centré |
| Login | `600 24px/1.2`, blanc 65 % |
| Badge | `PARTNER` / `AFFILIATE` seulement, `700 18px/1`, `padding 8×16`, `radius 999`, dégradé `135deg` (`#772ce8→#9146ff` / `#1f6feb→#3b82f6`) |
| Bio | `400 23px/1.45`, blanc 88 %, **rognée à 5 lignes** |
| Jeu | `Joue à : <jeu>`, `700 22px/1.2`, `#ffd77a` |
| Lien | `twitch.tv/<login>`, `600 20px/1`, blanc 55 %, `letter-spacing .04em` |
| Colonne droite | `gap 18px`, `max-width 620px` ; `align-items: center` en card |
| EN DIRECT | pastille `800 22px/1`, `padding 10×18`, `radius 10`, fond `#e91916`, pastille blanche 12 px + `gap 12` ; `align-self: flex-start` (centrée en card) ; texte `EN DIRECT · <viewers>` |
| Titre live | `600 22px/1.35`, blanc 80 %, largeur de colonne |
| Jeux récents | rangée `gap 12px`, boîte `96×128` (`radius 10`, `cover`) + nom `600 15px/1.2` blanc 75 %, centré, **2 lignes max** |

**Dimensionnement des colonnes** (`repartition_colonnes`, `flex` résolu à la
main) : chaque colonne prend la largeur **naturelle** de son contenu
(mesurée sans retour à la ligne), bornée par son `max-width` (460 / 620) ;
en théâtral, si `gauche + 56 + droite` dépasse la largeur de la carte
(73 % du canvas), les deux sont réduites **proportionnellement** — c'est
exactement ce que fait `flex-shrink: 1` avec des bases de contenu. En mode
`card` les colonnes sont empilées et bornées par 760 px. La carte est ensuite
centrée dans le canvas ; en théâtral chaque colonne est centrée verticalement
(`align-items: center`).

**Images** : avatar et boîtes de jeux sont des URLs distantes
(`static-cdn.jtvnw.net`) → **cache disque de l'app** (`image_cache`, lot G),
préchargées par le pont sur la frame `presentation-show`. Le crate ne fait
aucun réseau (§13.8). ⚠️ Les `box_art_url` Helix portent des placeholders
`{width}x{height}` : `url_box_art` (crate) les remplace par `96x128` et l'app
appelle **la même fonction** pour précharger — une seule vérité, donc aucune
dérive d'empreinte possible. Tant que le fichier manque, l'élément est omis
puis re-testé toutes les 500 ms (`pres_images_tentative`).

**La grille de clips 2×2 de la carte n'est PAS dans cette tranche** : ses
vignettes vivent sur `clips-media-assets2.twitch.tv`, un hôte absent du
contrat vie privée d'`AGENTS.md`. Elle part avec **E6c** (lecteur clip
`z 10004`), en même temps que l'hôte sera documenté.

**Limites assumées** : `text-shadow` des textes non porté (même limite qu'E3) ;
`-webkit-line-clamp` de la bio et des noms de jeux = **rognage** à N lignes,
sans ellipse ; `word-break: break-word` du nom non appliqué (DirectWrite
coupe aux espaces) ; ombre portée de la carte, halo des badges et ombre des
boîtes non portés (seul l'anneau de l'avatar, sans flou, est rendu) ;
`aspect-ratio` / `grid` des clips → E6c.

**Vérification** : `cargo test -p sos-moteur-natif` (280),
`cargo test -p streamos-v0 --lib` (241), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé,
1280×720, cache disque local) : théâtral (avatar rond + anneau, nom, `@login`,
badge AFFILIATE dégradé, bio 5 lignes, jeu, lien, `EN DIRECT · 1 234`, titre
sur 2 lignes, 2 boîtes de jeux + noms) et mode `card` (colonnes empilées,
tout centré).

---

## 66. Présentation streamer — clips (lot E6c, 2026-11-01)

Miroir de `.pres-clips` / `.pres-clip` (CSS 3071-3111) + `playPresentationShow`
(10952-10972) pour la **grille 2×2**, et de `.pres-clip-root` (3112-3142) +
`presentationClipPlay` / `presentationClipStop` (11121-11147) pour le
**lecteur plein écran** `z 10004`.

**Canal** : frames `chat_tx` **`presentation-clip-play`** (payload `mp4_url`,
`titre`, `duree_ms`) et **`presentation-clip-stop`** (émise aussi par le
`onended` du DOM en fin de clip). `presentation-show` et `presentation-hide`
**coupent** le clip (`playPresentationShow` / `stopPresentation`).

| Élément | Valeur (DOM → natif) |
|---|---|
| Grille | `.pres-clips` : 2 colonnes, `gap 12px`, 4 premiers clips ; cellule `aspect-ratio 16/9`, `radius 12`, `overflow: hidden` |
| Vignette | `thumbnail_url`, `object-fit: cover`, fond `rgba(255,255,255,.06)` tant que le fichier n'est pas en cache |
| Durée | `Math.round(duration) + "s"`, `700 15px/1`, pastille noire 75 %, `padding 4×8`, `radius 6`, à 8 px du coin bas-droit |
| Titre | `600 15px/1.3`, bandeau `linear-gradient(180deg, transparent, rgba(0,0,0,.85))`, `padding 18px 10px 8px`, **`nowrap` + ellipse** |
| Lecteur | voile `rgba(0,0,0,.9)` plein canvas ; vidéo à sa taille intrinsèque bornée à **88 % × 82 %** (jamais agrandie), centrée, `radius 14` |
| Titre du lecteur | `700 30px/1.3`, `top 44px`, `left/right 8%`, centré, **`nowrap` + ellipse** |

**Préchargement disque (décision utilisateur du 2026-11-01)** : `mp4_url` est
une URL **signée** Twitch — le crate ne fait aucun réseau (§13.8). L'app
télécharge le MP4 dans **`medias/clips/<fnv16(url)>.mp4`**
(`cache_clips.rs`, écriture atomique, purge au-delà de 8 fichiers, 256 Mio
max) **au clic « jouer un clip »**, puis émet la frame. Le moteur calcule le
même chemin (`presentation::chemin_clip`, même `empreinte_url`) et lit un
**fichier local**. Tant qu'il manque, le lecteur affiche le voile + le titre
(« buffering ») et re-tente toutes les 500 ms — la signature peut expirer côté
Twitch sans conséquence, le fichier reste lisible.

**Nouvel hôte réseau** : `clips-media-assets2.twitch.tv` (vignettes + MP4),
documenté dans le CONTRAT FIGÉ vie privée d'`AGENTS.md` **avant** implémentation
(règle du contrat).

**Fin de clip** : `duree_ms` (portée par la frame) → retour à la carte, comme
`onended` ; `duree_ms: 0` = inconnue → le clip reste jusqu'au `-clip-stop`.
Le **son du clip** rejoint le mixeur (anneau du média, gain 1000, 0 dès la fin)
— le DOM ne le coupe qu'en `PREVIEW`, notion absente du moteur.

**Écart assumé** : le DOM place la pastille de durée **avant** le titre, donc
sous son bandeau dégradé, et laisse le titre courir dessous — les deux
deviennent illisibles sur un titre long. Le moteur peint la pastille
**au-dessus** et rogne le titre **avant** elle (largeur réservée au layout).

**Piège payé** : `DWRITE_WORD_WRAPPING_NO_WRAP` **ne rogne pas** au layout —
DirectWrite laisse le texte déborder (le titre passait sous la pastille de
durée). Il faut `SetTrimming` + `CreateEllipsisTrimmingSign` (miroir de
`text-overflow: ellipsis`).

**Limites assumées** : `box-shadow` de la cellule et ombre de la vidéo non
portés ; la grille prend **toujours** la largeur de la colonne (en mode `card`
le `align-items: center` du DOM laisse la largeur d'une grille
`repeat(2, 1fr)` indéterminée) ; vignette manquante = fond translucide seul.

**Vérification** : `cargo test -p sos-moteur-natif` (283),
`cargo test -p streamos-v0 --lib` (244), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé,
1280×720, cache disque + MP4 local) : grille 2×2 (vignettes, durées `30s` /
`14s`, titres tronqués à l'ellipse) et lecteur plein écran (voile 90 %, vidéo
centrée arrondie, titre 30 px).

---

## 67. Présentation streamer — audio (lot E6d, 2026-11-01)

Miroir de `presentationAudioPlay` / `presentationAudioStop`
(`diffusion.html` 11151-11171). Dernière tranche du lot E6.

**Canal** : frames `chat_tx` **`presentation-audio-play`** (payload `piste`,
`src`) et **`presentation-audio-stop`** (payload `piste` = `voix` | `musique` |
`toutes`). `presentation-show` et `presentation-hide` arrêtent **toutes** les
pistes (`presentationAudioStop("toutes")` dans `playPresentationShow` /
`stopPresentation`).

| Élément | Valeur (DOM → natif) |
|---|---|
| Pistes | 2 fixes : `voix` (one-shot, `audio.loop = false`) et `musique` (boucle, `audio.loop = true`) |
| Source | `src` = `medias/…` → **fichier local** (`base.join(src)`), aucun réseau |
| Lecture | `media::AudioFichier` (lot E4, Source Reader audio seul) — `ouvrir(chemin, temps_us)` puis `avancer(temps_us)` à chaque frame |
| Boucle | `musique` : le fichier est **réouvert** en fin de flux (équivalent de `loop`) ; `voix` s'arrête au bout |
| Mixage | une entrée par piste en **queue** du mixeur (`entrees.push`), gain 1000 tant que la piste est active, 0 sinon |
| Arrêt | `-audio-stop` d'une piste → `pause()` + `currentTime = 0` (l'anneau est libéré) ; `toutes` → les deux |

**Dégradations assumées** : `muted`/`PREVIEW` du DOM n'existent pas côté
moteur (le son sort toujours) ; le fichier est rouvert à chaque boucle
(~0 ms sur un MP3 de quelques Mo, aucune coupure audible) ; une piste
illisible est **omise** une fois (journalisée en `debug`), jamais réessayée
en boucle — comme le pad (E4).

**Vérification** : `cargo test -p sos-moteur-natif` (286),
`cargo test -p streamos-v0 --lib` (245), clippy workspace 0 warning,
`npm run check` 0 erreur. La piste est **chargée et avancée** en conditions
réelles (harnais temporaire : `AudioFichier::ouvrir` réussit sur un WAV,
aucune ligne « piste ignorée »). ⚠️ **Pas de vérification audio de bout en
bout possible sur cette machine** : le harnais avec loopback se fige dans
`Apercu::lire` — voir l'encart ci-dessous, **reproduit sans la présentation**.

> ✅ **RÉSOLU le 2026-11-02 (lot I-a, §74)** — la cause n'était pas
> `Apercu::lire` mais l'écriture du **pipe audio** (voir §74). Après
> correction, le même harnais tient 890 composites en 15 s (≈59,9 fps, 0
> dégradation) et le MP4 garde ses deux flux. Le diagnostic ci-dessous est
> conservé pour l'historique.
>
> ⚠️ **BUG PRÉ-EXISTANT (diagnostic d'origine, 2026-11-01)** : avec
> `audio_live` **et** l'aperçu actif (`ApercuCfg::default()`), le thread de
> composition se figeait au bout de quelques secondes — `composites` cessait
> d'augmenter et `arreter()` ne rendait jamais la main. Reproduit avec
> `examples/audio_scene.rs` **sans aucune présentation** (388 composites puis
> gel à 8 s). La localisation réelle : `PipeAudio::ecrire` (pipe nommé vers
> FFmpeg), pas `Apercu::lire` — voir §74.

---

## 68. Pop de validation des splits (lot E7, 2026-11-01)

Miroir de `.splitpop-root` / `.splitpop-carte` (`diffusion.html` 1150-1310) et
de `srPopSplit` / `splitPopCacher` / `applySplitPopOverlayConfig`
(6415-6620 + 6870-6990). `z 9993` — sous le pad (9995), les alertes (9997) et
le bandeau (9998), au-dessus des modules HUD (9000).

**Canal** : le **contenu** ne vient pas d'une frame dédiée — le pop est dérivé
des **mêmes** mutations que le widget speedrun (frames `chat_tx`
`speedrun-etat` → `MajSpeedrun`), lues **avant** application
(`EtatSpeedrun::pop_pour`, miroir de `srPointEtatFranchi`). Seule la
**position** arrive par la frame **`splitpop-pos`** (squelette dédié
`position_splitpop.json`, défaut 660/60/600/120) — jamais par
`position-overlay-config` (squelette partagé welcome/alertes/commandes/pad).

| Élément | Valeur (DOM → natif) |
|---|---|
| Racine | `(x, y)` + `largeur × hauteur` du squelette, `flex` centré ; `opacity 0 → 1` et `scale(0.6 → 1)` en 0,3 s `cubic-bezier(.16,1.08,.38,.98)` |
| Police de base | `em = max(18, round(hauteur × 0,4))` px — tout le reste est en `em` |
| Badge | « SPLIT n » (`n = index + 1`), 0,3em poids 800, `letter-spacing .28em`, `padding .24em .8em .24em 1.08em`, pastille `color-mix(couleur 26%, rgba(0,0,0,.72))` + bord `color-mix(couleur 75%, transparent)`, rayon 999 |
| Hero | Delta (`+1.23` / `−0.45`) en **Anton** 1,5em, `skewX(-5deg)`, couleur = `srCouleurAhead` (ok) / `srCouleurBehind` (ko) |
| Mot | `GAGNÉ` / `PERDU` sous le hero (0,3em, 800, `letter-spacing .5em`) ; **sans delta** → `VALIDÉ` / `SKIP` devient le hero (0,85em Anton, `.14em`, couleur du résultat) |
| Impact | `scale(.15) rotate(-9deg)` → `1.22 rotate(2.5deg)` (55 %) → `0.95 rotate(-1deg)` (78 %) → `1` en 0,5 s `cubic-bezier(.22,1.32,.36,1)` |
| Aura | `::before` `inset -0.5em -1.1em`, halo sombre + teinte du résultat |
| Durée | `srPointsDureePop` (défaut 2 s, min 1) puis fondu 0,3 s, nœud retiré à +350 ms |
| Re-pop | à 2 s puis 4 s si `durée > t + 0,6 s` — `scale 1 → 1,12 rotate(-1.5deg) → 0,97 → 1` en 0,45 s |
| Dédup | même signature (`split:n:cumul` / `skip:n`) à moins de **1,5 s** = même event (plusieurs widgets speedrun) |
| Coupe | `started` / `reset` / `goto` / `stopped` / `disconnected` cachent le pop |

**Options du widget** (`srVoirPoints`, `srPointsDureePop`,
`srCouleurAhead`/`Behind`) : portées par `SpeedrunCfg` (lot E7, deux champs
ajoutés) et lues au moment du push.

**Limites assumées** : `filter: blur(2px)` de l'aura et `box-shadow` de la
pastille non portés ; le `flash` (`brightness(2.1) saturate(1.6)` sur le hero)
est approché par un **éclaircissement** vers le blanc (65 % max) sur les 45 %
premiers ; `text-shadow`/`drop-shadow` non portés (E3).

**Son du pop (`srPointsSon`) — porté le 2026-11-02** : `SpeedrunCfg` porte
`points_son` + `points_son_volume` (fichier **local** `medias/…`, aucun
réseau) ; la frame `MajSplitPop::Afficher` les transporte ;
`EtatSplitPop::son()` rend `(chemin, volume, son_version)` — la version
change à **chaque pop accepté** (pas sur un doublon), le rendu rouvre alors
le fichier (`AudioFichier::ouvrir` à `temps_s`, équivalent du `new Audio` du
DOM, un par pop) et l'avance à chaque frame ; le mixeur y ajoute une entrée
de gain `volume × 1000` (0 hors pop). Fichier illisible = pop muet, jamais
fatal (politique du pad / E6d).

⚠️ **Implémenté et testé unitairement, pas encore vérifié de bout en bout** :
le harnais audio + widget speedrun se fige encore (voir §74, gel n° 2), et il
se fige **sans** le son aussi — le son du pop n'est donc pas en cause.

**Piège payé** : une ligne `DWRITE_TEXT_ALIGNMENT_CENTER` est centrée dans
**sa boîte de mise en page** (4096 px) → tout le texte du pop partait 2 000 px
à droite, hors écran (même piège qu'E3). `LEADING` + centrage manuel
(`x = centre − largeur / 2`) pour le hero et le mot, `padding-left` pour le
badge.

**Vérification** : `cargo test -p sos-moteur-natif` (293),
`cargo test -p streamos-v0 --lib` (246), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification visuelle (harnais temporaire supprimé,
1280×720) : badge « SPLIT 8 », hero `-12.34` en vert, mot `GAGNÉ`.

---

## 69. Just Chatting côté moteur natif (lot F, 2026-11-01)

Miroir de `syncObsJC` (`src/lib/stores/justChatting.ts`) : en mode JC, seuls
la caméra et le(s) chat(s) restent visibles, repositionnés aux rects du layout
JC (formule Rust, fractions 0-1 → × canvas réel).

**Le problème** : la scène dérivée (`sceneDerivee`, copie **en mémoire** —
caméra + chats aux rects JC, autres widgets absents, `bgObsSource` vidé,
modules HUD vidés) n'était poussée qu'à **OBS** (`scene_sync_captures`). Le
moteur natif continuait d'afficher la scène complète : un trou
`destination-out` non percé laisse la capture de fond plein canvas visible.

**Correctif** : commande Tauri **`moteur_appliquer_scene(scene)`** — applique
une scène **fournie** au moteur natif par le même chemin qu'une scène à chaud
(`MoteurNatif::appliquer_scene`, application **différentielle** : ni session de
capture ni encodeur recréés). Aucune écriture disque, aucune scène persistée
mutée, aucun état métier dupliqué (règle « une seule vérité de scène »).

`justChatting.ts` pousse la **même** copie aux deux rendus :
- `setJustChatting(true)` → `syncObsJC()` + `syncNatifJC(derivee)` ;
- `setJustChatting(false)` → `syncNatifJC(scene réelle)` (le moteur reprend la
  scène jamais mutée) ;
- hook `registerApresSyncCaptures` (changement de scène/preset/reconnexion
  pendant JC) → la dérivée de la **nouvelle** scène part aussi au moteur.

Non fatal : hors session, l'application ne fait que préparer la suivante
(`appliquer_scene` est idempotent hors enregistrement).

**Vérification** : `cargo test -p sos-moteur-natif` (293),
`cargo test -p streamos-v0 --lib` (246), clippy workspace 0 warning,
`npm run check` 0 erreur (la commande est un passe-plat de 1 ligne ; la
dérivation `sceneDerivee` et l'application différentielle sont déjà couvertes
par les lots A/B).

---

## 70. Titres de widget dans l'ordre z (lot H1, 2026-11-01)

**Le problème** : tous les titres (`TexteCfg`) étaient peints dans une **passe
finale**, après la boucle du plan — donc au-dessus de **toute** la pile :
overlays (attente 9990, pad 9995, alertes 9997, bandeau 9998, bienvenue 9999,
présentation 10000+), cadres et bordure. Un titre de widget apparaissait
par-dessus une alerte ou un écran d'attente, là où le DOM le laisse dans son
widget.

**Correctif** : nouveau `GenreCalque::Titre`, inséré dans l'ordre du plan
**entre le contenu et la bordure** (`Boite…Musique` → `Titre` → `Bordure` →
`Cadre`) et une entrée par titre de **widget** à son `z` :

```
for (i, texte) in voie.textes.iter().enumerate() {
    if texte.cible == CibleTitre::Widget {
        plan.push((texte.z, GenreCalque::Titre, i));
    }
}
```

Le titre passe donc au-dessus du contenu de **son** widget (média, capture,
chat…), sous sa bordure puis son cadre — et sous tout overlay de `z`
supérieur, comme `.titre` dans `.widget` (`.cadre-overlay` a `z-index: 5`).

⚠️ **Les titres de FOND (`bgTitre*`) restent dans la passe finale** : leur `z`
vaut `i32::MIN + 1` (sous le média de fond) ; les mettre dans le plan les
aurait fait disparaître derrière le fond. Leur ordre d'avant est conservé.

**Vérification** : `cargo test -p sos-moteur-natif` (293),
`cargo test -p streamos-v0 --lib` (246), clippy workspace 0 warning,
`npm run check` 0 erreur. Vérification **par construction** (entrée de plan +
position dans l'ordre d'égalité `z`), pas de harnais visuel pour ce diff.

**Reste du lot H** (à découper, non fait ici) :
- **H2 / H2b / H2c — contenus de widget et titres sous perspective** :
  **faits le 2026-11-02** (§71).
- **H3 — `line-height`** : **fait le 2026-11-02** (§72).
- **H4 — contour exact** : **fait le 2026-11-02** (§73) — tracé réel des
  glyphes, repli sur l'approximation pour les polices simulées.

---

## 71. Contenus de widget sous perspective (lots H2 / H2b / H2c, 2026-11-02)

**Le problème** : `GenreCalque::Chat`, `Input`, `Goal`, `Speedrun`, `Musique`
et `Titre` tombaient dans la branche `None` de la boucle de plan. Un widget
sous `perspective(d) rotateX/Y` gardait donc sa boîte (voile, média de fond,
bordure, cadre) **projetée**, mais son **contenu plat**, peint par-dessus le
trapèze. En OBS tout le `.widget` est tourné : `applyWidgetStyle` pose le
`transform` sur `.widget` (`diffusion.html` 4560) et toutes les branches de
contenu l'appellent — `.chat-body`, les touches, le compteur goal, le timer,
le lecteur musique et `.titre` sont des **enfants**.

**Correctif** : trois lignes, les cinq genres de contenu passent par
`groupe_de(rect, z)` comme le média — plus le titre de widget (H2c).

```rust
GenreCalque::Chat => voie.groupe_de(voie.chats[*index].rect, *z),
GenreCalque::Input => voie.groupe_de(voie.inputs[*index].rect, *z),
GenreCalque::Goal => voie.groupe_de(voie.goals[*index].rect, *z),
GenreCalque::Speedrun => voie.groupe_de(voie.speedruns[*index].rect, *z),
GenreCalque::Musique => voie.groupe_de(voie.musiques[*index].rect, *z),
GenreCalque::Titre => voie.groupe_de(voie.textes[*index].cadre, *z),
```

Rien d'autre n'a bougé : `cible_pour` redirige déjà le dessin.

| Sujet | Décision | Motif |
|---|---|---|
| Redirection du texte | **aucune** : `cible_rt` est `cible.cast::<ID2D1RenderTarget>()` du **même** `ID2D1DeviceContext` (`texte/mod.rs`, `nouveau`) | `DrawTextLayout`, `PushAxisAlignedClip` et `PushLayer` passés par `cible_rt` suivent le `SetTarget` courant — c'est déjà le chemin de `dessiner_media`. Aucun offscreen « de contenu », aucun blit intermédiaire |
| Clé de regroupement | `(rect, z)` du widget | `persps_depuis_scene` et `ChatCfg`/`InputCfg`/`GoalCfg`/`SpeedrunCfg`/`MusiqueCfg` la calculent des **mêmes** champs (`w.x/y/largeur/hauteur`, `w.z`) — comparaison `f64` exacte, verrouillée par test |
| Clé d'un **titre** | `(TexteCfg.cadre, TexteCfg.z)` | `textes_depuis_scene` pose `cadre = (w.x, w.y, w.largeur, w.hauteur)` et `z = w.z` : le rect sert d'**ancre** du placement hors widget (`haut`/`bas`/`gauche`/`droite`) et se trouve être exactement la clé du plan. Aucun champ ajouté |
| Anticrénelage | inchangé (`D2D1_TEXT_ANTIALIAS_MODE_DEFAULT`) | ClearType sur le canvas opaque, repli grayscale automatique sur l'offscreen prémultiplié (vérifié au rendu : aucun liseré sombre) |
| Ordre dans le plan | inchangé | `Chat`/`Input`/`Goal`/`Speedrun`/`Musique` puis `Titre` sont déclarés entre `Capture` et `Bordure`/`Cadre` : le contenu passe sur le voile et le média de fond, le titre sur son contenu, sous le cadre |

**Reste hors plan à perspective** — **plus rien** côté scène : les cinq
contenus de widget (chat, input viewer, goal, speedrun, musique) et les titres
de widget sont routés. Les modules HUD (`Horloge`, `Objectifs`,
`MusiqueModule`, `Equaliseur`, `Spots`) et les overlays restent hors plan,
**légitimement** : `#modules-root`, `.alerte-root`, `.pad-root`… sont hors
`.widget` dans le DOM (aucun `transform` 3D ne les porte).

**Vérification** : deux harnais temporaires (supprimés) en 1360×768, widgets
plats à gauche (référence) et `rotateY 38,25` à droite.
1. **chat** — deux widgets sur le **même** buffer : pseudo, `:`, texte,
   bulles et bordures du widget de droite sont **dans** le trapèze (bord
   droit qui recule, texte qui rétrécit avec la profondeur) ;
2. **titre** — `titreHaut` sur chacun : « TITRE TOURNE » suit le trapèze
   (incliné, à son bord haut), « TITRE PLAT » reste droit.

`goal`/`speedrun`/`musique` ne sont vérifiés que **par la clé** (même
mécanisme, même `cible_rt`, test `persp_et_contenu_chat_input_partagent_la_cle`) :
leur contenu exige un état live poussé par l'app pour produire des pixels.
`cargo test -p sos-moteur-natif` (298), `cargo test -p streamos-v0 --lib`
(246), clippy workspace 0, `npm run check` 0.

**Bug pré-existant corrigé dans la même passe (lot G)**
`construire_bloc` poussait un `ID2D1SolidColorBrush` dans `_inline:
Vec<IDWriteInlineObject>` (puce, `texte/chat.rs` ~714 ; séparateur `:` ~761).
Le `cast()` rendait `E_NOINTERFACE` → **tout** message avec pseudo mettait le
moteur en `Erreur("chat : texte (système) : séparateur inline …")`, zéro
composite, sink FFmpeg mort (« Output file does not contain any stream »).
Les deux `push` sont supprimés (les brosses sont déjà retenues par
`SetDrawingEffect`) ; un commentaire sur `_inline` interdit de les
réintroduire. Vérifié au rendu (`examples/chat_scene.rs` : 4 messages
d'historique + 4 en direct, pseudos colorés, logos Twitch/Kick, accents).
⚠️ Écart **corrigé** depuis (2026-11-02) : le DOM espace `.pseudo` et
`.separateur` de `margin-right: 2px` ; le moteur pose un espacement **de fin**
de 2 px (`SetCharacterSpacing(0, 2, 0, …)`) sur le dernier caractère du
pseudo et sur le `:` — un trailing sur toute la plage espacerait chaque
lettre du pseudo.

---

## 72. `line-height` et marges des titres (lot H3, 2026-11-02)

**Le problème** : `rendre` mesurait le titre avec la hauteur de ligne
**naturelle** de la police (~1,2-1,35 em) alors que `.widget-titre` et
`.bg-titre` portent `line-height: 1.1`. Trois écarts qui grandissent avec
`titreTaille` :

1. **boîte trop haute** — `metriques.height` au lieu de `1,1 × taille` ;
2. **glyphes non centrés** dans la boîte de ligne : le CSS répartit le
   *half-leading* au-dessus et au-dessous, DirectWrite empile ascent puis
   descent ;
3. **placement** : `placement` positionnait la **texture** (qui inclut la
   marge de contour `debord`), donc le titre visible était décalé de
   `debord` (≈3 px) vers l'extérieur, et les titres de widget n'avaient pas
   les `margin-*: ±2px` du DOM.

| Sujet | Décision | Motif |
|---|---|---|
| Interligne | `SetLineSpacing(UNIFORM, 1,1 × taille, baseline)` sur le **layout** | miroir exact de `line-height: 1.1` ; le format est partagé avec d'autres titres |
| Baseline | `(interligne − hauteur_naturelle) / 2 + baseline_naturel` (`GetLineMetrics`) | reproduit le *half-leading* CSS ; à interligne = naturel, la valeur est inchangée |
| Boîte vs texture | `CacheTexte.largeur/hauteur` = **boîte CSS** ; la texture est plus grande de `marge = (contour, contour + débord)` | `placement` doit positionner la boîte CSS, pas le bitmap ; le débord (`line-height` plus court que la police) empêche le rognage des glyphes |
| Dessin | `DrawBitmap` dans `place.rect` **élargi** de `marge` | la boîte CSS tombe pile sur `place.rect`, contour compris |
| Marges | `MARGE_TITRE_WIDGET = 2.0` (`margin-*: ±2px` de `applyTitreWidget`) | les titres de widget n'étaient pas écartés du bord |

**Vérification** : `titre_mesure_la_boite_de_ligne_css` (test **GPU**, 24 / 40
/ 96 px) vérifie `hauteur == 1,1 × taille` à 0,01 px près et la présence de
la marge ; tests purs de `placement` étendus aux quatre positions de widget.
Harnais temporaire (supprimé) : 4 titres de widget + 4 titres de fond sur une
scène 1360×768 — aucun glyphe rogné, titres de widget à 2 px de leur boîte,
titres de fond à 8 px.
`cargo test -p sos-moteur-natif` (298), `cargo test -p streamos-v0 --lib`
(246), clippy workspace 0, `npm run check` 0.

**Limite assumée** : `text-align: center` ne joue que si la boîte de mise en
page est celle de l'élément — ici la texture est mesurée à la largeur du
texte, donc le centrage est sans objet (le titre est déjà à sa largeur).

---

## 73. Contour exact par géométrie de glyphes (lot H4, 2026-11-02)

**Le problème** : le contour d'un titre (`-webkit-text-stroke` du DOM) était
une **approximation** par 16 passes de `DrawTextLayout` décalées en couronne —
halo irrégulier, double alpha aux recouvrements, artefact en « soleil » sur les
traits fins, et écart croissant avec `contourEpaisseur`.

**Correctif** : le contour est désormais le **tracé réel** des glyphes.

1. `IDWriteTextLayout` n'expose aucune géométrie → un `IDWriteTextRenderer`
   maison (`CollecteContours`) intercepte `DrawGlyphRun` et appelle
   `IDWriteFontFace::GetGlyphRunOutline(emSize, indices, advances, offsets…)`
   dans une `ID2D1PathGeometry` par run ;
2. chaque contour est translaté à son origine de ligne de base
   (`CreateTransformedGeometry` + `translation(baselineX, baselineY)`),
   puis à l'origine de la texture (`marge`) au moment du `Widen` ;
3. `ID2D1Geometry::Widen(epaisseur, …)` donne le trait **centré** (comme
   `-webkit-text-stroke`), rempli avec la brosse du contour ;
4. le **remplissage** reste `DrawTextLayout` (dégradé, soulignement, objets
   inline) : `paint-order: stroke fill` est donc respecté tel quel.

| Sujet | Décision | Motif |
|---|---|---|
| Repli | police **simulée** (gras/oblique synthétique) → 16 passes | `GetGlyphRunOutline` rend le contour **non embelli** : le tracé serait plus fin que le glyphe rendu. Les 20 familles embarquées n'ont qu'une graisse, donc tout `titreGras`/`titreItalique` reste sur l'approximation |
| Détection | `IDWriteFontFace::GetSimulations() != 0` dans le renderer | signal fiable, pas de comparaison de graisse |
| Jointure | `Widen` avec le style par défaut (miter) | même famille que le tracé CSS ; `flatteningTolerance` 0,25 px |
| Coût | 1 collecte par **rendu** de titre (cache par contenu+style) | jamais par frame |

**Vérification** : `contour_exact_sauf_police_simulee` (test **GPU**) —
une police réelle donne des contours non vides, une police simulée (`gras`)
rend `None` (repli). Harnais temporaire (supprimé) : trois titres 64-72 px à
contour 6 px — halo régulier sur les deux tracés exacts, comportement
inchangé sur le gras simulé, aucun glyphe rogné.
`cargo test -p sos-moteur-natif` (298), `cargo test -p streamos-v0 --lib`
(246), clippy workspace 0, `npm run check` 0.

---

## 74. Gel de composition — pipe audio non bloquant (lot I-a, 2026-11-02)

**Symptôme** : avec `audio_live` (loopback ou micro), la composition se
figeait définitivement après quelques secondes — `composites` bloqué à 388,
`arreter()` sans effet, `etat` toujours `Actif`, aucune dégradation.

**Localisation** (instrumentation temporaire, marqueurs retirés depuis) :

```
boucle apres attendre t=390
mix avant t=390            ← mixeur.produire()
mix apres t=390
   ... 10 s de rien (le harnais continue de s'afficher) ...
debut avant t=390          ← voie.debut()
```

Le seul appel bloquant entre `mix apres` et `debut avant` est
**`PipeAudio::ecrire`** (`WriteFile` sur le pipe nommé qui porte le PCM vers
FFmpeg). Le diagnostic d'origine (« gel dans `Apercu::lire` ») était faux :
l'aperçu n'était qu'un ralentisseur qui rendait l'interblocage visible.

**Cause** : le pipe était créé en `PIPE_WAIT`. FFmpeg cesse de drainer le
pipe audio quand il attend de la vidéo ; la composition, bloquée dans
`WriteFile`, ne lui envoie plus de vidéo → **interblocage** à deux flux.

**Correctif** (`output/ffmpeg.rs`) :

| Sujet | Décision | Motif |
|---|---|---|
| Mode du pipe | `SetNamedPipeHandleState(PIPE_NOWAIT)` sur le **handle serveur**, juste après `CreateNamedPipeW` (création laissée en `PIPE_WAIT`, le client FFmpeg garde ses lectures bloquantes) | `WriteFile` rend la main immédiatement, quel que soit l'état de FFmpeg |
| Tampon plein | `ERROR_PIPE_LISTENING` / `ERROR_PIPE_NOT_CONNECTED` / `ERROR_NO_DATA` / `ERROR_BUSY` ou `n == 0` → **bloc sauté** (`Ok`) | trou audio borné (≈5 ms) plutôt qu'un gel du flux entier ; politique déjà appliquée au démarrage (`ERROR_PIPE_LISTENING`) |
| Échec du mode | `SetNamedPipeHandleState` refusé → `log::warn` et comportement d'avant | jamais fatal, la sortie fonctionne |
| ⚠️ Comparaison | `HRESULT::from_win32(code)` — **jamais** `code` nu | `0x8007_00E7 ≠ 231` : comparer le code Win32 laissait passer `ERROR_PIPE_LISTENING` en erreur fatale |

**Vérification** : `examples/audio_scene.rs` (loopback, 15 s) — avant :
388 composites puis gel ; après : **890 composites**, `degradation=None`,
`Fin — état Inactif`, et `ffmpeg -i` confirme **les deux flux** (H.264
1360×768 @ 59,91 fps, 14,85 s + AAC 48 kHz stéréo).
`cargo test -p sos-moteur-natif` (298), `cargo test -p streamos-v0 --lib`
(246), clippy workspace 0, `npm run check` 0.

**Reste assumé** : l'écriture du **stdin vidéo** (`FfmpegSink`) reste
bloquante — si FFmpeg cessait de lire la vidéo, le même interblocage serait
possible ; non reproduit, la vidéo étant le flux qui débloque l'autre.

---

## 75. Gel n° 2 — `audio_live` + pop de split (lot I-b, 2026-11-02, **levé**)

Second gel, distinct de celui du §74 (corrigé), trouvé en vérifiant le son du
pop. **Il n'est pas causé par le son du pop** : le déclencheur réel est le
**débit vidéo des premières secondes**, pas le pop.

**Repro** (harnais `examples/gel_ib.rs`, supprimé) : scène 1360×768 avec un
widget `speedrun`, `AudioCfg { loopback: true }`, split poussé à t = 2 s.

| Variante | Résultat (avant correctif) |
|---|---|
| audio + widget speedrun + pop | **gel** — 143 composites, **0 o écrits** |
| audio + widget speedrun, sans pop | passe (352 composites, 43 Ko) |
| **sans** audio (`audio: None`) | passe (352 composites) |
| `examples/audio_scene.rs` (audio, **sans** widget speedrun) | passe (§74) |

**Localisation** (`SOS_FFMPEG_DEBUG` = `-loglevel debug -debug_ts` + compteurs
d'écriture, instrumentation retirée depuis). La dernière ligne de FFmpeg est
`[s16le] Before avformat_find_stream_info() pos: 0` et le fichier de sortie
n'est **jamais créé** : FFmpeg n'atteint pas `avformat_write_header`, le muxeur
n'a jamais tourné. Côté composition, la trace est sans ambiguïté :
`sink : écriture de 1157 o bloquée 3.1506259s (cumul 74055 o)` — le stdin
vidéo absorbe **74 055 o** puis bloque, et `pousser` rend l'erreur à +3 s.

**Correctif appliqué (I-b) — le gel devient un échec borné** :

| Sujet | Décision | Motif |
|---|---|---|
| Fil d'écriture | un fil par `FfmpegSink` possède le stdin ; `pousser` empile dans une `sync_channel` de 8 paquets (~0,13 s à 60 fps) | la composition ne touche plus jamais `WriteFile` |
| Échéance | `try_send` en boucle, **3 s** maximum, puis `Err("FFmpeg ne draine plus (sortie figee)")` | `send_timeout` est instable (`std_internals`) ; un FFmpeg figé devient une dégradation explicite, jamais un gel muet |
| Fin de vie | `fermer` : drop du `Sender` + `join` (flush + EOF) ; `Drop`/`couper` : drop + `kill` | `wait()` seul pouvait attendre un FFmpeg figé au teardown |
| Cause d'arrêt | `"toutes les sorties ont échoué — <cause>"` | l'erreur du sink était perdue, l'arrêt était muet |

**Cause** — `avformat_find_stream_info` sur l'entrée PCM lit **819 200 o =
4,27 s de PCM au rythme réel** (défaut `max_analyze_duration` : 5 s) **sans
lire stdin**. Pendant cette fenêtre la composition remplit le tampon du stdin
(≈ 64 Ko de pipe OS + 8 paquets) et se bloque ; bloquée, elle n'alimente plus
le pipe audio → l'analyse ne peut jamais atteindre ses 819 200 o →
**interblocage définitif**. Le pop ne fait qu'ajouter du débit (première frame
IDR de 9 685 o, ~86 kbps) et franchir le seuil du tampon : sans pop, 36 Ko
sont poussés sur la même fenêtre et ça tient.

Ce que ça réfute, mesuré et non supposé : **pas d'interleave** (le muxeur n'a
jamais démarré, `Output #0` absent), **pas de famine audio** (après connexion,
tous les blocs PCM sont écrits, aucun saut), et la piste « pipe audio plein à
+1,37 s » est **fausse** — c'est le **stdin vidéo** qui sature, jamais le pipe
audio.

**Correctif** (`output/ffmpeg.rs`) : le format PCM est entièrement spécifié par
`-f/-ar/-ac`, donc `-analyzeduration 1` + `-probesize 32` sur l'entrée audio.
La fenêtre tombe de 819 200 o (4,27 s) à **16 384 o (~0,09 s)**.

**Vérification** : les quatre variantes du harnais passent (472 composites en
8 s, fichiers de 73 à 186 Ko), la variante qui gelait produit H.264 1360×768
@ 59,84 fps + **AAC 48 kHz stéréo 193 kbps** ; `audio_scene` 10 s → 590
composites, `degradation=None`, deux flux, 9,87 s. `cargo clippy --workspace
--all-targets` 0 warning, `cargo test --workspace` (299 + 246 + 12 + 9).

**Reste assumé** :

| Sujet | Mesure | Suite |
|---|---|---|
| Marge à haut débit | fenêtre résiduelle ~0,09 s ≈ 64 Ko à 6 000 kbps, soit exactement le tampon du stdin vidéo → marge **nulle** sur une scène très chargée au démarrage | exprimer `FILE_ECRITURE` en **octets** plutôt qu'en paquets |
| Décalage audio au démarrage | les blocs PCM écrits avant le `CreateFile` du client sont sautés (~50 blocs ≈ 190 Ko ≈ **1 s**) ; si l'audio FFmpeg démarre à PTS 0, le son serait ~1 s en avance sur l'image — **hypothèse non vérifiée** | test : son connu à t = 0, puis écart image/son |

**État** : `srPointsSon` est **implémenté** (contrat + rendu + mixeur, test
unitaire `son_du_pop_une_version_par_pop_et_gain_suivi`) et le harnais
pop + audio termine désormais avec une piste AAC pleine. Le **son** du pop
lui-même reste **non vérifié à l'oreille** : la scène du harnais n'avait pas de
`srPointsSon`. `examples/audio_scene.rs` reste la référence qui passe.
