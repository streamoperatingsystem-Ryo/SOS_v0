# StreamOS v0 — Documentation technique

## Protocole d'exécution — périmètre et validation avant écriture

> Règles de méthode pour tout agent travaillant sur ce dépôt. Elles priment
> sur la tâche en cours, quelle qu'elle soit.

1. **Diagnostic avant écriture, systématiquement** — même sur une tâche qui
   semble simple. Vérifier le code/la doc réels (read, grep, git log) et
   afficher le diagnostic AVANT toute modification.
2. **Un fichier, un diff, une pause** — jamais plusieurs fichiers modifiés
   d'affilée sans montrer le diff de chacun et attendre validation.
3. **Le périmètre annoncé est une liste blanche fermée** — tout ce qui n'y
   figure pas est hors de portée. Un besoin hors périmètre se signale, il ne
   se corrige pas de soi-même.
4. **Les sections figées de ce fichier priment toujours** — « RÈGLE FIGÉE » /
   « NE JAMAIS RÉINTRODUIRE » / « CONTRAT FIGÉ » / « RÈGLE ABSOLUE »
   l'emportent sur toute instruction de la tâche en cours.
5. **Le doute s'exprime, il ne se résout pas seul** — ambiguïté ou
   contradiction → poser la question, ne pas trancher seul.

6. **La doc d'une feature vit dans `docs/features/`** — avant de modifier
   une feature, lire le fichier correspondant listé dans l'index en bas de
   ce document. Ce fichier ne contient que les règles transversales et
   figées ; le détail d'implémentation est dans `docs/features/`.

## Vie privée — tout local, zéro envoi (CONTRAT FIGÉ)

> **CONTRAT FIGÉ (2026-10-19)** — différenciateur produit assumé : StreamOS
> ne prélève aucune donnée. Il n'existe aucun serveur StreamOS ; rien ne
> quitte la machine de l'utilisateur vers un tiers. Ce contrat prime sur
> toute feature future : une fonctionnalité qui enverrait une donnée
> utilisateur hors des APIs officielles de plateforme est interdite par
> défaut — toute exception se documente ici AVANT implémentation.

- **Stocké en local uniquement** : `%APPDATA%\com.streamos.v0\` (scènes,
  configs JSON, `telemetrie/events.jsonl`, snapshots followers) + keyring
  Windows natif (tokens Twitch/YouTube). Aucune DB distante, aucun compte
  StreamOS, aucun tracker.
- **Cache d'images du chat** (2026-11-01, lot G) : GET `https` vers les CDN
  d'emotes déjà listés (BTTV/7TV/FFZ), `static-cdn.jtvnw.net` (avatars
  Twitch) et les CDN d'avatars YouTube/TikTok, **déclenché par un message de
  chat** quand le moteur natif est actif. Taille bornée (2 Mio), User-Agent
  générique, aucune auth, aucune donnée utilisateur envoyée ; écrit uniquement
  dans `medias/cache/`. **Même cache, même politique** pour les images de la
  présentation streamer (`static-cdn.jtvnw.net` : bannière de chaîne en E6a,
  avatar / boîtes de jeux / vignettes de clips en E6b-E6c), déclenché par
  l'action manuelle « présenter un streamer ».
- **Clips de la présentation — nouvel hôte** (2026-11-01, lot E6c, accord
  utilisateur explicite) : GET `https` vers **`clips-media-assets2.twitch.tv`**
  uniquement — vignettes `…-preview-WxH.jpg` (grille 2×2 de la carte,
  déclenchées par `presentation-show`) et MP4 **signés** Twitch
  (`…-N.mp4?sig=…`, déclenchés par le clic « jouer un clip »). Aucune auth
  StreamOS, aucune donnée utilisateur envoyée, User-Agent générique, taille
  bornée. Vignettes → `medias/cache/` (cache images existant) ; MP4 →
  **préchargés sur disque** dans `medias/clips/` (nom = FNV-1a 64 de l'URL,
  purge bornée, écriture atomique) : le moteur ne lit qu'un **fichier local**
  (§13.8), et l'URL signée peut expirer côté Twitch sans casser la lecture.
- **Ce qui sort réseau** : UNIQUEMENT les APIs officielles des plateformes
  (Twitch Helix/IRC/EventSub, YouTube Data API, Kick Pusher, TikTok
  Webcast) + APIs publiques d'emotes (BTTV/7TV/FFZ) + jacquettes
  (`static-cdn.jtvnw.net`, hôte figé) + clips (`clips-media-assets2.twitch.tv`,
  hôte figé) + GQL public des clips Twitch. Toutes
  initiées par une action utilisateur ou une feature activée — jamais de
  beacon, d'analytics, de crash-reporting ni de fingerprinting.
- **Téléchargement entrant — plugin fond vert** (2026-10-22, accord
  utilisateur explicite) : l'installation AUTOMATIQUE du plugin OBS
  « Background Removal » (occ-ai/obs-backgroundremoval) fait un GET anonyme
  `api.github.com` (dernière release) + télécharge le zip windows-x64 depuis
  `github.com` (SHA256 vérifié contre le digest publié par l'API, extraction
  dans `C:\ProgramData\obs-studio\plugins` via `ZipLecteur`, AUCUN exécutable
  lancé, aucun admin). UNIQUEMENT déclenché par le toggle « Fond vert
  virtuel » de la carte caméra — JAMAIS au boot ni en tâche de fond. Aucune
  donnée utilisateur ne sort (User-Agent générique, pas d'auth) ; le plugin
  lui-même (inférence ONNX) tourne ensuite 100 % en local.
- **La « télémétrie » interne** (`telemetrie/`, `analyse.rs`) est un
  journal JSONL LOCAL d'observabilité du stream — écrit sur disque, jamais
  envoyé. Le nom est trompeur : ce n'est pas de la télémétrie produit.
- **Serveur :4321** : bind `127.0.0.1` uniquement ; `/api/*` sans CORS +
  garde Host (anti-DNS-rebinding) — un client local (Stream Deck) passe,
  une page web arbitraire non.

## Commandes de build/vérification

```bash
# Rust (backend Tauri)
cd src-tauri && cargo check          # vérification compilation
cd src-tauri && cargo clippy         # lints Rust (ERREURS réelles + style)
cd src-tauri && cargo build          # build debug

# Frontend (Svelte + Vite)
npm run check                        # svelte-check : types + identifiants inconnus (0 erreur exigé)
npm run build                        # build production
npm run tauri dev                    # dev complet (Rust + frontend)
```

> 🧹 **HYGIÈNE DISQUE — `target/debug/deps` grossit sans limite (2026-11-02)** :
> chaque compilation interrompue (`Ctrl+C` sur `npm run tauri dev`) laisse ses
> `*.rcgu.o` (≈20 Mo pièce, ~300 par recompilation de `streamos_v0_lib`) et
> parfois un dossier `.tmp*.temp-archive/` ; rustc ne les supprime jamais.
> Constat : 33 786 objets orphelins + 37 archives → 60 Go rendus au volume,
> `deps` retombé de 141 Go à 23 Go. Purge :
> `powershell -ExecutionPolicy Bypass -File tools\nettoyage-cargo.ps1`
> (`-Simulation` pour voir, `-InclureIncremental` pour les sessions de
> `debug/incremental`). Ne touche JAMAIS `.rlib`/`.rmeta`/`.pdb`.

> ⚠️ **BASE ZÉRO-WARNING (2026-09-14)** : `cargo clippy` (exit 0) et
> `npm run check` (0 erreur) sont le niveau de référence du dépôt. Toute
> nouvelle erreur/warning doit être corrigée AVANT de pousser — une base propre
> rend les régressions visibles immédiatement. Les warnings `non_snake_case`
> ont été neutralisés par `#[allow(non_snake_case)]` sur `CadreConfig`/`Widget`/
> `Scene` (scene.rs) : les champs camelCase y sont VOLONTAIRES (contrat JSON
> direct avec le frontend). Ne pas "corriger" ces champs sans mettre à jour
> scene.ts + les scènes JSON sur disque.

## Piège Rust — Mutex non réentrant (écran noir figé au boot, 2026-09-14)

> ⚠️ **NE JAMAIS appeler `.lock()` plusieurs fois sur le même `std::sync::Mutex`
> dans une même expression** (args d'un `eprintln!`/`format!`, conditions, …).
> Les temporaires (MutexGuard) vivent jusqu'à la fin du statement → le 2e
> `lock()` du même thread bloque à jamais → thread principal Tauri figé dans
> `setup()` : fenêtre noire, impossible à déplacer/fermer, AUCUN log `eprintln!`
> du setup dans la sortie `tauri dev`.
> Correctif : lire UNE fois (`let c = m.lock().unwrap().clone();`) puis logger.
> Correctif appliqué : `position_overlay.rs` (`new`, causait le gel au boot
> 2026-09-14) et `bandeau.rs` (`set_config`, gel dès la sauvegarde bandeau).

## Plateformes de chat — méthodes de connexion

| Plateforme | Fichier Rust       | Méthode                          | Auth             | Persistance       | Auto-resume |
|------------|--------------------|----------------------------------|------------------|-------------------|-------------|
| Twitch     | `twitch_chat.rs`   | IRC sur WebSocket (wss, 443)     | OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| YouTube    | `youtube_chat.rs`  | HTTP polling `/liveChat/messages`| OAuth2 Device Flow + keyring | keyring (token)   | oui (token coffre) |
| Kick       | `kick.rs`          | WebSocket Pusher cloud (us2)     | Aucune (anonyme) | `kick.json` (slug)| oui (slug) |
| TikTok     | `tiktok_chat.rs`   | WebSocket Webcast (PirateTok)    | Aucune (anonyme, ttwid) | `tiktok.json` (username) | oui (username) |

> Détails par plateforme → `docs/features/chat-plateformes.md` ; flux des messages → `docs/features/architecture-chat.md`.

## Règles UI condensées — détails dans docs/features/

- **Couleurs jamais en dur** : messages/états passent par les tokens
  `--t-*` (source `src/theme/glace.css`). Détails + exceptions volontaires
  → `docs/features/theme-css.md`.
- **Menus clic droit** : recette `.ctx-*` (app.css) + `CtxMenu.svelte`
  unique — ne jamais re-copier le bloc dans un composant. Détails
  → `docs/features/ctx-menus.md`.
- **i18n** : `fr.json` = source de vérité, interpolation `{var}` (jamais
  de concaténation), noms de langues natifs, ordre figé en, fr, zh puis
  de, es, it, ru, ja. Détails → `docs/features/i18n.md`.

## Persistance (config.rs)

Fichiers dans `%APPDATA%\com.streamos.v0\` (identifiant Tauri `com.streamos.v0` —
PAS "StreamOS" ; `config.rs` → `app_data_dir()`) :
- `config.json` — `{ "sceneId": "..." }` (scène courante, migration legacy)
- `obs_canvas.json` — résolution canvas OBS (globale)
- `moteur.json` — moteur natif : `mode` (`"obs"`|`"natif"`), blocs `sortie`
  (codec, conteneur, fps, débit, preset, résolution), `apercu` (projecteur)
  et **`audio`** — mixeur à N sources `{ id, genre, peripherique, gain, mute }`,
  appliqué **à chaud** (cf. `docs/features/audio.md`). Migration one-shot
  depuis l'ancien `sortie.audio_loopback/micro` au premier lancement.
- `alertes_config.json` — config des alertes par type (alertes.rs)
- `commandes_config.json` — commandes chat `!cmd` (commandes.rs)
- `musique_config.json` — config musique `!rs` : noms de commandes, cooldowns,
  file_max, max_par_viewer (musique.rs)
- `fight_config.json` — config Emoji Arena `!fight` : actif, 8 noms de
  commandes, PV, durées lobby/tour/match, cooldown combat (fight.rs)
- `cadres_chat.json` — config de la commande `!cadres <couleur>` (signature
  dérivée d'une couleur : base = NOIR `#000000` (trait principal atténué du
  rendu par défaut), vive = couleur choisie — `signature_couleurs` Rust,
  miroir `signatureCadresChat` stores/cadresChat.ts ; ⚠️ jamais de base
  teintée, cf. `docs/features/cadres-chat.md`) : actif, nom de commande,
  accusés chat (cadres_cmd.rs — cooldown global 60 s fixe, non configurable).
  ⚠️ **La couleur choisie par un viewer n'est JAMAIS persistée** : override
  éphémère `ScenesState.cadres_chat` injecté dans les snapshots WS —
  la couleur du streamer prime au redémarrage ; l'override est aussi purgé
  à la DÉSACTIVATION de la commande (`set_config` actif:false → purge +
  push_snapshot) → `docs/features/cadres-chat.md`
- `pad_numerique.json` — config du pad numérique (pad_numerique.rs)
- `bandeau_premier_message.json` — config du bandeau 1er message (bandeau.rs)
- `position_overlay.json` — squelette de position unifié (clip de bienvenue + alertes)
- `position_splitpop.json` — squelette DÉDIÉ du pop de validation des splits
  (position_overlay.rs, frame WS `splitpop-pos`)
- `clips_bienvenue.json` — registre des viewers Twitch + config globale du
  clip de bienvenue (welcome.rs)
- `presentation_favoris.json` — favoris de la présentation streamer
  (presentation.rs)
- `kick.json` — `{ "slug": "..." }`
- `tiktok.json` — `{ "username": "..." }`
- `followers_snapshot.json` — snapshot des followers Twitch (user_id, login, followed_at + timestamp). Sert à détecter les unfollows au démarrage suivant.
- `unfollows.json` — historique append-only des unfollows détectés (user_id, login, date_unfollow). Dédupliqué par user_id.
- `raccourcis.json` — `{ "visible": bool, "bord": "gauche|droite|haut|bas", "monitor_index": n }` (barre de raccourcis)
- `onboarding.json` — `{ "fait": true }` (présence de la galerie de presets au 1er lancement ; absent = premier démarrage)
- `langue.json` — `{ "langue": "fr" }` (langue UI du dashboard — i18n, voir section dédiée)
- `fenetre.json` — écran + mode au démarrage + géométrie fenêtrée (fenetre.rs)
- `speedrun.json` — chemins .asl/.lss + settings du splitter (config.rs)
- `telemetrie/events.jsonl` — journal append-only des événements télémétrie (schéma v1, voir section dédiée)
- `telemetrie/boite_noire.jsonl` — journal persistant du diagnostic
  (`conso.rs`, 1 point / 5 s) : RAM + CPU de StreamOS, RAM système occupée,
  `obs_present`, `obs_silence_s` (secondes depuis le dernier RPC OBS réussi).
  Rotation à 2 Mio → `boite_noire.1.jsonl`. Écrit **en permanence, modale
  fermée comprise** — dérogation assumée au contrat « non énergivore »,
  décidée le 2026-10-27 : c'est la seule trace qui survit à un gel.
  → `docs/features/diagnostic.md`
- `ecran_attente.json` — config de l'écran d'attente BRB (duree_s, titre,
  afficher_titre, son, volume, media/kind/nom, source_audio OBS)
- `scenes/` — scènes SOS (JSON)
- `medias/` — médias importés (`medias/musique/` = bibliothèque musicale,
  noms de fichiers CONSERVÉS ≠ uuid des alertes)
- `medias/cache/` — images du chat (avatars + emotes), nom = FNV-1a 64 de
  l'URL, **sans extension** (WIC détecte le format) ; téléchargées par l'app
  à la volée (cache_images.rs), purgées au-delà de 1 024 fichiers. Le moteur
  ne lit que ces fichiers (aucun accès réseau, §13.8). Sert aussi aux images
  de la présentation (bannière, avatar, boîtes de jeux, vignettes de clips)
- `medias/clips/` — clips de la présentation **préchargés** (lot E6c), nom =
  FNV-1a 64 de l'URL signée + `.mp4` (cache_clips.rs, écriture atomique,
  purge au-delà de 8 fichiers, 256 Mio max). Le moteur ne lit que ces
  fichiers : l'URL signée Twitch peut expirer sans casser la lecture
- `obs/` — cache des masques PNG caméra `masque-{camera|fond}-<hash16>.png`
  (obs_trou.rs + mask_png.rs — nettoyage des orphelins intégré)

Twitch et YouTube utilisent keyring (coffre OS) pour les tokens OAuth2 — pas de fichier JSON.

## Widget caméra — source OBS "SOS-Caméra" (2026-09-12)

> ⚠️ **CONTRATS FIGÉS** (refonte 2026-09-12, remplace l'ancien système getUserMedia) :
> 1. **1 widget `type: "camera"` / scène** (singleton), source OBS fixe **"SOS-Caméra"** (`dshow_input`, constante `obs_trou::SOS_CAMERA`).
> 2. **Create même si OBS offline** → widget créé quand même ; la source est créée par `ensure_camera` au prochain `scene_sync_captures` (boot / openScene / connexion OBS).
> 3. **Delete → `SetSceneItemEnabled(false)` sur l'item, JAMAIS `RemoveInput`** — l'input OBS est conservé (conflits d'accès exclusif du capteur Windows, perte des settings OBS). Recréation = réutilisation de l'input + réactivation de l'item.
> 4. **Device change → `SetInputSettings video_device_id` seulement si `apply_device == true`** (jamais au boot — pas de restart du capteur dshow).
> 5. **Diffusion = trou + cadre SVG, dashboard = placeholder 📷, pas de getUserMedia** — le widget caméra passe par le chemin trou de `diffusion.html` (`renderScene` + `peindreFond` : `w.trou === true || w.type === "camera"`).
> 6. **Transform = sémantique `sync_trous`** : position = centre du widget, bounds w×h, `OBS_BOUNDS_SCALE_OUTER` (cover), alignment 0 → aucun saut entre create et move/resize. **+ crop ratio w/h** (`crop_source`, 2026-10-01) : SCALE_OUTER sans crop déborde de la boîte quand le ratio capteur ≠ ratio widget, et obs-websocket n'expose pas « rogner selon la boîte » → crop explicite calculé depuis `sourceWidth/Height`.
> 7. **Masque = filtre OBS sur la SOURCE, jamais de rustine diffusion.html** (2026-09-29) : `SOS-Caméra` ET `SOS-Caméra-Fond` sont découpés à la forme du cadre SVG via le filtre « Image mask/Blend » (**`mask_filter_v2`**, mode alpha) — le débord du rectangle caméra ne peut plus apparaître à travers les autres trous (jeu) ni sans média de fond.
> 8. **Bloc SOS tout en HAUT de la scène OBS, SAUF `SOS-Fond-*`** (2026-10-01) : `reorder_sos_sources` part de l'index max de la scène (`arr.len()-1`), pas du nombre d'items SOS — sinon le bloc se range en bas et toute source utilisateur passe devant SOS-Diffusion. Les `SOS-Fond-*` (captures plein canvas) restent aux indices 0..F-1, tout en bas : au-dessus des sources utilisateur, elles les couvriraient entièrement.
> 9. **Forme dédiée caméra (`cameraForme`, 2026-10-22)** : `"cercle"` (cercle inscrit, diamètre min(w,h), centré) / `"carre"` (rect) / absent = forme du cadre de scène (historique). Découplée du cadre de scène ET de `sansCadre`. UN SEUL polygone partout — `pathCercle` (cadres/points.ts) = miroir vanilla `pathCercle` (diffusion.html) : trou destination-out, clip-path DOM, clip evenodd `#sos-cam-clip` (soustraction du CERCLE, pas du rect) et masque OBS (points → PNG) partagent exactement la même forme — zéro dérive. Anneau optionnel `cameraBord` (couleurs du cadre, épaisseur bornée 1.5..12) ; le cadre de scène NE S'APPLIQUE PAS à une caméra à forme dédiée.
> 10. **Fond vert virtuel (`cameraFondVert`, 2026-10-22)** : filtre plugin **`background_removal`** (occ-ai/obs-backgroundremoval, inférence ONNX **100% locale** — conforme au CONTRAT FIGÉ vie privée) sur `SOS-Caméra`. À l'extinction le filtre est **DÉSACTIVÉ, jamais retiré** (réglages conservés). La plaque `SOS-Caméra-Fond` est **MASQUÉE** quand actif — gate `fond_vert` dans `ensure_camera_fond` (`sceneItemEnabled: !fond_vert`, jamais RemoveInput) — sinon la personne flotte sur du noir au lieu de la scène OBS derrière. Plugin absent du OBS en cours → **INSTALLATION AUTOMATIQUE** au toggle (release GitHub, zip windows-x64, SHA256 vérifié, extraction `ZipLecteur` dans **`C:\ProgramData\obs-studio\plugins`** — aucun exe/admin, voir l'encart vie privée) puis statut **`redemarrer-obs`** : un plugin posé sur disque n'est chargé qu'au prochain démarrage d'OBS, l'état persisté s'applique alors tout seul à la reconnexion. ⚠️ **LEÇON 2026-10-26 : le dossier plugins « sans admin » scanné par OBS est `C:\ProgramData\obs-studio\plugins` (preuve : le « Install Into Here.lnk » du zip officiel), PAS `%APPDATA%\obs-studio\plugins` — une extraction dans %APPDATA% n'est JAMAIS chargée (log OBS muet, aucune ligne de chargement). L'installateur migre automatiquement un ancien emplacement %APPDATA% (rename même volume, zéro re-téléchargement).** L'état est persisté dans le widget : `scene_sync_captures` et `camera_sync` le ré-appliquent (auto-guérison, chaque chemin atomique).

### ⚠️ Pièges obs-websocket v5 — NE JAMAIS RÉINTRODUIRE (2026-10-01)

> Trois bugs silencieux découverts ensemble (symptômes : angle droit de la
> caméra visible dans le trou jeu en dessous, puis « voile noir » sur la
> caméra). Tous dans `obs_trou.rs`, tous vérifiés par mesure réelle sur
> OBS 32.2.2 / obs-websocket 5.7.4.

1. **`GetSceneItemTransform` répond `{ "sceneItemTransform": { sourceWidth, sourceHeight, … } }`** —
   un niveau d'imbrication. Lire `resp["sourceWidth"]` à la racine renvoie
   toujours 0 → crop zoom, crop ratio et masque silencieusement inopérants
   (log `masque: « SOS-Caméra-Fond » sans taille de base → skip` alors qu'une
   color_source a TOUJOURS une taille = signal de ce bug).
2. **Deux kinds de filtre masque** (`GetSourceFilterKindList`) : `mask_filter`
   = **v1 OBSOLÈTE, `opacity` ENTIER 0-100** ; `mask_filter_v2` = `opacity`
   double 0-1. Créer `mask_filter` avec `opacity: 1.0` → lu comme **1 %** →
   source quasi transparente, plaque noire visible à travers. Toujours
   `SOS_MASQUE_KIND = "mask_filter_v2"` ; un filtre d'un autre kind est retiré
   puis recréé (le kind ne se change pas par `SetSourceFilterSettings`).
3. **`SetSceneItemIndex` : index 0 = bas, max = haut** — le reorder doit
   partir de `arr.len()-1` (taille totale de la scène) pour le bloc
   Diffusion/Caméra/Fond/Trou-* ; les `SOS-Fond-*` occupent explicitement
   les indices 0..F-1 (plein canvas → ne doivent couvrir aucune source).

> Méthode de vérification sans deviner : `GetSourceScreenshot` de `SOS-Caméra`
> avec/sans `SetSourceFilterEnabled` et moyenne d'alpha (1,5/255 vs 255 = le
> filtre est la cause) ; `GetSceneItemList` pour les indices réels.

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `getUserMedia` / `<video>` / `startCamera` / `stopCameraStream` dans diffusion.html — la caméra vient d'OBS, pas du navigateur.
2. ❌ `RemoveInput` sur "SOS-Caméra" dans le hot path delete widget — item caché seulement.
3. ❌ `SetInputSettings` au boot (sync_scene_captures) — restart du capteur dshow à chaque ouverture de scène.
4. ❌ Exclure le widget caméra du cadre SVG ou du trou `peindreFond` — il suit la règle figée des cadres.
5. ❌ Créer un second input si "SOS-Caméra" existe déjà — CreateSceneItem / SetInputSettings sur l'existant.
6. ❌ Masquer le débord caméra côté diffusion.html (peindre le fond sur le rect, plaque DOM…) — la découpe se fait au niveau de la SOURCE OBS (crop `crop_source` + filtre SOS-Masque), sinon le débord réapparaît à travers les autres trous.
7. ❌ Régénérer le masque à chaque move — cache par forme (`cleMasqueCamera`) : le masque est en pixels de la source, indépendant de la position.
8. ❌ `SCALE_OUTER` (fit « remplir ») sans `crop_source` — la source déborde de la boîte w×h et perce dans tout trou voisin (cf. « Pièges obs-websocket »).
9. ❌ Kind `mask_filter` (v1) — `opacity` entière, 1.0 = 1 % → voile noir ; toujours `SOS_MASQUE_KIND`.
10. ❌ Reorder avec `top_to_bottom.len()-1` comme index de départ — range le bloc SOS en BAS de scène ; départ = `arr.len()-1`.

> Détails caméra (architecture, masque, hot-plug) → `docs/features/camera-details.md`.

## Cadres SVG — règle absolue (NE PAS REGRESSER)

> ⚠️ **RÈGLE FIGÉE — ne jamais modifier ce comportement sans accord explicite.**
> Cette section documente un fix définitif (2026-08-31) après plusieurs
> régressions. Tout changement ici casse l'affichage côté diffusion/OBS.

### Principe

Les cadres SVG (`scene.cadreWidget`) s'appliquent à **TOUS les widgets**, sans
exception : médias, chat, welcome-clip, et **widgets troués**. Aucun type de
widget ne doit être exclu du cadre.

**Cadre unifié (2026-09-26) — UN SEUL système, deux rendus.** Une seule
`CadreConfig` (`scene.cadreWidget`, champ canonique) alimente EN MÊME TEMPS
le cadre de chaque widget ET le cadre du bord extérieur du canvas
(`Canvas.svelte` `.cadre-app-overlay`, diffusion `applyCadreApp` →
`#cadre-app-overlay` — les deux lisent `cadreWidget`). `CadresModal` n'a
plus d'onglets : un « Cadre activé », un writer unique `mettreAJourCadre`
(stores/scene.ts — écrit `cadreWidget` et purge `cadreApp`). Le champ
`scene.cadreApp` est LEGACY : conservé pour désérialiser les vieilles
scènes, migré vers `cadreWidget` par `Scene::migrer_cadres()` à la lecture
(load_scene_file, importer, migrer_ancien_config, presets::instancier —
cadreWidget actif gagne, sinon cadreApp repris même inactif, cadreApp vidé
dans tous les cas). Jamais écrit par l'UI ; ne jamais réintroduire une
seconde config divergente.

**Portée du bord (`bordActif`)** : `CadreConfig.bordActif` (bool, défaut
`true` — `#[serde(default = "defaut_vrai")]` côté Rust, optionnel côté TS)
masque UNIQUEMENT l'overlay du bord extérieur (`Canvas.svelte` gate
`cadreBordActif`, `applyCadreApp` → `bordActif !== false`) ; le cadre des
widgets reste actif. Ce n'est PAS une seconde config : même CadreConfig,
juste une cible de rendu masquable. Vieilles scènes sans le champ → `true`
(rétro-compat). Toggles : `CadresModal` (« Bord de l'app », désactivé si
cadre inactif) + carte fond de `CarteEdition`.

**Animation (`animer`, 2026-10-05)** : `CadreConfig.animer` (bool,
`#[serde(default)]` → false, opt-in via le toggle « Animation » de
`CadresModal`). Quand true, chaque renderer émet UNE animation signature en
SMIL **sur le tracé du bord** — jamais dans la marge extérieure : la marge
est rognée par `overflow:hidden` + `clip-path` du parent sur les widgets
(.widget / .widget-3d / .apercu de la modale) et hors canvas pour le bord
de l'app → une anim en marge est invisible (leçon 2026-10-05 : halo diode
mgstyle et étincelles story ne s'affichaient nulle part). Signatures :
cyberpunk = runner lumineux, hexagon = nœud voyageur — les deux en
`animateMotion` avec attribut `path=` direct (PAS de `mpath`/`xlink:href` :
non résolus par les vieux CEF d'OBS → runner figé) + grappes d'alvéoles
rotatives (`animateTransform`, décor marge), story = étincelle voyageuse
(`animateMotion` sur borderPath), tribal = nœud voyageur (`animateMotion`
path= direct) + respiration SMIL des griffes (`animate` opacity), arena =
nœud voyageur (`animateMotion` path= direct) + pulsation SMIL des 4 orbs,
thin-line/mgstyle/carre/arrondis = segment comète (`stroke-dashoffset`
sur `pathLength="100"`). Jamais de
filtre animé. `animer` est propagé :
`CadreOpts.animer` → `genererSVG`/`genererCadreSVG` (les 3 sites : TS +
diffusion.html + chat-popout.html), `cadreKey`/`majEtatCadre` l'incluent
(re-render au toggle). EXCEPTION perf : les bulles chat forcent
`animer=false` (`decorerCadreMsg`/`decorerCadreMsgDims` — ~200 SVG animés
par widget hors budget).

**Rampe « liserés » (2026-10-26)** : le dégradé PRINCIPAL du bord de
chaque style (arrondis/carre, mgstyle, cyberpunk, story, hexagon, arena,
tribal + anneau caméra `grad-cam`) n'est plus un simple 2-stops — c'est
`stopsLiseres(sombre, vive)` (couleurs.ts, miroir vanilla `stopsLiseres`
dans diffusion.html + chat-popout.html) : dominante `couleur` (sombre,
~85 % de la rampe) + deux fins liserés — « éclat » (vive éclaircie 45 %)
à ~33 % puis `couleurFin` pure à ~68 %. Règle de mappage : sombre =
`couleur`, vive = `couleurFin` (donc stopsLiseres(profond, principal) pour
les styles où principal=vive). thin-line reste plat (pas de dégradé,
identité minimaliste). Les dégradés secondaires (inner cyberpunk/story,
orb arena, décorations) sont hors rampe.

**Cadre par défaut** (`CadreConfig::defaut_scene`, scene.rs — MIROIR de
`CADRE_WIDGET_DEFAUT`, scene.ts) : MGStyle, noir → violet #8b5cf6, épaisseur
50, actif, bordActif. Posé dès la **naissance** d'une scène (Scene::new —
nouvelle sauvegarde « + Scène », premier boot, fallback scène illisible) →
cadre actif sur widgets + bord dès le départ. Les sauvegardes existantes ne
sont PAS touchées (`#[serde(default)]` → cadreWidget reste None au load) ;
`sceneAvecWidget` (scene.ts) le pose encore au 1er widget créé sur ces
vieilles scènes (`??` — un cadre explicitement désactivé n'est jamais
réactivé). ⚠️ `scene_vierge` (scenes.rs — garde onboarding) traite le
défaut auto comme « pas de contenu utilisateur » : `cadreWidget ==
defaut_scene()` compte vierge, toute autre valeur = choix utilisateur →
pas vierge. CadreConfig dérive PartialEq pour ce test.

**Opt-out par widget** (`w.sansCadre`, 2026-09-19) : les widgets créés par
drop dans la zone Widgets (`ajouterALaBibliotheque` → `poserMediaDepuisBibliotheque`,
auto-pose — l'entrée reste dans `bibliothequeMedias`) portent `sansCadre: true`
— le cadre de scène ne s'applique pas à EUX (clip-path + overlay absents, trou
éventuel rectangulaire, titre en gradient fallback). Le bouton « Transformer
en widget » (`transformerEnWidget`) repasse `sansCadre` à false sur le widget
déjà posé (pas de doublon) et consomme la ligne. `setWidgetSansCadre(id, v)`
est le setter générique exposé dans la carte d'édition pour TOUS les types
(toggle « cadre sur ce widget », caméra incluse — réactiver consomme aussi
l'entrée bibliothèque si le widget porte un média droppé). Flag par instance
sérialisé dans la scène, pas par type : la règle ci-dessus reste vraie pour
tous les autres widgets.

**Extension caméra** (`cameraForme`, accord explicite 2026-10-22) : même
famille d'exception par instance — la caméra peut porter une forme DÉDIÉE
(« cercle » / « carré ») qui REMPLACE le cadre de scène pour elle (contrat 9
de la section Widget caméra ; détails → `docs/features/camera-details.md`).
Ce n'est PAS une seconde CadreConfig : pas de couleurs ni d'épaisseur
propres, l'anneau éventuel (`cameraBord`) dérive du cadre de scène.

### Comportement attendu

| Type de widget | Cadre SVG dashboard | Cadre SVG diffusion/OBS | Trou canvas de fond |
|----------------|---------------------|-------------------------|---------------------|
| média          | oui (clip-path + overlay SVG) | oui | n/a |
| chat           | oui (clip-path + overlay SVG) | oui | n/a |
| **trou**       | **oui** (clip-path + overlay SVG) | **oui** | **forme du clip-path du cadre** |

> Le clip de bienvenue n'est PAS un type de widget (l'ancien type
> `welcome-clip` est purgé au chargement par `scenes.rs` — overlay autonome
> piloté par WS) ; sa carte reçoit le cadre SVG via `appliquerCadreOverlay`,
> comme les alertes/commandes/écran d'attente.

### Fichiers concernés

#### `src/lib/components/Widget.svelte` (dashboard)
- `cadreActif` = `!!cadre?.actif && (cadre?.strokeWidth ?? 0) > 0` — **PAS de `!isTrou`**.
  L'exclusion des trous était une ancienne décision de design ("un trou n'a pas
  de bordure") qui a été **supprimée**. Ne jamais la réintroduire.
- Le bloc `{#if cadreActif && cadre}` s'applique à tous les types (chat, media, trou).
- `trou-overlay` (bordure pointillée) est masqué quand un cadre est actif
  (`{#if isTrou && !cadreActif}`) pour éviter la superposition pointillé + cadre.

#### `src-tauri/resources/diffusion.html` (diffusion OBS :4321)
- **CSS** : `.widget.trou` = `background: transparent; border: none;` — le widget
  trou est invisible (le trou est dans le canvas de fond), seul le cadre SVG est
  visible au-dessus.
- **`renderScene()`** : les widgets troués ne sont **PAS skipés**. Un élément DOM
  `div.widget.trou` est créé (transparent) pour recevoir `applyCadreWidget()`
  (clip-path + overlay SVG). Ne jamais réintroduire un `continue` sans DOM pour
  les trous.
- **`renderScene()` — chat** : `applyCadreWidget()` est appelé sur le widget chat
  (UPDATE + CREATE). L'entry chat a les champs `cadreOverlayEl` et `cadreKey`.
  Ne jamais les retirer.
- **`peindreFond()` — trous** : quand un cadre avec clip-path est actif, le trou
  dans le canvas de fond suit la **forme du clip-path** (coins biseautés cyberpunk,
  hexagone, story, arrondis) et non un rectangle. Utilise `svgPathToPoints()` +
  `getCadreClipPathPoints()` (cache) pour parser le path SVG en points, puis
  `projectCorner()` pour la projection 3D. Sans cadre actif → rectangle simple
  (fast path `fillRect`).

### Anti-patterns à ne JAMAIS réintroduire

1. ❌ `if (w.trou === true) { seen.add(w.id); continue; }` dans `renderScene()` —
   les trous doivent avoir un élément DOM pour recevoir le cadre.
2. ❌ `&& !isTrou` dans `cadreActif` (Widget.svelte) — les trous ont le cadre.
3. ❌ Omettre `applyCadreWidget()` sur le chemin chat dans `renderScene()`.
4. ❌ Dessiner le trou comme un rectangle quand un cadre avec clip-path est actif —
   les coins débordent du cadre côté OBS.
5. ❌ Retirer les champs `cadreOverlayEl`/`cadreKey` de l'entry chat dans
   `widgetMap`.
6. ❌ Dessiner des éléments décoratifs **à l'intérieur** de la zone du widget
   (crosshair, radar, grille, scan lines, data ticks, cadran, etc.) — un cadre
   **borde**, jamais n'obscurcit le contenu (média, chat, caméra). Tous les
   éléments décoratifs doivent être dans la **marge extérieure** (coordonnées
   négatives ou > largeur/hauteur). Leçon MGStyle (2026-09-15) : un crosshair
   au centre du widget = cible par-dessus la vidéo → contresens fonctionnel.

## Média de fond sur les widgets à contenu (chat / input-viewer / speedrun / goal / musique) — 2026-09-15

> Les widgets à contenu — `chat`, `input-viewer`, `speedrun`, `goal` et
> `musique` — acceptent un média (image/vidéo) en **arrière-plan**, avec le
> contenu (messages chat, keycaps, timer+splits, compteur d'objectif,
> mini-lecteur) affiché **par-dessus** — comme un widget média, avec les mêmes
> contrôles (Affichage/fit, zoom, rotation, offsets, PlayerBar vidéo).
> **Zéro changement Rust** : les champs `media`/`kind`/`mediaFit`/`mediaZoom`/
> `mediaRot`/`mediaOffsetX/Y`/`mediaPaused`/`mediaTime` existent déjà sur TOUS
> les widgets (`scene.rs`), et `import_media` (lib.rs) accepte n'importe quel
> widget.

### Contrat de rendu (règle figée)

1. **Le média = PREMIER enfant** du `.widget` (diffusion) / `.widget-3d`
   (dashboard), `class="media-fond"`, `position:absolute; inset:0; z-index:0`.
2. **Le contenu au-dessus** : `.chat-body` (diffusion) / `.chat-actif`
   (dashboard), `.widget.inputviewer > *:not(.media-fond):not(.cadre-overlay)`,
   `.widget.speedrun > *:not(.media-fond):not(.cadre-overlay)` (diffusion),
   `.contenu-dessus` (wrapper WidgetSpeedrun dashboard), badge
   `.input-viewer-obs` (z 2) — tous `position:relative; z-index:1`.
   JAMAIS de z-index négatif sur le média (passerait derrière le fond du widget).
   ⚠️ **`:not(.cadre-overlay)` OBLIGATOIRE** dans les sélecteurs `> *` :
   l'overlay du cadre SVG est un ENFANT DIRECT du widget (`applyCadreWidget`
   fait `el.appendChild`). Sans l'exclusion, le sélecteur `> *` (spécificité
   0,3,0) écrase `.cadre-overlay` (0,1,0 : `position:absolute; z-index:5`)
   → le cadre SVG disparaissait sur input-viewer et speedrun (régression
   2026-09-15).
3. **Le cadre SVG reste au-dessus de tout** (z-index 5) — règle cadres
   inchangée, le clip-path découpe aussi le média de fond.
4. **Fit "ajuster" (défaut)** : le fond glass sombre du widget reste visible
   dans les bandes ; "remplir" couvre tout. Les messages chat gardent leur
   glass `backdrop-filter` (floute le média derrière eux).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/resources/diffusion.html` | `syncMediaFond(entry, w)` : (re)crée le média de fond si `mediaKey` change ou l'entry n'en a plus, `insertBefore(firstChild)`, applique `applyMediaStyle` + `applyMediaState` à chaque snapshot ; appelé dans UPDATE+CREATE des branches chat / input-viewer / speedrun / goal / musique |
| `src/lib/stores/scene.ts` | `clearWidgetMedia(id)` : retire `media`/`kind` + commit (pattern `clearFond` — le fichier dans medias/ n'est PAS supprimé) |
| `src/lib/components/Widget.svelte` | Snippet `{#snippet mediaFond()}` rendu dans les branches isChat/isInputViewer/isSpeedrun/isGoal/isMusique (img figée ou vidéo vignette via `videoEl` — les $effect src/registerVideo/mediaTime existants pilotent l'élément) |
| `src/lib/components/CarteEdition.svelte` | Contrôles média dans les onglets de la carte widget : Contenu (« Importer/Remplacer/Supprimer un média » — offert à TOUS les widgets non-caméra, le média devient alors un fond sous le contenu), Transformation/Effets (fit, zoom, rotation, offsets, filtres) + PlayerBar vidéo |

### Pièges

1. **`applyMediaState` garde par tagName** (`entry.mediaEl.tagName !== "VIDEO"`),
   PAS par `entry.kind` — les entries chat/input-viewer/speedrun ont un kind
   propre ("chat", …) mais portent une vidéo de fond dans `entry.mediaEl`.
2. **Input-viewer UPDATE + `innerHTML`** : quand mode/layout/skin change,
   `ivEntry.el.innerHTML = ivGenererHTML(w)` DÉTRUIT le média de fond →
   resetter `ivEntry.mediaEl = null; ivEntry.mediaKey = ""` juste après
   (comme pour l'overlay cadre SVG), puis `syncMediaFond` recrée.
3. **Le bouton Morphing est limité aux widgets type "media"** (`isMediaWidget`
   dans CarteEdition) — les morphs ne sont PAS rendus sur les widgets à contenu
   (les branches chat/input-viewer/speedrun de `renderScene` précèdent la
   branche morph).
4. **Dashboard = config, pas un player** (règle d'or) : le média de fond y est
   une vignette figée (vidéo : `preload="metadata"`, pause, frame `mediaTime`)
   ; la lecture réelle est pilotée par la PlayerBar via snapshot WS (:4321).
5. **Pas de composant séparé pour les contrôles média** : ils vivent en ligne
   dans `CarteEdition.svelte` (onglets Contenu/Transformation/Effets) — les
   classes CSS (`.fit-group`, `.fit-btn`, `.media-ctrl`, …) sont scopées au
   composant et ne s'appliqueraient pas à un composant enfant sans duplication.
   (L'ancien snippet `controlesMedia` de Toolbar n'existe plus — migration
   vers la carte d'édition contextuelle.)

## Plan de suivi — parité moteur natif (OBS → Rust)

> **Cible actée (2026-11-01)** : parité complète avec le mode OBS, pas une
> liste de « limites assumées ». Toute scène doit être **acceptée** et rendue
> à l'identique. Cette table est le **point de reprise** d'une nouvelle
> fenêtre de contexte : état, prochain lot, pièges déjà payés.
> Convention : un lot = liste blanche fermée ; `cargo clippy` exit 0 +
> `cargo test` vert + `npm run check` 0 erreur avant le lot suivant.

### Architecture — un seul contrat, deux rendus (RÈGLE)

> Réponse à la question « faut-il décupler chaque fonctionnalité en deux ? » :
> **non**. Une fonctionnalité n'a **jamais** deux logiques. Ce qui existe en
> double est uniquement la **couche de rendu** (et la capture, par nécessité).

1. **Une seule vérité de scène** : `Scene` (JSON, `sos-scene`) + `RuntimeState`
   (frames `chat_tx`, events Tauri) alimentent **les deux** rendus. Aucun état
   métier dupliqué, aucune config « mode OBS » vs « mode Rust ».
2. **Deux renderers, une seule spec** : `diffusion.html` (DOM/Chromium, mode
   OBS) et `sos-moteur-natif` (Direct2D, mode Rust). La parité est la
   **spec** : même géométrie, mêmes couleurs, mêmes timings. Un écart est un
   bug, pas une variante.
3. **Capture : deux backends, une cible** : OBS (`obsSource`) vs WGC/MF
   (`captureCible`, `bgCaptureCible`). La cible vit dans la scène ; chaque
   mode lit la sienne — on ne recopie pas une config d'un mode à l'autre.
4. **La dérive se mesure** : chaque lot de rendu porte des tests de géométrie
   contre les valeurs TS **mesurées** (polygones, points) ; le harnais SSIM
   (plan §20) reste à brancher pour les pixels.

| Lot | État | Contenu | Périmètre (fichiers) |
|---|---|---|---|
| A | **fait** (2026-11-01) | Accepter toutes les scènes : la garde ne juge plus le contenu, seulement « moteur jamais configuré ». Dégradation signalée pour `bgObsSource` non rendu. | `crates/sos-moteur-natif/src/moteur.rs` |
| B | **fait** (2026-11-01) | Fond = capture native (`bgCaptureCible`), prioritaire sur `bgObsSource` / `bgSlides` / `bgMedia` ; dialogue de fond en mode Rust **sans OBS**. Vérifié : 204 + 9 tests, clippy workspace 0, svelte-check 0. | `crates/sos-scene/src/lib.rs`, `crates/sos-moteur-natif/src/scene_sources.rs`, `crates/sos-moteur-natif/src/moteur.rs`, `src/lib/tauri.ts`, `src/lib/stores/scene.ts`, `src/lib/components/ObsSourceDialog.svelte`, `src/lib/components/CarteEdition.svelte` |
| C1 | **fait** (2026-11-01) | `animer` : segment comète (7 % du périmètre, 4 s/tour, `couleurFin`, `0,7 × trait`, bouts ronds) sur `mgstyle`/`thin-line` ; `trace_comete` = **chemin du SVG** (rect plein / rect arrondi), pas le polygone de découpe ; horloge `pts_us` ; carte alerte animée aussi ; `Capacites.cadres_animes = true`. 207 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs`, `scene_sources.rs`, `texte/mod.rs`, `texte/alertes.rs`, `moteur.rs` |
| C2a | **fait** (2026-11-01) | `carre` + `arrondis` : trait dégradé (rect / rect arrondi `rx=8`), clip `arrondis` `r=min(8,W/4,H/4)` et `carre` **aucun**, comète `rx=8` non bornée (rognée par le clip). 209 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs` |
| C2b-1 | **fait** (2026-11-01) | `tribal` : octogone `tribalPath`, 4 traits (halo / 22 % / dégradé / filet), motif corps+balayures rempli au dégradé, équerres (boîte > 130×90), respiration `opacity 0.85;1;0.85` (4 s), nœud voyageur 5,5 s **hors clip**. 212 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs` |
| C2b-2 | **fait** (2026-11-01) | `cyberpunk` : octogone biseauté (`bevel = max(8, round(3.5·ep))`, borné W/4·H/4), 4 traits (néon 22 % / dégradé / liseré intérieur 3 stops / filet blanc), runner 5 s **hors clip**. Ornements de marge (équerres, circuits, code-barres) **invisibles** dans la référence → non portés. 213 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs` |
| C2b-3 | **fait** (2026-11-01) | `story` : bord festonné en cubiques (`s = max(12, round(4·ep))`), 3 traits ronds, cœurs de coin + de bord, volutes (rognées au bord), étincelle voyageuse 7 s. Aplatissement cubique 12 pas partagé. 215 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs` |
| C2b-4 | **fait** (2026-11-01) | `hexagon` (bord octogonal, nœuds, cadre intérieur inséré, nœud voyageur 6 s) et `arena` (octogone allongé, canal + double filet, écho, nœuds, cartouches/ailerons, 4 orbs radial + croix X pulsées 3,5 s, nœud voyageur 5 s). **9/9 styles portés.** 217 tests, clippy 0. | `crates/sos-moteur-natif/src/cadres.rs` |
| D1 | **fait** (2026-11-01) | Caméra : forme = cadre de scène quand `cameraForme` est absent (ou inconnu) ; une forme dédiée (`cercle`/`carre`) remplace le cadre. `camera_forme_dediee` + tests (`camera_sans_forme_dediee_suit_le_cadre_de_scene` ; `cadres_bord_plus_widgets_avec_exclusions` attend 4 cadres). 205 tests, clippy 0. | `crates/sos-moteur-natif/src/scene_sources.rs` |
| D2 | **fait** (2026-11-01) | Anneau `cameraBord` des formes dédiées : cercle/rect, trait `clamp(strokeWidth,1.5,12)`, dégradé `stopsLiseres` **diagonal TL→BR** (le SVG n'utilise pas `gradientAngle`), fallback `#e0e0e0→#9a9a9a` ép. 4, rogné à la forme (couche cercle / clip rect). `SourceCfg.anneau` + `anneau_camera_de`. 219 tests, clippy 0. | `cadres.rs`, `scene_sources.rs`, `texte/mod.rs`, `moteur.rs` |
| D3 | **fait** (2026-11-01) | Caméra : libération du capteur dshow **après** une bascule acceptée (`camera_sync visible=false`, jamais `RemoveInput`), réactivation au retour (`syncCapturesOBS`) ; hot-plug natif = session morte jamais réutilisée (recréée à la prochaine application de scène) + dégradation 1 Hz « débranchée ». Reste : relance sans mutation de scène (lot I). 219 tests, clippy 0, svelte-check 0. | `src/lib/stores/moteur.ts`, `capture/camera.rs`, `moteur.rs` |
| E1 | **fait** (2026-11-01) | Overlay **welcome** (clip de bienvenue) : frames `chat_tx` `welcome-clip-play`/`-stop`/`-config` + `position-overlay-config` **partagé** avec les alertes (⚠️ `welcome:etat` = état dashboard, PAS le canal du rendu) ; squelette (carte + cadre de scène + pseudo/CTA/bio) et spotlight (voile 350 ms + carte 85 % + intro 3 s + split 600 ms), `z = 9999`. Sans média (URLs distantes → lot G) et sans son, dégradations documentées. 228 + 223 tests, clippy 0, svelte-check 0. | `texte/welcome.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `src/moteur/mod.rs` |
| E2 | **fait** (2026-11-01) | **Bandeau 1er message** : frames `chat_tx` `bandeau-premier-message-play`/`-stop`/`-config` (⚠️ `bandeau:etat` = dashboard, PAS le rendu) ; barre pleine largeur bas/haut, fond dégradé + filet 2 px + ligne lumineuse aux couleurs du cadre, pastille « ✦ PREMIER MESSAGE », avatar rond 44 px (cache disque), pseudo 15 px + message 20 px, entrée opacité 0,45 s + glissière 0,5 s, sortie 550 ms, `z = 9998`. Mascotte Streamy non portée (asset du serveur). 238 + 230 tests, clippy 0, svelte-check 0. | `texte/bandeau.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `src/moteur/mod.rs` |
| E3 | **fait** (2026-11-01) | **Écran d'attente BRB** : canal réel = frames `chat_tx` **`ecran-attente-etat`** (⚠️ `ecran-attente:etat` = dashboard) ; `muter_bureau` **ajouté à la frame** (le DOM ignore le champ). Aurore teintée cadre, média local plein cadre (`cover`, vidéo en boucle + recalage + son mixé), voile, titre dégradé, carte minuteur (label + chrono + cadre de scène masqué au polygone), layout `signature` (stries à comète, Streamy + orbites, wordmark `screen`→alpha de luminance, morse) — PNG de marque **embarqués** (`include_bytes!` + `decoder_pbgra_memoire`). Mute « bureau » = **gain 0 de l'entrée loopback du mixeur** (aucun changement dans `audio/live.rs`), `z = 9990`. 248 + 233 tests, clippy 0, svelte-check 0. Écart signalé : bord du canvas `i32::MAX` (crate) vs `z-index: 100` (DOM) → peint au-dessus des overlays. | `texte/attente.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `gpu.rs`, `media.rs`, `src/moteur/mod.rs`, `src/ecran_attente.rs` |
| E4 | **fait** (2026-11-01) | **Pad numérique** : canal réel = frames `chat_tx` **`pad-play`/`pad-stop`** + `position-overlay-config` partagé (⚠️ `pad:touche*` = events dashboard). Carte média (image/vidéo une seule lecture) + pop-in `scale(0.6)→1` + fondu 0,3 s, effets `bounce`/`zoom`, mode **plein écran** (contain, sans cadre, transparence), cadre de scène sur la carte, `z = 9995`. Son : vidéo / fichier `son` / touche `audio` — nouveau `media::AudioFichier` (Source Reader audio seul) + `LectureCfg::boucle_infinie` (le pad ne boucle pas). 253 + 236 tests, clippy 0, svelte-check 0. **Chrono 2 non porté** (overlay distinct du même module, à planifier). | `texte/pad.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `media.rs`, `src/moteur/mod.rs` |
| E5 | **fait** (2026-11-01) | **Emoji Arena** : canal réel = frames `chat_tx` **`fight-state`** (⚠️ `fight:update` = dashboard). Arène 1v1 (zones, PV dégradées 3 paliers, emojis **couleur**, chips de coups, badge « À TOI », timers), colonne centrale (tour + timer + temps de match), bannière lobby/fin, toast d'info ; échéances décomptées **en local** (tick 250 ms du DOM), animations lunge/secousse/esquive/popups/impact/KO, `z = 9993`. **Piège payé** : `ID2D1RenderTarget::DrawTextLayout` n'active pas les glyphes COLR/CPAL → emojis blancs ; il faut `ID2D1DeviceContext` + `ENABLE_COLOR_FONT`. 261 + 238 tests, clippy 0, svelte-check 0. | `texte/fight.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `src/moteur/mod.rs` |
| E6 | **FAIT** (2026-11-01) — E6a + E6b + E6c + E6d | **Présentation streamer** : canal réel = frames `chat_tx` **`presentation-show`** (payload `mode` + `donnees`), `-hide`, `-mode`, `-clip-play`/`-clip-stop`, `-audio-play`/`-audio-stop` (⚠️ `presentation:etat` = dashboard). **E6a** : voile `10000` (fond radial, bannière + scrim, 3 orbes, sol, 3 spots, 20 poussières), rideaux + cantonné + embrasses `10001`, public `10003`, entrée/sortie théâtrale et mode `card`. **E6b** : carte `10002` (avatar rond + anneau, nom, `@login`, badges, bio 5 lignes, jeu, lien, `EN DIRECT · viewers`, titre, jeux récents) — colonnes dimensionnées par `repartition_colonnes` (miroir de `flex-shrink`), images par le cache disque (`url_box_art` partagé app/crate). **E6c** : grille 2×2 de vignettes dans la carte (durée + titre tronqué) et lecteur plein écran `10004` — MP4 **préchargé sur disque** par l'app (`medias/clips/`), hôte `clips-media-assets2.twitch.tv` ajouté au contrat vie privée, son du clip mixé. **E6d** : 2 pistes audio (`voix` one-shot, `musique` en boucle) sur `medias/…` → `media::AudioFichier` (E4) + 2 entrées de mixeur, gain 1000 ; `show`/`hide` coupent tout. 286 + 245 tests, clippy 0, svelte-check 0. **Lot E6 complet.** | `texte/presentation.rs` (nouveau), `texte/mod.rs`, `moteur.rs`, `src/moteur/mod.rs` |
| E7 | **fait** (2026-11-01) | **Pop de validation des splits** : **le contenu ne vient pas d'une frame dédiée** — il est dérivé des **mêmes** frames `chat_tx` `speedrun-etat` (`EtatSpeedrun::pop_pour`, miroir `srPointEtatFranchi`), lues **avant** application ; seule la **position** arrive par `splitpop-pos` (squelette dédié `position_splitpop.json`). Badge « SPLIT n », hero Anton `skewX(-5deg)` (delta vs PB cumulé) ou mot VALIDÉ/SKIP, impact `scale(.15) rotate(-9deg) → 1.22 → 1`, re-pop 2 s/4 s, dédup 1,5 s, `z = 9993`. Son du pop non porté (signalé). **Piège payé** : `CENTER` dans une boîte de 4096 px envoie le texte hors écran (E3). 293 + 246 tests, clippy 0, svelte-check 0. | `texte/splitpop.rs` (nouveau), `texte/speedrun.rs`, `scene_sources.rs`, `texte/mod.rs`, `moteur.rs`, `src/moteur/mod.rs` |
| F | **fait** (2026-11-01) | **Just Chatting** : la scène dérivée (`sceneDerivee`, copie en mémoire) partait **seulement** à OBS — le moteur natif gardait la scène complète (trou non percé → capture de fond plein canvas visible). Nouvelle commande **`moteur_appliquer_scene(scene)`** (application différentielle, aucune écriture disque, scène persistée jamais mutée) ; `justChatting.ts` pousse la **même** copie aux deux rendus (ON, OFF = scène réelle, et hook post-sync captures). 293 + 246 tests, clippy 0, svelte-check 0. | `src/lib/stores/justChatting.ts`, `src/lib/tauri.ts`, `src-tauri/src/moteur/mod.rs`, `src-tauri/src/lib.rs` |
| G | **fait** (2026-11-01) | Chat : badges emoji + logo plateforme + avatar rond + emotes inline + bordure par message. **Images = cache disque côté app** (`medias/cache/<fnv16(url)>`, téléchargement à la volée, le moteur ne lit qu'un fichier local — invariant §13.8). Objets inline DirectWrite via un `IDWriteTextRenderer` maison (`DrawTextLayout` ne peint pas les objets inline). 233 + 228 tests, clippy 0, svelte-check 0. | `texte/chat.rs`, `chat.rs`, `scene_sources.rs`, `texte/mod.rs`, `texte/goal.rs`, `moteur.rs`, `cache_images.rs` (app), `moteur/mod.rs`, `Cargo.toml` |
| H | **FAIT** (2026-11-02) | **Lot H complet.** **H1** : titres de widget dans l'**ordre z** — `GenreCalque::Titre` entre le contenu et la bordure, une entrée par titre à son `z` ; les titres de **fond** (`bgTitre*`, `z = i32::MIN+1`) restent en passe finale. **H2/H2b/H2c** : les cinq contenus de widget (chat, input viewer, goal, speedrun, musique) **et** les titres de widget dessinent **dans** l'offscreen du plan `perspective(d) rotateX/Y` (`groupe_de(rect, z)`) — `cible_rt` étant une vue `ID2D1RenderTarget` du même `ID2D1DeviceContext`, le `SetTarget` redirige le texte DirectWrite sans blit ; clé `(rect, z)` verrouillée par test (titre = `TexteCfg.cadre`). **H3** : `line-height: 1.1` des titres (`SetLineSpacing` UNIFORM + baseline demi-interligne), boîte CSS distincte de la texture (marge contour), `margin-*: ±2px` des titres de widget. **H4** : contour **exact** par géométrie de glyphes (`GetGlyphRunOutline` + `Widen`), repli 16 passes pour les polices **simulées** (gras/oblique synthétique). Corrigé au passage : chat cassé par deux `inline.push(brush.cast())` (lot G) + espacement 2 px pseudo/`:` du DOM. 298 + 246 tests, clippy 0, svelte-check 0. | `moteur.rs`, `scene_sources.rs`, `texte/mod.rs`, `texte/chat.rs` |
| I | **en cours** — I-a + I-b faits (2026-11-02) | **I-a** : gel de composition avec `audio_live` — le pipe audio nommé était en `PIPE_WAIT` : FFmpeg cessait de le drainer en attendant de la vidéo, la composition bloquait dans `WriteFile` → interblocage (388 composites puis gel). Passé en `PIPE_NOWAIT` + « bloc sauté » sur tampon plein. Vérifié : 890 composites en 15 s, MP4 à deux flux. **Son du pop de split (`srPointsSon`) porté** (§68) : contrat + rendu + entrée de mixeur, test unitaire. **I-b** : `FfmpegSink` écrit par un **fil dédié** + file bornée (8 paquets, échéance 3 s) → la composition ne bloque plus jamais sur le stdin vidéo, un FFmpeg figé devient une dégradation explicite ; cause d'arrêt conservée dans l'erreur. **Gel n° 2 levé** : la cause n'était ni le pop ni l'audio mais la **fenêtre d'analyse PCM** de FFmpeg (819 200 o = 4,27 s lus au rythme réel **sans lire stdin**) — le stdin vidéo débordait (74 055 o absorbés), la composition bloquée n'alimentait plus le pipe audio → interblocage. Correctif : `-analyzeduration 1` + `-probesize 32` sur l'entrée audio (§75). **Reste** : `FILE_ECRITURE` en octets (marge nulle à 6 000 kbps), décalage audio au démarrage (~1 s de PCM sautée, à vérifier), canvas différent en cours de sortie, `hwnd` périmé, PlayerBar/seek, `capture_jeu`, relance caméra sans mutation. | `output/ffmpeg.rs`, `moteur.rs`, `texte/splitpop.rs`, `texte/speedrun.rs`, `scene_sources.rs`, `texte/mod.rs` |
| J | plus tard | **Fond vert sans OBS** — solution maison (décision utilisateur 2026-11-01), remplace le plugin OBS `background_removal`. | à définir |

### Reprise — prochaine fenêtre de contexte

1. Lire ce plan, puis `docs/features/moteur-natif.md` §58 (reste à faire) et
   la doc de l'overlay concerné dans `docs/features/` (§57 bienvenue, §60
   bandeau, §61 écran d'attente — déjà portés).
2. **E6 — présentation streamer** : **canal réel déjà vérifié** (2026-11-01,
   ne pas le re-supposer) — frames `chat_tx` `presentation-show` (payload
   `mode` + `donnees`), `-hide`, `-mode`, `-clip-play`/`-clip-stop`,
   `-audio-play`/`-audio-stop` ; `presentation:etat` = dashboard.
   Le lot est **volumineux** : l'attaquer par tranches, chacune verte.
   - **E6a — FAIT** (2026-11-01) : décor théâtral. Trois entrées de plan
     `GenreCalque::Presentation` (l'index porte la **couche**) : voile
     `z 10000` (fond radial, bannière + scrim, 3 orbes, sol, 3 spots,
     20 poussières), rideaux + cantonné + embrasses `z 10001`, public
     `z 10003`. Chorégraphie locale (`debut` / `debut_chore` / `fin`) :
     voile 0,5 s, décor 0,6 s, rideaux 1,2 s après le double `rAF` (~33 ms),
     embrasses après 0,75 s, masquage à 1 250 ms (théâtral) / 500 ms (card).
     Mode `card` : **le voile reste affiché** (le code du DOM fait foi, pas
     son commentaire). `Math.random()` → PRNG déterministe.
   - **E6b — FAIT** (2026-11-01, §65) : carte `z 10002` — avatar rond (image
     `cover` clippée au cercle, bord blanc 5 px, anneau violet 6 px), nom,
     `@login`, badges PARTNER/AFFILIATE (dégradé 135°), bio rognée à 5 lignes,
     jeu courant, `twitch.tv/<login>`, `EN DIRECT · <viewers>`, titre live,
     boîtes des jeux récents. Colonnes dimensionnées à leur contenu puis
     réduites proportionnellement (`repartition_colonnes`, miroir de
     `flex-shrink`) ; en mode `card` elles s'empilent (760 px). Images par le
     cache disque ; `url_box_art` (crate) est **la** substitution
     `{width}x{height}` → `96x128`, appelée aussi par l'app au préchargement.
   - **E6c — FAIT** (2026-11-01, §66) : grille 2×2 de vignettes dans la carte
     (cellule 16/9, pastille de durée, titre `nowrap` tronqué à l'ellipse) et
     lecteur plein écran `z 10004` (voile 90 %, vidéo bornée 88 % × 82 %,
     titre 30 px). **MP4 préchargé sur disque** par l'app
     (`medias/clips/<fnv16(url)>.mp4`, `cache_clips.rs`) — hôte
     `clips-media-assets2.twitch.tv` **documenté dans le contrat vie privée**
     avant le code. Fin de clip sur `duree_ms` ; son du clip mixé (gain 1000).
     ⚠️ `DWRITE_WORD_WRAPPING_NO_WRAP` ne rogne pas : il faut `SetTrimming`.
   - **E6d — FAIT** (2026-11-01, §67) : 2 pistes (`voix` one-shot, `musique`
     en boucle) sur `medias/…` (fichiers **locaux**) → `media::AudioFichier`
     (E4) + 2 entrées de mixeur, gain 1000 ; `show`/`hide` arrêtent tout.
     ⚠️ **Pas de vérification audio de bout en bout** : le harnais avec
     loopback se fige dans `Apercu::lire` (bug **pré-existant**, reproduit
     sans la présentation) — cf. l'encart du §67.

### Reprise — après le lot E7

Lots **E6, E7 et F terminés**, **H1** fait (titres dans l'ordre z). Restent
**H2** (3D chat/input), **H3** (`line-height` des titres) et **H4** (contour
exact par géométrie de glyphes) — chacun une tranche à part (§70), puis **I**
(robustesse : gel `Apercu::lire` §67, son du pop §68).
3. Vérifier le canal de **chaque** overlay avant de le porter : events Tauri
   (`presentation:etat`) ou frames `chat_tx` (`splitpop-pos`, `chrono2-etat`).
   Ne jamais supposer.
4. **Chrono 2** (module pad, `chrono2-etat` + `chrono2-pos`) : overlay distinct
   non porté par E4 — à planifier (position dédiée `position_chrono.json`).

### Pièges déjà payés — ne pas les redécouvrir

- La garde « scène vide » **ne doit pas** énumérer les catégories de contenu :
  chaque nouveau renderer créerait un faux refus. Sentinel = « rien n'a jamais
  été configuré » (`jamais_configure`, `moteur.rs`).
- `bgObsSource` est un concept OBS : le moteur natif ne le rend pas (signalé
  en dégradation). Le fond capturé natif vit dans `Scene.bgCaptureCible`
  (lot B) — `bgObsSource` et `bgCaptureCible` coexistent, chaque mode lit le
  sien. Nom de source réservé côté moteur : `scene_sources::FOND_CAPTURE`
  (`"__fond"`), jamais un id de widget.
- Un switch de layout vers une scène avec caméra bloque la composition 1–2 s
  (négociation MF sur le thread de composition) — lot D3.
- Caméra : le cadre de scène s'applique **sauf** forme dédiée reconnue
  (`camera_forme_dediee`, `scene_sources.rs`). Un `cameraForme` inconnu
  (« hexagone ») n'est pas une forme dédiée : cadre de scène, comme le DOM.
- Comète `animer` : le tracé est le **chemin du SVG du style** (rectangle
  plein `mgstyle`, rectangle arrondi `thin-line`), **pas** le polygone de
  découpe — la rognure vient de la couche. Phase = `temps_s / 4`, segment =
  7 % du périmètre, couleur `couleurFin` (jamais le dégradé).
- **Tous les nœuds/runners/étincelles voyagent dans la couche** (donc rognés
  au bord) : le `clip-path` du parent s'applique aussi aux éléments dessinés
  après le groupe clippé du SVG. Ne pas les dessiner hors couche.
- Ornements de **marge** (équerres cyberpunk, circuits, code-barres, alvéoles
  hexagon, étincelles story, volutes) : **hors polygone → invisibles dans la
  référence**, donc non portés. Ne pas les « rajouter » sans preuve visuelle.
- Carrousel : `mediaSlides` non vide est **prioritaire** sur `media` — un
  widget sans `media` doit quand même produire un média (sinon son carrousel
  n'existe pas côté moteur). Même règle pour le fond (`bgSlides` > `bgMedia`).
- `arrondis` : le **clip** est borné (`r = min(8, W/4, H/4)`) mais le
  **tracé de la comète** utilise `rx = 8` non borné (le DOM fait exactement
  ça) — ne pas « corriger » l'un par l'autre.
- Le canvas de session est figé au démarrage : un layout d'un autre canvas est
  signalé mais composé dans l'ancien — lot I.
- **Une ligne « centrée » n'est pas centrée sur la carte** : `CreateTextLayout`
  avec une largeur de 4096 px et `DWRITE_TEXT_ALIGNMENT_CENTER` centre le texte
  dans *la boîte de mise en page*, pas dans la carte — les chiffres du chrono
  de l'écran d'attente atterrissaient 2 000 px à droite (hors du masque, donc
  invisibles). Mise en page en `LEADING` + centrage manuel
  (`x = centre − metrics.width / 2`) ; `CENTER` réservé aux boîtes qui sont
  réellement la boîte de l'élément (titre, largeur 90 % du canvas).
- **Les emojis sortent en blanc avec `ID2D1RenderTarget::DrawTextLayout`** :
  cette interface n'active pas les glyphes COLR/CPAL. Il faut le
  `ID2D1DeviceContext` avec `D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT`
  (vérifié au rendu sur l'arène `!fight`, 2026-11-01).
- **Le bord du canvas est au-dessus des overlays dans le crate** :
  `cadres_depuis_scene` pose `z = i32::MAX` alors que `#cadre-app-overlay` du
  DOM est à `z-index: 100` (sous pad/alertes/bandeau/bienvenue/attente).
  Invisible avec les cartes E1/E2, **visible en plein écran** (E3). Écart
  **signalé, non tranché** (cf. `docs/features/moteur-natif.md` §61).
- **Un arc `D2D1_ARC_SEGMENT` qui prend le grand arc évide la forme** : le
  fauteuil du public (E6a) sortait en « M » (deux lobes + un creux au milieu)
  avec `D2D1_SWEEP_DIRECTION_CLOCKWISE` + `D2D1_ARC_SIZE_SMALL` — les
  extrémités étant à 90° l'une de l'autre dans le **sens anti-horaire**, D2D
  satisfaisait le drapeau en traçant les 270° restants (vers l'intérieur).
  Un contour parcouru bas → droite → haut → gauche (y vers le bas) est
  **anti-horaire** : les quatre coins sont
  `D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE`. Vérifié au rendu (2026-11-01).
- **`repeating-linear-gradient` = une seule brosse `D2D1_EXTEND_MODE_WRAP`**
  sur la période (88 px des plis de rideau) : ne pas paver à la main, et ne
  pas confondre avec `CLAMP` (les plis se figeraient au premier arrêt).
- **`presentation-show` n'arrive qu'une fois** : toute la chorégraphie est
  une horloge **locale** (`debut` / `debut_chore` / `fin`). Un `show` pendant
  le fondu de sortie repart des phases **courantes** (`depart`), jamais du
  noir — le CSS interpole depuis la valeur calculée, pas depuis 0.
- **`cible_rt` n'est PAS une seconde cible** : `DessinateurTexte::nouveau`
  fait `let cible_rt: ID2D1RenderTarget = cible.cast()` sur le **même**
  `ID2D1DeviceContext`. Tout dessin passé par `cible_rt`
  (`DrawTextLayout`, `DrawBitmap`, `PushAxisAlignedClip`, `PushLayer`) suit
  donc le `SetTarget` courant — c'est ce qui permet à un contenu DirectWrite
  (chat, input viewer, lot H2) d'être composé **dans** l'offscreen d'un plan
  à perspective sans offscreen dédié ni blit. Corollaire : un
  `SetTarget` non restauré après un offscreen imbriqué détourne **tout** le
  rendu suivant (motif correct : `GetTarget` avant, restauration après, cf.
  `texte/media.rs`).
- **Ne jamais écrire sur le stdin de FFmpeg depuis la boucle de
  composition** : `WriteFile` y bloque dès que FFmpeg cesse de drainer, et
  la composition bloquée ne produit plus la vidéo qu'il attend — interblocage
  à deux flux (mesuré : 1,5 s puis gel définitif). `FfmpegSink` écrit par un
  **fil dédié** + file bornée + échéance de 3 s (§75). Même règle pour tout
  futur sink bloquant.
- **Une entrée PCM de FFmpeg doit avoir son analyse coupée** : `-f s16le
  -ar -ac` spécifie tout, mais `avformat_find_stream_info` lisait quand même
  **819 200 o (4,27 s de PCM) au rythme réel sans lire stdin**. Le stdin
  vidéo (tampon mesuré : 74 055 o) débordait, la composition bloquée
  n'alimentait plus le pipe audio → **interblocage définitif** (gel n° 2,
  §75). Correctif : `-analyzeduration 1` + `-probesize 32` sur l'entrée
  audio. Toute nouvelle entrée temps réel de FFmpeg doit être traitée de
  même — et non « corrigée » en agrandissant la file d'écriture.
- **Un pipe en `PIPE_WAIT` peut geler la composition** : `PipeAudio::ecrire`
  bloquait dans `WriteFile` quand FFmpeg cessait de drainer le pipe audio
  (il attendait de la vidéo, que la composition bloquée ne produisait plus) —
  interblocage à deux flux, 388 composites puis gel. Le pipe serveur est
  passé en `PIPE_NOWAIT` (bloc sauté sur tampon plein). Corollaire : sur un
  `HRESULT`, comparer `HRESULT::from_win32(code)` et **jamais** le code
  Win32 nu (`0x8007_00E7 ≠ 231`). Détails §74.
- **Une police simulée n'a pas de tracé exact** : `GetGlyphRunOutline` rend
  le contour **non embelli** d'un gras/oblique synthétique (les 20 familles
  embarquées n'ont qu'une graisse, donc tout `titreGras`/`titreItalique` est
  simulé). Le contour exact (H4, §73) est donc réservé aux runs dont
  `IDWriteFontFace::GetSimulations() == 0` ; les autres gardent
  l'approximation par 16 passes décalées. Ne pas « uniformiser » sans
  comparer au rendu actuel.
- **`SetCharacterSpacing` s'applique à **chaque** glyphe de la plage** : un
  trailing de 2 px sur `{pseudo}` espacerait toutes les lettres. Le
  `margin-right: 2px` du DOM se miroite avec une plage d'**un** caractère
  (dernier du pseudo, puis le `:`).
- **Une brosse n'est pas un objet inline** : `cast()` un
  `ID2D1SolidColorBrush` vers `IDWriteInlineObject` rend `E_NOINTERFACE`.
  `construire_bloc` (chat) le faisait pour la puce et le séparateur `:` →
  **tout** message avec pseudo mettait le moteur en
  `Erreur(… séparateur inline …)`, zéro composite, et le sink FFmpeg mourait
  sur « Output file does not contain any stream ». Les deux `push` sont
  supprimés (2026-11-02, §71) : les brosses sont déjà retenues par
  `SetDrawingEffect`. Ne jamais pousser autre chose qu'un vrai
  `IDWriteInlineObject` dans `_inline`.

## Index — docs/features/

> Règle 6 du protocole : avant de modifier une feature, lire son fichier.

| Feature | Doc |
|---|---|
| Plateformes de chat (détails connexion par plateforme) | `docs/features/chat-plateformes.md` |
| Flux des messages chat + emotes | `docs/features/architecture-chat.md` |
| Contrôles Twitch « En direct » (modale Modération, Helix) | `docs/features/twitch-live.md` |
| Planning Twitch (modale « Interactions Live », Helix) | `docs/features/planning-twitch.md` |
| Compteur viewers (widget chat diffusion) | `docs/features/viewers.md` |
| Widgets objectif (goal) + mode lié Creator Goals | `docs/features/objectifs-goal.md` |
| Ordre de plan z des widgets (boutons −/+) | `docs/features/z-order.md` |
| Menus contextuels — recette `.ctx-*` / `CtxMenu.svelte` | `docs/features/ctx-menus.md` |
| Pastille « Vos scènes » masquée + confirmation | `docs/features/pastille-masquee.md` |
| Tableau « Vos scènes » enrichi (mini-map) | `docs/features/tableau-scenes.md` |
| Résolution canvas — miroir OBS + rescale des scènes | `docs/features/canvas-resolution.md` |
| API Stream Deck — HTTP :4321 | `docs/features/api-deck.md` |
| État partagé (tauri::State) | `docs/features/etat-partage.md` |
| Stores frontend | `docs/features/stores.md` |
| i18n dashboard (8 locales) | `docs/features/i18n.md` |
| Unfollows Twitch | `docs/features/unfollows.md` |
| Presets de scènes + onboarding | `docs/features/presets.md` |
| Export ZIP complet | `docs/features/export-zip.md` |
| Import ZIP complet | `docs/features/import-zip.md` |
| Import média MKV → MP4 (ffmpeg sidecar) | `docs/features/import-media-mkv.md` |
| Carrousel d'images | `docs/features/carrousel.md` |
| Caméra — archi/masque OBS/hot-plug (détails) | `docs/features/camera-details.md` |
| Fond de scène = capture OBS (SOS-Fond-*) | `docs/features/fond-capture-obs.md` |
| Sélecteur de capture — miniatures WGC | `docs/features/selecteur-capture.md` |
| Alertes multi-plateformes + interactions chat | `docs/features/alertes.md` |
| Input Viewer (clavier/souris/manette) | `docs/features/input-viewer.md` |
| Morphing (bulge/pinch) | `docs/features/morphing.md` |
| Thème CSS — tokens `--t-*` + recettes UI | `docs/features/theme-css.md` |
| Titres multi-positions (widget + fond) | `docs/features/titres.md` |
| Speedrun Splitter | `docs/features/speedrun.md` |
| Handlers ASL + hot-reload + PB | `docs/features/speedrun-asl.md` |
| Effet miroir média (flip H/V) | `docs/features/miroir.md` |
| Barre de raccourcis TOPMOST | `docs/features/raccourcis.md` |
| Sorties OBS — stream & record | `docs/features/obs-sorties.md` |
| Prévisualisation + projecteur OBS | `docs/features/preview.md` |
| Just Chatting — overlay non-destructif | `docs/features/just-chatting.md` |
| Télémétrie (journal JSONL local) | `docs/features/telemetrie.md` |
| Diagnostic — modale à onglets | `docs/features/diagnostic.md` |
| Grille magnétique + guides + duplication miroir | `docs/features/snap-guides.md` |
| Écran d'attente (BRB) | `docs/features/ecran-attente.md` |
| Guide du streamer — livre feuilletable | `docs/features/guide.md` |
| Fenêtre principale — écran + mode | `docs/features/fenetre.md` |
| Taille d'interface (base adaptative) | `docs/features/taille-ui.md` |
| Bandeau premier message | `docs/features/bandeau.md` |
| Présentation streamer — rideaux + bio | `docs/features/presentation.md` |
| Modules HUD (horloge / musique / objectifs) | `docs/features/modules-hud.md` |
| Module Équaliseur | `docs/features/equaliseur.md` |
| Module Spots lumineux (faisceaux sur le fond) | `docs/features/spots.md` |
| Système CSS unifié + scale rem | `docs/features/css-unifie.md` |
| Modale « OBS WebSocket » — 3 options | `docs/features/obs-websocket-modal.md` |
| Emoji Arena — combat 1v1 piloté par le chat | `docs/features/fight.md` |
| Commande cadres chat « !cadres <couleur> » | `docs/features/cadres-chat.md` |
| Moteur natif — capture/encodage sans OBS (cible) | `docs/features/moteur-natif.md` |
| Mixeur audio natif — N sources, périphériques, vu-mètres | `docs/features/audio.md` |
