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
- **Ce qui sort réseau** : UNIQUEMENT les APIs officielles des plateformes
  (Twitch Helix/IRC/EventSub, YouTube Data API, Kick Pusher, TikTok
  Webcast) + APIs publiques d'emotes (BTTV/7TV/FFZ) + jacquettes
  (`static-cdn.jtvnw.net`, hôte figé) + GQL public des clips Twitch. Toutes
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
