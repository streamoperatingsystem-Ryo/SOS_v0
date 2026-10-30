# Widget caméra — architecture & masque

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


### Architecture

```
Widget caméra (dashboard) ── commitScene ── obsSyncTrous (item caméra, fit "remplir")
   │                             └─ syncCameraMask (cache par forme : move = no-op)
   │ createCameraWidget / setCameraDevice (forcer) / deleteWidget
   │ camera_sync / camera_hide / camera_mask_sync (commandes Tauri)
   ▼
OBS scène "SOS" : [SOS-Diffusion (browser :4321)] ← au-dessus
                  [SOS-Caméra (dshow + filtre SOS-Masque)]  ← juste dessous
                  [SOS-Caméra-Fond (color + filtre SOS-Masque)] ← plaque noire
                  [SOS-Trou-* (captures)]          ← tout en bas
diffusion.html : widget caméra = trou dans le canvas de fond (destination-out,
forme du clip-path du cadre) → la caméra OBS apparaît à travers ; widgets DOM
chevauchant le rect caméra clippés via clipPath SVG evenodd (#sos-cam-clip).
```

### Masque caméra — découpe à la forme du cadre (2026-09-29)

> Problème : la source OBS est un RECTANGLE (AABB du widget, cover) mais le
> trou visible a la FORME du clip-path du cadre (biseaux cyberpunk…). L'écart
> n'était masqué que par le canvas de fond opaque → pixels caméra visibles à
> travers les autres trous (jeu) et sans média de fond (« débordement »).

- **Filtre** : « Image mask/Blend », kind OBS **`mask_filter_v2`**
  (constant `SOS_MASQUE_KIND` — PAS `mask_filter` v1 : opacity entière 0-100,
  voir l'encart « Pièges obs-websocket » plus haut), settings
  `{ type: "mask_alpha_filter.effect", image_path, color: 0xFFFFFF, opacity: 1.0, stretch: true }`
  — l'alpha du PNG multiplie l'alpha de la source, le PNG est étiré à la
  taille de BASE de la source (d'où le mapping en pixels source).
- **Polygone** : `pointsMasqueCamera` (scene.ts) → `pointsClipPathCadre`
  (cadres/points.ts = `svgPathEnPoints`, port du `svgPathToPoints` de
  diffusion.html) — **même forme que le trou destination-out**, mêmes
  conditions (`cadre.actif && strokeWidth > 0 && !sansCadre`), même parser
  des deux côtés (zéro dérive trou/masque). `carre`/`sansCadre` → null →
  filtres retirés, caméra rectangulaire.
- **Mapping** (`obs_trou::points_en_espace_source`) : inverse du pipeline
  `sync_trous` (`crop_source` = crop zoom centré **+ crop ratio w/h en
  « remplir »**, puis bounds fit alignés au centre — mêmes valeurs que
  l'item). La rotation
  de l'item est ignorée VOLONTAIREMENT : le filtre s'applique avant le
  transform → la forme tourne avec la source, comme le cadre avec le widget.
  La position (x, y) n'intervient pas → un move ne change pas le masque.
- **PNG** : `mask_png.rs` — rasterisation scanline even-odd anti-aliasée
  (couverture fractionnaire) + encodeur PNG RGBA maison (deflate « stored »,
  zéro dépendance). Fail-open : entrée invalide → couverture pleine (caméra
  rectangulaire), jamais de caméra invisible. Fichier versionné par hash
  sha256 dans `<data>/obs/masque-{camera|fond}-<hash16>.png` (cache d'images
  OBS contourné par le changement de chemin) + nettoyage des orphelins en
  gardant les fichiers encore référencés par un filtre.
- **Commande** `camera_mask_sync` (one-shot) : retour `bool` — `false` =
  capteur dshow froid (sourceWidth 0, retry 1,2 s inclus) → le dashboard ne
  met PAS en cache et retente au prochain commit (auto-guérison).
- **Dashboard** (`scene.ts`) : `syncCameraMask(forcer)` — cache par forme
  (`cleMasqueCamera` = JSON des points, mémorisé seulement si Rust retourne
  true) ; fit/zoom = les constantes de l'item caméra de `commitScene`
  (« remplir »/1, contrat lié). `setCameraDevice` → `forcer` (nouveau device
  = résolution native potentiellement différente). `syncCapturesOBS()` =
  sceneSyncCaptures + masque, appelé par les chargements de scène
  (openScene/duplicateScene/preset/boot/connexion OBS — scenes.ts,
  presets.ts, App.svelte, Toolbar.svelte).

### Forme dédiée caméra (cameraForme, 2026-10-22)

> Accord explicite utilisateur : la cam peut être ronde/carrée même si la
> scène garde son cadre (cyberpunk, etc.). Extension de la famille
> `sansCadre` (exception par instance, cf. règle cadres AGENTS.md) — PAS une
> seconde CadreConfig : pas de couleurs propres, l'anneau dérive du cadre.

- **Champs widget** (`scene.rs` + `tauri.ts`, `#[serde(default)]`) :
  `cameraForme: "cercle" | "carre" | None` (None = forme du cadre de scène,
  comportement historique, rétro-compat totale) + `cameraBord: bool`
  (anneau). Découplés du cadre de scène ET de `sansCadre` : `cercle` gagne
  toujours, `carre` = rect.
- **Cercle = cercle inscrit** : diamètre min(w,h), centré, 4 cubiques kappa
  (~49 points après aplatissement). ⚠️ Leçon 2026-10-22 : le 4e arc
  (haut → droite) doit avoir `cx + k` comme premier point de contrôle —
  un `cx - k` (copié des 3 autres arcs) fait boucler la courbe sur elle-même
  → cercle « coupé en haut à droite » (écart rayon jusqu'à 28 px, zone
  annulée par le remplissage even-odd). Vérifié par script : écart rayon
  max 0,03 px sur les 50 points. UN SEUL générateur des deux côtés :
  `pathCercle`/`pointsCercle` (cadres/points.ts) = miroir vanilla
  `pathCercle`/`pathCercleScene` (diffusion.html) → trou destination-out
  (`peindreTrous`, branchement avant le clip-path du cadre), clip-path DOM
  (`appliquerFormeCamera`, appelé en tête d'`applyCadreWidget` — le cadre de
  scène ne s'applique PAS), clip evenodd `#sos-cam-clip` (soustraction du
  CERCLE en coords scène, pas du rect — sinon les widgets voisins perdent
  leurs pixels dans les coins hors cercle) et masque OBS
  (`pointsMasqueCamera` → `camera_mask_sync`) partagent exactement la même
  forme. Zéro dérive, même leçon que le masque 2026-09-29.
- **Anneau** (`cameraBord`): SVG cercle/rect stroke, gradient
  couleur/couleurFin du cadre de scène (fallback blanc→gris via
  `gradientCadre` si cadre absent/inactif), épaisseur = strokeWidth du cadre
  bornée **1.5..12** (un cadre de scène épais ne doit pas engloutir un
  cercle cam). Miroir dashboard/`svgAnneauCamera` (diffusion.html) ; la
  moitié extérieure du trait est rognée par overflow:hidden + clip-path du
  parent (règle cadres : la marge n'existe pas).
- **Dashboard** (`Widget.svelte`) : `cameraFormeActive` → clip-path cercle
  sur `.widget-3d` + anneau dans le bloc cadre (le bloc CadreSVG est
  exclus pour une cam à forme dédiée), classe `avec-cadre` (bordure native
  masquée). Aperçu dashboard = même silhouette que la diffusion.
- **Cache masque inchangé** : la clé = JSON des points → un changement de
  forme (= de clé) re-sync le masque au commit suivant, idempotent. La
  forme est indépendante de la position : move = no-op (leçon 2026-09-29).
- **Just Chatting** (`justChatting.ts`) : `sceneDerivee` copie les champs
  (spread) ; `syncObsJC` calcule le masque aux dims JC avec le MÊME
  branchement (cercle → `pointsCercle(jcW, jcH)`, carre → null, sinon
  cadre). `jcDeriverScene` (diffusion.html) copie aussi (Object.assign) →
  trou + anneau suivent le rect JC automatiquement.

### Fond vert virtuel (cameraFondVert, 2026-10-22)

> « Fond vert simulé » : ne voir que la personne sans fond vert physique.
> OBS n'a rien en natif (chroma_key_v2 = VRAI fond vert requis) → filtre
> plugin **`background_removal`** (occ-ai/obs-backgroundremoval, id
> vérifié `src/background-filter-info.c`), inférence ONNX 100% LOCALE —
> conforme au CONTRAT FIGÉ vie privée (aucun envoi réseau).

- **Rust** (`obs_trou.rs`) : `appliquer_fond_vert(write, read, sos, actif)`
  — actif : détection plugin via `GetSourceFilterKindList` (absent → Err
  préfixée **`PLUGIN_ABSENT`** en interne → installation automatique,
  ci-dessous), `CreateSourceFilter` (settings vides = défauts du plugin)
  puis `SetSourceFilterEnabled(true)` ; inactif : filtre **DÉSACTIVÉ
  jamais retiré** (réglages OBS de l'utilisateur conservés). L'ordre des
  filtres importe peu : masque (alpha ×) et suppression d'arrière-plan
  (alpha 0) sont multiplicatifs, ils commutent.
- **Installation automatique** (`installer_plugin_fond_vert`, accord
  utilisateur 2026-10-22) : déclenchée UNIQUEMENT par le toggle de la carte
  caméra (jamais au boot). Cible = **`C:\ProgramData\obs-studio\plugins`**
  (dossier « tous utilisateurs, sans admin » scanné par OBS 28+). ⚠️ LEÇON
  2026-10-26 : une première implémentation extrayait vers
  `%APPDATA%\obs-studio\plugins` — OBS ne scanne PAS %APPDATA%, le plugin
  n'était jamais chargé (symptôme : « installé mais rien ne change quand on
  presse ON », le log OBS ne mentionne AUCUN chargement ; preuve du bon
  chemin : le « Install Into Here.lnk » du zip officiel → `C:\ProgramData\
  obs-studio\plugins`). L'installateur MIGRE automatiquement un ancien
  emplacement %APPDATA% (rename même volume → zéro re-téléchargement) puis
  nettoie le reliquat. Déjà sur disque
  (`ProgramData\...\obs-backgroundremoval\bin\64bit`) → no-op (OBS l'a
  sûrement démarré avant l'install → il faut juste le redémarrer).
  Sinon : GET anonyme `api.github.com` (dernière release
  occ-ai/obs-backgroundremoval, User-Agent « StreamOS », timeout 300 s) →
  asset `*-windows-x64.zip` → téléchargement ~50 Mo → **SHA256 vérifié
  contre le `digest` publié par l'API** → extraction via
  `zip_store::ZipLecteur` (anti zip-slip + CRC par entrée ; seuls les
  fichiers sous `obs-backgroundremoval/` sont extraits, les fichiers
  d'aide à la racine du zip sont ignorés) vers un staging
  `.obs-backgroundremoval-install` puis rename atomique dans
  `C:\ProgramData\obs-studio\plugins\obs-backgroundremoval` (garde :
  `bin/64bit` doit exister avant de remplacer l'ancienne version ; NB OBS
  portable → installation manuelle). AUCUN exécutable lancé, aucun admin.
  La commande retourne alors le statut **`redemarrer-obs`** → hint i18n
  vert `fondVertInstalle` : le plugin n'est chargé qu'au prochain démarrage
  d'OBS, l'état persisté (`cameraFondVert`) s'applique tout seul à la
  reconnexion (auto-guérison). Statuts possibles : `applique` /
  `desactive` / `redemarrer-obs` ; erreurs brutes (réseau KO, OBS offline)
  → hint rouge.
- **Plaque SOS-Caméra-Fond** : l'arrière-plan supprimé rend la cam
  transparente → sans action, la personne flotterait sur la PLAQUE NOIRE.
  Gate `fond_vert` dans `ensure_camera_fond` (`sceneItemEnabled:
  !fond_vert` aux 4 spots : CreateInput v3/v2, SetSceneItemEnabled,
  CreateSceneItem — jamais RemoveInput) + lookup idempotent en fin
  d'`appliquer_fond_vert`.
- **Chemins d'application (tous atomiques, auto-guérison)** :
  `scene_sync_captures` (boot / openScene / connexion OBS — passe
  `cam.cameraFondVert` et ré-applique, erreur log-only), `camera_sync`
  (création + changement de device — nouveau param `fond_vert`) et la
  commande one-shot `camera_fond_vert` (toggle explicite, remonte
  l'erreur). L'état est persisté dans le widget AVANT l'appel → OBS
  offline au toggle = réappliqué au prochain sync.
- **Frontend** : `setCameraFondVert` (scene.ts, persiste + one-shot +
  retour d'erreur), toggle carte caméra (`CarteEdition.svelte` — section
  « Fond virtuel », hint plugin, erreur en rouge), wrapper
  `cameraFondVertSync` (tauri.ts). `createCameraWidget`/`setCameraDevice`
  passent le flag à `cameraSync`.

### Caméra masquée (cameraCachee, 2026-10-26)

> Bouton œil au survol du widget caméra (dashboard) : masquer/réafficher
> la caméra en DIFFUSION — équivalent de l'« œil » d'OBS, en plus fort :
> la zone disparaît complètement (pas de cadre vide).

- **Flag persisté** `cameraCachee: bool` (`scene.rs` + `tauri.ts`,
  `#[serde(default)]` — visible par défaut, rétro-compat totale).
- **OBS** : `visible = !cameraCachee` propagé sur TOUTE la chaîne —
  `ensure_camera` (`sceneItemEnabled: visible` aux 3 spots), `ensure_camera_fond`
  (`!fond_vert && visible` aux 4 spots), `appliquer_fond_vert` (plaque
  `!actif && visible` — un toggle fond vert ne ré-affiche pas la plaque
  pendant que la cam est cachée), `camera_sync` / `camera_fond_vert`
  (param `visible`) et `sync_scene_captures` (`!cam.cameraCachee` →
  auto-guérison boot/openScene/connexion OBS). Jamais RemoveInput : le
  capteur dshow reste actif → réaffichage instantané.
- **Diffusion** : `cameraCachee === true` → widget ignoré partout :
  pas de trou `destination-out` dans `peindreFond` (zone repeinte par
  le fond de scène), pas de div/cadre dans `renderScene` (`continue`
  sans `seen.add` → le DELETE en fin de boucle détruit l'entry au flip),
  pas de soustraction `#sos-cam-clip` (`camWdg = null` → les widgets
  voisins clippés redeviennent normaux). Flux gratuit : le flag suit le
  snapshot WS ; en Just Chatting il suit `jcDeriverScene`/`sceneDerivee`.
- **Frontend** : `setCameraCachee(id, v)` (scene.ts) — persiste +
  commitScene (snapshot → diffusion réagit seule) + `cameraSync(visible)`
  non-fatal (OBS offline → auto-guérison). `createCameraWidget`/
  `setCameraDevice`/`setCameraFondVert`/`syncObsJC` passent le flag.
- **Dashboard** : l'œil existe sur les 3 surfaces d'édition (règle
  « 3 surfaces », `docs/features/ctx-menus.md`) — badge `BadgeSurvol`
  (ancre `.wbadge` FRÈRE de `.widget`, z 9400 — jamais recouverte
  par un voisin ou un module HUD ; visible au survol, PERMANENTE quand
  masquée via prop `visible` → `.z-controls.visible` — état lisible),
  entrée « Masquer/Réafficher la caméra » du
  menu contextuel widget (Canvas.svelte, `wgType === "camera"`) et
  toggle « Caméra en diffusion » de la carte caméra (CarteEdition,
  section Visibilité). Placeholder `.camera-actif.cachee` atténué. i18n :
  `widget.cameraMasquer`/`cameraAfficher`/`cameraCachee` +
  `carteEdition.sectionVisibilite`/`cameraDiffusion`/`cameraDiffusionHint`.

### Fichiers

- `src-tauri/src/obs_trou.rs` — `SOS_CAMERA`, `ensure_camera()` (create/enable/transform/index + gates `fond_vert`/`visible`), `camera_sync()` (+ params `fond_vert`/`visible`), `camera_hide()` ; `ensure_camera_fond()` (gates `fond_vert && visible` sur les 4 spots d'activation) ; section « Fond vert virtuel » : `appliquer_fond_vert()` (+ gate `visible` plaque) + commande `camera_fond_vert()` (constantes `SOS_FOND_VERT_FILTRE`/`SOS_FOND_VERT_KIND`/préfixe `PLUGIN_ABSENT`) ; `crop_source()` (crop zoom + crop ratio w/h, partagé par `sync_trous`/`ensure_camera`/`points_en_espace_source`) ; `reorder_sos_sources()` (bloc SOS = HAUT de scène, départ à `arr.len()-1`) ; `sync_scene_captures()` : ensure caméra (apply_device=false, flags fond vert + `!cam.cameraCachee`) + cache l'item orphelin si pas de widget caméra ; section « Masque caméra » : `camera_mask_sync()` + helpers (`points_en_espace_source`, `appliquer/retirer_filtre_masque` avec migration kind v1→v2, `taille_base_item`).
- `src-tauri/src/mask_png.rs` — rasterisation polygone (scanline even-odd AA) + encodeur PNG RGBA (deflate stocké) + tests ; `src-tauri/src/lib.rs` — commandes `camera_sync` (+ `fond_vert`), `camera_hide`, `camera_mask_sync` (dossier PNG = `config::data_dir/obs`), `camera_fond_vert`.
- `src/lib/cadres/points.ts` — `svgPathEnPoints` (port de `svgPathToPoints` de diffusion.html) + `pointsClipPathCadre` (registre → polygone widget-local) + `pathCercle`/`pointsCercle` (forme caméra dédiée).
- `src/lib/tauri.ts` — wrappers `cameraSync` (+ `fondVert`/`visible`) / `cameraHide` / `cameraMaskSync` / `cameraFondVertSync` (+ `visible`) ; type Widget : champs `cameraForme`/`cameraBord`/`cameraFondVert`/`cameraCachee`.
- `src/lib/stores/scene.ts` — `createCameraWidget` (obsSource="SOS-Caméra" + cameraSync non-fatal), `setCameraDevice` (cameraSync avec device + syncCameraMask forcé), `setCameraForme`/`setCameraBord`/`setCameraFondVert`/`setCameraCachee` (toggle œil), `commitScene` (items incluent la caméra, fit "remplir" + syncCameraMask), `deleteWidget` (cameraHide avant remove), `pointsMasqueCamera` (branchement `cameraForme` prioritaire)/`syncCameraMask`/`syncCapturesOBS`.
- `src-tauri/resources/diffusion.html` — **PAS de getUserMedia** : le widget caméra est rendu comme un trou (élément transparent + `applyCadreWidget`), trou dans `peindreFond`/`peindreTrous`. Masque 2026-09-29 : **aucune modif** (découpe au niveau de la source OBS). Forme dédiée 2026-10-22 : `pathCercle`/`pathCercleScene` (miroir vanilla de points.ts), branchement `cameraForme` dans `peindreTrous` (cercle inscrit → trou non rect) + `#sos-cam-clip` (soustraction cercle) + `appliquerFormeCamera`/`svgAnneauCamera` (clip-path + anneau, en tête d'`applyCadreWidget`). Caméra masquée 2026-10-26 : gates `cameraCachee` (trou `peindreFond`, div `renderScene`, `camWdg` du clip).
- `src/lib/components/Widget.svelte` — aperçu dashboard : `cameraFormeActive` → clip-path cercle sur `.widget-3d` + anneau SVG (miroir de `svgAnneauCamera`), bloc CadreSVG exclu ; bouton œil via `BadgeSurvol` (`visible={cameraCachee}`, frère de `.widget`) + placeholder `.camera-actif.cachee` atténué.
- `src/lib/stores/justChatting.ts` — `syncObsJC` : masque aux dims JC avec le même branchement `cameraForme` ; `sceneDerivee` copie les champs (spread) ; `cameraCachee` propagé à `cameraSync` (visible).
- `src/lib/components/Canvas.svelte` — menu contextuel widget : entrée œil « Masquer/Réafficher la caméra » (`wgType === "camera"`, `setCameraCachee`) ; hit-test `closest("[data-widget-id]")` (widget + `.wbadge` + `.sel-overlay`).
- `src/lib/components/CarteEdition.svelte` — carte caméra : section « Forme » (Cadre de la scène / Cercle / Carré) + toggle « Bordure » + section « Visibilité » (toggle `cameraCachee`) + section « Fond virtuel » (toggle fond vert + hint plugin + erreur).

### Hot-plug devices — Android UVC + caméras virtuelles (2026-10-06)

> Un téléphone Android branché en USB n'expose aucune caméra par défaut :
> **Android 14+** en mode « Webcam » (notification USB du téléphone) expose
> un device UVC en classe PnP `Camera`/`Image` ; les apps de caméra
> virtuelle (DroidCam, Iriun, Camo…) créent un device dshow, souvent en
> `Image`. Le mode « Webcam » ne peut pas être activé à distance — choix
> obligatoire sur le téléphone à chaque branchement.

- **Énumération** (`pc_enum::enumerate_cameras`) : `Get-PnpDevice -Class
  Camera,Image -Status OK` (Image ajoutée pour Android UVC + caméras
  virtuelles ; fallback WMI identique). `CarteEdition.svelte` y ajoute,
  quand OBS est connecté, l'union de `obs_enumerate_targets` kind `camera`
  (liste dshow autoritaire — couvre les devices non-PnP).
- **Watch Rust** (spawn `setup()`, lib.rs) : tick 3 s **uniquement si un
  widget caméra existe** dans la scène (zéro coût sinon — le spawn
  PowerShell est sauté) → emit `camera:devices` (`Vec<String>`) SEULEMENT
  quand la liste change.
- **Frontend** (`scene.ts`) : `cameraDevicesLive` (store, refresh du
  sélecteur sans rouvrir la carte) + `initCameraHotplug` (App.svelte boot).
  **Auto-sélection** : le 1er device NOUVELLEMENT apparu est appliqué via
  `setCameraDevice` si le widget caméra n'a pas de `cameraDeviceId` valide
  (absent ou disparu de la liste). Les devices déjà branchés au boot ne
  sont JAMAIS auto-sélectionnés (snapshot initial = référence, pas de
  détournement du device par défaut d'OBS sans action utilisateur).
- **i18n** : `carteEdition.hintAndroidCam` — aide « Android → USB → Webcam »
  affichée sous `aucuneCamera` quand la liste est vide.
