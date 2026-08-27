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
];
