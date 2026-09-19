// TEMPORAIRE — panneau Dev. Retirer avant release.
// Tableau statique des validations v0. Pas d'API, pas de Git, pas de poll.
// Nouvelle validation = ajouter 1 ligne ici. Rien d'autre.

export interface DevEntry {
  id: number;
  titre: string;
  date: string;
  note: string;
}

export const devLog: DevEntry[] = [
  {
    id: 1,
    titre: "Widget + persist config.json",
    date: "2026-08-27",
    note: "Scène widgets, save/load AppData, source de vérité unique.",
  },
  {
    id: 2,
    titre: "Import image < 10 Mo",
    date: "2026-08-27",
    note: "Dialog + magic bytes + copie vers medias/<uuid>.<ext>.",
  },
  {
    id: 3,
    titre: "Diffusion http://127.0.0.1:4321/ + WebSocket",
    date: "2026-08-27",
    note: "Serveur axum + WS snapshot, page vanilla sans Tauri.",
  },
  {
    id: 4,
    titre: "OBS scène SOS + source SOS-Diffusion",
    date: "2026-08-27",
    note: "Connect one-shot, scène + source browser sans doublon.",
  },
  {
    id: 5,
    titre: "Image dashboard = image OBS",
    date: "2026-08-27",
    note: "Même média servi sur /medias, rendu identique dashboard/diffusion.",
  },
  {
    id: 6,
    titre: "Sidebar 2 sections + options au clic widget",
    date: "2026-08-27",
    note: "Accordéons Widgets/OBS, clic widget → options, clic vide → désélection.",
  },
  {
    id: 7,
    titre: "Refresh OBS au démarrage",
    date: "2026-08-27",
    note: "server_ready → obsConnect + refreshnocache sur SOS-Diffusion.",
  },
  {
    id: 8,
    titre: "Canvas = résolution OBS",
    date: "2026-08-27",
    note: "GetVideoSettings → canvasW/canvasH, dashboard + diffusion dynamiques.",
  },
  {
    id: 9,
    titre: "Inclinaison 3D widget (gizmo)",
    date: "2026-08-27",
    note: "rotateX/rotateY persistés, gizmo carré+croix+boule, perspective dashboard + :4321.",
  },
  {
    id: 10,
    titre: "Resize widget + clamp 50%",
    date: "2026-08-27",
    note: "Poignées coins+bords sur outer sélectionné, mini 80×80, clamp canvas moitié visible après drag et resize.",
  },
  {
    id: 11,
    titre: "Modes affichage média",
    date: "2026-08-27",
    note: "mediaFit (ajuster/remplir/etendre/etirer/centrer/vignette), 6 boutons UI, rendu dashboard + :4321.",
  },
  {
    id: 12,
    titre: "Import vidéo mp4/webm ≤80 Mo",
    date: "2026-08-27",
    note: "Bouton « Importer un média », kind image|video, <video> dashboard (clic play/pause) + diffusion (autoplay loop), mediaFit appliqué.",
  },
  {
    id: 13,
    titre: "Supprimer widget",
    date: "2026-08-27",
    note: "Bouton « Supprimer le widget » + confirmation Oui/Non, retire widget + deselect + commit, média conservé dans medias/.",
  },
  {
    id: 14,
    titre: "Zoom et rotation 2D média",
    date: "2026-08-27",
    note: "mediaZoom (0.2…5) + mediaRot (−180…180) sur l'élément média, cadre widget fixe, overflow:hidden, dashboard + :4321 identiques, vidéo non recréée.",
  },
  {
    id: 15,
    titre: "Fond de scène image/vidéo",
    date: "2026-08-27",
    note: "bgMedia/bgKind/bgFit/bgZoom/bgRot sur la scène, section Scène en haut de la sidebar, calque fond sous les widgets (dashboard clic play/pause, :4321 autoplay loop), import via helper commun, pas de reset vidéo.",
  },
  {
    id: 16,
    titre: "Barre lecteur dashboard (pilote :4321)",
    date: "2026-08-27",
    note: "Barre −5s/Lecture-Pause/+5s + seek dans la sidebar, pilote :4321 via mediaPaused/mediaTime (snapshot WS), dashboard figé (vignette à mediaTime), :4321 play/pause/loop selon mediaPaused, seek selon mediaTime.",
  },
  {
    id: 17,
    titre: "Scènes barre + API Stream Deck",
    date: "2026-08-27",
    note: "Multi-scènes sur disque (index.json ids+noms + <id>.json contenu, une seule en RAM), migration config.json au boot, barre haute pastilles + nom éditable au centre + boutons +/Exporter/Importer, commandes Tauri scenes_*, API HTTP /api/* sur :4321 (scenes/ouvrir/nouvelle/courante), export dossier portable (scene.json + medias/ référencés seulement, erreur si dossier existe), import dossier pack (copie medias uuid anti-collision + réécriture chemins) ou legacy .json (cases vides si médias manquants), changement de scène = selectedId null + snapshot.",
  },
  {
    id: 18,
    titre: "Trou alpha widget",
    date: "2026-08-27",
    note: "Widget kind=trou (géométrie inchangée, pas de média), dashboard cadre pointillé « Trou », :4321 zone transparente (alpha 0) via masques CSS SVG fill-rule=evenodd sur wrappers DOM par-widget, fond + widgets autour intacts, widgets z>trou recouvrent la caméra, html/body/#stage transparent.",
  },
  {
    id: 19,
    titre: "Trou 3D quad projeté",
    date: "2026-08-27",
    note: "Le trou suit rotateX/rotateY du widget : diffusion.html remplace fillRect par un quad projeté (projectCorner = perspective 800px, même ordre que CSS rotateY puis rotateX), fast path fillRect si rx=ry=0. Dashboard : trou-overlay tilté avec la même perspective → cadre pointillé = trou :4321 pixel près.",
  },
  {
    id: 20,
    titre: "Source OBS calée sur le widget",
    date: "2026-08-27",
    note: "Bouton sidebar « Source OBS sous SOS » si widget.trou : dialog type (caméra/fenêtre/jeu) → énumération cibles via OBS WebSocket (GetInputPropertiesList sur input temp) → création source sous SOS-Diffusion (SetSceneItemIndex + SetSceneItemTransform = widget x/y/w/h). Champ obsSource sur Widget, sync au commitScene (reconnect one-shot, pointerup). Désactiver le trou → suppression source OBS. OBS offline → message, pas de crash.",
  },
  {
    id: 21,
    titre: "Réseau : Twitch Device Code + chat IRC + widget chat",
    date: "2026-08-28",
    note: "Section Connexions 5 plateformes (Twitch réel, autres « Bientôt »). Device Code Flow Twitch (twitch_auth.rs) + coffre keyring (Windows Credential Manager, service streamos-v0-twitch). Auto-resume boot si token valide. IRC WebSocket (twitch_chat.rs) → emit chat:message (dashboard) + chat_tx broadcast (diffusion :4321 WS type chat). Widget type « chat » : bulles carte arrondie, zébrage --fond, logo Twitch SVG 12px, filtre unifie/plateforme, taille police. Pas de 2e overlay, pas de play() dashboard.",
  },
  {
    id: 22,
    titre: "Twitch Communauté Helix",
    date: "2026-08-28",
    note: "twitch_helix.rs : followers (pagination 100/page, login+followed_at), subs (total+points, liste tier 1/2/3 + gift), viewers (GetStreams), broadcaster (avatar+display_name+type). communaute.ts : chargerCommunaute (4 appels parallèle), 403 → communauteErreur=\"need_reauth\" → banner « Reconnecter Twitch pour la communauté » (reconnecterTwitch = revoke token + nouveau Device Flow scopes étendus). $effect Toolbar : twitch connecté → chargerCommunaute, déconnecté → resetCommunaute. Section Communauté sidebar : broadcaster (avatar+nom+type), viewers live ou Hors-ligne, followers (total + liste login+date), subs (total+points, liste tier+gift), bouton Rafraîchir.",
  },
  {
    id: 23,
    titre: "Pop-out chat (fenêtre détachable)",
    date: "2026-08-28",
    note: "chat_popout_toggle : WebviewWindowBuilder always_on_top 360×520 resizable, custom protocol streamos-chat://localhost/chat (HTML via include_str! chat-popout.html servi par register_uri_scheme_protocol). chat-popout.html : fond en dur #0e0e10 (pas de theme-sos.css), bulles + zébrage comme diffusion.html, écoute event chat:message. Bouton sidebar « Détacher »/« Réattacher » (toggle), croix native → emit popout-closed → bouton redevient « Détacher ». IRC inchangé dans les deux cas. chat_popout_fermer pour fermeture explicite.",
  },
  {
    id: 24,
    titre: "Kick : WebSocket Pusher cloud",
    date: "2026-08-29",
    note: "kick.rs : resolve_chatroom (GET kick.com/api/v2/channels/<slug>, headers UA+Referer+Origin pour Cloudflare, détection réponse HTML=blocage CF) → chatroom.id. run_ws : WS Pusher cloud wss://ws-us2.pusher.com (app key 32cbd69e4b950bf97679, cluster us2), subscribe chatrooms.<id>.v2, ping/pong 20s, App\\Events\\ChatMessageEvent (data double-encodée) → ChatMessage plateforme=\"kick\" + double diffusion emit chat:message + chat_tx. KickState : connected + slug + cancel(AtomicBool) + ws_handle(JoinHandle), stop_ws (cancel+abort), reset_cancel, cancel_clone. config.rs : kick.json {slug} sauver/lire/effacer. Auto-resume boot : lire_kick_slug → resolve_chatroom → run_ws. Store kick.ts : connecterKick(slug), deconnecterKick, lireSlugSauve. Toolbar : input slug + bouton Connecter/Déconnecter.",
  },
  {
    id: 25,
    titre: "TikTok : WebSocket Webcast (PirateTok)",
    date: "2026-08-29",
    note: "tiktok_chat.rs : crate piratetok-live-rs vendored dans vendor/ (build.rs neutralisé — bug Windows path separators). WS Webcast protobuf : Chat → ChatMessage plateforme=\"tiktok\" (pseudo=nickname, avatar=avatar_thumb.url_list[0]), RoomUserSeq → emit tiktok:viewers, Gift (log seulement, pas de forward — prévu event dédié plus tard), Like (ignoré). Auto-reconnect géré par PirateTok (50 retries, backoff exponentiel). TiktokState : chat_handle + cancel + connected + username. config.rs : tiktok.json {username} sauver/lire/effacer. Auto-resume boot : lire_tiktok_username → tiktok_chat::demarrer. Store tiktok.ts : connecterTiktok(username), deconnecterTiktok, lireUsernameSauve. Toolbar : input username + bouton Connecter/Déconnecter.",
  },
  {
    id: 26,
    titre: "YouTube : Device Flow + chat polling + Data API",
    date: "2026-08-29",
    note: "youtube_auth.rs : Device Code Flow Google (oauth2.googleapis.com), scope youtube.readonly, keyring streamos-v0-youtube, Tokens {access, refresh, user_id, channel_id, login}, refresh sur 401 + retry, revoke à la déconnexion. youtube_chat.rs : polling GET /liveChat/messages (pollingIntervalMillis respecté), resolve activeLiveChatId via /videos?part=liveStreamingDetails&mine=true, retry 30s si pas de live (emit youtube:erreur), backoff reconnexion, event youtube:pas-de-live → bouton Chat OFF. youtube_data.rs : channel (snippet+statistics → avatar, abonnés, vues, vidéos), members (GET /members, 403 géré gracieusement avec Device Flow), live viewers (concurrentViewers). YoutubeState : chat_handle + cancel + connected + login + channel_id + access. Auto-resume boot : lire_tokens keyring → connect_with_tokens_youtube. Store youtube.ts : connecterYoutube, deconnecterYoutube, demarrerChatYoutube/arreterChatYoutube (toggle Chat ON/OFF indépendant de la connexion compte). Toolbar : bouton Connecter/Déconnecter + Chat ON/OFF. Communauté YouTube dans sidebar : channel avatar+abonnés, viewers live, vues totales, vidéos, members liste.",
  },
  {
    id: 27,
    titre: "Topbar multi-plateformes + séparateurs",
    date: "2026-08-29",
    note: "App.svelte header : 6 indicateurs (Twitch/Kick/YouTube/TikTok/OBS/:4321) avec points vert/gris + libellé. Séparateurs verticaux (1px, opacité 25%) entre chaque indicateur. Slug Kick + username TikTok affichés à côté du libellé quand connecté (symétrie avec login Twitch/YouTube). Stores kickSlug + tiktokUsername dans chat.ts : listeners kick:connecte (lit slug persisté via kickSlugCourant car l'event émet le channel Pusher) + tiktok:connecte (payload=username directement), reset à null sur déconnexion, lecture au boot si auto-resume a déjà connecté.",
  },
  {
    id: 28,
    titre: "Clips de bienvenue (welcome-clip)",
    date: "2026-08-29",
    note: "welcome.rs : registre viewers Twitch (persisté clips_bienvenue.json), détection 1er message d'un streamer enregistré, file d'attente + timer, overlay autonome diffusion (welcome-clip-play/-stop/-config). InteractionViewerModal : attribution auto followers→clips, contrôles queue (stop/skip/remonter/descendre/vider/reset session), tester clip, durée forcée, config overlay via mini-canvas drag/resize (ratio portrait contraint). Widget welcome-clip = placeholder dashboard, rendu 100% diffusion.",
  },
  {
    id: 29,
    titre: "Grille magnétique + guides d'alignement",
    date: "2026-08-29",
    note: "Snap entre widgets + bords/centre canvas (seuil 6px, cibles figées au pointerdown, bypass Alt = free-drag). AlignmentGuides.svelte : lignes visuelles pendant le drag/resize. Intégré moveWidgetLocal/resizeWidgetLocal (scene.ts).",
  },
  {
    id: 30,
    titre: "Bandeau premier message",
    date: "2026-08-30",
    note: "bandeau.rs : 1er message de N'IMPORTE QUEL viewer (toutes plateformes) → overlay diffusion bas/haut (config durée + position, persisté bandeau_premier_message.json), seen set en RAM (reset session), émission WS bandeau-premier-message-play/-stop/-config. Appelé par les 4 modules chat. Contrôles : stop, reset session, tester.",
  },
  {
    id: 31,
    titre: "Cadres SVG (widgets + bord de l'application)",
    date: "2026-08-31",
    note: "Registre 6 styles (carré, arrondis, cyberpunk, story, thin-line, hexagone) + variantes couleurs + dégradé personnalisable. Cadre widget = clip-path + overlay SVG sur TOUS les widgets (y compris trous) ; cadre app = bord du canvas. Le trou du fond suit la forme du clip-path (coins biseautés, hexagone…) via svgPathToPoints + projection 3D. RÈGLE FIGÉE (AGENTS.md — ne pas régresser). Galerie unique à onglets (widgets / bord application) dans Personnalisation → Vos cadres.",
  },
  {
    id: 32,
    titre: "Widget caméra → source OBS SOS-Caméra",
    date: "2026-09-12",
    note: "Refonte : getUserMedia supprimé. La création du widget caméra crée une source OBS dshow_input « SOS-Caméra » sous SOS-Diffusion (ensure_camera : create/réactive/transform centre+SCALE_OUTER/index). Singleton. Suit move/resize (obsSyncTrous). Changement de device = SetInputSettings sur le MÊME input (jamais de doublon). Suppression = item caché, input conservé (jamais RemoveInput — capteur Windows en accès exclusif). Auto-guérison : scene_sync_captures crée la source au boot/openScene/connexion OBS même si créée offline. Côté diffusion le widget est un trou (forme du clip-path du cadre).",
  },
  {
    id: 33,
    titre: "Alertes Twitch (follow/raid/sub/resub/subgift/bits)",
    date: "2026-09-12",
    note: "alertes.rs (pattern bandeau) : config par type persistée alertes_config.json, file FIFO max 5, cooldowns viewer+global, templates à variables ({pseudo}, {nbViewers}, {nbBits}, {niveauSub}, {nbMoisCumul}, {destinataire}). Sources : USERNOTICE IRC (sub/resub/subgift/raid via msg-id + msg-param-*) + tag bits PRIVMSG (twitch_chat.rs) + diff followers Helix frontend (poll 60s, baseline + followed_at > boot). WS alerte-config/-play/-stop → overlay diffusion z 9997 (icône emoji + texte, son via /medias/). Sons importés mp3/ogg/wav (import_son, magic bytes). Panneau Toolbar par type (toggle/template/durée/cooldown/couleur/taille/son/test) + overlay réglable (presets 6 positions + X/Y/L/H, même mécanique que welcome). Point d'extension multi-plateformes : alerte_declencher.",
  },
  {
    id: 34,
    titre: "UI sobre + UX (vocabulaire, états, messages)",
    date: "2026-09-12",
    note: "Palette noir/blanc/gris variableisée (--gris/--gris-fonce/--bordure/--rayon-*) + arrondis globaux + transitions + scrollbars sombres + focus visible. Sections TOUTES pliées au boot. En-tête section OBS rouge (non connecté) / vert (connecté). Messages d'erreur actionnables (auth OBS 4009 → guide mot de passe + auto-open section ; OBS injoignable → vérifications). Mot de passe OBS persisté localStorage. Vocabulaire user-friendly : « Arrière-plan de l'application » (ex Scène), « Vos scènes ▸ », « Vos cadres » (galerie unique à onglets), « fenêtre de capture » (ex trou), « Diffusion » (ex :4321), « Lier une source OBS » (ex Source OBS sous SOS).",
  },
  {
    id: 35,
    titre: "Base zéro-warning + piège Mutex non réentrant",
    date: "2026-09-14",
    note: "cargo clippy (exit 0) + npm run check (0 erreur) = niveau de référence du dépôt. Warnings non_snake_case neutralisés par #[allow] sur CadreConfig/Widget/Scene (champs camelCase volontaires — contrat JSON frontend). Piège : 2× .lock() sur le même std::sync::Mutex dans une expression = deadlock (temporaires vivent jusqu'à la fin du statement → écran noir figé au boot). Correctif : lire UNE fois (clone) puis logger. Appliqué sur position_overlay.rs (new) + bandeau.rs (set_config).",
  },
  {
    id: 36,
    titre: "Squelette de position unifié (welcome + alertes)",
    date: "2026-09-14",
    note: "Refonte : un seul réglage pilote les overlays clip de bienvenue + alertes via PositionOverlayConfig (position_overlay.json, source de vérité Rust unique). PositionOverlayState enregistré AVANT welcome/alertes. WS position-overlay-config → diffusion.html applique la position aux deux overlays en live. Anciens champs overlay gardés pour rétro-compat deserialization mais inutilisés.",
  },
  {
    id: 37,
    titre: "Médias par alerte + cadre SVG sur les overlays",
    date: "2026-09-14",
    note: "Modale Alertes = master-detail (6 types, 4 blocs par type). Média par type (image/vidéo, Option + serde(default) → rétro-compat). Rendu diffusion : texte au-dessus, média dessous. Cadre SVG appliqué aux cartes overlays (appliquerCadreOverlay + mesure APRÈS affichage via rAF, re-mesure au canplay vidéo). Couleur du texte = couleur du cadre widget si actif. Import média alerte via validate_and_copy_media (helper commun refactoré).",
  },
  {
    id: 38,
    titre: "Commandes chat — !commande tapée par un viewer",
    date: "2026-09-14",
    note: "Module commandes.rs : même moteur que alertes (file FIFO max 5, cooldowns viewer+global, timer Rust + fallback diffusion) mais déclenché par un MESSAGE chat (1er mot = !cmd). Le message continue de circuler dans le chat (overlay EN PLUS). Config persistée commandes_config.json. Hooks chat dans les 4 modules (twitch/kick/tiktok/youtube). WS commande-config/-play/-stop → overlay z 9996 (sous alertes). Squelette de position partagé. Onglet Commandes dans la modale Interactions chat (master-detail).",
  },
  {
    id: 39,
    titre: "Import MKV remuxé en MP4 via sidecar ffmpeg",
    date: "2026-09-14",
    note: "Chromium ne lit pas le MKV dans <video>. MKV accepté (magic bytes EBML partagés avec WebM) → remux_to_mp4 : sidecar ffmpeg -c copy -sn -movflags +faststart → medias/<uuid>.mp4 (sans réencodage, rapide). MP4/WebM → fs::copy direct. Sidecar embarqué via tauri-plugin-shell + externalBin (binaries/ffmpeg, exclu de git). remux blocking sync sur thread worker Tauri (pas de gel dashboard). Limitations : codecs non-MP4 (-c copy échoue), sous-titres dropped (-sn).",
  },
  {
    id: 40,
    titre: "Input Viewer — capture globale clavier + souris",
    date: "2026-09-14",
    note: "Widget de scène type input-viewer affichant les entrées en temps réel côté diffusion. Rust (input_viewer.rs) : poll clavier 60Hz (GetAsyncKeyState, table vkCode→KeyboardEvent.code) + hook souris WH_MOUSE_LL (thread dédié avec boucle GetMessageW, arrêt via PostThreadMessageW WM_QUIT). Publication WS throttlée 30Hz + skip si signature identique (CEF OBS saccade sinon). Maj DOM directe (classList.toggle, zéro re-render). Bouton ON/OFF dans les options widget. Modes clavier/souris/numpad/manette, layout AZERTY/QWERTY.",
  },
  {
    id: 41,
    titre: "Speedrun Splitter — Auto Splitter natif Rust",
    date: "2026-09-14",
    note: "Moteur ASL 100% Rust : parser ASL + transpileur C#→JS + moteur Boa (JS pur Rust ~5MB) + scan mémoire Win32 (ReadProcessMemory) + parser LSS + timer. Plus léger que LiveSplit (.NET WPF + Roslyn ~50MB). 9 commandes Tauri. Thread moteur 15Hz : connexion processus → init/update/start/split/reset. Events Tauri speedrun:event (store dashboard) + WS speedrun-etat (diffusion majSpeedrunEtat, maj DOM directe). Bugs legacy corrigés (vars/settings persistants en Boa, strip .exe, new_state_var prepend 0, module vide, gameTime Option fallback, init guard, transpiler char-safe, Mutex non réentrant, Boa Context non Send, throttle WS, maj DOM directe).",
  },
  {
    id: 42,
    titre: "Unfollows Twitch — détection au démarrage",
    date: "2026-09-15",
    note: "À chaque boot, liste followers comparée au snapshot précédent (followers_snapshot.json). user_id absents du nouveau = unfollows → enregistrés dans unfollows.json (append-only, dédupliqué par user_id, date ISO 8601). enrichir_avatars (batch /users max 100) remplit profile_image_url + display_name (non-fatal si rate limit). Affichage : modale Modération & Rôles → onglet Twitch → sections Communauté (followers) + Unfollows (historique trié plus récent). Limitation : détection au démarrage seulement (pas d'event IRC unfollow).",
  },
  {
    id: 43,
    titre: "Thème HUD sci-fi du chrome dashboard",
    date: "2026-09-15",
    note: "Re-thème du chrome dashboard (header, sidebar, barre scènes, modales) en style HUD sci-fi violet/orange. Palette noir bleuté (#08080f) + accents néon violet #8b5cf6 / orange #f97316 + dégradé signature 135deg #f97316→#ec4899→#8b5cf6. Variables --accent-* / --gradient-sig / --glow-* / --fond-panneau / --fond-controle / --bordure-active / --plat-* (couleurs de marque plateformes, décoratif). Recette boutons « verre Aero HUD » + reflet animé one-shot au hover. Canvas/widgets/cadres SVG/diffusion.html INTACTS (règle figée). 95% dans app.css (variables :root) — 468 usages var() propagent automatiquement.",
  },
  {
    id: 44,
    titre: "Média de fond sur les widgets à contenu",
    date: "2026-09-15",
    note: "Les widgets chat / input-viewer / speedrun acceptent un média (image/vidéo) en arrière-plan, contenu affiché par-dessus (mêmes contrôles Affichage/fit/zoom/rot/offsets/PlayerBar). Zéro changement Rust (champs media existent déjà sur tous les widgets). Contrat figé : média = PREMIER enfant (.media-fond z-index 0), contenu au-dessus (z-index 1), cadre SVG au-dessus de tout (z-index 5). :not(.cadre-overlay) OBLIGATOIRE dans les sélecteurs > * (sinon le cadre SVG disparaît). syncMediaFond dans diffusion.html (re)crée le média si mediaKey change. Dashboard = vignette figée (config, pas player).",
  },
  {
    id: 45,
    titre: "Carte d'édition contextuelle — 3 cartes colorées",
    date: "2026-09-15",
    note: "CarteEdition adopte une couleur selon la cible : widget classique = rouge bordeaux (--dash-danger), fond = orange (--dash-orange), widget input-viewer = jaune (--dash-jaune). Classe CSS dérivée (carteClasse) — la cible du store reste \"widget\", seul le rendu change. Boutons .action internes suivent la couleur de la carte dès l'état repos (bordure + surface teintée via --btn-tint = --accent-carte), pas seulement au hover/active. Variables --dash-orange / --dash-jaune ajoutées dans app.css (palette --dash-* assombrie).",
  },
  {
    id: 46,
    titre: "Drop → widget média auto sans cadre SVG",
    date: "2026-09-19",
    note: "Chaque fichier droppé dans la zone Widgets devient immédiatement un widget média (dashboard + :4321) : ajouterALaBibliotheque auto-transforme chaque ok via transformerEnWidget (la bibliothèque se vide au fur et à mesure). Nouveau flag par widget sansCadre (serde default false → anciennes scènes OK) : opt-out du cadre SVG de scène pour CE widget — cadreActif gâté dans Widget.svelte, applyCadreWidget le traite comme cadre inactif, trou éventuel rectangulaire (peindreFond), titre en gradient fallback (applyTitreWidget, parité dashboard). Cadre de scène / cadreApp / CadresModal inchangés — les autres widgets gardent leur cadre.",
  },
  {
    id: 47,
    titre: "Bouton « Transformer en widget ! » dans le bandeau drop",
    date: "2026-09-19",
    note: "Le bouton Transformer quitte les lignes du DropBar (liste = noms seuls) pour un bouton rouge contextuel aligné à droite du bandeau glisser/déposer : visible uniquement quand le widget sélectionné a sansCadre === true (selectedIdStore + sceneStore), .action.danger compacte (recette --dash-danger identique .header.etat-erreur). Groupe .droite margin-left:auto → bouton au bord droit même biblio vide. transformerEnWidget(rel) assoupli : plus d'early-return sur entrée bibliothèque absente (store volatile) — widget media===rel → sansCadre=false, entrée biblio filtrée si présente, sinon poserMediaDepuisBibliotheque (ancien flux createWidget). Un widget sansCadre rechargé depuis une scène sauvegardée peut donc être transformé.",
  },
  {
    id: 48,
    titre: "Barre de raccourcis TOPMOST dockée à un bord d'écran",
    date: "2026-09-19",
    note: "Fenêtre WebviewWindow « raccourcis » (raccourcis.rs, modèle pop-out chat) : decorations/resizable off, skip_taskbar, focusable(false) = ne vole pas le focus au jeu, transparent(true) → bande noire translucide rgba(0,0,0,0.7), 56px×côté du moniteur (physique, ×scale_factor), bord gauche/droite/haut/bas + moniteur persistés dans raccourcis.json + restore boot (fallback primaire si index mort). Timer tokio 2s SetWindowPos(HWND_TOPMOST) pour repasser devant un jeu borderless topmost — AUCUN hook/inject ; plein écran EXCLUSIF non couvert (limite Win32 documentée). 14 icônes SVG émettent raccourcis:action {id} → App.svelte dispatche vers les fonctions existantes (widgets, toggles plateformes avec slug sauvegardé, OBS=re-sync one-shot, modales, masquer). Bord passé par ?bord= + emit_to raccourcis:bord ; pastilles vertes via emitTo raccourcis:connexions + handshake raccourcis:ready. withGlobalTauri:true requis (HTML vanilla). Section « Raccourcis » dans la Toolbar : afficher/masquer, 4 bords, sélecteur moniteur.",
  },
];
