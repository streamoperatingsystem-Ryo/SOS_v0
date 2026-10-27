# Alertes multi-plateformes + interactions chat

> Section déplacée depuis `AGENTS.md` — les règles figées (CONTRAT/RÈGLE/anti-patterns) restent dans `AGENTS.md`.


## Alertes multi-plateformes — follow/raid/sub/resub/subgift/bits/gift/superchat (2026-09-12)

> Architecture inspirée de l'ancienne app (RUST_SOS_2026) et réécrite pour la
> base v0 — même pattern que `bandeau.rs`. **Multi-plateformes : les 4 sources
> passent par le même point d'entrée** (`AlertesState::on_event` +
> `EventTwitch`). 8 types : les 6 historiques Twitch + `gift` (objet
> monétaire : TikTok gift, Kick KicksGifted) + `superchat` (don monétaire :
> YouTube SuperChat/SuperSticker).

### Flux

```
Twitch IRC (twitch_chat.rs)
  ├─ USERNOTICE (msg-id: sub|resub|subgift|raid, tags msg-param-*)
  └─ PRIVMSG tag bits=<n> (cheers)
Follows : EventSub WS channel.follow v2 (twitch_eventsub.rs,
push temps réel — plus de poll Helix)
YouTube polling (youtube_chat.rs) : superChatEvent/superStickerEvent
  → superchat, newSponsorEvent → sub, memberMilestoneChatEvent → resub,
  membershipGiftingEvent → subgift
Kick Centrifugo (kick.rs) : SubscriptionEvent → sub,
  GiftedSubscriptionsEvent → subgift, StreamHostEvent → raid,
  KicksGifted → gift
TikTok Webcast (tiktok_chat.rs) : Gift → gift (combos agrégés via
  GiftStreakTracker — 1 alerte en fin de streak), Follow → follow
                    │
                    ▼ moteur : config par type → cooldowns (viewer + global)
                              → file FIFO (max 5) → UNE alerte à la fois
                    ▼ WS chat_tx : alerte-config + alerte-play + alerte-stop
                    ▼ diffusion.html : overlay autonome z 9997 (haut centré,
                      icône emoji + texte template, son via /medias/)
```

### Règles et pièges (leçons de l'ancienne app)

1. **USERNOTICE exige `CAP REQ :twitch.tv/commands`** — avec `tags` seul,
   Twitch n'envoie JAMAIS les USERNOTICE : tout le parsing derrière est du
   code mort (bug "resub sans alerte" en live, corrigé 2026-09-30 — la
   capability est demandée dans `connect_irc` à chaque connexion). Le test
   bouton « Test » masque ce bug (il appelle `on_event` sans passer par
   l'IRC). Le message doit ensuite être **parsé explicitement** dans
   `twitch_chat.rs` — tombé dans un `_ =>` ignoré, aucun raid/sub n'est
   jamais émis (bug "raid sans alerte" ancienne app). La détection de
   commande utilise `irc_command()` (pas `contains` — un message peut
   contenir le mot "USERNOTICE"). msg-id couverts : `sub`, `resub`
   (+ continuations `giftpaidupgrade` / `primepaidupgrade` / `extendsub`),
   `subgift` (+ dons communautaires `submysterygift` / `standardpayforward` /
   `communitypayforward` → destinataire = mass-gift-count ou « la communauté »),
   `raid`. ⚠️ Un resub NON partagé en chat n'émet aucun USERNOTICE — limite
   Twitch : aucun event push (EventSub inclus) ne couvre les renouvellements
   silencieux.
2. **Le follow n'a pas d'event IRC** → EventSub WebSocket `channel.follow` v2
   (`twitch_eventsub.rs`, push temps réel). ⚠️ L'abonnement doit être créé
   dans les 10 s suivant le `session_welcome` (sinon Twitch ferme la socket),
   et 409 Conflict = abonnement identique déjà actif = SUCCÈS (cas
   session_reconnect, où Twitch migre les abonnements). L'ancien fallback
   poll Helix 60s côté frontend (diff snapshots + garde followed_at > boot)
   a été retiré avec l'arrivée d'EventSub (2026-09-26).
3. **Un cheer EST un message** : le PRIVMSG bits continue de circuler dans le
   chat normalement, l'alerte est émise EN PLUS.
4. **Une seule alerte à la fois** : file FIFO (max 5, oldest évincé) + timer
   Rust (`alerte-stop` autoritaire) + fallback timer diffusion (+500ms).
5. **Cooldowns** : par viewer (`type:user_id`, anti-spam) + global (rythme),
   par type, contournés par le bouton Test.
6. **Sons** : importés via `import_son` (mp3/ogg/wav ≤ 10 Mo, magic bytes) dans
   `medias/`, servis par :4321 sur `/medias/`, joués côté diffusion (viewers).
7. **Multi-plateformes (implémenté)** : tous les producteurs appellent
   `AlertesState::on_event(EventTwitch)` — `EventTwitch.plateforme` porte
   l'attribution (`"twitch"|"youtube"|"kick"|"tiktok"`, vide = test manuel)
   et remonte à la racine de la frame `alerte-play` pour la télémétrie.
   Variables template supplémentaires : `{montant}` `{objet}` `{nbObjet}`
   (superchat/gift). La commande Tauri `alerte_declencher` reste dispo pour
   tests/intégrations futures. Payloads Kick non documentés → chaque event
   non-chat loggé à sa 1re occurrence (`[Kick]`) pour ajuster le mapping.

### Squelette de position unifié (clip de bienvenue + alertes) — 2026-09-14

> Refonte : avant, welcome.rs et alertes.rs avaient chacun leur `OverlayConfig`
> (deux fichiers JSON, deux réglages séparés dans la modale). Désormais, un
> seul réglage dans l'onglet "Squelette de position" pilote les deux overlays
> via une source de vérité Rust unifiée.

- `PositionOverlayConfig { x, y, largeur, hauteur }` dans `position_overlay.rs`,
  persistée dans `position_overlay.json` (source de vérité unique).
- `PositionOverlayState` enregistré dans `app.manage()` AVANT welcome_state et
  alertes_state (ils y accèdent via `app.state::<PositionOverlayState>()`).
- `welcome.rs` (`emit_overlay_config`, `set_overlay_config`, `QueueEtat.overlay`)
  et `alertes.rs` (`emit_config_ws`, `set_overlay_config`) lisent la position
  depuis `PositionOverlayState` — les anciens champs `overlay` dans
  `ConfigGlobale` (welcome) et `AlertesConfig` (alertes) sont gardés pour
  rétro-compat deserialization mais inutilisés.
- Poussée à la diffusion via WS `position-overlay-config` → `diffusion.html`
  applique la position au root welcome ET au root alerte en live
  (`applyWelcomeOverlayConfig` + `applyAlerteOverlayConfig`). Les handlers
  `welcome-clip-config` et `alerte-config` sont conservés (ré-émission au
  moment d'un play/test, garantit la position si diffusion reconnecte).
- Réglage dashboard : modale Interactions chat → onglet "Squelette de position"
  (mini-canvas drag/resize rectangle LIBRE — pas de ratio contraint, presets
  6 positions + champs X/Y/Largeur/Hauteur) → `position_overlay_set`.
- **Le DOM de chaque overlay reste spécifique** (carte texte alerte ≠ carte
  avatar+bio+vidéo welcome) — c'est la position/taille qui est partagée, pas
  le rendu. La hauteur est auto côté diffusion (la carte s'ajuste à son contenu).
- **Squelettes DÉDIÉS** (même recette, un slot `config_*` par overlay) :
  chrono 2 (`position_chrono.json`, frame `chrono2-pos`) et pop de validation
  des splits (`position_splitpop.json`, frame `splitpop-pos`, commandes
  `position_splitpop_etat/set`, store `positionSplitpop`, event
  `position-splitpop:etat`). Le pop des splits a été sorti du cadre partagé
  (2026-10-19) : il se déclenche à CHAQUE split pendant une run et pouvait
  chevaucher les alertes. ⚠️ Les events speedrun ne portent PAS la position
  (contrairement aux frames `chrono2-etat` 10 Hz) → resync dédié : la
  diffusion envoie `splitpop-resync` à la connexion WS → `server.rs` appelle
  `position_overlay::reemit_splitpop` qui repousse la frame `splitpop-pos`.
- **« Tester le rendu » par squelette** (onglet Position, bouton à droite des
  presets) : overlays partagés → `alertes_tester("follow")` (pipeline réel,
  no-op si le type follow est désactivé — même comportement que les boutons
  Test de l'onglet Alertes) ; chrono 2 → `position_chrono_test` →
  `PadNumeriqueState::tester_chrono` (chrono factice de 10 s silencieux) ;
  pop des splits → `position_splitpop_test` → frame WS `splitpop-test` →
  diffusion affiche un pop factice « SPLIT 3 / −1.3 / GAGNÉ » reprenant les
  `srOpts` du 1er widget speedrun (couleurs custom, durée, son).

### Médias par alerte + cadre SVG sur les overlays — 2026-09-14

> Refonte modale Alertes (master-detail) + médias (image/vidéo) + cadre SVG
> choisi par l'utilisateur sur les cartes alerte ET clip de bienvenue.

- **Modale Alertes = liste + panneau détail** (master-detail) : colonne gauche
  = 6 types (icône + nom + badge ON/OFF, clic = sélection), panneau droit =
  config du type sélectionné en 4 blocs (Média / Texte / Comportement / Test).
- **Modale Interactions chat = 5 onglets, TOUS en master-detail** (2026-09-14) :
  Clip de bienvenue (sections Général / File d'attente / Attribution), Bandeau
  premier message (Général / Durée / Position / Test & session / Apparence),
  Alertes (6 types), **Commandes** (commandes chat `!cmd`), Squelette de
  position. Mêmes classes CSS (`.alertes-layout` / `.alertes-liste` /
  `.alerte-select` / `.alerte-detail`).
- **Média par type** : `AlerteTypeConfig.media` + `media_kind` ("image" |
  "video", `Option` + `serde(default)` → rétro-compat des anciens
  `alertes_config.json`). Mode dérivé : vidéo → vidéo seule (son porté par la
  vidéo, pas d'audio séparé) + texte ; image → image + son (`son`) + texte ;
  aucun média → icône emoji + texte (comportement historique).
- **Rendu diffusion** (`playAlerte`) : texte au-dessus, média dessous
  (`.alerte-media`, width 100%). Son image via `alerteAudio` ; vidéo en
  `<video autoplay playsinline>` (son inclus, pas d'audio séparé). `stopAlerte`
  pause + reset la vidéo. Fix au passage : `cfg.duree_ms` → `cfgType.duree_ms`
  (référence indéfinie latente, masquée par court-circuit `||`).
- **Couleur du texte = couleur du cadre widget** : si `currentScene.cadreWidget`
  actif → `bpmResoudreCouleurs().p` (même résolution que le bandeau — `.p` =
  extrémité la plus lumineuse du gradient depuis 2026-10-04) ; sinon
  la couleur configurée du type/commande.
- **Import média alerte** : commande `import_alerte_media(kind_attendu)` —
  dialog filtré (Images/Vidéos) → `validate_and_copy_media` (helper commun
  refactoré depuis `pick_and_copy_media` : validation extension + taille +
  magic bytes + copie `medias/`) + garde-fou kind ≠ attendu. Servi par :4321
  sur `/medias/`.
- **Cadre SVG sur les overlays** : `appliquerCadreOverlay(carteEl, idCadre)` +
  `nettoyerCadreOverlay` dans diffusion.html — les cartes des overlays
  autonomes suivent `currentScene.cadreWidget` (le cadre unifié choisi dans
  la modale Cadres — un seul réglage, widgets + bord canvas). Mesure de la carte APRÈS affichage (rAF au play,
  re-mesure au `canplay` vidéo — la hauteur change) + refresh au snapshot si
  l'overlay est visible (changement de cadre à chaud). Clip-path appliqué à la
  carte pour les cadres à forme (cyberpunk/story/hexagon/arrondis). Le bandeau
  premier message reste en couleurs dérivées (pas de SVG — barre pleine largeur).

### Clip de bienvenue — mode spotlight plein écran + favoris (2026-10-05)

> Checkbox « utiliser le squelette de position » (section Général, défaut
> COCHÉ = comportement historique). Décochée → mode spotlight : fond image
> de chaîne plein écran + présentation centrée à 85 % sur panneau glass,
> indépendante du squelette.

- **Config** : `ConfigGlobale.utiliser_squelette: bool` (`serde(default=true)`
  → rétro-compat des `clips_bienvenue.json` existants) + miroir dans
  `QueueEtat` + flag `utiliser_squelette` dans les frames `welcome-clip-play`
  (queue ET test) et `welcome-clip-config`. Commande
  `welcome_set_utiliser_squelette`.
- **Timer** : `INTRO_SPOTLIGHT_MS = 3000` (welcome.rs) — ajouté au timer
  (`jouer_item` + `tester_clip`) en spotlight : le clip ne démarre qu'après
  l'intro avatar de 3 s, la durée configurée reste inchangée (elle s'applique
  à la présentation complète). Sync avec le `setTimeout(3000)` de
  diffusion.html.
- **Bannière de chaîne (2026-10-19)** : `fetch_user_infos` (twitch_chat.rs)
  retourne un triplet `(avatar, bio, banniere)` — `offline_image_url` lu dans
  la même réponse `GET /users` (zéro requête en plus) ; `avatar_cache` en
  triplet. Transport : `on_message` 6e param → `ViewerConfig.banniere` +
  `QueueItem.banniere` (`Option<String>`, serde default → rétro-compat) →
  `viewer.banniere` dans les frames `welcome-clip-play` (queue + test).
  `tester_clip` (lib.rs) passe le 3e élément du fetch Helix.
- **Rendu diffusion** (`playWelcomeClip`) : `.welcome-clip-voile` (noir
  opaque plein écran, z 9999 inséré AVANT `welcomeRoot` → la carte reste
  au-dessus, fondu 350 ms via rAF) contenant `.welcome-clip-fond` en 1er
  enfant — **bannière de chaîne du viewer plein écran** (`object-fit: cover`
  + scrim dégradé `::after`, même recette que `.pres-fond` du mode théâtre) ;
  repli = **avatar flouté** (`.flou` : `blur(70px) brightness(.55)` +
  `scale(1.25)`) ; ni l'un ni l'autre → le voile reste noir. Carte
  `.welcome-clip-root.spot` (centrée `translate(-50%,-50%)`, `width: 85%`,
  flex row) en **panneau glass neutre** (`rgba(12,14,22,.55)` +
  `backdrop-filter: blur(14px) saturate(140%)`, filet `rgba(255,255,255,.14)`,
  radius 18px, ombre douce) — **PAS de cadre SVG de scène en spotlight** :
  `appliquerCadreOverlay(welcomeClipEl,"welcome")` gardé `!welcomeSpotlight`
  (au play, au `canplay`, à la fin d'intro et au refresh snapshot) +
  `nettoyerCadreOverlay` à la bascule `.spot` (cadre résiduel si le mode
  change à chaud). Phase `.intro` : avatar seul 44cqw, textes/clip/favoris
  masqués ; au bout de 3 s → split : colonne gauche 42 % (avatar + pseudo +
  message + CTA + bio + favoris), colonne droite = titre « STREAMER TWITCH »
  (dégradé violet `#9146ff`, letter-spacing, glow) + clip **16:9 strict**
  (`aspect-ratio`, `align-self: center`, rien en dessous). Transition
  `flex-basis` 600 ms. `welcomeDroiteEl` = la colonne droite entière — c'est
  elle qu'on affiche/masque (squelette : canplay ; spotlight : fin d'intro),
  pas le wrap vidéo.
- **Entrée spotlight (2026-10-19)** : classes `.enter` posées sur
  `welcomeFondEl` (si image) + `welcomeRoot` au tick du `display:block`,
  retirées au rAF suivant → le fond part zoomé/flouté (`scale(1.12)` +
  `blur(24px)`, ou `scale(1.42)` + `blur(95px)` en repli avatar) et se
  stabilise (`transform 1600ms ease-out` + `filter 900ms`), la carte pop en
  fondu + `translateY(26px) scale(0.94)` décalée de 120 ms. Squelette :
  aucune `.enter`. Retirées aussi au `stopWelcomeClip`.
- **Favoris du viewer** : `twitch_clips::favoris_channel(login, max)` → GQL
  public `user.channel.home.shelves.streamerShelf` (étagère AUTOHOST =
  streamers mis en avant sur la page d'accueil Twitch, Client-Id web, zéro
  auth, timeout 4 s, jamais fatal → `[]`). Fetch en `tokio::join!` avec la
  résolution MP4 (queue + test), spotlight+Twitch uniquement. Champ
  `favoris` de la frame play : `{login, display_name, avatar, en_live}`
  (max 6) — rendu sous la bio à gauche (avatars ronds + pastille verte si
  live). ⚠️ Test manuel : `tester_clip` n'a que `display_name` → login
  dérivé en lowercase (fetch silencieusement vide si display ≠ login).
- **i18n** : `interactions.bienvenue.utiliserSquelette` +
  `utiliserSqueletteHint` (les 8 locales).

### Commandes chat — "!commande" tapée par un viewer (2026-09-14)

> Nouveau module `commandes.rs` — même moteur que `alertes.rs` (file FIFO max
> 5, UNE carte à la fois, cooldowns viewer + global, timer Rust autoritaire +
> fallback diffusion) mais déclenché par un MESSAGE chat : le 1er mot (ex:
> `!hype`) est comparé aux commandes enregistrées. Le message continue de
> circuler dans le chat (comme un cheer : l'overlay est EN PLUS).

- **Config** : `CommandeConfig { id (uuid), actif, commande (sans `!`,
  normalisée lowercase), duree_ms, texte_template, couleur, taille_px,
  cooldown_viewer_s, cooldown_global_s, son, media, media_kind }` — liste
  persistée `commandes_config.json`. Variables template : `{pseudo}`
  `{commande}` `{message}` (reste de la ligne après la commande).
- **Hooks chat** : `CommandesState::on_message(pseudo, display_name, texte)`
  appelé à côté du hook bandeau dans twitch_chat.rs / kick.rs / tiktok_chat.rs
  / youtube_chat.rs (non-fatal si état absent). Le 1er mot du message (sans
  `!`, lowercase) match une commande enregistrée + active.
- **Cooldowns** : par viewer (clé `id:pseudo` — ChatMessage n'a pas de
  user_id) + global par commande, contournés par le bouton Test.
- **WS** : `commande-config` (liste complète, poussée avant chaque play) +
  `commande-play` + `commande-stop` → diffusion.html overlay `.commande-root`
  z 9996 (sous alertes 9997). Même carte que les alertes : texte au-dessus +
  média dessous + cadre SVG (`appliquerCadreOverlay(carte, "commande")`) +
  couleur du texte = couleur du cadre widget si actif.
- **Squelette de position** : partagé (position_overlay.rs) — le WS
  `position-overlay-config` positionne les 3 overlays (welcome + alerte +
  commande).
- **Commandes Tauri** : `commandes_etat`, `commande_set_config` (upsert par
  id, id généré si vide, nom normalisé trim/!-/lowercase), `commande_supprimer`,
  `commande_tester`. Frontend : `stores/commandes.ts` + onglet "Commandes"
  (master-detail : liste gauche + bouton « + Ajouter », détail = Commande /
  Média / Texte / Comportement / Test & suppression).

### Pad numérique — 16 touches Numpad × 3 plages (2026-09-17)

> Porté de l'ancienne app (`features/pad-numerique/`), réécrit pour v0
> (`pad_numerique.rs`, même pattern que alertes.rs / commandes.rs).
> Déclenchement par le STREAMER uniquement : capture clavier GLOBALE des
> touches Numpad (thread poll `GetAsyncKeyState` dédié 60Hz, indépendant
> de input_viewer.rs) → sons/images/vidéos sur l'overlay de diffusion.
> Le message chat n'existe PAS ici — rien à voir avec les commandes `!cmd`.

- **Config** `pad_numerique.json` : `actif`, `plage_actuelle`, touches
  (14 assignables — `NumpadAdd`/`NumpadSubtract` RÉSERVÉES : navigation
  entre les 3 plages 0/1/2) → `media_type` (son|image|video) +
  `media`/`son`/`duree_ms`/`volume`/`bounce`/`zoom`/`plein_ecran`/
  `transparence`/`transparence_opacite` (0-1, défaut 0.5) par touche.
- **Plein écran + transparence (2026-10-18)** : `plein_ecran` (image/video)
  → le média couvre TOUT le canvas de diffusion — position d'overlay
  IGNORÉE (`applyPadOverlayConfig` mémorise sans appliquer tant que la
  classe `pad-plein` est posée), PAS de cadre SVG (`nettoyerCadreOverlay`
  retire celui d'un play précédent + clip-path, sinon la vidéo serait
  découpée à l'ancienne taille carte), pas de pop-in scale ni coins
  arrondis, `object-fit: contain` (toute la vidéo visible, bandes si ratio
  différent), bounce/zoom ignorés. `transparence` = sous-option du plein
  écran (case affichée seulement si plein écran coché) : opacité
  `transparence_opacite` posée inline sur le média → la scène derrière
  reste visible (superposition). UI : cases horizontales `.pad-check`
  (case à gauche + label à droite) dans le bloc Comportement, curseur
  d'opacité 0-100 % pas 5 seulement si les deux cases sont cochées.
- **Overlay diffusion** : `pad-play`/`pad-stop` sur chat_tx → z 9995
  (sous commandes 9996), même squelette de position que welcome/alertes/
  commandes (`position_overlay.rs`).
- **Audio double face** : joué côté diffusion (OBS le capte pour les
  viewers) ET côté dashboard (`event Tauri pad:touche` → le streamer
  l'entend sur son PC) ; vidéo = son porté par la vidéo côté diffusion
  uniquement (pas de doublon dashboard).
- **Commandes** : `pad_etat`, `pad_set_actif`, `pad_set_touche`,
  `pad_supprimer_touche`, `pad_set_plage`, `pad_tester_touche`,
  `pad_importer_media`, `pad_importer_son`. Frontend : `stores/padNumerique.ts`
  + onglet « Pad numérique » de la modale Interactions Live.
- **Télémétrie** : frames `pad-play` whitelistées → kind `pad`,
  plateforme `local`.

### Musique — "!rs <titre>" demandé par un viewer (bibliothèque LOCALE)

> Nouveau module `musique.rs`. Catalogue = fichiers audio importés par le
> streamer dans `medias/musique/` (servis par :4321 via le ServeDir `/medias`
> existant — sous-dossier = zéro changement serveur). **Rien n'est cherché
> en ligne** : le nom du fichier EST le titre demandable (nettoyé : `_` →
> espace). Le contraire de commandes.rs : les pistes durent des minutes →
> file NON bornée à 5 (refus explicite à `file_max`=20, jamais d'éviction
> silencieuse) et PAS de timer de libération — l'avancement est piloté par
> l'event `ended`/`error` de l'`<audio>` de la diffusion, remonté en WS
> upstream `musique-ended {id}` (id vérifié = dedup multi-clients).

- **Hook** : `MusiqueState::on_message(plateforme, pseudo, texte, est_mod)`
  appelé à côté du hook commandes dans les 4 modules chat. Retourne
  `Option<String>` = accusé chat (`✓ ajouté (position N)` / `✗ introuvable`
  / `⏳ cooldown` / `✗ file pleine`) — posté en PRIVMSG par twitch_chat.rs
  (seule plateforme avec canal d'envoi ; `reponses_chat` off = silence).
- **est_mod par plateforme** (`!skip` = mods + streamer) : Twitch = tag
  `badges` contient `moderator/`/`broadcaster/` OU pseudo == login ;
  YouTube = badges déjà normalisés (`moderator`/`broadcaster`) ; Kick =
  `sender.identity.badges[].type` OU `sender.slug` == `KickState.slug_courant()`
  (pub(crate) désormais) ; TikTok = `unique_id` == username suivi (pas de
  badge exposé par PirateTok).
- **Commandes chat** (noms configurables `musique_config.json`, défauts) :
  `!rs` demander (cooldown viewer + `max_par_viewer`=3 + `file_max`) ;
  `!skip` (mod uniquement — silence pour les viewers) ; `!retirer` (sa
  dernière demande en file) ; `!file` (en cours + 3 suivantes).
  **Collision** : `commande_set_config` refuse un nom repris par les
  commandes musique (`noms_reserves`), et inversement.
- **WS** : `musique-play {piste:{id,titre,src,demandeur}, volume}` /
  `musique-stop` / `musique-pause {pause}` / `musique-volume`. La diffusion
  joue UN `<audio>` global (`PREVIEW` → muet) + remplit `.chat-musique`
  (phrase défilante marquee, 2 copies + translateX -50%) dans l'en-tête de
  CHAQUE widget chat — élément SÉPARÉ de `.chat-viewers` (cachée en mode
  bulles, la ligne musique doit rester visible). `wsCourant`/`wsSend` =
  référence stable pour l'upstream (`ws` local à `connect()` est recréé
  aux reconnexions) ; `musique-resync` à l'open → `reemit` rejoue le play
  courant (reload OBS en cours de piste). Au passage : barre viewers
  centrée (`justify-content:center` sur `.chat-viewers`).
- **Dashboard** : event `musique:update` (état complet : config + en_cours
  + file + bibliothèque + progression + cooldowns_actifs — pas de diff).
  Onglet « Musique » de la modale Interactions Live (`MusiqueTab.svelte`,
  composant séparé comme PlanningTab) : encart explicite « catalogue
  local », lecteur Winamp (`LecteurMusique.svelte`, voir ci-dessous), file
  (monter/descendre/retirer/vider), bibliothèque (import multi, écouter
  = preview local `ecouterSon`, tester = play diffusion, supprimer).
- **Progression + cooldowns dans `etat()`** : `en_cours` reste sérialisé
  en `MusiqueItem` (contrat WS inchangé) ; le timing vit dans
  `LectureEnCours` {item, debut, pausee, debut_pause, pause_accum,
  duree_s}. `etat()` expose `progression {position_ms, pausee, duree_s}`
  (SNAPSHOT figé à l'emit — le lecteur l'avance en tick local 250 ms,
  zéro trafic périodique) et `cooldowns_actifs [{pseudo, restant_s}]`
  triés. `pause(bool)` trace `debut_pause`/`pause_accum` ; `reemit()`
  reset le timing (la piste repart à 0 après reload OBS, duree_s
  conservée) ; `on_meta(id, duree_s)` reçoit la durée remontée par
  `loadedmetadata` de l'<audio> diffusion (upstream `musique-meta`, seule
  source fiable — routée dans `server.rs` à côté de `musique-ended`).
- **Équalizer spectre réel** : côté diffusion, une `<audio>` « ombre »
  MUETTE joue le même fichier → `createMediaElementSource` → AnalyserNode
  → gain(0) → destination (l'<audio> AUDIBLE n'entre JAMAIS dans le
  graphe — un AudioContext suspendu couperait le son du stream). Tick
  66 ms → 16 bandes ; upstream `musique-spectrum {id, bands}` ~8 Hz →
  `server.rs` le re-émet en `musique:spectrum` (Tauri) pour le lecteur
  dashboard. Fallback bandes simulées si ctx indisponible ; pause de
  l'ombre synchronisée sur `musique-pause`.
- **Lecteur Winamp dashboard** (`LecteurMusique.svelte`, intégré dans le
  bloc « en cours » de l'onglet Musique — deck complet façon Winamp 2.x) :
  barre de titre dégradé nuit `WINAMP` + **shade cliquable** (tout le
  deck se replie sous la barre, état local), clutterbar `O A I D V`,
  grand compteur LED `mm:ss`, écran LCD vert phosphore (marquee `♪ titre
  — @demandeur [plateforme]`, 2 copies + translateX -50% ; indicateurs
  décoratifs kbps/kHz/stereo — pas de métadonnées bitrate, le fichier
  joue côté diffusion), équalizer 16 bandes (réel via `musique:spectrum`,
  simulé si frames >600 ms) + **peak dots** ambrés (crêtes accrochant la
  bande, retombées ~1,5 s), barre de progression crantée (affichage seul
  — pas de seek en v1), transport ⏮⏯⏹⏭⏏✕≡ (⏮ = `musiqueTester(src)`
  rejoue la piste, ⏏ = `importerMusique()` ; stop garde la confirmation
  de l'onglet via prop `demanderConfirm`), **playlist intégrée** (piste
  courante `1▸` surlignée + file numérotée ↑↓✕ — le bloc « File
  d'attente » de l'onglet a été SUPPRIMÉ, la file vit dans le deck),
  chips `CD pseudo Ns` décrémentés en local.
- **Widget scène `musique`** (`createMusiqueWidget`, 340×140) : rendu dans
  `diffusion.html` (branche `wtype === "musique"`, modèle `goal` — create/
  update/reuse via `widgetMap`, kind `musique`) : barre de titre `WINAMP`
  dégradé nuit + rangée écrans `.mu-lcd` (boîte LED temps `.mu-temps`
  pos/rest à gauche + écran marquee `.mu-defile-wrap` à droite, placeholder
  `♪ —` si rien ne joue) + 16 barres EQ avec **peak dots** (`eqPeaks[]`
  décrémenté dans `eqTick`, affiché si crête > bande) + barre de
  progression crantée ; tick 250 ms partagé démarré à la création.
  **Contrat visuel « skin classic »** (palette canonique : châssis
  #2e2e2e→#1c1c1c + bevel outset, écrans #0a140a/#050805 + inset #444,
  EQ #ff4040→#ffb040→#4aff4a + peak #ff9c40, fill #7dff7d→#2fd32f,
  "Lucida Console") — diffusion.html est la référence, toute retouche se
  reporte aux deux miroirs. Dimensionnement `--mu-w`/`--mu-h` inline.
  Vignette dashboard = `WidgetMusique.svelte` (miroir 1:1 du markup +
  CSS mu-*, tick local 250 ms sur `progression` + `musiqueBands`).
  Option `musiqueMode` : `"player"` (défaut, tout) / `"eq"` (bandes
  seules, classe `mu-eq-only` — titre masqué aussi) — `setMusiqueConfig`,
  carte d'édition.
- **Import** : `musique_importer` — dialog multi-fichiers, nom lisible
  CONSERVÉ (≠ uuid des alertes : c'est le titre demandé), exts
  `mp3/ogg/wav/m4a/flac`, magic bytes (`fLaC`, `ftyp` offset 4), ≤ 50 Mo
  (10 Mo des alertes est trop juste pour la musique), collision → `-2`.
- **Télémétrie** : `musique-play` whitelisté → kind `musique`, plateforme
  `multi` (attribution impossible depuis la frame seule).

### Fichiers

| Fichier | Rôle |
|---|---|
| `src-tauri/src/position_overlay.rs` | Source de vérité unifiée : `PositionOverlayConfig` + `PositionOverlayState` (persistée `position_overlay.json`), WS `position-overlay-config` |
| `src-tauri/src/alertes.rs` | Moteur : config 8 types (persistée `alertes_config.json`), file, cooldowns, templates, overlay (lit depuis PositionOverlayState), WS |
| `src-tauri/src/commandes.rs` | Commandes chat `!cmd` : config par commande (persistée `commandes_config.json`), file + cooldowns + timer, hooks chat, WS `commande-config/-play/-stop` |
| `src-tauri/src/musique.rs` | Musique `!rs` : bibliothèque locale `medias/musique/`, file non bornée + cooldowns + max/viewer, hooks chat (accusés Twitch), WS `musique-play/-stop/-pause/-volume`, upstream `musique-ended/-resync/-meta/-spectrum`, `LectureEnCours` (timing progression) + `cooldowns_actifs` dans `etat()` |
| `src/lib/stores/musique.ts` | Store musique : `musiqueEtat` (`musique:update`), `musiqueBands`/`musiqueBandsTs` (`musique:spectrum`), `initMusique`, config/import/contrôles file |
| `src/lib/components/LecteurMusique.svelte` | Lecteur « Winamp 2.x » de l'onglet Musique : barre de titre + shade, compteur LED, LCD marquee + indicateurs, EQ 16 bandes + peak dots (réel/simulé), barre progression, transport ⏮⏯⏹⏭⏏ + volume, playlist intégrée (file), chips cooldowns — tick local 250 ms sur snapshot `progression` — palette « skin classic » de référence |
| `src/lib/components/WidgetMusique.svelte` | Vignette dashboard du widget musique : miroir 1:1 du rendu `diffusion.html` (mu-titre / mu-lcd LED+marquee / mu-eq peaks / mu-posbar crantée), dimensionnement `--mu-w`/`--mu-h`, tick local 250 ms |
| `src/lib/components/MusiqueTab.svelte` | Onglet « Musique » de la modale Interactions Live : lecteur Winamp (file incluse), bibliothèque, réglages |
| `src-tauri/src/twitch_chat.rs` | `irc_command()` + `parse_irc_tags()` + `parse_usernotice()` + tag bits |
| `src-tauri/src/lib.rs` | `AlertesState` + `PositionOverlayState` + commandes `alertes_*` / `alerte_declencher` / `import_son` / `import_alerte_media` / `position_overlay_etat` / `position_overlay_set` |
| `src/lib/stores/positionOverlay.ts` | Store unifié : `positionOverlay`, `initPositionOverlay`, `chargerPositionOverlay`, `sauverPositionOverlay` |
| `src/lib/stores/alertes.ts` | Config store par type (événements gérés côté Rust — IRC + EventSub Twitch, polling YouTube, Centrifugo Kick, Webcast TikTok) — overlay retiré (géré par positionOverlay) |
| `src/lib/stores/commandes.ts` | Store commandes chat : `commandesStore`, `chargerCommandes`, `sauverCommande` (upsert), `supprimerCommande`, `testerCommande` |
| `src/lib/components/InteractionViewerModal.svelte` | Modale Interactions Live — 9 onglets : Clip de bienvenue / Bandeau premier message / Alertes / Commandes / Pad numérique / Présentation / Planning / Musique / Squelette de position |
| `src-tauri/resources/diffusion.html` | Overlay welcome (z 9999) + overlay alerte (z 9997) + overlay commande (z 9996) + handler `position-overlay-config` (applique aux trois) |

**NB organisation UI** : le panneau Alertes vit dans la modale **Interactions
Live** (ex "Interactions chat", ex "Interaction viewer") — 9 onglets : Clip de
bienvenue / Bandeau premier message / Alertes / Commandes / Pad numérique /
Présentation / Planning / Musique / Squelette de position. La section
"Alertes" de la sidebar Toolbar a été supprimée (tout passe par la modale).
La section sidebar "Interactions chat" est un **lanceur direct** (pas un
accordéon) : le clic ouvre la modale — seule section non dépliable de la
sidebar.
