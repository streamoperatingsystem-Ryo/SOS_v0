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
];
