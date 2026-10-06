// SEUL endroit où invoke/listen Tauri sont utilisés côté dashboard.
import { invoke } from "@tauri-apps/api/core";

/// Une slide du carrousel d'images (widget média ou fond de scène).
/// rel = chemin relatif "medias/<uuid>.<ext>" ; nom = nom original du
/// fichier (affichage liste, même rôle que mediaNom). Miroir de MediaSlide
/// (scene.rs).
export interface MediaSlide {
  rel: string;
  nom: string;
}

export interface Widget {
  id: string;
  type: "media" | "chat" | "camera" | "welcome-clip" | "input-viewer" | "speedrun" | "goal" | "musique";
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
  /// Miroir du média : retournement horizontal/vertical du contenu
  /// (scale ±1 combiné au zoom dans le rendu). undefined/false = pas de miroir.
  mediaMiroirH?: boolean;
  mediaMiroirV?: boolean;
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
  /// Carrousel d'images : liste non vide = ACTIF et prioritaire sur `media`
  /// (le média unique reste stocké, il redevient visible si la liste est
  /// vidée). Images uniquement.
  mediaSlides?: MediaSlide[];
  /// Intervalle du carrousel en secondes. undefined → 5 s (défaut diffusion).
  mediaSlidesSec?: number;
  trou?: boolean;
  /// Opt-out du cadre SVG de scène pour ce widget (widgets créés par drop
  /// bibliothèque → transformerEnWidget). undefined/false = cadre appliqué.
  sansCadre?: boolean;
  obsSource?: string;
  chatFiltre?: string;
  taillePolice?: number;
  /// Mise en forme des messages (widget "chat") : "compact" (défaut —
  /// messages joints, zébrage) ou "bulles" (messages détachés, chacun avec
  /// le cadre SVG de scène — même traitement qu'un widget objectif).
  chatStyle?: string;
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
  /// Disposition du splitter speedrun : "auto" | "colonne" | "ligne".
  /// "auto"/undefined = splits en ligne si la hauteur passe sous
  /// ~4×srTaillePolice (façon LiveSplit horizontal), colonne sinon.
  srLayout?: string;
  /// Plateforme suivie par un widget type "goal" :
  /// "twitch" | "youtube" | "kick" | "tiktok". undefined = non configuré.
  goalPlateforme?: GoalPlateforme;
  /// Métrique suivie par un widget type "goal". Les métriques indisponibles
  /// pour la plateforme choisie affichent « N/A » en diffusion.
  goalMetrique?: GoalMetrique;
  /// Valeur cible du widget type "goal". undefined = compteur seul
  /// (pas de frame goal-atteint).
  goalCible?: number;
  /// Id du Creator Goal Twitch LIÉ (objectif déclaré dans le Creator
  /// Dashboard — GET /goals, scope channel:read:goals). Présent = mode
  /// lié : actuel/cible/description viennent des frames "goal-twitch"
  /// (EventSub + poll viewers.rs), goalMetrique/goalCible ignorés.
  /// undefined = mode manuel.
  goalId?: string;
  /// Mode d'affichage des widgets type "musique" : "player" (défaut — LCD
  /// marquee + temps + barre de progression + équalizer) ou "eq" (bandes
  /// de spectre seules).
  musiqueMode?: "player" | "eq";
  /// Chaîne de morphs (bulge/pinch) appliquée au média, dans l'ordre.
  /// undefined/vide = rendu natif (img/video), zéro coût GL.
  morphs?: MorphPoint[];
  /// Texte du titre affiché au-dessus du widget. undefined/vide = aucun titre
  /// à cette position. Jusqu'à 4 titres simultanés (haut/bas/gauche/droite —
  /// côtés en texte vertical), style partagé (titrePolice, titreTaille, …) et
  /// gradient de couleur reprenant celui du cadre SVG actif de la scène
  /// (fallback blanc→gris si aucun cadre actif).
  titreHaut?: string;
  /// Texte du titre affiché en dessous du widget.
  titreBas?: string;
  /// Texte du titre affiché à gauche du widget (texte vertical).
  titreGauche?: string;
  /// Texte du titre affiché à droite du widget (texte vertical).
  titreDroite?: string;
  /// LEGACY — ancien titre à position unique (« dessus »/« dessous »).
  /// Migré vers titreHaut/titreBas côté Rust à la lecture (migrer_titres) ;
  /// jamais écrit par l'UI.
  titre?: string;
  /// LEGACY — position du titre legacy. Voir `titre`.
  titrePosition?: "dessus" | "dessous";
  /// Couleur de début du dégradé du texte. Absent → dégradé repris du cadre
  /// SVG actif (comportement automatique par défaut).
  titreCouleur?: string;
  /// Couleur de fin du dégradé du texte. Absent → couleur unie (reprend
  /// titreCouleur) ou fin du dégradé du cadre si titreCouleur absent.
  titreCouleurFin?: string;
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
  /// Couleur du contour des lettres du titre (hex, ex: "#ffffff").
  /// Défaut blanc.
  titreContourCouleur?: string;
  /// Épaisseur du contour des lettres en px (0-6 ; 0 = aucun contour).
  /// Défaut 2.
  titreContourEpaisseur?: number;
}

/// Plateformes supportées par les widgets type "goal" (miroir Rust scene.rs).
export type GoalPlateforme = "twitch" | "youtube" | "kick" | "tiktok";

/// Métriques de goal — disponibilité réelle par plateforme (frames
/// "communaute" / "viewers" de viewers.rs + tiktok_chat.rs) :
///   twitch  = followers, subs, sub_points, viewers
///   youtube = abonnes, viewers  (members = N/A, scope Device Flow absent)
///   kick    = followers, viewers
///   tiktok  = viewers uniquement
/// Les métriques non listées pour une plateforme affichent « N/A ».
export type GoalMetrique =
  | "followers"
  | "subs"
  | "sub_points"
  | "abonnes"
  | "viewers";

/// Métriques réellement mesurées par plateforme — la config n'en propose
/// que celles-là (pas de valeur fabriquée).
export const GOAL_METRIQUES: Record<GoalPlateforme, GoalMetrique[]> = {
  twitch: ["followers", "subs", "sub_points", "viewers"],
  youtube: ["abonnes", "viewers"],
  kick: ["followers", "viewers"],
  tiktok: ["viewers"],
};

/// Suggestion de palier « prochain chiffre reconnaissable » — aligné sur
/// l'algo officiel Twitch Creator Goals vérifié (11→20, 260→300) :
/// <100 → +10, <1000 → +100, <10000 → +1000, sinon +10000.
export function goalPalierSuivant(n: number): number {
  const pas = n < 100 ? 10 : n < 1000 ? 100 : n < 10000 ? 1000 : 10000;
  return (Math.floor(n / pas) + 1) * pas;
}

/// Suggestion contextuelle : palier générique, sauf le cas « Path to
/// Affiliate » — twitch/followers sous 25 → 25 (palier officiel).
export function goalSuggestion(
  plateforme: GoalPlateforme,
  metrique: GoalMetrique,
  actuel: number,
): number {
  if (plateforme === "twitch" && metrique === "followers" && actuel < 25) {
    return 25;
  }
  return goalPalierSuivant(actuel);
}

/// Creator Goal Twitch — objectif DÉCLARÉ par le streamer dans son Creator
/// Dashboard (miroir de twitch_helix::CreatorGoal — mêmes noms que la frame
/// WS "goal-twitch" et le retour de `twitch_goals_lister`, zéro mapping).
export interface TwitchGoal {
  /// id Helix — valeur de Widget.goalId quand le widget est lié.
  id: string;
  /// Type Helix : "follower" | "subscription" | "subscription_count" |
  /// "new_subscription" | "new_subscription_count" | "new_bit" | "new_cheerer".
  goal_type: string;
  /// Texte libre du streamer (Creator Dashboard) — peut être vide.
  description: string;
  actuel: number;
  cible: number;
  /// GET : dérivé (actuel >= cible) ; channel.goal.end : verdict Twitch.
  atteint: boolean;
  /// true seulement via channel.goal.end (ou disparition du GET /goals).
  termine: boolean;
}

// ===== Types planning Twitch (miroir twitch_helix::Schedule*) =====

/// Un segment du planning Twitch. Pour un récurrent, chaque occurrence
/// hebdo a un id propre (PATCH/DELETE d'une occurrence impacte la série).
export interface PlanningSegment {
  id: string;
  start_time: string;
  end_time: string;
  title: string;
  /// Vide = segment sans jeu déclaré.
  category_id: string;
  category_name: string;
  is_recurring: boolean;
  /// Non-null = annulé (récurrent : prochaine occurrence annulée).
  canceled_until: string | null;
}

/// Réponse GET /schedule : segments + vacances (dates ISO, "" = absent).
export interface PlanningResp {
  segments: PlanningSegment[];
  vacation_start: string;
  vacation_end: string;
}

/// Corps partiel de PATCH /schedule/segment — clés snake_case (format Helix,
/// le struct Rust se désérialise tel quel, sans conversion camelCase).
/// start_time modifiable sur les ponctuels uniquement.
export interface PlanningSegmentPatch {
  start_time?: string;
  /// Requis par Twitch quand start_time est envoyé (IANA).
  timezone?: string;
  /// Minutes 30-1380.
  duration_min?: number;
  category_id?: string;
  title?: string;
  /// Annuler (true) / rétablir (false) — récurrent : prochaine occurrence.
  is_canceled?: boolean;
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
/// `time` des events split/ended = durée du segment bouclé ; le cumul de
/// run voyage dans `cumul`. `segmentTime` = temps vivant du segment courant
/// (event time/etat) — null si timer armé en NotRunning.
export interface SpeedrunEvent {
  type:
    | "started"
    | "split"
    | "reset"
    | "ended"
    | "goto"
    | "etat"
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
  cumul?: string;
  index?: number;
  phase?: string;
  realTime?: string;
  gameTime?: string | null;
  segmentTime?: string | null;
  splitTimes?: (string | null)[];
  process?: string;
  pid?: number;
  message?: string;
  settings?: SpeedrunSettingInfo[];
}

/// Configuration d'un cadre SVG (widget ou bord canvas).
/// `style` = ID du registre ("carre", "arrondis", "cyberpunk", "story",
/// "thin-line", "hexagon", "mgstyle"). `variante` = champ legacy (désérialisé
/// pour lire les vieilles scènes, ignoré au rendu et jamais réécrit).
/// `couleur`/`couleurFin` = couleurs du cadre (tous les styles — les tons
/// internes des presets en sont dérivés).
/// `gradientAngle` = angle du dégradé en degrés (0-360, défaut 135 = diagonal).
/// `actif` = cadre visible.
/// `bordActif` = le cadre s'applique aussi au bord du canvas (absent/false
/// = widgets seuls). undefined = true (rétro-compat vieilles scènes).
/// `animer` = émet les éléments SMIL animés du cadre (toggle « Animer »).
/// undefined/false = rendu statique (défaut, rétro-compat).
export interface CadreConfig {
  style: string;
  variante?: string;
  strokeWidth: number;
  couleur: string;
  couleurFin: string;
  gradientAngle: number;
  actif: boolean;
  bordActif?: boolean;
  animer?: boolean;
}

/// Identifiants des modules HUD (singletons par scène).
export type ModuleId = "horloge" | "musique" | "objectifs";
export const MODULE_IDS: ModuleId[] = ["horloge", "musique", "objectifs"];

/// Ligne d'un module « Objectifs du stream » : métrique mesurée
/// (`plateforme` + `metrique` — mêmes valeurs que le widget goal, frames
/// communaute/viewers) OU Creator Goal Twitch lié (`goalId`, frames
/// goal-twitch). `cible` = cible du mode manuel (undefined = compteur
/// seul). `label` = libellé affiché (undefined = libellé auto).
export interface LigneObjectif {
  id: string;
  plateforme?: GoalPlateforme;
  metrique?: GoalMetrique;
  cible?: number;
  goalId?: string;
  label?: string;
}

/// Un module HUD de la scène — couche d'affichage SÉPARÉE des widgets
/// (rectangle libre en px canvas + options). Struct unique pour les trois
/// modules : les options spécifiques sont ignorées par les autres.
/// Miroir de `ModuleHud` (scene.rs) — contrat JSON direct, camelCase.
export interface ModuleHud {
  actif: boolean;
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
  /// Police Google Font (fonts.ts). undefined = police système.
  police?: string;
  /// Taille de police de base (px). undefined = dérivée de la hauteur.
  taille?: number;
  /// Cadre SVG de la scène autour du module. undefined/false = aucun.
  cadre?: boolean;
  /// Opacité du fond translucide (0–1). undefined = 0.45.
  opacite?: number;
  /// Horloge : date à gauche du trait (défaut true), secondes (défaut
  /// false), format 12 h (défaut false).
  afficherDate?: boolean;
  secondes?: boolean;
  format12h?: boolean;
  /// Musique : pastille temps/progression (défaut true).
  afficherTemps?: boolean;
  /// Objectifs : titre du bloc + lignes dans l'ordre.
  titre?: string;
  lignes?: LigneObjectif[];
}

/// Modules HUD d'une scène — un emplacement par module. undefined =
/// jamais configuré sur cette scène.
export interface ScenesModules {
  horloge?: ModuleHud;
  musique?: ModuleHud;
  objectifs?: ModuleHud;
}

export interface Scene {
  widgets: Widget[];
  canvasW?: number;
  canvasH?: number;
  bgMedia?: string;
  /// Carrousel d'images du fond : liste non vide = ACTIF et prioritaire
  /// sur bgMedia (qui reste stocké). bgObsSource reste prioritaire sur
  /// tout (page transparente). Images uniquement.
  bgSlides?: MediaSlide[];
  /// Intervalle du carrousel de fond en secondes. undefined → 5 s.
  bgSlidesSec?: number;
  bgKind?: string;
  bgFit?: string;
  bgZoom?: number;
  bgRot?: number;
  /// Inclinaison 3D du fond (gizmo, ±90° max) : perspective(800px)
  /// rotateX/rotateY sur l'élément #bg (diffusion) / .bg-layer (dashboard).
  /// 0 = fond plat. Distinct de bgRot (rotation 2D du média).
  bgRotX?: number;
  bgRotY?: number;
  bgOffsetX?: number;
  bgOffsetY?: number;
  /// Miroir du fond : retournement horizontal/vertical du média de fond.
  bgMiroirH?: boolean;
  bgMiroirV?: boolean;
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
  /// Nom de la source OBS de capture du FOND ("SOS-Fond-<sceneId8>").
  /// Présence = fond capture (prioritaire sur bgMedia — le média n'est pas
  /// peint tant que la capture est active).
  bgObsSource?: string;
  /// Cadre unifié de la scène : appliqué à chaque widget (clip-path +
  /// overlay SVG) ET au bord extérieur du canvas.
  cadreWidget?: CadreConfig;
  /// LEGACY — ancien cadre du bord de l'application. Migré vers cadreWidget
  /// par le Rust au chargement (migrer_cadres) ; jamais écrit par l'UI.
  cadreApp?: CadreConfig;
  /// Chaîne de morphs appliquée au FOND de scène (bulge/pinch), dans l'ordre.
  /// Coordonnées normalisées (0-1) du canvas. undefined/vide = rendu natif.
  bgMorphs?: MorphPoint[];
  /// Texte du titre affiché en haut à l'intérieur du fond de l'application.
  /// undefined/vide = aucun titre à cette position. Jusqu'à 4 titres
  /// simultanés (haut/bas/gauche/droite — côtés en texte vertical), style
  /// partagé (bgTitrePolice, bgTitreTaille, …) et gradient de couleur
  /// reprenant celui du cadre unifié (cadreWidget).
  bgTitreHaut?: string;
  /// Texte du titre affiché en bas à l'intérieur du fond.
  bgTitreBas?: string;
  /// Texte du titre affiché le long du bord gauche du fond (vertical).
  bgTitreGauche?: string;
  /// Texte du titre affiché le long du bord droit du fond (vertical).
  bgTitreDroite?: string;
  /// LEGACY — ancien titre de fond à position unique. Migré vers
  /// bgTitreHaut/bgTitreBas côté Rust à la lecture (migrer_titres) ; jamais
  /// écrit par l'UI.
  bgTitre?: string;
  /// LEGACY — position du titre de fond legacy. Voir `bgTitre`.
  bgTitrePosition?: "haut" | "bas";
  /// Couleur de début du dégradé du texte. Absent → dégradé repris du cadre
  /// unifié (cadreWidget), comportement automatique par défaut.
  bgTitreCouleur?: string;
  /// Couleur de fin du dégradé du texte. Absent → couleur unie (reprend
  /// bgTitreCouleur) ou fin du dégradé du cadre si bgTitreCouleur absent.
  bgTitreCouleurFin?: string;
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
  /// Couleur du contour des lettres du titre du fond (hex, ex: "#ffffff").
  /// Défaut blanc.
  bgTitreContourCouleur?: string;
  /// Épaisseur du contour des lettres du titre du fond en px (0-6 ; 0 =
  /// aucun contour). Défaut 2.
  bgTitreContourEpaisseur?: number;
  /// Aide « fond vide » (presets guidés) : encadré centré sur le canvas du
  /// dashboard, auto-masqué dès qu'un média de fond ou une capture OBS est lié.
  aideFond?: boolean;
  /// Aide « connecte-toi » au-dessus des widgets goal : auto-masquée quand
  /// toutes les plateformes des goals sont connectées.
  aideObjectifs?: boolean;
  /// Aide « personnalise tes cadres » (presets guidés) : encadré centré
  /// empilé avec aideFond, auto-masquée à la première ouverture de la
  /// modale Cadres ou via la croix (dismiss persistant).
  aideCadres?: boolean;
  /// Modules HUD par scène (horloge / musique en cours / objectifs du
  /// stream) — couche séparée des widgets, transportée par le snapshot WS.
  /// undefined = aucun module (vieilles scènes).
  modules?: ScenesModules;
}

/// Entrée de l'index des scènes (id + nom seulement, jamais le contenu).
export interface SceneIndex {
  id: string;
  nom: string;
  /// Titre masqué dans la pastille de la barre « Vos scènes » (œil) —
  /// l'onglet devient orange. Absent des anciens index.json → false.
  nomMasque?: boolean;
}

/// Métadonnées d'un preset de scène embarqué (galerie — sans le contenu).
export interface PresetInfo {
  id: string;
  nom: string;
  description: string;
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

/// Moniteur vu par OBS (GetMonitorList, obs-websocket v5) — sélecteur de la
/// section Prévisualisation. Index OBS, PAS l'index Win32 de
/// RaccourcisMoniteur.
export interface ObsMoniteur {
  monitorIndex: number;
  monitorName: string;
  monitorWidth: number;
  monitorHeight: number;
  monitorPositionX: number;
  monitorPositionY: number;
}

/// Mode d'affichage de la fenêtre principale (section Ergonomie).
export type ModeFenetre = "fenetre" | "maximise" | "fullscreen";

/// Préférence persistée de la fenêtre principale (fenetre.json) : écran de
/// démarrage + mode d'affichage.
export interface FenetreEtat {
  monitor_index: number;
  mode: ModeFenetre;
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

// ===== Types Musique (!rs → bibliothèque locale → diffusion) =====

/// Config musique (miroir de MusiqueConfig Rust — musique.rs).
export interface MusiqueConfig {
  /// false = !rs ignoré silencieusement.
  actif: boolean;
  /// Mot de commande SANS le "!" (défaut "rs").
  commande: string;
  /// Commande skip — mods/broadcaster (défaut "skip").
  commande_skip: string;
  /// Commande "retire ma demande" (défaut "retirer").
  commande_retirer: string;
  /// Commande "affiche la file" (défaut "file").
  commande_file: string;
  /// Cooldown par viewer entre deux demandes (s, 0 = off).
  cooldown_viewer_s: number;
  /// Nb max de pistes d'un viewer dans la file + en cours (0 = illimité).
  max_par_viewer: number;
  /// Taille max de la file (défaut 20).
  file_max: number;
  /// Volume de lecture côté diffusion (0.0–1.0).
  volume: number;
  /// false = aucun accusé chat (Twitch peut répondre).
  reponses_chat: boolean;
}

/// Piste demandée (en cours ou en file).
export interface MusiqueItem {
  id: string;
  /// Titre affiché (nom de fichier nettoyé).
  titre: string;
  /// Chemin relatif servi par :4321 ("medias/musique/<fichier>").
  src: string;
  demandeur: string;
  plateforme: string;
}

/// Entrée de bibliothèque = un fichier de medias/musique.
export interface MusiqueFichier {
  src: string;
  titre: string;
  taille: number;
}

/// Progression de la piste en cours (snapshot figé à l'emit — le lecteur
/// l'avance en tick local quand pausee=false). null = rien en cours.
export interface MusiqueProgression {
  /// Position de lecture en ms au moment de l'emit.
  position_ms: number;
  /// true = en pause côté diffusion.
  pausee: boolean;
  /// Durée totale en secondes (remontée par musique-meta, null = inconnue).
  duree_s: number | null;
}

/// Cooldown viewer actif (!rs) — restant_s figé à l'emit (décrément local).
export interface MusiqueCooldown {
  pseudo: string;
  restant_s: number;
}

/// État complet retourné par musique_etat / l'event "musique:update".
export interface MusiqueEtat {
  config: MusiqueConfig;
  en_cours: MusiqueItem | null;
  file: MusiqueItem[];
  bibliotheque: MusiqueFichier[];
  progression: MusiqueProgression | null;
  cooldowns_actifs: MusiqueCooldown[];
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

// ===== Types Écran d'attente (mirroir des structs Rust ecran_attente.rs) =====

/// Config de l'écran d'attente (persistée dans ecran_attente.json).
export interface EcranAttenteConfig {
  /// Durée du chrono en secondes. Défaut 600.
  duree_s: number;
  /// Titre affiché en haut de l'écran (dégradé aux couleurs du cadre).
  titre: string;
  /// Afficher le titre. Défaut true.
  afficher_titre: boolean;
  /// Son du média vidéo (porté par la vidéo côté diffusion). Défaut true.
  son: boolean;
  /// Volume du média + de la source audio OBS dédiée (0.0-1.0). Défaut 0.5.
  volume: number;
  /// Chemin relatif du média ("medias/<uuid>.<ext>"). undefined = pas de média.
  media?: string;
  /// Kind du média : "image" | "video". undefined = pas de média.
  media_kind?: string;
  /// Nom original du fichier importé (affichage UI).
  media_nom?: string;
  /// Source audio OBS (wasapi_output_capture) choisie — undefined = créer
  /// « SOS Audio Écran d'attente ».
  source_audio?: string;
  /// Centre horizontal de la carte minuteur (px canvas diffusion).
  /// Défaut 960. Positionné par drag dans la modale.
  minuteur_x: number;
  /// Centre vertical de la carte minuteur (px canvas). Défaut 540.
  minuteur_y: number;
  /// Échelle de la carte minuteur (0.4-3.0). Défaut 1.0.
  minuteur_echelle: number;
  /// Layout de l'overlay : "signature" (marque StreamOS — Streamy + stries
  /// néon + wordmark, défaut) | "minimal" (rendu historique).
  layout: string;
  /// Centre horizontal de Streamy (px canvas). Défaut 170 (haut-gauche).
  streamy_x: number;
  /// Centre vertical de Streamy (px canvas). Défaut 180.
  streamy_y: number;
  /// Échelle de Streamy (0.3-2.0). Défaut 0.6.
  streamy_echelle: number;
  /// Centre horizontal du wordmark (px canvas). Défaut 960.
  logo_x: number;
  /// Centre vertical du wordmark (px canvas). Défaut 140 (haut-centre).
  logo_y: number;
  /// Échelle du wordmark (0.3-2.0). Défaut 1.0.
  logo_echelle: number;
}

/// Snapshot complet (config + runtime) : commande `ecran_attente_etat` +
/// event `ecran-attente:etat` (émis à chaque tick Rust 1 Hz / transition).
export interface EcranAttenteEtat {
  config: EcranAttenteConfig;
  actif: boolean;
  en_pause: boolean;
  /// Secondes restantes (détenues par Rust — le dashboard reflète).
  temps_restant: number;
}

// ===== Types Just Chatting (mirroir des structs Rust just_chatting.rs) =====

/// Rectangle en FRACTIONS du canvas (0–1) — converti en pixels côté
/// diffusion/dashboard (× canvasW/H de la scène réelle).
export interface JcRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/// Paire caméra / chat pour une orientation.
export interface JcLayout {
  camera: JcRect;
  chat: JcRect;
}

/// Snapshot JC : commande `just_chatting_etat` + event `just-chatting:etat`
/// + charge utile de la frame WS `just-chatting-etat`. Les deux orientations
/// sont calculées en Rust (formule unique) ; on choisit selon canvasW/H
/// (paysage si W >= H). `actif` est runtime, NON persisté — OFF au boot.
export interface JustChattingEtat {
  actif: boolean;
  paysage: JcLayout;
  portrait: JcLayout;
}

// ===== Types Diagnostic conso (mirroir des structs Rust conso.rs) =====

/// Un processus mesuré (section « PC » de la modale Diagnostic).
export interface ConsoProcessus {
  nom: string;
  /// % du CPU total (tous cœurs confondus), 0-100.
  cpu_pct: number;
  /// Mémoire de travail, en Mo.
  ram_mo: number;
  /// % GPU (compteurs « GPU Engine » WDDM). null = non mesurable → « N/A ».
  gpu_pct: number | null;
}

/// Un sous-système StreamOS (threads agrégés par label).
export interface ConsoThread {
  nom: string;
  /// % D'UN cœur (un thread saturé = 100 %).
  cpu_pct: number;
}

/// Compteurs d'activité sur la dernière seconde (tap passif chat_tx).
export interface ConsoActivite {
  chat_msgs: number;
  speedrun_events: number;
  input_viewer_pubs: number;
}

/// Un échantillon d'une seconde.
export interface ConsoEchantillon {
  /// Epoch millisecondes.
  ts: number;
  processus: ConsoProcessus[];
  threads: ConsoThread[];
  activite: ConsoActivite;
}

/// Réponse de `conso_etat` : historique roulant (≤300 pts, ~5 min à 1 Hz).
export interface ConsoEtat {
  nb_coeurs: number;
  historique: ConsoEchantillon[];
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

  /// Importe PLUSIEURS images en un seul dialog multi-sélection (carrousels
  /// mediaSlides widget + bgSlides fond — images uniquement). Mêmes règles
  /// de validation/copie qu'importMedia. Dialog annulé → ok vide. Un échec
  /// sur un fichier n'annule pas les autres (erreurs concaténées).
  async importMediasMulti(): Promise<{ ok: [string, string, string][]; erreurs: string[] }> {
    return invoke("import_medias_multi");
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

  /// Bascule le streaming OBS (ToggleStream). Retourne le nouvel état.
  async obsToggleStream(host: string, port: number, password: string): Promise<boolean> {
    return await invoke<boolean>("obs_toggle_stream", { host, port, password });
  },

  /// Bascule l'enregistrement OBS (ToggleRecord). Retourne le nouvel état.
  async obsToggleRecord(host: string, port: number, password: string): Promise<boolean> {
    return await invoke<boolean>("obs_toggle_record", { host, port, password });
  },

  /// État des sorties OBS (stream + record, une connexion). Pas de polling :
  /// lu au connect + après chaque toggle.
  async obsOutputsStatus(
    host: string,
    port: number,
    password: string,
  ): Promise<{ stream: boolean; record: boolean }> {
    return await invoke("obs_outputs_status", { host, port, password });
  },

  /// Liste les moniteurs vus par OBS (GetMonitorList) — sélecteur de la
  /// section Prévisualisation. Index OBS, PAS les index Win32.
  async obsProjecteurMoniteurs(
    host: string,
    port: number,
    password: string,
  ): Promise<ObsMoniteur[]> {
    return await invoke<ObsMoniteur[]>("obs_projecteur_moniteurs", {
      host,
      port,
      password,
    });
  },

  /// Ouvre le projecteur OBS « prévisualisation » (OpenVideoMixProjector) —
  /// équivalent du clic droit OBS. monitorIndex : index OBS, -1 = fenêtré.
  async obsProjecteurPreview(
    host: string,
    port: number,
    password: string,
    monitorIndex: number,
  ): Promise<void> {
    await invoke("obs_projecteur_preview", {
      host,
      port,
      password,
      monitorIndex,
    });
  },

  /// Ferme les fenêtres projecteur OBS (WM_CLOSE local, pas de WebSocket).
  /// Comportement par défaut au boot : OBS restaure ses projecteurs,
  /// StreamOS impose « fermé » sauf si l'option auto est activée.
  async obsProjecteurFermer(): Promise<number> {
    return await invoke<number>("obs_projecteur_fermer");
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

  // ===== Fenêtre principale (écran + mode au démarrage — Ergonomie) =====

  /// Préférence persistée de la fenêtre main (fenetre.json) : écran + mode.
  async fenetreEtat(): Promise<FenetreEtat> {
    return await invoke<FenetreEtat>("fenetre_etat");
  },

  /// Applique écran + mode IMMÉDIATEMENT à la fenêtre main et persiste
  /// fenetre.json. La géométrie fenêtrée est recapturée au passage.
  async fenetreAppliquer(
    monitorIndex: number,
    mode: ModeFenetre
  ): Promise<FenetreEtat> {
    return await invoke<FenetreEtat>("fenetre_appliquer", {
      monitorIndex,
      mode,
    });
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
  /// `scene` optionnel (Just Chatting) : synchronise avec la copie FOURNIE
  /// au lieu de la scène chargée — jamais écrite côté Rust (état ni disque).
  async sceneSyncCaptures(host: string, port: number, password: string, scene?: Scene): Promise<void> {
    await invoke("scene_sync_captures", { host, port, password, scene });
  },

  // ===== Just Chatting (overlay non-destructif, état runtime non persisté) =====

  /// Snapshot JC (actif + layouts paysage/portrait). OFF au boot.
  async justChattingEtat(): Promise<JustChattingEtat> {
    return invoke<JustChattingEtat>("just_chatting_etat");
  },

  /// Active ou coupe le mode. N'écrit pas la scène, pas de JSON.
  async justChattingSetActif(actif: boolean): Promise<void> {
    await invoke("just_chatting_set_actif", { actif });
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

  /// Duplique une scène (ids widgets régénérés, obsSource conservé — source
  /// OBS partagée). Copie ajoutée en fin de liste, bascule dessus.
  async sceneDupliquer(id: string, nom: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_dupliquer", { id, nom });
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

  // ===== Presets de scènes embarqués =====

  /// Liste les métadonnées des presets embarqués (galerie). Parse non-fatal
  /// côté Rust : un JSON cassé est loggé + absent de la liste.
  async presetsLister(): Promise<PresetInfo[]> {
    return invoke<PresetInfo[]>("presets_lister");
  },

  /// Crée une NOUVELLE scène depuis un preset embarqué (l'existante jamais
  /// touchée). Ids régénérés + canvas OBS appliqués côté Rust.
  async sceneCreerDepuisPreset(nom: string, presetId: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_creer_depuis_preset", { nom, presetId });
  },

  /// ONBOARDING : initialise la scène de départ (courante, vide au 1er boot)
  /// depuis un preset — remplace son contenu + renomme. Contrat : uniquement
  /// au premier lancement.
  async sceneInitialiserPreset(presetId: string): Promise<SceneIndex> {
    return invoke<SceneIndex>("scene_initialiser_preset", { presetId });
  },

  /// État de l'onboarding : true = déjà fait (ne pas montrer la galerie).
  async onboardingEtat(): Promise<boolean> {
    return invoke<boolean>("onboarding_etat");
  },

  /// Marque l'onboarding comme fait (choix preset OU page vierge).
  async onboardingMarquerFait(): Promise<void> {
    await invoke("onboarding_marquer_fait");
  },

  // ===== Langue UI (i18n dashboard) =====

  /// Langue persistée dans langue.json. null = jamais choisie → le front
  /// détecte navigator.language (fallback "fr").
  async langueEtat(): Promise<string | null> {
    return invoke<string | null>("langue_etat");
  },

  /// Persiste la langue de l'UI (langue.json).
  async langueSauver(langue: string): Promise<void> {
    await invoke("langue_sauver", { langue });
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

  /// Découpe "SOS-Caméra" + "SOS-Caméra-Fond" à la forme du cadre SVG du
  /// widget caméra (filtre OBS « Image mask/Blend » + PNG régénéré côté
  /// Rust dans <data>/obs). points = polygone widget-local
  /// (pointsClipPathCadre — même forme que le trou destination-out de
  /// diffusion.html) ; null = cadre sans forme → filtres retirés, caméra
  /// rectangulaire. fit/zoom = mêmes valeurs que l'item obsSyncTrous de la
  /// caméra. Retourne false si la source n'était pas encore prête (capteur
  /// en démarrage) → ne pas mettre en cache, retenter au prochain commit.
  async cameraMaskSync(
    host: string,
    port: number,
    password: string,
    points: [number, number][] | null,
    w: number,
    h: number,
    fit: string,
    zoom: number
  ): Promise<boolean> {
    return invoke<boolean>("camera_mask_sync", { host, port, password, points, w, h, fit, zoom });
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

  // ===== Musique (!rs → bibliothèque locale → diffusion) =====

  /// État complet : config + en_cours + file + bibliothèque.
  async musiqueEtat(): Promise<MusiqueEtat> {
    return invoke<MusiqueEtat>("musique_etat");
  },

  /// Sauvegarde la config musique (noms normalisés + distincts côté Rust).
  async musiqueSetConfig(config: MusiqueConfig): Promise<MusiqueConfig> {
    return invoke<MusiqueConfig>("musique_set_config", { config });
  },

  /// Importe des fichiers audio (multi-sélection) dans medias/musique/.
  async musiqueImporter(): Promise<MusiqueFichier[]> {
    return invoke<MusiqueFichier[]>("musique_importer");
  },

  /// Supprime un fichier de la bibliothèque (par nom de fichier).
  async musiqueSupprimerFichier(nom: string): Promise<void> {
    await invoke("musique_supprimer_fichier", { nom });
  },

  /// Skip la piste courante → la suivante démarre.
  async musiqueSkip(): Promise<void> {
    await invoke("musique_skip");
  },

  /// Pause ou reprend la piste courante côté diffusion.
  async musiquePause(pause: boolean): Promise<void> {
    await invoke("musique_pause", { pause });
  },

  /// Stoppe la lecture ET vide la file (arrêt complet).
  async musiqueStop(): Promise<void> {
    await invoke("musique_stop");
  },

  /// Vide la file d'attente (la piste courante continue).
  async musiqueVider(): Promise<void> {
    await invoke("musique_vider");
  },

  /// Retire une entrée de la file par id.
  async musiqueRetirer(id: string): Promise<void> {
    await invoke("musique_retirer", { id });
  },

  /// Déplace une entrée dans la file (delta = -1 monter, +1 descendre).
  async musiqueDeplacer(id: string, delta: number): Promise<void> {
    await invoke("musique_deplacer", { id, delta });
  },

  /// Test manuel : joue un fichier de la bibliothèque côté diffusion.
  async musiqueTester(src: string): Promise<void> {
    await invoke("musique_tester", { src });
  },

  /// Change le volume de lecture côté diffusion (persisté).
  async musiqueVolume(volume: number): Promise<void> {
    await invoke("musique_volume", { volume });
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

  // ===== Écran d'attente (BRB — overlay plein écran diffusion) =====

  /// État complet (config + runtime) de l'écran d'attente.
  async ecranAttenteEtat(): Promise<EcranAttenteEtat> {
    return invoke<EcranAttenteEtat>("ecran_attente_etat");
  },

  /// Remplace la config + persiste + re-émet la frame WS si l'écran est actif.
  async ecranAttenteSetConfig(config: EcranAttenteConfig): Promise<void> {
    await invoke("ecran_attente_set_config", { config });
  },

  /// Lance l'écran d'attente : chrono + overlay diffusion + routage audio
  /// OBS (non-fatal si OBS absent). Creds OBS des stores (localStorage).
  async ecranAttenteLancer(host: string, port: number, password: string): Promise<void> {
    await invoke("ecran_attente_lancer", { host, port, password });
  },

  /// Bascule pause/reprise du chrono (le média n'est pas affecté).
  async ecranAttentePauseToggle(): Promise<void> {
    await invoke("ecran_attente_pause_toggle");
  },

  /// Arrête l'écran + restaure l'audio bureau OBS (non-fatal).
  async ecranAttenteArreter(host: string, port: number, password: string): Promise<void> {
    await invoke("ecran_attente_arreter", { host, port, password });
  },

  /// Importe un média (image OU vidéo) : dialog « Médias » → validation +
  /// copie (ou remux MKV→MP4) vers medias/. Retourne [rel, kind, nom] ou
  /// null si annulé.
  async ecranAttenteImporterMedia(): Promise<[string, string, string] | null> {
    return invoke<[string, string, string] | null>("ecran_attente_importer_media");
  },

  /// Liste les sources audio de sortie OBS (wasapi_output_capture) pour le
  /// picker de la modale. Connexion one-shot — lève une erreur si OBS absent.
  async ecranAttenteListerSourcesAudio(
    host: string,
    port: number,
    password: string,
  ): Promise<string[]> {
    return invoke<string[]>("ecran_attente_lister_sources_audio", { host, port, password });
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

  // ===== Contrôles en direct Twitch (Helix — section « En direct ») =====
  // Erreur "need_reauth" si 403 (scopes manquants → bandeau de reconnexion).

  /// Infos de la chaîne : titre, game_id/game_name, tags.
  async twitchLiveChannelInfo(): Promise<{
    title: string;
    game_id: string;
    game_name: string;
    tags: string[];
  }> {
    return invoke("twitch_live_channel_info");
  },

  /// Modifier titre/catégorie/tags — PATCH partiel : un champ absent
  /// (undefined) n'est pas envoyé à Helix. tags n'est jamais un tableau vide.
  async twitchLiveMajChannel(opts: {
    title?: string;
    gameId?: string;
    tags?: string[];
  }): Promise<void> {
    await invoke("twitch_live_maj_channel", opts);
  },

  /// Recherche de catégorie (suggestions jeu).
  async twitchLiveSearchCategories(
    query: string
  ): Promise<{ id: string; name: string; box_art_url: string }[]> {
    return invoke("twitch_live_search_categories", { query });
  },

  /// Infos d'un jeu par id (jacquette) : GET /games?id=<id>. Sert à
  /// récupérer box_art_url quand la section s'ouvre avec un game_id déjà
  /// connu — la recherche ci-dessus reste la source au choix. null si
  /// l'id n'existe plus côté Twitch.
  async twitchLiveGameInfo(
    gameId: string
  ): Promise<{ id: string; name: string; box_art_url: string } | null> {
    return invoke("twitch_live_game_info", { gameId });
  },

  /// Recherche de chaîne (suggestions raid).
  async twitchLiveSearchChannels(query: string): Promise<
    {
      id: string;
      login: string;
      display_name: string;
      is_live: boolean;
      thumbnail_url: string;
    }[]
  > {
    return invoke("twitch_live_search_channels", { query });
  },

  /// Créer un marqueur de stream (description optionnelle).
  /// Échoue hors-ligne (message Helix, pas de retry).
  async twitchLiveMarker(description?: string): Promise<void> {
    await invoke("twitch_live_marker", { description });
  },

  /// Lancer une pub — durées Helix uniquement : 30/60/90/120/150/180.
  async twitchLivePub(duree: number): Promise<void> {
    await invoke("twitch_live_pub", { duree });
  },

  /// Lancer un raid vers un login (résolution côté Rust).
  async twitchLiveRaidLancer(login: string): Promise<void> {
    await invoke("twitch_live_raid_lancer", { login });
  },

  /// Annuler le raid en cours.
  async twitchLiveRaidAnnuler(): Promise<void> {
    await invoke("twitch_live_raid_annuler");
  },

  /// Modes de chat actuels (GET /chat/settings).
  async twitchLiveChatSettings(): Promise<{
    emote_mode: boolean;
    follower_mode: boolean;
    follower_mode_duration: number | null;
    slow_mode: boolean;
    slow_mode_wait_time: number | null;
    subscriber_mode: boolean;
  }> {
    return invoke("twitch_live_chat_settings");
  },

  /// Modifier les modes de chat — PATCH partiel : seuls les champs présents
  /// sont envoyés. Les clés internes restent snake_case (format Helix).
  /// Bornes : slow_mode_wait_time 3–120 s, follower_mode_duration 0–129600 min.
  async twitchLiveChatSettingsMaj(settings: {
    emote_mode?: boolean;
    follower_mode?: boolean;
    follower_mode_duration?: number;
    slow_mode?: boolean;
    slow_mode_wait_time?: number;
    subscriber_mode?: boolean;
  }): Promise<void> {
    await invoke("twitch_live_chat_settings_maj", { settings });
  },

  /// Envoyer une annonce colorée (blue|green|orange|purple|primary).
  async twitchLiveAnnonce(message: string, couleur: string): Promise<void> {
    await invoke("twitch_live_annonce", { message, couleur });
  },

  /// Shield Mode actif ?
  async twitchLiveShieldStatus(): Promise<boolean> {
    return invoke<boolean>("twitch_live_shield_status");
  },

  /// Activer/désactiver le Shield Mode (confirmation UI avant activation).
  async twitchLiveShieldSet(actif: boolean): Promise<void> {
    await invoke("twitch_live_shield_set", { actif });
  },

  /// Demandes d'unban en attente (status=pending).
  async twitchLiveUnbanRequests(): Promise<
    {
      id: string;
      user_id: string;
      login: string;
      display_name: string;
      text: string;
      created_at: string;
    }[]
  > {
    return invoke("twitch_live_unban_requests");
  },

  /// Approuver/refuser une demande d'unban.
  async twitchLiveUnbanResoudre(
    unbanRequestId: string,
    approuver: boolean
  ): Promise<void> {
    await invoke("twitch_live_unban_resoudre", { unbanRequestId, approuver });
  },

  /// Autoriser/refuser un message retenu par AutoMod (file EventSub).
  async twitchLiveAutomodResoudre(
    msgId: string,
    autoriser: boolean
  ): Promise<void> {
    await invoke("twitch_live_automod_resoudre", { msgId, autoriser });
  },

  /// Télécharge la jacquette du jeu dans medias/jacquette-<gameId>.jpg
  /// (écrasée à chaque pose). Commande dédiée — gardes côté Rust : hôte
  /// figé static-cdn.jtvnw.net, JPEG par magic bytes, taille plafonnée.
  /// Retourne le chemin relatif "medias/...jpg" à poser sur le widget.
  async twitchLivePoserJacquette(
    boxArtUrl: string,
    gameId: string
  ): Promise<string> {
    return invoke("twitch_live_poser_jacquette", { boxArtUrl, gameId });
  },

  /// Creator Goals Twitch déclarés (GET /goals — objectifs actifs, un par
  /// type max). Relu à chaque appel (zéro persistance). 403 = vieux token
  /// sans channel:read:goals → erreur NeedReauth côté UI (reconnexion).
  async twitchGoalsLister(): Promise<TwitchGoal[]> {
    return invoke<TwitchGoal[]>("twitch_goals_lister");
  },

  // ===== Planning Twitch (schedule Helix — onglet Planning) =====
  // Erreur "need_reauth" si 403 (vieux token sans channel:manage:schedule).

  /// Lire le planning : segments + vacances. startTime = borne basse
  /// RFC3339 optionnelle (début de la vue courante). Relu à chaque appel.
  async twitchPlanningSchedule(startTime?: string): Promise<PlanningResp> {
    return invoke("twitch_planning_schedule", { startTime });
  },

  /// Créer un segment. isRecurring=false tente un ponctuel ; le backend
  /// retombe sur récurrent si Twitch refuse (chaîne non affiliée).
  /// durationMin : 30-1380. Retourne le segment créé.
  /// Le corps interne reste snake_case (format Helix — même convention que
  /// PlanningSegmentPatch).
  async twitchPlanningSegmentCreer(opts: {
    startTime: string;
    timezone: string;
    isRecurring: boolean;
    durationMin: number;
    categoryId?: string;
    title?: string;
  }): Promise<PlanningSegment> {
    return invoke("twitch_planning_segment_creer", {
      params: {
        start_time: opts.startTime,
        timezone: opts.timezone,
        is_recurring: opts.isRecurring,
        duration_min: opts.durationMin,
        category_id: opts.categoryId,
        title: opts.title,
      },
    });
  },

  /// Modifier un segment — PATCH partiel (clés snake_case, voir type).
  /// ⚠️ Récurrent : s'applique à toute la série.
  async twitchPlanningSegmentMaj(
    id: string,
    patch: PlanningSegmentPatch
  ): Promise<void> {
    await invoke("twitch_planning_segment_maj", { id, patch });
  },

  /// Supprimer un segment. ⚠️ Récurrent : supprime toute la série.
  async twitchPlanningSegmentSupprimer(id: string): Promise<void> {
    await invoke("twitch_planning_segment_supprimer", { id });
  },

  /// Définir/désactiver les vacances (dates RFC3339 quand actif).
  async twitchPlanningVacances(opts: {
    actif: boolean;
    debut?: string;
    fin?: string;
    timezone: string;
  }): Promise<void> {
    await invoke("twitch_planning_vacances", opts);
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

  /// Bascule le mode d'affichage : true = squelette de position (défaut),
  /// false = spotlight plein écran (voile noir + carte 85 % + intro 3 s).
  async welcomeSetUtiliserSquelette(actif: boolean): Promise<void> {
    await invoke("welcome_set_utiliser_squelette", { actif });
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

  // ===== Présentation streamer (portage « streamers favoris ») =====

  /// État courant : favoris persistés + présentation en cours.
  async presentationEtat(): Promise<PresentationEtat> {
    return invoke<PresentationEtat>("presentation_etat");
  },

  /// Ajoute/remplace un favori (upsert par login) + persiste.
  async presentationFavoriSauver(
    favori: PresentationFavoriConfig
  ): Promise<void> {
    await invoke("presentation_favori_sauver", { favori });
  },

  /// Retire un favori + persiste.
  async presentationFavoriSupprimer(login: string): Promise<void> {
    await invoke("presentation_favori_supprimer", { login });
  },

  /// Affecte/retire une piste audio d'un favori
  /// ("voix"|"musique", src = "medias/<uuid>.<ext>" ou null).
  async presentationFavoriSetSon(
    login: string,
    piste: string,
    src: string | null
  ): Promise<void> {
    await invoke("presentation_favori_set_son", { login, piste, src });
  },

  /// Change le mode d'un favori ("theatrical"|"card") — bascule à chaud côté
  /// diffusion si c'est le streamer actuellement présenté.
  async presentationFavoriSetMode(
    login: string,
    mode: string
  ): Promise<void> {
    await invoke("presentation_favori_set_mode", { login, mode });
  },

  /// Recherche de chaînes Twitch (GET /search/channels).
  async presentationRechercher(
    query: string
  ): Promise<PresentationSearchResult[]> {
    return invoke<PresentationSearchResult[]>("presentation_rechercher", {
      query,
    });
  },

  /// Lance la présentation d'un streamer : fetch Helix complet + frame
  /// presentation-show. mode = "theatrical"|"card" pour un non-favori
  /// (le mode persisté d'un favori gagne toujours).
  async presentationLancer(
    userId: string,
    mode?: string
  ): Promise<PresentationDonnees> {
    return invoke<PresentationDonnees>("presentation_lancer", {
      userId,
      mode,
    });
  },

  /// Ferme la présentation côté diffusion.
  async presentationFermer(): Promise<void> {
    await invoke("presentation_fermer");
  },

  /// Joue un clip sur la diffusion (résolution MP4 signée côté Rust).
  async presentationClipJouer(slug: string): Promise<void> {
    await invoke("presentation_clip_jouer", { slug });
  },

  /// Arrête le clip en cours côté diffusion (retour à la carte).
  async presentationClipStopper(): Promise<void> {
    await invoke("presentation_clip_stopper");
  },

  /// Joue une piste audio ("voix"|"musique") sur la diffusion.
  /// src = "medias/<uuid>.<ext>" (issu d'importSon).
  async presentationAudioJouer(
    piste: string,
    src: string
  ): Promise<void> {
    await invoke("presentation_audio_jouer", { piste, src });
  },

  /// Stoppe une piste audio ("voix"|"musique"|"toutes") sur la diffusion.
  async presentationAudioStopper(piste: string): Promise<void> {
    await invoke("presentation_audio_stopper", { piste });
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
    position?: "bas" | "haut",
    police?: string
  ): Promise<void> {
    await invoke("bandeau_set_config", { actif, dureeMs, position, police });
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

  /// Clic sur une ligne de split : arme l'index si le timer est arrêté
  /// (le prochain start ASL/manuel part de là), sinon saute à l'index en
  /// retry de segment (tous les temps de la tentative effacés).
  async speedrunAllerSplit(index: number): Promise<void> {
    await invoke("speedrun_aller_split", { index });
  },

  // ===== Diagnostic consommation (modale Diagnostic — polling 1 Hz) =====

  /// État courant du diagnostic : historique roulant (≤300 pts, ~5 min) +
  /// nb cœurs. À poller UNIQUEMENT quand la modale Diagnostic est ouverte.
  async consoEtat(): Promise<ConsoEtat> {
    return invoke<ConsoEtat>("conso_etat");
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
  /// true = squelette de position (défaut), false = spotlight plein écran.
  utiliser_squelette: boolean;
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
  /// Catégorie du jeu au moment du clip (Helix game_id, "" si absent).
  game_id: string;
}

// ===== Types présentation streamer (mirroir des structs Rust) =====

/// Favori persisté (presentation_favoris.json) : streamer présentable +
/// ses réglages (2 pistes audio medias/… + mode d'affichage).
export interface PresentationFavoriConfig {
  user_id: string;
  /// Login lowercase (clé de la map côté Rust).
  login: string;
  display_name: string;
  /// Avatar Twitch (rafraîchi à chaque lancer — cache).
  avatar: string;
  son_voix?: string | null;
  son_musique?: string | null;
  /// "theatrical" (défaut) | "card".
  mode: string;
}

/// Jeu récent (déduit des game_id des clips — mirroir CategoryEntry Helix).
export interface PresentationJeuRecent {
  id: string;
  name: string;
  box_art_url: string;
}

/// Données complètes d'une présentation : payload de la frame WS
/// presentation-show ET retour de presentation_lancer (contrôles live).
export interface PresentationDonnees {
  user_id: string;
  login: string;
  display_name: string;
  avatar: string;
  bio: string;
  /// Bannière/entête de chaîne (offline_image_url Helix, "" si absente).
  banniere: string;
  /// "partner" | "affiliate" | "".
  broadcaster_type: string;
  en_live: boolean;
  viewers_live?: number | null;
  titre_live: string;
  jeu_courant: string;
  tags: string[];
  jeux_recents: PresentationJeuRecent[];
  clips: WelcomeClipInfo[];
  /// Mode d'affichage effectif : "theatrical" | "card".
  mode: string;
}

/// État exposé (commande presentation_etat + event "presentation:etat").
export interface PresentationEtat {
  favoris: PresentationFavoriConfig[];
  visible: boolean;
  courant: PresentationDonnees | null;
}

/// Résultat de recherche de chaîne (mirroir ChannelSearchEntry Helix).
export interface PresentationSearchResult {
  id: string;
  login: string;
  display_name: string;
  is_live: boolean;
  thumbnail_url: string;
}

// ===== Types bandeau premier message (mirroir des structs Rust) =====

export interface BandeauEtat {
  actif: boolean;
  duree_ms: number;
  position: "bas" | "haut";
  /// Id de police Google embarquée ("BebasNeue"…) — "" = police système.
  police: string;
}
