# HANDOFF — StreamOS (réécrit à chaque fin de session)

> Session 2026-11-02 — **parité moteur natif** : **lot H terminé**
> (H1 ordre z des titres, H2/H2b/H2c contenus de widget + titres sous
> perspective, H3 `line-height` + marges, H4 contour exact par géométrie de
> glyphes) + correction du **chat** cassé (lot G), de l'espacement pseudo/`:`
> du DOM, du **gel audio n° 1** (lot I-a), du **gel n° 2** (lot I-b, cause
> mesurée : fenêtre d'analyse PCM de FFmpeg) et **son du pop de split** (§68).
> Arbre **vert** : `cargo clippy --workspace --all-targets` 0 warning,
> `cargo test --workspace` (299 crate + 246 app), `npm run check` 0 erreur.
>
> **Prochain lot : reste de I** — deux points ouverts du §75 (marge nulle à
> 6 000 kbps : `FILE_ECRITURE` en octets ; ~1 s de PCM perdue au démarrage →
> décalage audio à vérifier), puis canvas différent en cours de sortie,
> `hwnd` périmé, PlayerBar/seek, `capture_jeu`, relance caméra.

## Prompt de reprise (à coller dans une nouvelle fenêtre)

```text
Contexte : dépôt StreamOS v0 (C:/Users/Ryo/Desktop/Rust-SOS/SOS_v0), moteur
natif Direct2D en parité avec le mode OBS. Lis d'abord, dans cet ordre :
1) AGENTS.md — « Protocole d'exécution », tableau « Plan de suivi — parité
   moteur natif » (lots A→J) + « Reprise » + « Pièges déjà payés ».
2) docs/HANDOFF.md — état de la session + le prompt de reprise ci-dessous.
3) docs/features/moteur-natif.md §58 (reste à faire), §70-§75 (lot H
   complet + I-a/I-b) comme modèles de structure, §41 pour la perspective,
   §74-§75 pour les deux gels.

Objectif : **reste du lot I**. Le gel n° 2 est levé (§75) ; ce qui reste :
(a) marge nulle à 6 000 kbps — exprimer `FILE_ECRITURE` en **octets** ;
(b) ~1 s de PCM perdue au démarrage → vérifier si le son avance sur l'image ;
(c) canvas différent en cours de sortie, (d) `hwnd` périmé, (e) PlayerBar/seek,
(f) `capture_jeu`, (g) relance caméra sans mutation de scène. Chaque tranche
sur un arbre vert.

État : E6a-d, E7, F, **lot H complet** et I-a/I-b sont faits (299 + 246
tests, clippy 0).
- E6 = présentation streamer complète (`texte/presentation.rs`) ;
- E7 = pop de validation des splits (`texte/splitpop.rs`, z 9993) ;
- F = Just Chatting (commande `moteur_appliquer_scene`) ;
- H1 = `GenreCalque::Titre` (titres de widget à leur z) ;
- H2/H2b/H2c = les cinq contenus de widget + les titres dessinent dans
  l'offscreen du plan `perspective(d) rotateX/Y` ;
- H3 = `line-height: 1.1`, marges ±2 px, boîte CSS distincte de la texture ;
- H4 = contour exact (`GetGlyphRunOutline` + `Widen`), repli 16 passes pour
  les polices simulées ;
- chat réparé (deux `inline.push(brush.cast())` du lot G) + espacement 2 px
  pseudo/`:` du DOM ;
- I-a = pipe audio non bloquant (gel n° 1 levé, §74) ;
- I-b = son du pop de split porté (§68) + **gel n° 2 levé** (`-analyzeduration
  1` / `-probesize 32` sur l'entrée PCM, §75).

Diagnostic lot I (à re-vérifier dans le code) :
- (f) le gel n° 2 est **résolu** : la fenêtre d'analyse PCM de FFmpeg
  (`avformat_find_stream_info`, 819 200 o = 4,27 s au rythme réel) empêchait
  toute lecture du stdin vidéo ; le tampon (74 055 o) débordait, la composition
  bloquée n'alimentait plus le pipe audio → interblocage. Ne pas réintroduire
  d'entrée PCM sans couper son analyse ;
- ⚠️ le harnais `examples/audio_scene.rs` (sans widget speedrun) **ne se fige
  plus** : c'est la référence de vérification de l'audio (§74).

Règles de travail (inchangées) :
- DIAGNOSTIC AVANT ÉCRITURE pour chaque tranche : lire le code réel et
  afficher le diagnostic AVANT toute modification.
- Un fichier = un diff ; une tranche se termine sur un ARBRE VERT
  (cargo clippy --workspace --all-targets, cargo test --workspace,
  npm run check).
- Le crate sos-moteur-natif ne fait AUCUN réseau (§13.8) : images et MP4
  passent par le cache disque de l'app (`medias/cache/`, `medias/clips/`).
- Toute exception réseau se documente dans le CONTRAT FIGÉ vie privée
  d'AGENTS.md AVANT le code.
- Les dégradations assumées s'écrivent en tête du renderer ET dans la doc.

Commence par : (1) lire les 3 documents, (2) produire le diagnostic du point
(a) ou (f), (3) proposer le découpage en fichiers, puis implémenter.
```

## État

| Lot | Contenu | Tests |
|---|---|---|
| **E1** | Overlay **clip de bienvenue** `z 9999` (squelette + spotlight) | 228 + 223 |
| **G** | **Chat** : badges emoji, logos, avatars, emotes inline, cache disque | 233 + 228 |
| **E2** | **Bandeau premier message** `z 9998` | 238 + 230 |
| **E3** | **Écran d'attente BRB** `z 9990` (plein écran + média + signature) | 248 + 233 |
| **E4** | **Pad numérique** `z 9995` (carte média + son + plein écran) | 253 + 236 |
| **E5** | **Emoji Arena** `z 9993` (arène 1v1 + animations) | 261 + 238 |
| **E6a** | **Présentation streamer — décor + rideaux** `z 10000/10001/10003` | 273 + 241 |
| **E6b** | **Présentation streamer — carte** `z 10002` | 280 + 241 |
| **E6c** | **Présentation streamer — clips** `z 10004` (grille + lecteur) | 283 + 244 |
| **E6d** | **Présentation streamer — audio** (`voix` / `musique`) | 286 + 245 |
| **E7** | **Pop de validation des splits** `z 9993` | 293 + 246 |
| **F** | **Just Chatting** — scène dérivée poussée au moteur natif | 293 + 246 |
| **H1** | **Titres de widget dans l'ordre z** | 293 + 246 |
| **H2** | **Contenus de widget + titres sous perspective** | 294 + 246 |
| **H3** | **`line-height` + marges des titres** | 297 + 246 |
| **H4** | **Contour exact par géométrie de glyphes** | 298 + 246 |
| **I-a** | **Gel de composition — pipe audio non bloquant** | 298 + 246 |
| **I-b** | **Son du pop + fil d'écriture du sink** (§68, §75) — gel n° 2 ouvert | 299 + 246 |

## Fait [EXÉCUTÉ] — E6a

### Canal réel (diagnostic)

- Frames **`chat_tx` `presentation-show`** (payload `mode` + `donnees`),
  **`-hide`**, **`-mode`** (`presentation.rs` de l'app, 367-392 + 267-272).
  ⚠️ `presentation:etat` = **dashboard** (favoris + contrôles live).
- `presentation-show` n'arrive **qu'une fois** : toute la chorégraphie est
  détenue par l'horloge locale de l'overlay (aucun tick reçu).

### Fichiers

| Fichier | Rôle |
|---|---|
| `texte/presentation.rs` (nouveau, ~2080 l.) | `ModePresentation` / `PresentationDonnees` / `JeuRecent` / `ClipPresentation` / `MajPresentation` (donnees **boxé**) / `EtatPresentation` (phases `voile`/`valance`/`public`/`embrasses`/`rideaux`) + rendu : voile (fond radial, bannière + scrim, orbes, sol, spots, poussière), rideaux + cantonné + embrasses, public (3 rangées, fauteuils en géométrie) |
| `texte/mod.rs` | module + exports (`Z_PRES_*`, `COUCHE_*`) + champs `bloc_presentation`, `pres_banniere*` |
| `moteur.rs` | `presentation_etat`, `pousser_presentation`, `presentation_version`, `GenreCalque::Presentation` (**l'index porte la couche**), 3 entrées de plan, dispatch, `VoieRendu::dessiner_presentation` |
| `src/moteur/mod.rs` | `decoder_frame_presentation` + abonnement + préchargement de la bannière + 3 tests |

### Décisions / pièges payés

- **Trois couches, trois entrées de plan** (10000 voile, 10001 rideaux,
  10003 public) : la carte 10002 (E6b) et le clip 10004 (E6c) s'intercalent
  sans renuméroter. Le public est **devant** la carte, comme au théâtre.
- **Mode `card` : le voile reste affiché** — le commentaire CSS/JS dit « sans
  voile complet » mais le code fait `display: block` + `.visible`. Le code
  fait foi (vérifié au rendu).
- **Piège payé (rendu)** : fauteuils en « M ». `D2D1_ARC_SEGMENT` +
  `SWEEP_DIRECTION_CLOCKWISE` + `ARC_SIZE_SMALL` sur des extrémités à 90°
  **anti-horaires** → D2D trace les 270° restants vers l'intérieur. Les 4
  coins sont `COUNTER_CLOCKWISE`.
- `repeating-linear-gradient` = **une** brosse `D2D1_EXTEND_MODE_WRAP` sur la
  période de 88 px (plis) — pas de pavage manuel.
- `Math.random()` (poussière + public) → xorshift à graine fixe : décor
  stable entre sessions, pas identique au DOM.
- `border-radius: 46% 46% 8% 8% / 32% 32% 5% 5%` → géométrie à deux rayons
  par coin (un `D2D1_ROUNDED_RECT` n'en a qu'un).
- Bannière distante : préchargement app + `image_cache`, **re-tentative
  500 ms** tant que le fichier manque (équivalent du `<img>` asynchrone).
- Un `show` pendant le fondu de sortie repart des phases **courantes**
  (`depart`), jamais du noir ; une bascule de mode rejoue la chorégraphie
  sans re-fondre le voile.

### Vérification visuelle

Harnais temporaire (`examples/presentation_scene.rs`, supprimé) : rendu de
l'overlay seul en 1280×720, frames extraites — théâtral à mi-ouverture
(rideaux froncés, bord doré, ourlet, embrasses), théâtral ouvert (ouverture
~11,5 % par côté, public + cantonné en place) et mode `card` (voile + 3
faisceaux + orbes + poussière + sol, sans rideaux ni public).

## Fait [EXÉCUTÉ] — E6b

### Fichiers

| Fichier | Rôle |
|---|---|
| `texte/presentation.rs` | `COUCHE_CARTE` / `Z_PRES_CARTE`, phases `carte_opacite` / `carte_avance` (retard 0,65 s, courbe à dépassement), `BlocCarte` = **liste d'éléments** (`Texte`/`Pastille`/`Cercle`/`Avatar`/`Image`), `construire_carte` (mesures + `repartition_colonnes`), `peindre_carte` (couche d'opacité + transform), `url_box_art`, `formater_viewers`, `image_presentation` (cache + re-tentative 500 ms) |
| `texte/mod.rs` | `bloc_carte`, `pres_images_tentative` (remplace les 3 champs `pres_banniere*`), exports |
| `moteur.rs` | entrée de plan `(Z_PRES_CARTE, Presentation, COUCHE_CARTE)` |
| `src/moteur/mod.rs` | préchargement avatar + `url_box_art` des boîtes de jeux (même fonction que le crate) |

### Décisions / pièges

- **Le `flex` du DOM est résolu à la main** : largeur naturelle de chaque
  colonne (mesurée sans wrap), bornée par son `max-width` (460/620), puis
  réduction **proportionnelle** si le total dépasse 73 % du canvas — c'est
  `flex-shrink: 1` avec des bases de contenu. En mode `card` : colonnes
  empilées, carte bornée à 760 px, textes centrés.
- **`url_box_art` est partagé app/crate** (comme `empreinte_url`) : les
  `box_art_url` Helix portent `{width}x{height}`, l'app précharge la **même**
  URL que celle que le moteur cherche dans `medias/cache/`.
- **La grille de clips 2×2 reste en E6c** : ses vignettes vivent sur
  `clips-media-assets2.twitch.tv`, hôte absent du contrat vie privée.
- `box-shadow: 0 0 0 6px` (anneau de l'avatar) **est porté** — débordement
  seul, sans flou : c'est un trait, pas une ombre.
- Images : un fichier manquant **omet** l'élément (pas de trou visuel) et
  re-tente toutes les 500 ms.
- Pas de test GPU : la mise en page n'est testée que par ses invariants purs
  (`repartition_colonnes`, formats, phases) ; le rendu est vérifié au harnais.

### Vérification visuelle

Harnais temporaire (supprimé) en 1280×720 avec un **cache disque local**
(faux avatar + fausses boîtes) : théâtral — avatar rond + anneau violet, nom,
`@login`, badge AFFILIATE en dégradé, bio rognée à 5 lignes, « Joue à : … »,
`twitch.tv/manu`, `EN DIRECT · 1 234`, titre sur 2 lignes, 2 boîtes + noms ;
mode `card` — colonnes empilées et tout centré (carte plus haute que le canvas
en 720p, rognée haut/bas comme le DOM).

## Fait [EXÉCUTÉ] — E6c

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/cache_clips.rs` (**nouveau**) | Préchargement des MP4 signés → `medias/clips/<fnv16(url)>.mp4` (client partagé, écriture atomique, purge à 8 fichiers, 256 Mio max) + 2 tests |
| `src-tauri/src/lib.rs` | `mod cache_clips;` |
| `src-tauri/src/presentation.rs` | `clip_jouer` précharge le MP4 (tâche de fond) avant d'émettre la frame |
| `texte/presentation.rs` | `MajPresentation::ClipPlay/ClipStop`, `ClipEnCours` (fin sur `duree_ms`), `Z_PRES_CLIP`/`COUCHE_CLIP`, `chemin_clip`, `Elem::Clip` + `dessiner_clip` (grille 2×2), `peindre_clip` (lecteur), `synchroniser_clip` (fichier local, re-tentative 500 ms), `SetTrimming` (ellipse) + 4 tests |
| `texte/mod.rs` | `pres_clip_media` / `pres_clip_url` / `pres_clip_tentative` / `bloc_clip`, exports |
| `moteur.rs` | entrée de plan `10004`, **son du clip** dans le mixeur (`anneau_audio_presentation`, gain 1000) |
| `src/moteur/mod.rs` | décodage `presentation-clip-play`/`-stop`, préchargement des vignettes de la grille + test |

### Décisions / pièges

- **Contrat vie privée d'abord** : `clips-media-assets2.twitch.tv` documenté
  dans `AGENTS.md` **avant** la première ligne de code (règle du dépôt).
- **La frame n'est pas modifiée** : `mp4_url` reste l'URL signée (le DOM la
  lit) et sert de **clé** au fichier local (`chemin_clip` ↔ `cache_clips::chemin`,
  même `empreinte_url`).
- Émission **immédiate** + re-tentative 500 ms côté moteur : le bouton du
  dashboard ne bloque pas, le lecteur affiche voile + titre (« buffering »).
- **Piège payé** : `DWRITE_WORD_WRAPPING_NO_WRAP` ne rogne pas — le titre de
  vignette passait sous la pastille de durée. `SetTrimming` +
  `CreateEllipsisTrimmingSign` (miroir de `text-overflow: ellipsis`).
- **Écart assumé** : la pastille de durée est peinte **au-dessus** du bandeau
  de titre (le DOM la met dessous, les deux deviennent illisibles) et le titre
  est rogné **avant** elle.
- Son du clip mixé (anneau du média, gain 1000) — le DOM ne le coupe qu'en
  `PREVIEW`.

### Vérification visuelle

Harnais temporaire (supprimé) en 1280×720, cache disque + MP4 local : grille
2×2 (vignettes, `30s` / `14s`, titres `Le run com…`), puis lecteur plein écran
(voile 90 %, vidéo centrée arrondie, titre 30 px en haut).

## Fait [EXÉCUTÉ] — E6d

| Fichier | Rôle |
|---|---|
| `texte/presentation.rs` | `PistePresentation` (`voix`/`musique`, `boucle()`), `MajPresentation::AudioPlay/AudioStop`, `EtatPresentation.audio`, `RenduPresentation.audio`, `gain_piste_milliemes`, `synchroniser_audios` (ouverture, avance, **réouverture** pour la boucle `musique`), `anneau_audio_piste` + 3 tests |
| `texte/mod.rs` | `pres_audios` / `pres_audio_rel` |
| `moteur.rs` | 2 entrées de mixeur (gain de la piste) ; `anneau_audio_presentation` → **`anneau_audio_clip`** (renommage, E6c) |
| `src/moteur/mod.rs` | décodage `presentation-audio-play` / `-audio-stop` (`toutes`) + test |

- `src` = `medias/…` → fichier **local** : aucun nouvel hôte, aucun contrat
  vie privée à modifier.
- `musique` reboucle par **réouverture** du fichier en fin de flux (`loop` du
  DOM) ; `voix` s'arrête seule.
- `show` et `hide` coupent **toutes** les pistes (miroir
  `presentationAudioStop("toutes")`).
- Piste illisible : omise une fois (log `debug`), jamais réessayée en boucle.

**Vérification** : la piste est bien **ouverte et avancée** en conditions
réelles (harnais : aucune ligne « piste ignorée » sur un WAV local, et
l'exécution complète reste verte sans audio). **Pas de vérification de bout
en bout** : le harnais avec loopback se fige dans `Apercu::lire` — bug
**pré-existant reproduit sans la présentation** (`audio_scene`, 388
composites puis gel). Signalé au §67 de `moteur-natif.md` et ci-dessus.

## Fait [EXÉCUTÉ] — E7

| Fichier | Rôle |
|---|---|
| `texte/splitpop.rs` (**nouveau**, ~1050 l.) | `PopSplit` / `MajSplitPop` / `EtatSplitPop` (dédup 1,5 s, durée, re-pop) + rendu : aura, badge « SPLIT n », hero Anton `skewX(-5deg)`, mot VALIDÉ/SKIP, impact, flash approché + 7 tests |
| `texte/speedrun.rs` | `SpeedrunCfg.voir_points` / `points_duree_pop`, `EtatSpeedrun::pop_pour` (miroir `srPointEtatFranchi`) |
| `scene_sources.rs` | alimente les 2 nouveaux champs |
| `texte/mod.rs` / `moteur.rs` | `bloc_splitpop`, `splitpop_etat`, `pousser_splitpop`, entrée de plan `9993`, `GenreCalque::SplitPop` |
| `src/moteur/mod.rs` | `decoder_frame_splitpop` (`splitpop-pos`, squelette **dédié**) + test |

- **Le contenu ne vient d'aucune frame dédiée** : le pop est dérivé des
  mutations `speedrun-etat` lues avant application — c'est le même flux que
  le widget, donc aucune divergence possible entre le splitter et le pop.
- Le squelette `splitpop-pos` est **distinct** de `position-overlay-config`
  (vérifié par test : les deux frames ne se confondent pas).
- **Piège payé** : `CENTER` dans une boîte de 4096 px → texte hors écran.

## Fait [EXÉCUTÉ] — F (Just Chatting)

| Fichier | Rôle |
|---|---|
| `src-tauri/src/moteur/mod.rs` | commande **`moteur_appliquer_scene(scene)`** → `MoteurNatif::appliquer_scene` (différentiel, rien sur disque) |
| `src-tauri/src/lib.rs` | enregistrement de la commande |
| `src/lib/tauri.ts` | wrapper `moteurAppliquerScene(scene)` |
| `src/lib/stores/justChatting.ts` | `syncNatifJC` : la **même** copie part aux deux rendus — ON (dérivée), OFF (scène réelle), hook post-sync captures (nouvelle scène pendant JC) |

Le mode JC reste **non destructif** : `sceneDerivee` est une copie en mémoire,
`state.scene` et le disque ne sont jamais touchés.

## Fait [EXÉCUTÉ] — H1

`GenreCalque::Titre` (entre le contenu et la bordure) + une entrée de plan par
titre de **widget** à son `z` : un titre n'est plus peint au-dessus de toute la
pile (overlays, cadres). Les titres de **fond** (`bgTitre*`, `z = i32::MIN+1`)
restent en passe finale, sinon ils passeraient sous le média de fond.
Vérifié par construction (§70) ; pas de harnais visuel pour ce diff.

## Fait [EXÉCUTÉ] — H2 / H2b / H2c

| Fichier | Rôle |
|---|---|
| `moteur.rs` | `Chat`, `Input`, `Goal`, `Speedrun`, `Musique` et `Titre` sortent de la branche `None` et passent par `groupe_de(rect, z)` comme le média |
| `scene_sources.rs` | test `persp_et_contenu_chat_input_partagent_la_cle` : la clé `(rect, z)` de `persps_depuis_scene` est **identique** à celle des cinq cfgs de contenu **et** du titre de widget (`TexteCfg.cadre`), et un widget plat n'a pas de plan |
| `texte/chat.rs` | chat réparé : les deux `inline.push(brush.cast())` (puce + séparateur `:`) sont supprimés, `_inline` documenté |

- **Rien d'autre n'a changé** : `cible_rt` est `cible.cast::<ID2D1RenderTarget>()`
  du **même** `ID2D1DeviceContext` (`texte/mod.rs`, `nouveau`) → `SetTarget`
  redirige déjà `DrawTextLayout`, `PushAxisAlignedClip` et `PushLayer`. C'est
  exactement le chemin de `dessiner_media` ; aucun offscreen « de contenu »,
  aucun blit. Le diagnostic de reprise (« demande de composer leur contenu
  dans l'offscreen, pas seulement un blit ») était trop pessimiste.
- **Titre** : `textes_depuis_scene` pose déjà `cadre = (w.x, w.y, w.largeur,
  w.hauteur)` + `z = w.z` pour un titre de widget — la clé du plan, sans
  aucun champ ajouté. Le titre est placé **hors** du rect, mais le DOM le
  tourne autour du centre de `.widget` : projeter l'offscreen entier est
  fidèle.
- Anticrénelage : `D2D1_TEXT_ANTIALIAS_MODE_DEFAULT` suffit — ClearType sur
  le canvas opaque, repli grayscale sur l'offscreen prémultiplié.
- **Piège payé** : le harnais a buté sur un bug **pré-existant** du lot G —
  `construire_bloc` poussait un `ID2D1SolidColorBrush` dans
  `_inline: Vec<IDWriteInlineObject>` → `E_NOINTERFACE`, moteur en `Erreur`,
  zéro composite (reproduit **sans** H2, `examples/chat_scene.rs`). Corrigé
  ici (deux `push` supprimés) et documenté dans `AGENTS.md`.
- **Écart restant, non corrigé** : le DOM espace `.pseudo` et `.separateur`
  de `margin-right: 2px` ; le moteur compose `"{pseudo}:{texte}"` sans jeu
  (le `:` est collé). Correctif possible : `SetCharacterSpacing(0, 0, 2.0,
  plage)` sur les deux plages.

### Vérification visuelle

Deux harnais temporaires (supprimés) en 1360×768, widget plat à gauche
(référence), `rotateY 38.25` à droite :
1. **chat** (même buffer des deux côtés) — pseudo, `:`, texte, bulles et
   bordures dans le trapèze (bord droit qui recule, texte qui rétrécit) ;
2. **titre** (`titreHaut` sur chacun) — « TITRE TOURNE » suit le trapèze,
   « TITRE PLAT » reste droit.

`goal`/`speedrun`/`musique` : vérifiés **par la clé** seulement (même
mécanisme, même `cible_rt`) — leur contenu exige un état live pour produire
des pixels.

## Fait [EXÉCUTÉ] — H3

| Fichier | Rôle |
|---|---|
| `texte/mod.rs` | `INTERLIGNE_TITRE = 1.1` + `SetLineSpacing(UNIFORM, 1,1 × taille, baseline)` calculé depuis `GetLineMetrics` (half-leading CSS) ; `CacheTexte.largeur/hauteur` = **boîte CSS**, `marge` séparée (contour + débord des glyphes) et destination élargie au dessin ; `MARGE_TITRE_WIDGET = 2.0` |

- Avant : DirectWrite mesurait la hauteur **naturelle** (~1,2-1,35 em) et la
  boîte servait au placement **avec** la marge de contour → titre trop haut
  et décalé de `debord` (≈3 px), sans les `margin-*: ±2px` du DOM.
- Test GPU `titre_mesure_la_boite_de_ligne_css` : `hauteur == 1,1 × taille` à
  0,01 px près pour 24 / 40 / 96 px.

## Fait [EXÉCUTÉ] — H4

| Fichier | Rôle |
|---|---|
| `texte/mod.rs` | `CollecteContours` (`IDWriteTextRenderer` → `GetGlyphRunOutline` par run, translaté à son origine), `contours_glyphes`, `Widen(epaisseur)` + `FillGeometry` pour le contour ; **repli 16 passes** si `GetSimulations() != 0` |

- Le **remplissage** reste `DrawTextLayout` (dégradé, soulignement, objets
  inline) : `paint-order: stroke fill` est respecté tel quel.
- **Piège payé** : `GetGlyphRunOutline` rend le contour **non embelli** d'une
  police simulée. Les 20 familles embarquées n'ont qu'une graisse → tout
  `titreGras`/`titreItalique` est simulé et garde l'approximation.
- Test GPU `contour_exact_sauf_police_simulee` : tracé non vide en normal,
  `None` en gras.
- Vérifié au rendu : halos réguliers (6 px) sur deux polices, gras inchangé.

## Fait [EXÉCUTÉ] — I-a (gel de composition)

| Fichier | Rôle |
|---|---|
| `output/ffmpeg.rs` | `PipeAudio` en `PIPE_NOWAIT` (`SetNamedPipeHandleState` sur le handle serveur, création laissée en `PIPE_WAIT`) ; `ERROR_PIPE_LISTENING`/`NOT_CONNECTED`/`NO_DATA`/`BUSY` et `n == 0` → **bloc sauté** |

- **Le diagnostic d'origine était faux** : le gel n'était pas dans
  `Apercu::lire` mais dans `PipeAudio::ecrire`. Localisé par marqueurs
  temporaires : `mix apres` → 10 s de rien → `debut avant`, le seul appel
  bloquant entre les deux étant `WriteFile` sur le pipe nommé.
- **Interblocage à deux flux** : FFmpeg cesse de drainer l'audio en attendant
  la vidéo, la composition bloquée n'envoie plus de vidéo.
- **Vérification** : `examples/audio_scene.rs` (loopback, 15 s) — avant 388
  composites puis gel ; après **890 composites**, `degradation=None`,
  `Fin — état Inactif`, `ffmpeg -i` → H.264 1360×768 @ 59,91 fps, 14,85 s +
  AAC 48 kHz stéréo.
- **Reste assumé** : le stdin vidéo (`FfmpegSink`) reste bloquant — non
  reproduit, mais le risque est symétrique (documenté §74).

## Fait [EXÉCUTÉ] — I-b (son du pop de split)

| Fichier | Rôle |
|---|---|
| `texte/speedrun.rs` | `SpeedrunCfg.points_son` / `points_son_volume` |
| `scene_sources.rs` | alimente les deux champs depuis `srPointsSon` / `srPointsSonVolume` |
| `texte/splitpop.rs` | `MajSplitPop::Afficher { son, volume }`, `EtatSplitPop::son()` + `son_version` (une par pop, pas sur doublon), `gain_son_milliemes`, `synchroniser_son_splitpop` (réouverture par pop + avance), `anneau_audio_pop` |
| `texte/mod.rs` | 3 champs (`splitpop_audio`, `_rel`, `son_vu`) |
| `moteur.rs` | transporte `son`/`volume` au pop + une entrée de mixeur (gain du pop) |

- Miroir du DOM : `new Audio("/" + pointsSon)` **par pop** (pas sur re-pop),
  volume `pointsSonVolume`.
- Test unitaire `son_du_pop_une_version_par_pop_et_gain_suivi` : pas de son
  hors pop, version stable sur doublon, version neuve au pop suivant, gain
  0/500/1000.
- ⚠️ **Pas de vérification de bout en bout** : le harnais audio + widget
  speedrun se fige (§75, gel n° 2) — **et il se fige sans le son aussi**,
  donc le son du pop n'est pas en cause. Statut identique à E6d à sa
  livraison.

## Fait [EXÉCUTÉ] — I-b (fil d'écriture du sink)

| Fichier | Rôle |
|---|---|
| `output/ffmpeg.rs` | `FfmpegSink` : fil d'écriture dédié + `sync_channel(8)` ; `pousser` = `try_send` borné à 3 s puis `Err("FFmpeg ne draine plus")` ; `fermer` = drop + `join` (flush/EOF) ; `Drop`/`couper` = drop + `kill` |
| `moteur.rs` | l'arrêt d'une sortie garde sa **cause** : `"toutes les sorties ont échoué — …"` |

- Mesuré avant : `sink avant t=195` → **1,5 s** → `sink apres t=195` → plus
  rien (blocage définitif). Après : la composition ne bloque plus jamais ;
  FFmpeg figé = `Erreur("… FFmpeg ne draine plus (sortie figee)")` au bout de
  3 s, et le harnais **termine proprement**.
- **Gel n° 2 levé** (§75) : la cause n'était pas le pop mais la **fenêtre
  d'analyse PCM** de FFmpeg. Voir `## Fait [EXÉCUTÉ] — I-b (gel n° 2)`.

## Fait [EXÉCUTÉ] — I-b (gel n° 2 : fenêtre d'analyse PCM)

| Fichier | Rôle |
|---|---|
| `output/ffmpeg.rs` | entrée audio : `-analyzeduration 1` + `-probesize 32` — le format est spécifié par `-f/-ar/-ac`, aucune analyse n'est due |

- **Cause mesurée** (`SOS_FFMPEG_DEBUG` = `-loglevel debug -debug_ts`, retiré) :
  `avformat_find_stream_info` sur le pipe PCM lisait **819 200 o = 4,27 s au
  rythme réel** *sans lire stdin* ; le stdin vidéo absorbait **74 055 o** puis
  bloquait, la composition cessait d'alimenter le pipe audio → interblocage.
  Le pop n'était qu'un seuil de débit (IDR de 9 685 o).
- **Réfuté** : interleave (muxeur jamais démarré, fichier de sortie jamais
  créé), famine audio (tous les blocs PCM écrits après connexion), et
  « pipe audio plein ».
- **Vérification** : 4 variantes du harnais (widget/aucun × pop/nopop) → 472
  composites en 8 s ; la variante qui gelait donne H.264 59,84 fps + AAC
  193 kbps ; `audio_scene` 10 s → 590 composites, `degradation=None`.
- **Reste ouvert** : marge nulle à 6 000 kbps (~0,09 s ≈ 64 Ko = le tampon du
  stdin) et ~1 s de PCM perdue au démarrage (décalage audio à vérifier).

## Suite

**Lot I (suite)** : `FILE_ECRITURE` en octets (marge à haut débit), décalage
audio au démarrage, canvas différent en cours de sortie, `hwnd` périmé,
PlayerBar/seek, `capture_jeu`, relance caméra sans mutation de scène, puis
**son du pop de split à l'oreille** (§68 — le harnais ne se fige plus, il reste
à lui donner un `srPointsSon`). Enfin **J** (fond vert sans OBS).

## Repères de méthode (ne pas les redécouvrir)

- **Diagnostic avant écriture**, puis un fichier = un diff. Un lot se termine
  sur un arbre vert.
- **Le plan se trompe sur les canaux** : tous les overlays portés passent par
  `chat_tx`, pas par les events Tauri `*:etat` (dashboard).
- `docs/features/moteur-natif.md` : §57 (welcome), §59 (chat), §60 (bandeau),
  §61 (écran d'attente), §62 (pad), §63 (arène `!fight`), §64 (présentation
  E6a), §58 (reste à faire), §70-§74 (lot H complet + I-a). `AGENTS.md` : tableau des lots + « Reprise » +
  pièges.
- Commandes : `cd src-tauri && cargo clippy --workspace --all-targets` (exit 0),
  `cargo test --workspace`, `npm run check` (0 erreur).
- Harnais visuel : `examples/*_scene.rs` + `SOS_FFMPEG=<sidecar>` puis
  `ffmpeg -ss <t> -i out.mp4 -frames:v 1 frame.png` (à supprimer après usage).

## Acquis antérieurs (rappel)

9/9 styles de cadres, animations comète, fond capture natif, carrousels,
caméra forme/anneau/exclusivité/hot-plug, toutes les scènes acceptées,
modules HUD (objectifs/musique/équaliseur/spots), chat images (G),
welcome (E1), bandeau (E2), écran d'attente (E3), pad numérique (E4),
Emoji Arena (E5), présentation — décor et rideaux (E6a).
