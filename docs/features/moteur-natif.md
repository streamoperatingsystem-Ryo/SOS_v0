# Moteur natif — capturer, composer et encoder sans OBS

> **Feature NON IMPLÉMENTÉE** au 2026-10-27 (LOT 0 non commencé). Ce fichier
> fixe ce qui est **décidé** et ce qui est **interdit** ; le déroulé complet
> (mesures, matrices matérielles, algorithmes) vit dans
> `PLAN-MOTEUR-NATIF.md` à la racine.
> ⚠️ **Aucun chiffre de ce fichier n'est mesuré.** Toute valeur est une
> **cible** du plan, jamais un résultat observé. Ne pas citer ces nombres
> comme acquis.

## Objectif

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

### Cibles mesurables (plan §1.2 — à confirmer par les mesures P0)

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

## Décisions tranchées (session du 2026-10-27)

| # | Décision | Conséquence |
|---|---|---|
| D1 | Moteur **full natif en Rust**, aucun DOM Chromium dans la sortie | pas de calque WebView2 capturé |
| D2 | Fond vert virtuel **hors périmètre** | `Capacites.fond_vert = false` en natif |
| D3 | Le système de cadres **sera refait** | le moteur n'implémente **pas** les 9 styles historiques : tuyau + 2 styles représentatifs |
| D4 | Le chat doit **fonctionner comme aujourd'hui** | fonctionnel inchangé, seul le rendu change |
| D5 | Support matériel **total** | MFT universel + fallback logiciel obligatoire |
| D6 | Diagnostic moteur dans la section **Diagnostic existante** | onglet « Moteur », lisible par un non-expert |
| D7 | Priorité : capture + enregistrement fluides, moins lourds qu'OBS | gouverneur de dégradation |

## Architecture — trois anneaux + règle de dépendance

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

### Trait `Moteur` (contrat de l'anneau 2)

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

### Trait `Sink` — un seul encode, N sorties

```rust
pub trait Sink: Send {
    fn pousser(&mut self, paquet: &PaquetEncode) -> Result<(), ErreurMoteur>;
    fn fermer(&mut self) -> Result<(), ErreurMoteur>;
}
// FileSink(fMP4/MKV) · PreviewSink · RtmpSink · SrtSink · TeeSink(Vec<Sink>)
```

**MVP = `FileSink` + `PreviewSink`.** RTMP/SRT arrivent ensuite **sans
réécriture** : ce sont des sinks supplémentaires, pas un second encodeur.

## Mode dual — états et règles de sécurité

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

## Support matériel

- **Détection d'encodeur** au démarrage de sortie : énumération des MFT
  (`MFT_ENUM_FLAG_HARDWARE`), choix par constructeur, repli **logiciel**
  obligatoire — jamais d'échec silencieux faute d'encodeur.
- **Matrice de capacités** à détecter au démarrage (plan §6.1) et **matrice de
  test** à remplir par l'utilisateur, une ligne par machine (§6.4).
- Pièges matériels documentés dans le code (§6.3) : ne pas les redécouvrir.
- Vendor IDs attendus : `0x10DE` NVIDIA, `0x1002` AMD, `0x8086` Intel,
  `0x1414` Microsoft (logiciel / WARP).

## Rendu natif

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

### Chat — ce qui change, ce qui ne change pas (D4)

| Aspect | Sort |
|---|---|
| **Fonctionnel** — 4 plateformes (Twitch IRC, YouTube polling, Kick Pusher, TikTok Webcast), emotes, badges, avatars, filtres, viewers, commandes `!cmd`, `!cadres` | **Aucun changement.** Aucun lot du moteur ne touche `twitch_chat.rs`, `kick.rs`, `youtube_chat.rs`, `tiktok_chat.rs`, `twitch_emotes.rs` |
| **Représentation** — le rendu à l'écran | Seul point concerné : DOM Chromium en mode OBS, **texte rasterisé en natif**. Le résultat visuel doit être identique (couvert par le harnais) |

> La question du chat n'est donc **pas une question produit** : c'est un choix
> d'implémentation du renderer.

## Diagnostic « Moteur » — onglet de `ConsoModal.svelte`

> Exigence utilisateur (2026-10-27) : dans la section Diagnostic existante,
> afficher **clairement, pour un non-expert**, ce que le moteur consomme —
> et **tourne-t-on avec le pilote graphique, lequel, en matériel ou en
> logiciel ?**

### Principe

- **Aucune valeur fabriquée.** Mesure échouée → `N/A`, **jamais `0`**.
- **Une phrase de verdict** en tête, pas seulement des chiffres.
- **Zéro polling** hors modale ouverte (invariant `conso.rs`).
- **100 % local**, conforme au CONTRAT FIGÉ vie privée.
- Le diagnostic fonctionne **même en mode OBS** : l'énumération d'adaptateur
  est sans effet de bord, l'utilisateur doit toujours voir son matériel.
- **Découplage strict** : le moteur écrit des atomics, le diagnostic lit. Le
  moteur ne dépend **jamais** du diagnostic.

### Ce qui existe déjà (à réutiliser, pas à réécrire)

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

### Données ajoutées

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

### Modèle

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

### UI — ordre de lecture, du verdict vers le détail

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

### Boîte noire — champs ajoutés

`boite_noire.jsonl` gagne, de façon **additive** (rétro-compatibilité des
lignes existantes) : `moteur_actif`, `encodeur`, `materiel`, `fps_composite`,
`drops`, `gpu_moteur_pct`, `degradation`. C'est la seule trace qui survit à un
gel : elle doit dire **quel étage** a lâché, pas seulement que l'app a gelé.

### Règles

1. Aucune valeur inventée ; `N/A` explicite.
2. Aucun polling hors modale.
3. Énumération d'adaptateur autorisée au boot et à la demande (coût
   négligeable, aucun effet de bord).
4. Le diagnostic n'est **jamais** nécessaire au fonctionnement du moteur.
5. Toute nouvelle mesure doit être documentée dans
   `docs/features/diagnostic.md`.

## Invariants

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

## Lots et gates

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
| **P5** — Différenciation | robustesse, cas dégradés | — |

## Fichiers — cible (⚠️ aucun n'existe à ce jour)

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

## Points ouverts (à trancher avant la phase concernée)

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
