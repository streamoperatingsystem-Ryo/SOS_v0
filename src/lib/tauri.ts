// SEUL endroit où invoke/listen Tauri sont utilisés côté dashboard.
import { invoke } from "@tauri-apps/api/core";

export interface Widget {
  id: string;
  type: "media" | "chat" | "camera" | "welcome-clip" | "input-viewer" | "speedrun";
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
  /// Nom original du fichier média importé (sans chemin, avec extension).
  /// undefined pour les widgets sans média. Affiché dans l'en-tête de la
  /// carte d'édition pour identifier le widget au lieu de l'ID technique.
  mediaNom?: string;
  mediaZoom?: number;
  mediaRot?: number;
  mediaOffsetX?: number;
  mediaOffsetY?: number;
  /// Effets visuels du média (0 = neutre) : luminosité (-100..100),
  /// contraste (-100..100), teinte (0..360 deg), flou (0..20 px),
  /// pixelisation (0..50, 0 = désactivé).
  mediaLum?: number;
  mediaContraste?: number;
  mediaTeinte?: number;
  mediaFlou?: number;
  mediaPixel?: number;
  mediaPaused?: boolean;
  mediaTime?: number;
  trou?: boolean;
  /// Opt-out du cadre SVG de scène pour ce widget (widgets créés par drop
  /// bibliothèque → transformerEnWidget). undefined/false = cadre appliqué.
  sansCadre?: boolean;
  obsSource?: string;
  chatFiltre?: string;
  taillePolice?: number;
  /// Identifiant du device caméra pour les widgets type "camera" :
  /// FriendlyName PnP (pc_enumerate_cameras) = valeur video_device_id de la
  /// source OBS dshow_input "SOS-Caméra". undefined = device par défaut.
  cameraDeviceId?: string;
  /// Mode d'affichage des widgets type "input-viewer" : "clavier" (défaut),
  /// "souris", "numpad" ou "manette".
  inputMode?: string;
  /// Layout clavier ("azerty" défaut | "qwerty") pour les widgets
  /// type "input-viewer" en mode clavier.
  inputLayout?: string;
  /// Skin manette ("xbox" défaut | "ps" | "8bitdo") pour les widgets
  /// type "input-viewer" en mode manette.
  inputSkin?: string;
  /// Couleur des touches pressées (input-viewer). Défaut #8b5cf6.
  inputCouleur?: string;
  /// Mapping des touches manette (input-viewer mode manette). Permet de
  /// remapper chaque bouton affiché vers un index physique différent +
  /// inverser les axes X/Y du D-pad (bug 8BitDo etc.).
  inputMapping?: InputMapping;
  /// Chemin du fichier .asl (Auto Split Language) pour les widgets
  /// type "speedrun". undefined = pas de script ASL chargé.
  speedrunCheminAsl?: string;
  /// Chemin du fichier .lss (LiveSplit Splits) pour les widgets
  /// type "speedrun". undefined = pas de fichier LSS chargé.
  speedrunCheminLss?: string;
  /// Settings ASL (start/split/reset activés) pour les widgets
  /// type "speedrun". undefined = defaults {start:true, split:true, reset:true}.
  speedrunSettings?: SpeedrunSettings;
  /// Police Google Font pour les widgets type "speedrun" (id police, voir
  /// fonts.ts). undefined = "Rajdhani" (défaut). Reprise de la liste POLICES_TITRE.
  srPolice?: string;
  /// Taille de police de base (px) pour les widgets type "speedrun". undefined = 16.
  /// Toutes les tailles internes du splitter sont en em → suit cette base.
  srTaillePolice?: number;
  /// Chaîne de morphs (bulge/pinch) appliquée au média, dans l'ordre.
  /// undefined/vide = rendu natif (img/video), zéro coût GL.
  morphs?: MorphPoint[];
  /// Titre optionnel affiché au-dessus ou en dessous du widget. undefined/vide
  /// = pas de titre. Le texte est rendu avec une police Google Font
  /// (titrePolice) et un gradient de couleur reprenant celui du cadre SVG
  /// actif de la scène (fallback blanc→gris si aucun cadre actif).
  titre?: string;
  /// Position du titre : "dessus" (défaut) ou "dessous".
  titrePosition?: "dessus" | "dessous";
  /// Identifiant de la police Google Font (ex: "BebasNeue"). Voir fonts.ts.
  titrePolice?: string;
  /// Taille du titre en pixels (12-72). Défaut 24.
  titreTaille?: number;
  /// Titre en gras. Défaut false.
  titreGras?: boolean;
  /// Titre en italique. Défaut false.
  titreItalique?: boolean;
  /// Titre souligné. Défaut false.
  titreSouligne?: boolean;
  /// Espacement des lettres en px (-2 à 20). Défaut 0.
  titreEspacement?: number;
}

/// Mapping des touches manette pour un widget input-viewer.
/// `buttons` : tableau de 16 entrées, index = bouton affiché, valeur = index
/// physique lu par gilrs/XInput. Défaut = identité [0,1,...,15].
/// `dpadInvertY`/`dpadInvertX` : inverser l'axe Y/X du D-pad quand la manette
/// reporte le D-pad sur les axes (8BitDo rétro en D-Input).
export interface InputMapping {
  buttons: number[];
  dpadInvertY: boolean;
  dpadInvertX: boolean;
}

// ===== Types Speedrun Splitter (mirroir des structs Rust) =====

/// Settings ASL (quelles méthodes auto-split sont activées).
export interface SpeedrunSettings {
  start: boolean;
  split: boolean;
  reset: boolean;
}

/// Config speedrun persistée (speedrun.json) pour rechargement auto au boot.
export interface SpeedrunConfig {
  chemin_asl?: string;
  chemin_lss?: string;
  settings: SpeedrunSettings;
  /// Settings ASL individuels (120+ pour MGS) persistés entre sessions.
  /// Map : code-signature → bool (coché/décoché).
  settings_asl?: Record<string, boolean>;
}

/// Segment LSS (un split du fichier LiveSplit).
export interface LssSegment {
  nom: string;
  pb_real_time: number | null;
  pb_game_time: number | null;
}

/// Run LSS complète (fichier LiveSplit Splits).
export interface LssRun {
  nom_jeu: string;
  nom_categorie: string;
  segments: LssSegment[];
}

/// Setting info retourné par le chargement ASL.
export interface SpeedrunSettingInfo {
  id: string;
  label: string;
  value: boolean;
  parent?: string | null;
}

/// Setting ASL individuel (ex: "OL-s00a" → "Dock", coché/décoché).
/// Retourné par speedrunChargerAsl pour la modale de configuration.
export interface AslSettingDetail {
  id: string;
  label: string;
  value: boolean;
  parent: string | null;
}

/// Résultat du chargement d'un ASL : settings de base + settings individuels +
/// dictionnaire code→nom (D.Names.Split) pour l'auto-mapping LSS.
export interface AslLoadResult {
  settings: SpeedrunSettingInfo[];
  settings_detailles: AslSettingDetail[];
  /// Paires [code, nom] (ex: [["OL-s00a", "Dock"], ...]).
  noms_splits: [string, string][];
}

/// Event speedrun émis par le moteur (event Tauri speedrun:event).
export interface SpeedrunEvent {
  type:
    | "started"
    | "split"
    | "reset"
    | "ended"
    | "gameTime"
    | "time"
    | "connected"
    | "disconnected"
    | "error"
    | "settings-list"
    | "stopped"
    | "loaded"
    | "paused"
    | "resumed"
    | "splitSkipped"
    | "splitUndone";
  time?: string;
  index?: number;
  realTime?: string;
  gameTime?: string | null;
  process?: string;
  pid?: number;
  message?: string;
  settings?: SpeedrunSettingInfo[];
}

/// Configuration d'un cadre SVG (widget ou bord canvas).
/// `style` = ID du registre ("carre", "arrondis", "cyberpunk", "story",
/// "thin-line", "hexagon", "mgstyle"). `variante` = variante de couleur (presets).
/// `couleur`/`couleurFin` = dégradé pour les cadres personnalisables.
/// `gradientAngle` = angle du dégradé en degrés (0-360, défaut 135 = diagonal).
/// `actif` = cadre visible.
export interface CadreConfig {
  style: string;
  variante?: string;
  strokeWidth: number;
  couleur: string;
  couleurFin: string;
  gradientAngle: number;
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
  bgOffsetX?: number;
  bgOffsetY?: number;
  /// Effets visuels du fond (0 = neutre) : luminosité (-100..100),
  /// contraste (-100..100), teinte (0..360 deg), flou (0..20 px),
  /// pixelisation (0..50, 0 = désactivé).
  bgLum?: number;
  bgContraste?: number;
  bgTeinte?: number;
  bgFlou?: number;
  bgPixel?: number;
  bgPaused?: boolean;
  bgTime?: number;
  cadreWidget?: CadreConfig;
  cadreApp?: CadreConfig;
  /// Chaîne de morphs appliquée au FOND de scène (bulge/pinch), dans l'ordre.
  /// Coordonnées normalisées (0-1) du canvas. undefined/vide = rendu natif.
  bgMorphs?: MorphPoint[];
  /// Titre optionnel affiché à l'intérieur du fond de l'application, en haut
  /// ou en bas. undefined/vide = pas de titre. Le texte est rendu avec une
  /// police Google Font (bgTitrePolice) et un gradient de couleur reprenant
  /// celui du cadre de l'application (cadreApp).
  bgTitre?: string;
  /// Position du titre du fond : "haut" (défaut) ou "bas" (intérieur du fond).
  bgTitrePosition?: "haut" | "bas";
  /// Identifiant de la police Google Font (ex: "BebasNeue"). Voir fonts.ts.
  bgTitrePolice?: string;
  /// Taille du titre du fond en pixels (12-72). Défaut 24.
  bgTitreTaille?: number;
  /// Titre du fond en gras. Défaut false.
  bgTitreGras?: boolean;
  /// Titre du fond en italique. Défaut false.
  bgTitreItalique?: boolean;
  /// Titre du fond souligné. Défaut false.
  bgTitreSouligne?: boolean;
  /// Espacement des lettres du titre du fond en px (-2 à 20). Défaut 0.
  bgTitreEspacement?: number;
}

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
export interface SceneIndex {
  id: string;
  nom: string;
  /// Titre masqué dans la pastille de la barre « Vos scènes » (œil) —
  /// l'onglet devient orange. Absent des anciens index.json → false.
  nomMasque?: boolean;
}

/// Un point de morphing (bulge/pinch radial) — coordonnées normalisées (0-1)
/// relatives à la zone visible (widget ou canvas pour le fond).
export interface MorphPoint {
  x: number;
  y: number;
  rayon: number;
  intensite: number;
  mode: "agrandir" | "retrecir";
}

/// Config d'un type d'alerte (follow/raid/sub/resub/subgift/bits).
export interface AlerteTypeConfig {
  actif: boolean;
  duree_ms: number;
  texte_template: string;
  couleur: string;
  taille_px: number;
  cooldown_viewer_s: number;
  cooldown_global_s: number;
  /// Chemin relatif du son importé ("medias/<uuid>.<ext>"). undefined = silence.
  /// Utilisé en mode image (son séparé) ; en mode vidéo le son est porté par
  /// la vidéo elle-même.
  son?: string;
  /// Média de l'alerte ("medias/<uuid>.<ext>") : image (avec son séparé) ou
  /// vidéo (son inclus). undefined = icône emoji + texte (comportement historique).
  media?: string;
  /// Kind du média : "image" | "video".
  media_kind?: string;
}

/// Position/taille de l'overlay alerte côté diffusion (pixels canvas).
/// Même mécanique que l'overlay des clips de bienvenue.
export interface AlertesOverlayConfig {
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
}

/// Position/taille du squelette de position unifié (source de vérité partagée
/// par les overlays "clip de bienvenue" et "alertes"). Voir position_overlay.rs.
export interface PositionOverlayConfig {
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
}

/// Config complète des alertes (champ absent = défaut du type côté Rust).
/// Bord de l'écran où la barre de raccourcis est dockée.
export type BordRaccourcis = "gauche" | "droite" | "haut" | "bas";

/// Moniteur Windows pour le sélecteur de la section Raccourcis (Toolbar).
export interface RaccourcisMoniteur {
  index: number;
  nom: string | null;
  largeur: number;
  hauteur: number;
  primaire: boolean;
}

/// État barre de raccourcis : config persistée + fenêtre réellement ouverte.
export interface RaccourcisEtat {
  visible: boolean;
  bord: BordRaccourcis;
  monitor_index: number;
}

export interface AlertesConfig {
  follow?: AlerteTypeConfig;
  raid?: AlerteTypeConfig;
  sub?: AlerteTypeConfig;
  resub?: AlerteTypeConfig;
  subgift?: AlerteTypeConfig;
  bits?: AlerteTypeConfig;
  overlay?: AlertesOverlayConfig;
}

/// Config d'une commande chat ("!commande" tapée par un viewer → overlay
/// diffusion, même moteur que les alertes : file + cooldowns + timer).
export interface CommandeConfig {
  /// ID unique (uuid) — vide sur une nouvelle commande, généré côté Rust.
  id: string;
  actif: boolean;
  /// Mot de la commande SANS le "!" (ex: "hype"). Normalisé lowercase.
  commande: string;
  duree_ms: number;
  /// Variables : {pseudo} {commande} {message} (reste de la ligne).
  texte_template: string;
  couleur: string;
  taille_px: number;
  cooldown_viewer_s: number;
  cooldown_global_s: number;
  /// Chemin relatif du son importé ("medias/<uuid>.<ext>"). Mode image.
  son?: string;
  /// Média ("medias/<uuid>.<ext>") : image (avec son séparé) ou vidéo (son
  /// inclus). undefined = icône + texte.
  media?: string;
  media_kind?: string;
}

// ===== Types Pad numérique (mirroir des structs Rust pad_numerique.rs) =====

/// Configuration d'une touche du pad (16 touches × 3 plages).
export interface ToucheConfig {
  /// Type de média : "audio" | "image" | "video". undefined = touche vide.
  media_type?: string;
  /// Chemin relatif du média ("medias/<uuid>.<ext>"). undefined = pas de média.
  media?: string;
  /// Kind du média : "image" | "video" (pour image/video).
  media_kind?: string;
  /// Chemin relatif du son ("medias/<uuid>.<ext>"). Pour audio = média principal,
  /// pour image = son séparé. undefined = silence.
  son?: string;
  /// Durée d'affichage en ms (1000-60000). Défaut 5000.
  duree_ms: number;
  /// Volume de lecture (0.0-1.0). Défaut 1.0.
  volume: number;
  /// Effet bounce activé (image/video).
  bounce: boolean;
  /// Effet zoom activé (image/video).
  zoom: boolean;
}

/// Config globale du pad (persistée dans pad_numerique.json).
export interface PadConfig {
  actif: boolean;
  /// Plage courante (0, 1 ou 2).
  plage_actuelle: number;
  /// Clé "NumpadX_plage" → config de la touche.
  touches: Record<string, ToucheConfig>;
}

export const tauri = {
  async getScene(): Promise<Scene> {
    return invoke<Scene>("get_scene");
  },

  async updateScene(scene: Scene): Promise<void> {
    await invoke("update_scene", { scene });
  },

  /// Importe un média (image OU vidéo) pour un widget. Retourne le chemin
  /// relatif ("medias/<uuid>.<ext>") + le nom original du fichier, ou null si
  /// le dialog a été annulé. Lance une erreur en cas de refus (taille/format).
  /// Le kind ("image"|"video") est déduit côté TS de l'extension du chemin.
  async importMedia(widgetId: string): Promise<[string, string] | null> {
    return invoke<[string, string] | null>("import_media", { widgetId });
  },

  /// Importe des médias depuis des chemins disque (drop Tauri). Mêmes règles
  /// qu'importMedia (validation + copie/remux vers medias/) mais sans dialog
  /// ni mutation scène. Retourne les succès (rel, kind, nom) + les erreurs
  /// concaténées — un échec sur un fichier n'annule pas les autres.
  async importMediaFromPaths(
    paths: string[]
  ): Promise<{ ok: [string, string, string][]; erreurs: string[] }> {
    return invoke("import_media_from_paths", { paths });
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

  // ===== Barre de raccourcis (fenêtre TOPMOST dockée) =====

  /// Liste les moniteurs Windows pour le sélecteur de la Toolbar.
  async raccourcisMoniteurs(): Promise<RaccourcisMoniteur[]> {
    return await invoke<RaccourcisMoniteur[]>("raccourcis_moniteurs");
  },

  /// État courant de la barre : config persistée (bord/moniteur) + fenêtre
  /// réellement ouverte (visible).
  async raccourcisEtat(): Promise<RaccourcisEtat> {
    return await invoke<RaccourcisEtat>("raccourcis_etat");
  },

  /// Applique bord + moniteur : crée/montre la barre dockée et persiste
  /// raccourcis.json. Index hors bornes → fallback moniteur primaire.
  async raccourcisAppliquer(
    bord: BordRaccourcis,
    monitorIndex: number
  ): Promise<RaccourcisEtat> {
    return await invoke<RaccourcisEtat>("raccourcis_appliquer", {
      bord,
      monitorIndex,
    });
  },

  /// Masque la barre (détruit la fenêtre) + persiste visible:false.
  /// Non-fatal si déjà fermée.
  async raccourcisMasquer(): Promise<void> {
    await invoke("raccourcis_masquer");
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

  /// Déplace une scène dans l'index (glisser-déposer barre « Vos scènes »).
  /// position = index cible après retrait de l'entrée (borné côté Rust).
  async sceneDeplacer(id: string, position: number): Promise<void> {
    await invoke("scene_deplacer", { id, position });
  },

  /// Masque/affiche le titre d'une scène dans sa pastille (œil, orange).
  async sceneMasquerNom(id: string, masque: boolean): Promise<void> {
    await invoke("scene_masquer_nom", { id, masque });
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

  // ===== Widget caméra (source OBS "SOS-Caméra" sous SOS-Diffusion) =====

  /// Crée / met à jour la source "SOS-Caméra" (dshow_input) dans la scène
  /// « SOS », sous SOS-Diffusion, calée sur le widget (centre + cover).
  /// device = FriendlyName PnP (= video_device_id dshow) ou null (device
  /// par défaut d'OBS). Appelée à la création du widget et au changement
  /// de device. Non-fatal si OBS offline (auto-guérison au prochain
  /// scene_sync_captures : boot / openScene / connexion OBS).
  async cameraSync(
    host: string,
    port: number,
    password: string,
    device: string | null,
    x: number,
    y: number,
    w: number,
    h: number
  ): Promise<void> {
    await invoke("camera_sync", { host, port, password, device, x, y, w, h });
  },

  /// Cache l'item de scène "SOS-Caméra" (SetSceneItemEnabled false) à la
  /// suppression du widget. L'input OBS est conservé (jamais RemoveInput) —
  /// la recréation du widget le réutilisera. Non-fatal si l'item est absent.
  async cameraHide(host: string, port: number, password: string): Promise<void> {
    await invoke("camera_hide", { host, port, password });
  },

  // ===== Alertes (follow/raid/sub/resub/subgift/bits) =====

  /// Config complète des alertes.
  async alertesEtat(): Promise<AlertesConfig> {
    return invoke<AlertesConfig>("alertes_etat");
  },

  /// Remplace la config d'un type d'alerte + persiste + push WS diffusion.
  async alertesSetConfig(typeAlerte: string, config: AlerteTypeConfig): Promise<void> {
    await invoke("alertes_set_config", { typeAlerte, config });
  },

  /// Remplace la position/taille de l'overlay alerte (même mécanique que
  /// welcomeSetOverlayConfig) + persiste + push WS diffusion.
  async alertesSetOverlayConfig(
    x: number,
    y: number,
    largeur: number,
    hauteur: number
  ): Promise<void> {
    await invoke("alertes_set_overlay_config", { x, y, largeur, hauteur });
  },

  /// Retourne la config du squelette de position unifié (source de vérité
  /// partagée par les overlays "clip de bienvenue" et "alertes").
  async positionOverlayEtat(): Promise<PositionOverlayConfig> {
    return invoke<PositionOverlayConfig>("position_overlay_etat");
  },

  /// Remplace la config du squelette de position unifié + persiste
  /// (position_overlay.json) + push WS `position-overlay-config` vers la
  /// diffusion (applique aux deux overlays en live).
  async positionOverlaySet(config: PositionOverlayConfig): Promise<void> {
    await invoke("position_overlay_set", { config });
  },

  /// Test manuel : déclenche une alerte avec données factices (bypass cooldowns).
  async alertesTester(typeAlerte: string): Promise<void> {
    await invoke("alertes_tester", { typeAlerte });
  },

  /// Déclenche une alerte depuis le frontend. Utilisé par la diff des follows
  /// (pas d'event IRC) — point d'extension pour les futures plateformes.
  async alerteDeclencher(
    typeAlerte: string,
    pseudo: string,
    userId: string,
    nbViewers: number,
    nbBits: number,
    nbMois: number,
    destinataire: string
  ): Promise<void> {
    await invoke("alerte_declencher", {
      typeAlerte, pseudo, userId, nbViewers, nbBits, nbMois, destinataire,
    });
  },

  /// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une alerte. Retourne le chemin
  /// relatif ("medias/<uuid>.<ext>") ou null si dialog annulé.
  async importSon(): Promise<string | null> {
    return invoke<string | null>("import_son");
  },

  // ===== Commandes chat (!commande → overlay diffusion) =====

  /// Liste complète des commandes chat.
  async commandesEtat(): Promise<CommandeConfig[]> {
    return invoke<CommandeConfig[]>("commandes_etat");
  },

  /// Ajoute/remplace une commande (upsert par id) + persiste + push WS.
  /// Retourne la config stockée (id généré si vide).
  async commandeSetConfig(config: CommandeConfig): Promise<CommandeConfig> {
    return invoke<CommandeConfig>("commande_set_config", { config });
  },

  /// Supprime une commande + persiste + push WS diffusion.
  async commandeSupprimer(id: string): Promise<void> {
    await invoke("commande_supprimer", { id });
  },

  /// Test manuel : déclenche une commande côté diffusion (bypass cooldowns).
  async commandeTester(id: string): Promise<void> {
    await invoke("commande_tester", { id });
  },

  // ===== Input Viewer (capture globale clavier + souris → overlay diffusion) =====

  /// Active/désactive la capture globale des entrées (clavier + souris).
  /// ON → thread poll clavier 60Hz + hook souris WH_MOUSE_LL. Zéro coût quand
  /// OFF. L'état est émis directement sur le WS :4321 (input-viewer-etat).
  async inputViewerSetActif(actif: boolean): Promise<void> {
    await invoke("input_viewer_set_actif", { actif });
  },

  /// true si la capture Input Viewer est active (état du bouton ON/OFF).
  async inputViewerEtatActif(): Promise<boolean> {
    return invoke<boolean>("input_viewer_etat_actif");
  },

  /// Importe un média (image OU vidéo) pour une alerte. Le dialog est filtré
  /// selon le kind demandé. Retourne le chemin relatif ("medias/<uuid>.<ext>")
  /// ou null si dialog annulé. Lance une erreur en cas de refus
  /// (taille/format/kind inattendu).
  async importAlerteMedia(kind: "image" | "video"): Promise<string | null> {
    return invoke<string | null>("import_alerte_media", { kindAttendu: kind });
  },

  // ===== Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) =====

  /// Retourne la config complète du pad (actif, plage, touches).
  async padEtat(): Promise<PadConfig> {
    return invoke<PadConfig>("pad_etat");
  },

  /// Active/désactive le pad + démarre/arrête la capture clavier globale.
  async padSetActif(actif: boolean): Promise<void> {
    await invoke("pad_set_actif", { actif });
  },

  /// Remplace la config d'une touche (upsert) + persiste + émet état.
  async padSetTouche(code: string, plage: number, config: ToucheConfig): Promise<void> {
    await invoke("pad_set_touche", { code, plage, config });
  },

  /// Supprime (reset) la config d'une touche + persiste + émet état.
  async padSupprimerTouche(code: string, plage: number): Promise<void> {
    await invoke("pad_supprimer_touche", { code, plage });
  },

  /// Change la plage courante (0-2) + persiste + émet état.
  async padSetPlage(plage: number): Promise<void> {
    await invoke("pad_set_plage", { plage });
  },

  /// Test manuel : déclenche une touche côté diffusion (bypass capture).
  async padTesterTouche(code: string, plage: number): Promise<void> {
    await invoke("pad_tester_touche", { code, plage });
  },

  /// Importe un média (image OU vidéo) pour une touche du pad. Dialog filtré
  /// selon le kind. Retourne le chemin relatif ou null si annulé.
  async padImporterMedia(kind: "image" | "video"): Promise<string | null> {
    return invoke<string | null>("pad_importer_media", { kindAttendu: kind });
  },

  /// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une touche du pad.
  async padImporterSon(): Promise<string | null> {
    return invoke<string | null>("pad_importer_son");
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
  /// Enrichi avec display_name + profile_image_url (batch /users).
  /// Détecte les unfollows par comparaison avec le snapshot précédent.
  /// Erreur "need_reauth" si 403 (scopes manquants).
  async twitchCommunauteFollowers(): Promise<{
    total: number;
    liste: {
      login: string;
      user_id: string;
      followed_at: string;
      display_name: string;
      profile_image_url: string;
    }[];
  }> {
    return invoke("twitch_communaute_followers");
  },

  /// Followers allégé (user_id + followed_at uniquement) pour le polling
  /// des alertes. Pas d'avatars, pas d'unfollows, pas de snapshot disque.
  async twitchFollowersLight(): Promise<{
    user_id: string;
    login: string;
    followed_at: string;
  }[]> {
    return invoke("twitch_followers_light");
  },

  /// Subs : liste + total + points (pagination max 10 pages).
  /// Enrichi avec display_name + profile_image_url (batch /users).
  async twitchCommunauteSubs(): Promise<{
    total: number;
    points: number;
    liste: {
      login: string;
      user_id: string;
      tier: string;
      is_gift: boolean;
      display_name: string;
      profile_image_url: string;
    }[];
  }> {
    return invoke("twitch_communaute_subs");
  },

  /// Viewers live : chiffre si en live, null si hors-ligne.
  async twitchCommunauteViewers(): Promise<number | null> {
    return invoke<number | null>("twitch_communaute_viewers");
  },

  /// Unfollows : historique des unfollows détectés au démarrage (lecture disque).
  async twitchCommunauteUnfollows(): Promise<
    {
      user_id: string;
      login: string;
      date_unfollow: string;
      display_name: string;
      profile_image_url: string;
    }[]
  > {
    return invoke("twitch_communaute_unfollows");
  },

  /// Broadcaster : user_id, login, display_name, avatar, type, description.
  async twitchBroadcaster(): Promise<{
    user_id: string;
    login: string;
    display_name: string;
    profile_image_url: string;
    broadcaster_type: string;
    description: string;
  }> {
    return invoke("twitch_broadcaster");
  },

  // ===== Modération Twitch (Helix lecture + écriture) =====

  /// Liste les VIPs de la chaîne.
  async twitchListerVips(): Promise<
    { user_id: string; login: string; display_name: string; profile_image_url: string }[]
  > {
    return invoke("twitch_lister_vips");
  },

  /// Liste les modérateurs de la chaîne.
  async twitchListerModerateurs(): Promise<
    { user_id: string; login: string; display_name: string; profile_image_url: string }[]
  > {
    return invoke("twitch_lister_moderateurs");
  },

  /// Liste les utilisateurs bannis/timeout de la chaîne.
  async twitchListerBannis(): Promise<
    {
      user_id: string;
      login: string;
      created_at: string;
      expires_at: string | null;
      reason: string;
      display_name: string;
      profile_image_url: string;
    }[]
  > {
    return invoke("twitch_lister_bannis");
  },

  /// Résout un login Twitch en user_id (recherche par pseudo).
  /// Retourne null si l'utilisateur n'existe pas.
  async twitchResoudreUser(
    login: string
  ): Promise<{ user_id: string; login: string; display_name: string } | null> {
    return invoke("twitch_resoudre_user", { login });
  },

  /// Bannir ou timeout un utilisateur. duree = undefined → ban permanent,
  /// number → timeout de n secondes.
  async twitchBannir(
    userId: string,
    raison: string,
    duree?: number
  ): Promise<void> {
    await invoke("twitch_bannir", { userId, raison, duree });
  },

  /// Débannir un utilisateur.
  async twitchDebannir(userId: string): Promise<void> {
    await invoke("twitch_debannir", { userId });
  },

  /// Ajouter un VIP.
  async twitchAjouterVip(userId: string): Promise<void> {
    await invoke("twitch_ajouter_vip", { userId });
  },

  /// Retirer un VIP.
  async twitchRetirerVip(userId: string): Promise<void> {
    await invoke("twitch_retirer_vip", { userId });
  },

  /// Ajouter un modérateur.
  async twitchAjouterModerateur(userId: string): Promise<void> {
    await invoke("twitch_ajouter_moderateur", { userId });
  },

  /// Retirer un modérateur.
  async twitchRetirerModerateur(userId: string): Promise<void> {
    await invoke("twitch_retirer_moderateur", { userId });
  },

  /// Supprimer un message de chat (par message_id).
  async twitchSupprimerMessage(messageId: string): Promise<void> {
    await invoke("twitch_supprimer_message", { messageId });
  },

  /// Envoie une commande IRC au chat Twitch (ex: "/clear").
  /// Retourne false si l'IRC n'est pas démarré.
  async twitchEnvoyerCommandeChat(commande: string): Promise<boolean> {
    return invoke<boolean>("twitch_envoyer_commande_chat", { commande });
  },

  // ===== Clips de bienvenue (welcome) =====

  /// État courant de la file d'attente (clip en cours + queue + config globale).
  async welcomeEtat(): Promise<WelcomeQueueEtat> {
    return invoke<WelcomeQueueEtat>("welcome_etat");
  },

  /// Registre Twitch complet : liste [login, ViewerConfig].
  async welcomeRegistreTwitch(): Promise<[string, WelcomeViewerConfig][]> {
    return invoke<[string, WelcomeViewerConfig][]>("welcome_registre_twitch");
  },

  /// Sauvegarde la config d'un viewer Twitch (registre + disque).
  async welcomeSauverViewerTwitch(
    login: string,
    config: WelcomeViewerConfig
  ): Promise<void> {
    await invoke("welcome_sauver_viewer_twitch", { login, config });
  },

  /// Supprime un viewer Twitch du registre.
  async welcomeSupprimerViewerTwitch(login: string): Promise<void> {
    await invoke("welcome_supprimer_viewer_twitch", { login });
  },

  /// Active/désactive la config globale des clips de bienvenue.
  async welcomeConfigGlobaleActif(actif: boolean): Promise<void> {
    await invoke("welcome_config_globale_actif", { actif });
  },

  /// Met à jour la config de l'overlay welcome (position/taille côté diffusion).
  async welcomeSetOverlayConfig(config: WelcomeOverlayConfig): Promise<void> {
    await invoke("welcome_set_overlay_config", { config });
  },

  /// Test manuel : lance un clip côté diffusion (bypass queue). Résout MP4 +
  /// émet welcome-clip-play. Utilisé par la modale Interactions chat.
  async welcomeTesterClip(
    clipId: string,
    clipTitre: string,
    clipDureeMs: number,
    displayName: string,
  ): Promise<void> {
    await invoke("welcome_tester_clip", {
      clipId,
      clipTitre,
      clipDureeMs,
      displayName,
    });
  },

  /// Définit la durée d'affichage globale des clips (0 = durée naturelle).
  async welcomeSetDureeAffichage(ms: number): Promise<void> {
    await invoke("welcome_set_duree_affichage", { ms });
  },

  /// Stop le clip courant + vide la queue.
  async welcomeStop(): Promise<void> {
    await invoke("welcome_stop");
  },

  /// Skip le clip courant → passe au suivant.
  async welcomeSkip(): Promise<void> {
    await invoke("welcome_skip");
  },

  /// Retire un item spécifique de la queue (par id).
  async welcomeRetirer(id: string): Promise<void> {
    await invoke("welcome_retirer", { id });
  },

  /// Remonte un item dans la queue (vers le début).
  async welcomeRemonter(id: string): Promise<void> {
    await invoke("welcome_remonter", { id });
  },

  /// Descend un item dans la queue (vers la fin).
  async welcomeDescendre(id: string): Promise<void> {
    await invoke("welcome_descendre", { id });
  },

  /// Vide la queue (sans stopper le clip courant).
  async welcomeVider(): Promise<void> {
    await invoke("welcome_vider");
  },

  /// Reset le seen set (nouveau stream → tous les streamers redeviennent éligibles).
  async welcomeResetSession(): Promise<void> {
    await invoke("welcome_reset_session");
  },

  /// Liste les clips récents d'un broadcaster Twitch (Helix GET /clips).
  async welcomeListerClipsStreamer(
    broadcasterId: string,
    first: number
  ): Promise<WelcomeClipInfo[]> {
    return invoke<WelcomeClipInfo[]>("welcome_lister_clips_streamer", {
      broadcasterId,
      first,
    });
  },

  /// Résout l'URL MP4 signée d'un clip Twitch (slug) via GQL.
  async welcomeResoudreMp4(slug: string): Promise<string> {
    return invoke<string>("welcome_resoudre_mp4", { slug });
  },

  /// Attribution automatique : pour chaque follower, fetch ses clips → clip
  /// aléatoire → résout MP4 → sauve config. Retourne { succes, echecs }.
  /// Émet "welcome:attribution-progress" pendant le traitement.
  async welcomeAttribuerAuto(
    followers: { login: string; user_id: string; display_name?: string }[]
  ): Promise<{ succes: number; echecs: number }> {
    return invoke("welcome_attribuer_auto", { followers });
  },

  // ===== Bandeau premier message =====

  /// État courant du bandeau (config : actif, duree_ms, position).
  async bandeauEtat(): Promise<BandeauEtat> {
    return invoke<BandeauEtat>("bandeau_etat");
  },

  /// Met à jour la config du bandeau (merge partiel). undefined = inchangé.
  async bandeauSetConfig(
    actif?: boolean,
    dureeMs?: number,
    position?: "bas" | "haut"
  ): Promise<void> {
    await invoke("bandeau_set_config", { actif, dureeMs, position });
  },

  /// Stop immédiat du bandeau courant.
  async bandeauStop(): Promise<void> {
    await invoke("bandeau_stop");
  },

  /// Reset le seen set (nouveau stream → tous les viewers redeviennent éligibles).
  async bandeauResetSession(): Promise<void> {
    await invoke("bandeau_reset_session");
  },

  /// Test manuel : lance un bandeau côté diffusion (bypass détection).
  async bandeauTester(displayName: string, message: string): Promise<void> {
    await invoke("bandeau_tester", { displayName, message });
  },

  // ===== Speedrun Splitter =====

  /// Charge un fichier .asl (Auto Split Language). Retourne les settings de
  /// base (start/split/reset) + les settings individuels (120+ pour MGS) +
  /// le dictionnaire code→nom (D.Names.Split) pour l'auto-mapping LSS.
  async speedrunChargerAsl(path: string): Promise<AslLoadResult> {
    return invoke<AslLoadResult>("speedrun_charger_asl", { path });
  },

  /// Charge un fichier .lss (LiveSplit Splits). Retourne les segments
  /// (nom + PB real/game time) + nom du jeu + catégorie.
  async speedrunChargerLss(path: string): Promise<LssRun> {
    return invoke<LssRun>("speedrun_charger_lss", { path });
  },

  /// Démarre le splitter avec un nombre total de splits + settings ASL.
  async speedrunDemarrer(
    totalSplits: number,
    start: boolean,
    split: boolean,
    reset: boolean
  ): Promise<void> {
    await invoke("speedrun_demarrer", { totalSplits, start, split, reset });
  },

  /// Arrête le splitter (stop la boucle de polling + cleanup thread).
  async speedrunArreter(): Promise<void> {
    await invoke("speedrun_arreter");
  },

  /// Met à jour les settings ASL pendant que le splitter tourne.
  async speedrunMajSettings(
    start: boolean,
    split: boolean,
    reset: boolean
  ): Promise<void> {
    await invoke("speedrun_maj_settings", { start, split, reset });
  },

  /// Retourne true si le splitter est actif (boucle de polling en cours).
  async speedrunEstActif(): Promise<boolean> {
    return invoke<boolean>("speedrun_est_actif");
  },

  /// Envoie une action manuelle au splitter : "start", "split", "skip",
  /// "undo", "reset", "pause".
  async speedrunActionManuelle(action: string): Promise<void> {
    await invoke("speedrun_action_manuelle", { action });
  },

  /// Lit la config speedrun sauvegardée (speedrun.json).
  async speedrunLireConfig(): Promise<SpeedrunConfig> {
    return invoke<SpeedrunConfig>("speedrun_lire_config");
  },

  /// Sauve la config speedrun (speedrun.json).
  async speedrunSauverConfig(config: SpeedrunConfig): Promise<void> {
    await invoke("speedrun_sauver_config", { config });
  },

  /// Met à jour les settings ASL individuels (120+ pour MGS) dans le runtime
  /// Boa. Appelé quand l'utilisateur valide la modale de configuration.
  async speedrunMajSettingsAsl(valeurs: Record<string, boolean>): Promise<void> {
    await invoke("speedrun_maj_settings_asl", { valeurs });
  },
};

// ===== Types welcome (mirroir des structs Rust) =====

export interface WelcomeViewerConfig {
  actif: boolean;
  clip_id: string;
  clip_titre: string;
  clip_thumbnail: string;
  clip_mp4_url: string;
  clip_duree_ms: number;
  message: string;
  display_name: string;
  avatar?: string | null;
  bio?: string | null;
}

export interface WelcomeQueueItem {
  id: string;
  login: string;
  display_name: string;
  avatar: string | null;
  bio: string | null;
  plateforme: string;
  clip_id: string;
  clip_titre: string;
  clip_mp4_url: string;
  clip_duree_ms: number;
  message: string;
}

export interface WelcomeOverlayConfig {
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
}

export interface WelcomeQueueEtat {
  en_attente: WelcomeQueueItem[];
  current: WelcomeQueueItem | null;
  config_globale_actif: boolean;
  overlay: WelcomeOverlayConfig;
  duree_affichage_ms: number;
}

export interface WelcomeClipInfo {
  id: string;
  title: string;
  url: string;
  thumbnail_url: string;
  duration: number;
  created_at: string;
  broadcaster_id: string;
  broadcaster_login: string;
  broadcaster_name: string;
}

// ===== Types bandeau premier message (mirroir des structs Rust) =====

export interface BandeauEtat {
  actif: boolean;
  duree_ms: number;
  position: "bas" | "haut";
}
