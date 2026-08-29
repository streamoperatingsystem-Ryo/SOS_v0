// SEUL endroit où invoke/listen Tauri sont utilisés côté dashboard.
import { invoke } from "@tauri-apps/api/core";

export interface Widget {
  id: string;
  type: "media" | "chat";
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
  z: number;
  media?: string;
  rotateX?: number;
  rotateY?: number;
  mediaFit?: string;
  kind?: string;
  mediaZoom?: number;
  mediaRot?: number;
  mediaPaused?: boolean;
  mediaTime?: number;
  trou?: boolean;
  obsSource?: string;
  chatFiltre?: string;
  taillePolice?: number;
}

/// Configuration d'un cadre SVG (widget ou bord canvas).
/// `style` = ID du registre ("carre", "arrondis", "cyberpunk", "story",
/// "thin-line", "hexagon"). `variante` = variante de couleur (presets).
/// `couleur`/`couleurFin` = dégradé pour les cadres personnalisables.
/// `actif` = cadre visible.
export interface CadreConfig {
  style: string;
  variante?: string;
  strokeWidth: number;
  couleur: string;
  couleurFin: string;
  actif: boolean;
}

export interface Scene {
  widgets: Widget[];
  canvasW?: number;
  canvasH?: number;
  bgMedia?: string;
  bgKind?: string;
  bgFit?: string;
  bgZoom?: number;
  bgRot?: number;
  bgPaused?: boolean;
  bgTime?: number;
  cadreWidget?: CadreConfig;
  cadreApp?: CadreConfig;
}

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
export interface SceneIndex {
  id: string;
  nom: string;
}

export const tauri = {
  async getScene(): Promise<Scene> {
    return invoke<Scene>("get_scene");
  },

  async updateScene(scene: Scene): Promise<void> {
    await invoke("update_scene", { scene });
  },

  /// Importe un média (image OU vidéo) pour un widget. Retourne le chemin
  /// relatif ("medias/<uuid>.<ext>") ou null si le dialog a été annulé.
  /// Lance une erreur en cas de refus (taille/format). Le kind
  /// ("image"|"video") est déduit côté TS de l'extension du chemin retourné.
  async importMedia(widgetId: string): Promise<string | null> {
    return invoke<string | null>("import_media", { widgetId });
  },

  /// Importe un média (image OU vidéo) comme fond de scène. Retourne le
  /// chemin relatif ("medias/<uuid>.<ext>") ou null si dialog annulé.
  /// Mêmes limites que importMedia (helper commun Rust). Le kind est déduit
  /// côté TS de l'extension du chemin retourné.
  async importFond(): Promise<string | null> {
    return invoke<string | null>("import_fond");
  },

  /// Connecte à OBS WebSocket, authentifie, lit la résolution canvas OBS
  /// (GetVideoSettings → baseWidth/baseHeight, fallback 1920×1080), s'assure
  /// que la scène "SOS" + source navigateur "SOS-Diffusion" (dims = résolution
  /// OBS, URL :4321) existent sans doublon, puis mute la scène + save + snapshot.
  async obsConnect(host: string, port: number, password: string): Promise<void> {
    await invoke("obs_connect", { host, port, password });
  },

  /// Refresh standalone de la source navigateur "SOS-Diffusion" (refreshnocache).
  /// Appelé quand serveur :4321 ready + OBS connecté. One-shot, non-fatal.
  async obsRefreshDiffusion(host: string, port: number, password: string): Promise<void> {
    await invoke("obs_refresh_diffusion", { host, port, password });
  },

  /// Toggle fenêtre pop-out chat (always_on_top, 360×520).
  /// Ouvre si fermée, ferme si ouverte. Retourne true si maintenant ouverte.
  /// Fermer ≠ couper IRC (le pipeline WS est indépendant).
  async chatPopoutToggle(): Promise<boolean> {
    return await invoke<boolean>("chat_popout_toggle");
  },

  /// Ferme explicitement la fenêtre pop-out chat. Non-fatal si déjà fermée.
  async chatPopoutFermer(): Promise<void> {
    await invoke("chat_popout_fermer");
  },

  /// Arrête l'application (quitte proprement).
  async appArreter(): Promise<void> {
    await invoke("app_arreter");
  },

  /// Redémarre l'application (relance le processus puis quitte).
  async appRedemarrer(): Promise<void> {
    await invoke("app_redemarrer");
  },

  /// Synchronise les captures SOS-Trou-* d'OBS avec la scène chargée.
  /// Active + sync transform les items liés, cache les autres. Non-fatal.
  async sceneSyncCaptures(host: string, port: number, password: string): Promise<void> {
    await invoke("scene_sync_captures", { host, port, password });
  },

  // ===== Scènes (v0.14) =====

  /// Liste l'index des scènes (ids + noms seulement, jamais le contenu).
  async scenesLister(): Promise<SceneIndex[]> {
    return invoke<SceneIndex[]>("scenes_lister");
  },

  /// Crée une nouvelle scène vide (1920×1080, 0 widget, pas de fond), bascule
  /// dessus. Sauve la courante d'abord. Retourne l'entrée créée.
  async sceneCreer(nom: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_creer", { nom });
  },

  /// Ouvre une scène : sauve la courante, charge <id>.json, bascule, snapshot.
  async sceneOuvrir(id: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_ouvrir", { id });
  },

  /// Renomme une scène dans l'index.
  async sceneRenommer(id: string, nom: string): Promise<void> {
    await invoke("scene_renommer", { id, nom });
  },

  /// Supprime une scène (fichier + entrée index). Refuse si dernière scène.
  /// Si la scène supprimée est la courante, bascule sur la 1ère restante.
  async sceneSupprimer(id: string): Promise<void> {
    await invoke("scene_supprimer", { id });
  },

  /// Retourne l'entrée courante ({ id, nom }).
  async sceneCourante(): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_courante");
  },

  /// Exporte la scène courante comme pack dossier portable.
  /// nom_pack = nom du dossier à créer (fourni par l'UI, défaut = nom scène).
  /// Dialog = dossier PARENT. Retourne true si exporté, false si annulé.
  async sceneExporter(nomPack: string): Promise<boolean> {
    return invoke<boolean>("scene_exporter", { nomPack });
  },

  /// Importe une scène depuis un .json (dialog Ouvrir). Valide schéma min,
  /// copie en <nouvel-id>.json, bascule. Retourne l'entrée ou null si annulé.
  async sceneImporter(): Promise<SceneIndex | null> {
    return invoke<SceneIndex | null>("scene_importer");
  },

  // ===== Sources OBS "trou" (Lot 2) =====

  /// Énumère les cibles de capture OBS pour un type (camera/window/game).
  /// Jeu → [] (capture_any_foreground_window, pas de cible).
  async obsEnumerateTargets(
    host: string,
    port: number,
    password: string,
    kind: "camera" | "window" | "game"
  ): Promise<string[]> {
    return invoke<string[]>("obs_enumerate_targets", { host, port, password, kind });
  },

  /// Crée une source OBS de capture sous SOS-Diffusion, calée sur le widget.
  /// Retourne le nom de la source créée.
  async obsCreateTrouSource(
    host: string,
    port: number,
    password: string,
    sourceName: string,
    kind: "camera" | "window" | "game",
    target: string | null,
    x: number,
    y: number,
    w: number,
    h: number
  ): Promise<string> {
    return invoke<string>("obs_create_trou_source", {
      host, port, password, sourceName, kind, target, x, y, w, h,
    });
  },

  /// Synchronise les transforms OBS de toutes les sources trou en une connexion.
  /// fit/zoom/rot pilotent SetSceneItemTransform (boundsType + scale/bounds +
  /// rotation) de la source OBS du trou, en plus de la position/taille.
  async obsSyncTrous(
    host: string,
    port: number,
    password: string,
    items: { sourceName: string; x: number; y: number; w: number; h: number; fit: string; zoom: number; rot: number }[]
  ): Promise<void> {
    await invoke("obs_sync_trous", { host, port, password, items });
  },

  /// Supprime une source trou d'OBS. Idempotent (déjà supprimée = Ok).
  async obsDeleteTrouSource(
    host: string,
    port: number,
    password: string,
    sourceName: string
  ): Promise<void> {
    await invoke("obs_delete_trou_source", { host, port, password, sourceName });
  },

  /// Énumère les inputs OBS (GetInputList) filtrés par type de capture.
  /// Caméra → dshow_input ; Fenêtre → window_capture ; Jeu → game_capture.
  /// Exclut SOS-Diffusion, browser_source, audio, scènes, trous liés.
  async obsEnumerateInputsByKind(
    host: string,
    port: number,
    password: string,
    kind: "camera" | "window" | "game"
  ): Promise<{ inputName: string; inputKind: string }[]> {
    return invoke("obs_enumerate_inputs_by_kind", { host, port, password, kind });
  },

  /// Lie une source OBS existante au widget trou (pas de création, juste
  /// transform + reorder sous SOS-Diffusion).
  async obsLinkExistingSource(
    host: string,
    port: number,
    password: string,
    sourceName: string,
    x: number,
    y: number,
    w: number,
    h: number
  ): Promise<string> {
    return invoke("obs_link_existing_source", {
      host, port, password, sourceName, x, y, w, h,
    });
  },

  // ===== Énumération PC + création capture depuis SOS (Lot 3) =====

  /// Énumère les caméras du PC (PnP/DirectShow, SANS OBS).
  async pcEnumerateCameras(): Promise<string[]> {
    return invoke<string[]>("pc_enumerate_cameras");
  },

  /// Énumère les fenêtres visibles du PC (EnumWindows, SANS OBS).
  async pcEnumerateWindows(): Promise<{ title: string; exe: string; obs_value: string }[]> {
    return invoke("pc_enumerate_windows");
  },

  /// Énumère les jeux/fenêtres du PC (EnumWindows, SANS OBS).
  async pcEnumerateGames(): Promise<{ title: string; exe: string; obs_value: string }[]> {
    return invoke("pc_enumerate_games");
  },

  /// Crée une source OBS de capture depuis une cible PC (caméra/fenêtre/jeu)
  /// dans la scène contenant SOS-Diffusion. Si SOS-Trou-<id> existe déjà →
  /// SetInputSettings + transform. Sinon → CreateInput. Sous SOS + transform.
  async obsCreateTrouFromPc(
    host: string,
    port: number,
    password: string,
    sourceName: string,
    kind: "camera" | "window" | "game",
    target: string | null,
    x: number,
    y: number,
    w: number,
    h: number
  ): Promise<string> {
    return invoke<string>("obs_create_trou_from_pc", {
      host, port, password, sourceName, kind, target, x, y, w, h,
    });
  },

  // ===== Connexions réseau (Lot réseau) =====

  /// Démarre la connexion Twitch. Si token en coffre → IRC direct.
  /// Sinon → Device Code Flow (emit twitch:device → poll → twitch:connecte).
  async twitchConnecter(): Promise<void> {
    await invoke("twitch_connecter");
  },

  /// Annule le Device Code Flow en cours.
  async twitchAnnulerDeviceFlow(): Promise<void> {
    await invoke("twitch_annuler_device_flow");
  },

  /// Déconnecte Twitch : arrête IRC + efface le token du coffre.
  async twitchDeconnecter(): Promise<void> {
    await invoke("twitch_deconnecter");
  },

  /// Retourne l'état de connexion Twitch (true/false).
  async twitchEtat(): Promise<boolean> {
    return invoke<boolean>("twitch_etat");
  },

  /// Retourne le login du compte Twitch connecté (ou null si déconnecté).
  async twitchLoginCourant(): Promise<string | null> {
    return invoke<string | null>("twitch_login_courant");
  },

  /// Force une reconnexion Twitch : révoque l'ancien token + efface le coffre +
  /// lance un nouveau Device Code Flow avec les scopes étendus.
  async twitchReconnecter(): Promise<void> {
    await invoke("twitch_reconnecter");
  },

  // ===== Kick (lecture seule, WS côté Rust) =====

  /// Connecte au chat Kick : Rust fait TOUT (resolve slug → fetch token →
  /// WS Pusher avec Origin: https://kick.com → subscribe → forward messages).
  /// emit kick:connecte quand subscribed, kick:deconnecte sur close/error.
  async kickConnecter(slug: string): Promise<void> {
    await invoke("kick_connecter", { slug });
  },

  /// Déconnecte Kick : arrête le WS côté Rust + emit kick:deconnecte.
  async kickDeconnecter(): Promise<void> {
    await invoke("kick_deconnecter");
  },

  /// Retourne l'état de connexion Kick (true/false).
  async kickEtat(): Promise<boolean> {
    return invoke<boolean>("kick_etat");
  },

  /// Retourne le slug Kick sauvegardé (pour pré-remplir l'input).
  async kickSlugCourant(): Promise<string | null> {
    return invoke<string | null>("kick_slug_courant");
  },

  // ===== YouTube (chat polling + communauté Data API v3) =====

  /// Démarre la connexion YouTube. Si token en coffre → chat polling direct.
  /// Sinon → Device Code Flow (emit youtube:device → poll → youtube:connecte).
  async youtubeConnecter(): Promise<void> {
    await invoke("youtube_connecter");
  },

  /// Annule le Device Code Flow YouTube en cours.
  async youtubeAnnulerDeviceFlow(): Promise<void> {
    await invoke("youtube_annuler_device_flow");
  },

  /// Démarre manuellement le chat polling YouTube Live (bouton "Chat live ON").
  /// Si pas de live actif → emit youtube:pas-de-live et la tâche s'arrête.
  async youtubeDemarrerChat(): Promise<void> {
    await invoke("youtube_demarrer_chat");
  },

  /// Arrête le chat polling YouTube Live sans déconnecter le compte (bouton "Chat live OFF").
  async youtubeArreterChat(): Promise<void> {
    await invoke("youtube_arreter_chat");
  },

  /// Déconnecte YouTube : arrête chat + efface token du coffre.
  async youtubeDeconnecter(): Promise<void> {
    await invoke("youtube_deconnecter");
  },

  /// Retourne l'état de connexion YouTube (true/false).
  async youtubeEtat(): Promise<boolean> {
    return invoke<boolean>("youtube_etat");
  },

  /// Retourne le login de la chaîne YouTube connectée (ou null si déconnecté).
  async youtubeLoginCourant(): Promise<string | null> {
    return invoke<string | null>("youtube_login_courant");
  },

  /// Channel info : display_name, avatar, subscriberCount, viewCount, videoCount.
  async youtubeCommunauteChannel(): Promise<{
    display_name: string;
    profile_image_url: string;
    subscriber_count: number;
    view_count: number;
    video_count: number;
    description: string;
  }> {
    return invoke("youtube_communaute_channel");
  },

  /// Members : liste + total (displayName, membershipsLevel).
  async youtubeCommunauteMembers(): Promise<{
    total: number;
    liste: { display_name: string; memberships_level: string }[];
  }> {
    return invoke("youtube_communaute_members");
  },

  /// Live viewers : concurrentViewers (null si pas live).
  async youtubeCommunauteViewers(): Promise<number | null> {
    return invoke<number | null>("youtube_communaute_viewers");
  },

  // ===== TikTok (chat live via PirateTok) =====

  /// Connecte au chat TikTok Live d'un streamer (username sans @).
  /// Rust fait TOUT : resolve room ID → WebSocket PirateTok → forward messages.
  /// emit tiktok:connecte quand connecté, tiktok:deconnecte sur close/error.
  async tiktokConnecter(username: string): Promise<void> {
    await invoke("tiktok_connecter", { username });
  },

  /// Déconnecte TikTok : arrête le chat côté Rust + emit tiktok:deconnecte.
  async tiktokDeconnecter(): Promise<void> {
    await invoke("tiktok_deconnecter");
  },

  /// Retourne l'état de connexion TikTok (true/false).
  async tiktokEtat(): Promise<boolean> {
    return invoke<boolean>("tiktok_etat");
  },

  /// Retourne le username TikTok sauvegardé (pour pré-remplir l'input).
  async tiktokUsernameCourant(): Promise<string | null> {
    return invoke<string | null>("tiktok_username_courant");
  },

  // ===== Communauté lecture (Helix) =====

  /// Followers : liste + total (pagination max 10 pages).
  /// Erreur "need_reauth" si 403 (scopes manquants).
  async twitchCommunauteFollowers(): Promise<{
    total: number;
    liste: { login: string; user_id: string; followed_at: string }[];
  }> {
    return invoke("twitch_communaute_followers");
  },

  /// Subs : liste + total + points (pagination max 10 pages).
  async twitchCommunauteSubs(): Promise<{
    total: number;
    points: number;
    liste: { login: string; user_id: string; tier: string; is_gift: boolean }[];
  }> {
    return invoke("twitch_communaute_subs");
  },

  /// Viewers live : chiffre si en live, null si hors-ligne.
  async twitchCommunauteViewers(): Promise<number | null> {
    return invoke<number | null>("twitch_communaute_viewers");
  },

  /// Broadcaster : display_name, avatar, type, description.
  async twitchBroadcaster(): Promise<{
    display_name: string;
    profile_image_url: string;
    broadcaster_type: string;
    description: string;
  }> {
    return invoke("twitch_broadcaster");
  },
};
