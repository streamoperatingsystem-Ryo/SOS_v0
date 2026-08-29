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
];
