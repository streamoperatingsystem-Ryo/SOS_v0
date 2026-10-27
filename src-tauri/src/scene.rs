use serde::{Deserialize, Serialize};

const DEFAULT_CANVAS_W: u32 = 1920;
const DEFAULT_CANVAS_H: u32 = 1080;
const DEFAULT_MEDIA_FIT: &str = "ajuster";
const DEFAULT_KIND: &str = "image";
const DEFAULT_MEDIA_ZOOM: f64 = 1.0;
const DEFAULT_MEDIA_ROT: f64 = 0.0;
const DEFAULT_MEDIA_OFFSET: f64 = 0.0;
const DEFAULT_BG_FIT: &str = "remplir";
const DEFAULT_BG_KIND: &str = "image";
const DEFAULT_BG_ZOOM: f64 = 1.0;
const DEFAULT_BG_ROT: f64 = 0.0;
const DEFAULT_BG_OFFSET: f64 = 0.0;

fn default_canvas_w() -> u32 {
    DEFAULT_CANVAS_W
}

fn default_canvas_h() -> u32 {
    DEFAULT_CANVAS_H
}

fn default_media_fit() -> String {
    DEFAULT_MEDIA_FIT.to_string()
}

fn default_kind() -> String {
    DEFAULT_KIND.to_string()
}

fn default_media_zoom() -> f64 {
    DEFAULT_MEDIA_ZOOM
}

fn default_media_rot() -> f64 {
    DEFAULT_MEDIA_ROT
}

fn default_media_offset() -> f64 {
    DEFAULT_MEDIA_OFFSET
}

fn default_bg_fit() -> String {
    DEFAULT_BG_FIT.to_string()
}

fn default_bg_kind() -> String {
    DEFAULT_BG_KIND.to_string()
}

fn default_bg_zoom() -> f64 {
    DEFAULT_BG_ZOOM
}

fn default_bg_rot() -> f64 {
    DEFAULT_BG_ROT
}

fn default_bg_offset() -> f64 {
    DEFAULT_BG_OFFSET
}

fn default_true() -> bool {
    true
}

fn default_zero_f64() -> f64 {
    0.0
}

fn default_cadre_style() -> String {
    "carre".to_string()
}

fn default_cadre_stroke_width() -> f64 {
    4.0
}

fn default_cadre_couleur() -> String {
    "#ffffff".to_string()
}

fn default_cadre_couleur_fin() -> String {
    "#000000".to_string()
}

fn default_cadre_gradient_angle() -> f64 {
    135.0
}

/// Configuration d'un cadre SVG (widget ou bord canvas).
/// NB : champs camelCase VOLONTAIRES — contrat JSON direct avec le frontend
/// (mêmes noms que scene.ts). D'où le allow(non_snake_case).
/// PartialEq requis par scenes::scene_vierge (le défaut auto posé à la
/// naissance ne compte pas comme contenu utilisateur).
#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CadreConfig {
    #[serde(default = "default_cadre_style")]
    pub style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variante: Option<String>,
    #[serde(default = "default_cadre_stroke_width")]
    pub strokeWidth: f64,
    #[serde(default = "default_cadre_couleur")]
    pub couleur: String,
    #[serde(default = "default_cadre_couleur_fin")]
    pub couleurFin: String,
    #[serde(default = "default_cadre_gradient_angle")]
    pub gradientAngle: f64,
    #[serde(default)]
    pub actif: bool,
    /// Le cadre s'applique aussi au bord du canvas (false = widgets seuls).
    /// Défaut true : rétro-compat — les scènes existantes gardaient le bord.
    #[serde(default = "default_true")]
    pub bordActif: bool,
    /// Émet les éléments SMIL animés du cadre (toggle « Animer » de
    /// CadresModal). Défaut false : vieilles scènes = rendu statique.
    #[serde(default)]
    pub animer: bool,
}

impl Default for CadreConfig {
    fn default() -> Self {
        Self {
            style: default_cadre_style(),
            variante: None,
            strokeWidth: default_cadre_stroke_width(),
            couleur: default_cadre_couleur(),
            couleurFin: default_cadre_couleur_fin(),
            gradientAngle: default_cadre_gradient_angle(),
            actif: false,
            bordActif: true,
            animer: false,
        }
    }
}

impl CadreConfig {
    /// Cadre posé par défaut sur TOUTE scène neuve (Scene::new — nouvelle
    /// sauvegarde, premier boot, scène illisible) : MGStyle noir + accent
    /// violet marque StreamOS, épaisseur 50, actif, bord canvas inclus.
    /// ⚠️ MIROIR de CADRE_WIDGET_DEFAUT (scene.ts) — garder en sync.
    /// scene_vierge() traite CE défaut comme « pas de contenu utilisateur »
    /// (l'onboarding preset peut encore remplacer une scène neuve).
    pub fn defaut_scene() -> Self {
        Self {
            style: "mgstyle".to_string(),
            variante: None,
            strokeWidth: 50.0,
            couleur: "#000000".to_string(),
            couleurFin: "#8b5cf6".to_string(),
            gradientAngle: 135.0,
            actif: true,
            bordActif: true,
            animer: false,
        }
    }
}

/// Un point de morphing (bulge/pinch radial) sur un média.
/// Coordonnées NORMALISÉES (0-1) relatives au widget/canvas → survivent au
/// resize et au reload. Le rendu (WebGL) applique la chaîne de morphs dans
/// l'ordre — non destructif : le média source n'est jamais modifié.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MorphPoint {
    /// Centre X normalisé (0-1 de la largeur visible).
    pub x: f64,
    /// Centre Y normalisé (0-1 de la hauteur visible).
    pub y: f64,
    /// Rayon normalisé (fraction de la largeur visible).
    pub rayon: f64,
    /// Intensité en % (0-200).
    pub intensite: f64,
    /// "agrandir" (bulge) | "retrecir" (pinch).
    pub mode: String,
}

fn default_morph_intensite() -> f64 {
    35.0
}
fn default_morph_mode() -> String {
    "agrandir".to_string()
}

impl Default for MorphPoint {
    fn default() -> Self {
        Self {
            x: 0.5,
            y: 0.5,
            rayon: 0.15,
            intensite: default_morph_intensite(),
            mode: default_morph_mode(),
        }
    }
}

/// Une slide du carrousel d'images (widget média ou fond de scène).
/// `rel` = chemin relatif "medias/<uuid>.<ext>" servi par :4321 ;
/// `nom` = nom original du fichier (affichage liste, même rôle que mediaNom).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSlide {
    pub rel: String,
    pub nom: String,
}

/// Rect sauvegardé d'un widget avant sa mise « en plein écran » (menu
/// contextuel → « Mettre en plein écran »). Some = le widget EST en plein
/// écran et le rect est sa position/taille de restauration ; None = état
/// normal. Sérialisé dans la scène comme tout champ Widget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PleinEcranRect {
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
}

/// NB : champs camelCase VOLONTAIRES — contrat JSON direct avec le frontend
/// (mêmes noms que scene.ts). D'où le allow(non_snake_case).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    #[serde(rename = "type")]
    pub widget_type: String,
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
    pub z: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    #[serde(default)]
    pub rotateX: f64,
    #[serde(default)]
    pub rotateY: f64,
    #[serde(default = "default_media_fit")]
    pub mediaFit: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Nom original du fichier média importé (sans chemin, avec extension).
    /// None pour les widgets sans média. Affiché dans l'en-tête de la carte
    /// d'édition pour identifier le widget au lieu de l'ID technique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediaNom: Option<String>,
    #[serde(default = "default_media_zoom")]
    pub mediaZoom: f64,
    #[serde(default = "default_media_rot")]
    pub mediaRot: f64,
    #[serde(default = "default_media_offset")]
    pub mediaOffsetX: f64,
    #[serde(default = "default_media_offset")]
    pub mediaOffsetY: f64,
    /// Miroir du média : retournement horizontal/vertical du contenu
    /// (scale ±1 combiné au zoom dans la chaîne transform du rendu).
    /// false = pas de miroir (défaut, rétro-compat).
    #[serde(default)]
    pub mediaMiroirH: bool,
    #[serde(default)]
    pub mediaMiroirV: bool,
    /// Effets visuels du média (0 = neutre) : luminosité (-100..100),
    /// contraste (-100..100), teinte (0..360 deg), flou (0..20 px),
    /// pixelisation (0..50, 0 = désactivé).
    #[serde(default = "default_zero_f64")]
    pub mediaLum: f64,
    #[serde(default = "default_zero_f64")]
    pub mediaContraste: f64,
    #[serde(default = "default_zero_f64")]
    pub mediaTeinte: f64,
    #[serde(default = "default_zero_f64")]
    pub mediaFlou: f64,
    #[serde(default = "default_zero_f64")]
    pub mediaPixel: f64,
    #[serde(default = "default_true")]
    pub mediaPaused: bool,
    #[serde(default = "default_zero_f64")]
    pub mediaTime: f64,
    /// Boucle A–B (secondes) sur la vidéo : les deux définis avec mediaLoopB >
    /// mediaLoopA = la diffusion boucle sur le segment [A, B[ (reseek à A dès
    /// que currentTime ≥ B) au lieu de boucler sur toute la vidéo.
    /// None = boucle native complète (comportement historique).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediaLoopA: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediaLoopB: Option<f64>,
    /// Carrousel d'images : liste non vide = ACTIF et prioritaire sur `media`
    /// (le média unique reste stocké, il redevient visible si la liste est
    /// vidée). Images uniquement. Intervalle = mediaSlidesSec (défaut 5 s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediaSlides: Option<Vec<MediaSlide>>,
    /// Intervalle du carrousel en secondes. None → 5 s (défaut diffusion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mediaSlidesSec: Option<f64>,
    #[serde(default)]
    pub trou: bool,
    /// Opt-out du cadre SVG de scène pour CE widget (widgets créés par drop
    /// bibliothèque → transformerEnWidget). false/absent = cadre de scène
    /// appliqué (comportement historique, toutes anciennes scènes).
    #[serde(default)]
    pub sansCadre: bool,
    /// Nom de la source OBS liée au trou (None = pas de source OBS).
    /// Convention : "SOS-Trou-<id8>". Quand Some → commitScene sync la
    /// transform OBS (position + taille = widget x/y/largeur/hauteur).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obsSource: Option<String>,
    /// Filtre chat pour les widgets type "chat" : "unifie" (défaut) ou
    /// une plateforme ("twitch", "youtube", "kick", "tiktok").
    /// None pour les widgets non-chat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatFiltre: Option<String>,
    /// Taille de police (px) pour les widgets type "chat". Défaut 16.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taillePolice: Option<u32>,
    /// Mise en forme des messages pour les widgets type "chat" :
    /// "compact" (défaut — messages joints, zébrage) ou "bulles" (messages
    /// détachés, chacun avec le cadre SVG de scène comme un widget).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatStyle: Option<String>,
    /// Identifiant du device caméra (getUserMedia deviceId) pour les widgets
    /// type "camera". None = device par défaut du système.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cameraDeviceId: Option<String>,
    /// Forme DÉDIÉE du widget caméra : "cercle" ou "carre" découplée du
    /// cadre de scène ; None = suit le cadre de scène (comportement
    /// historique). Le cercle est inscrit dans le rect (diamètre min(w,h),
    /// centré) — trou diffusion et masque OBS partagent le même polygone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cameraForme: Option<String>,
    /// Anneau/bordure autour de la caméra quand cameraForme est "cercle"
    /// ou "carre" (couleurs du cadre de scène). false = découpe nue.
    #[serde(default)]
    pub cameraBord: bool,
    /// Incrustation « fond vert virtuel » (filtre plugin OBS
    /// background_removal sur SOS-Caméra, 100% local). true = la plaque
    /// noire SOS-Caméra-Fond est masquée et le filtre actif.
    #[serde(default)]
    pub cameraFondVert: bool,
    /// Caméra masquée côté diffusion (bouton œil au survol du widget
    /// dashboard) : les items OBS SOS-Caméra + SOS-Caméra-Fond sont
    /// désactivés ET diffusion.html ignore le widget (pas de trou
    /// destination-out, pas de cadre SVG — la zone est repeinte par le
    /// fond de scène). Persisté : ré-appliqué par scene_sync_captures.
    #[serde(default)]
    pub cameraCachee: bool,
    /// Mode d'affichage des widgets type "input-viewer" : "clavier" (défaut),
    /// "souris", "numpad" ou "manette".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputMode: Option<String>,
    /// Layout clavier ("azerty" défaut | "qwerty") pour les widgets
    /// type "input-viewer" en mode clavier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputLayout: Option<String>,
    /// Skin manette ("xbox" défaut | "ps" | "8bitdo") pour les widgets
    /// type "input-viewer" en mode manette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputSkin: Option<String>,
    /// Couleur des touches pressées (input-viewer). Défaut #8b5cf6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputCouleur: Option<String>,
    /// Mapping des touches manette (input-viewer mode manette). Permet de
    /// remapper chaque bouton affiché vers un index physique différent + 
    /// inverser les axes X/Y du D-pad (bug 8BitDo etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputMapping: Option<InputMapping>,
    /// Chemin du fichier .asl (Auto Split Language) pour les widgets
    /// type "speedrun". None = pas de script ASL chargé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speedrunCheminAsl: Option<String>,
    /// Chemin du fichier .lss (LiveSplit Splits) pour les widgets
    /// type "speedrun". None = pas de fichier LSS chargé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speedrunCheminLss: Option<String>,
    /// Settings ASL (start/split/reset activés) pour les widgets
    /// type "speedrun". None = defaults {start:true, split:true, reset:true}.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speedrunSettings: Option<crate::config::SpeedrunSettings>,
    /// Police Google Font pour les widgets type "speedrun" (id police, voir
    /// fonts.ts). None = "Rajdhani" (défaut). Reprise de la liste POLICES_TITRE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srPolice: Option<String>,
    /// Taille de police de base (px) pour les widgets type "speedrun". None = 16.
    /// Toutes les tailles internes du splitter sont en em → suit cette base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srTaillePolice: Option<u32>,
    /// Disposition du splitter speedrun : "auto" | "colonne" | "ligne".
    /// "auto"/None = splits en ligne si la hauteur passe sous ~4×srTaillePolice
    /// (façon LiveSplit horizontal), colonne sinon. "colonne"/"ligne" forcent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srLayout: Option<String>,
    /// ===== Options de layout du splitter (modale SpeedrunLayout, 2026-10-13)
    /// ===== Toutes Option<> — absent = défaut actuel (rendu identique).
    /// Éléments visibles : titre du jeu / catégorie / pastille statut /
    /// timer / pastille de phase / compteur « x / total » / barre de
    /// progression. None = visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirTitre: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirCategorie: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirStatut: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirTimer: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirPhase: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirCompteur: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirBarre: Option<bool>,
    /// Colonne « temps » des lignes de split : "cumul" (défaut — split time
    /// façon LiveSplit) | "segment" (durée de segment — comportement
    /// d'avant 2026-10-13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srModeTemps: Option<String>,
    /// Colonne « delta » : "cumul" (défaut — run vs PB cumulé) | "segment"
    /// (temps gagné/perdu sur CE segment vs sa durée PB — SegmentDelta
    /// LiveSplit) | "off" (colonne masquée).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srModeDelta: Option<String>,
    /// Précision du delta : "dixieme" (défaut) | "centieme" | "seconde".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srDeltaPrecision: Option<String>,
    /// Drop des décimales du delta au-delà d'1 min (LiveSplit
    /// with_decimal_dropping). None = true (défaut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srDeltaDrop: Option<bool>,
    /// Précision du timer principal : "centieme" (défaut) | "dixieme" |
    /// "seconde".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srTimerPrecision: Option<String>,
    /// Méthode de temps du timer : "auto" (défaut — game time si dispo,
    /// sinon real) | "real" | "game".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srTimerMethode: Option<String>,
    /// Couleur custom du timer (hex). None/vide = --sr-grad-from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srCouleurTimer: Option<String>,
    /// Bordures colorées des lignes de split. None = visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srSegBordure: Option<bool>,
    /// Épaisseur des bordures de split (px). None = 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srSegBordureEpaisseur: Option<u32>,
    /// Rayon des bordures de split (px). None = var(--rayon-petit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srSegBordureRayon: Option<u32>,
    /// Couleurs custom des bordures/delta (hex). None/vide = variables
    /// sémantiques (--message-ok-color / --message-error-color) ou
    /// --sr-grad-from pour l'actif.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srCouleurAhead: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srCouleurBehind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srCouleurActif: Option<String>,
    /// Nombre de lignes de split visibles (façon visual_split_count
    /// LiveSplit). None/0 = toutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srNbLignes: Option<u32>,
    /// Scroll auto vers le split actif. None = activé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srScrollAuto: Option<bool>,
    /// Hauteur des lignes de split (px). None/0 = auto (padding).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srHauteurLigne: Option<u32>,
    /// ===== Points de validation (bandeau par-dessus le splitter) =====
    /// Afficher les points numérotés de validation des splits.
    /// None = visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srVoirPoints: Option<bool>,
    /// Durée du gros pop à l'activation d'un point (secondes). None = 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srPointsDureePop: Option<u64>,
    /// Son joué côté diffusion à l'activation d'un point
    /// ("medias/<uuid>.<ext>"). None = silence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srPointsSon: Option<String>,
    /// Volume du son des points (0.0-1.0). None = 1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srPointsSonVolume: Option<f32>,
    /// ===== Métronome du splitter (dashboard SEUL, 2026-10-19) =====
    /// Battement BPM joué pendant la run (start ASL/manuel → il joue ;
    /// pause/reset/fin → il s'arrête). Interprété côté dashboard par le
    /// store metronome.ts (Web Audio) — JAMAIS envoyé à la diffusion.
    /// Activer/désactiver. None = désactivé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srMetronomeActif: Option<bool>,
    /// Tempo du battement (1-400). None = 120.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srMetronomeBpm: Option<u32>,
    /// Volume du battement (0.0-1.0). None = 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub srMetronomeVolume: Option<f32>,
    /// Plateforme suivie par un widget type "goal" :
    /// "twitch" | "youtube" | "kick" | "tiktok". None = non configuré.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goalPlateforme: Option<String>,
    /// Métrique suivie par un widget type "goal" : "followers" | "subs" |
    /// "sub_points" | "abonnes" | "viewers". Les métriques indisponibles pour
    /// la plateforme choisie sont ignorées (affichage « N/A » en diffusion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goalMetrique: Option<String>,
    /// Valeur cible du widget type "goal". None = pas de cible (compteur
    /// seul, jamais de frame goal-atteint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goalCible: Option<u64>,
    /// Id du Creator Goal Twitch LIÉ (objectif déclaré dans le Creator
    /// Dashboard, lu via GET /goals — scope channel:read:goals). Some =
    /// mode lié : actuel/cible/description viennent des frames "goal-twitch"
    /// (EventSub channel.goal.* + poll viewers.rs), goalMetrique/goalCible
    /// sont ignorés. None = mode manuel (compteurs communaute/viewers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goalId: Option<String>,
    /// Mode d'affichage des widgets type "musique" : "player" (défaut —
    /// LCD marquee + temps + barre de progression + équalizer) ou "eq"
    /// (bandes de spectre seules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musiqueMode: Option<String>,
    /// Chaîne de morphs (bulge/pinch) appliquée au média, dans l'ordre.
    /// None/vide = rendu natif (img/video), zéro coût GL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morphs: Option<Vec<MorphPoint>>,
    /// Texte du titre affiché au-dessus du widget. None/vide = aucun titre à
    /// cette position. Jusqu'à 4 titres simultanés (haut/bas/gauche/droite),
    /// le style est partagé (titrePolice, titreTaille, …) et le texte est
    /// rendu avec un gradient reprenant celui du cadre SVG actif de la scène.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreHaut: Option<String>,
    /// Texte du titre affiché en dessous du widget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreBas: Option<String>,
    /// Texte du titre affiché à gauche du widget (rendu vertical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreGauche: Option<String>,
    /// Texte du titre affiché à droite du widget (rendu vertical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreDroite: Option<String>,
    /// LEGACY — ancien titre à position unique (« dessus »/« dessous »).
    /// Conservé uniquement pour désérialiser les vieilles scènes : migré vers
    /// titreHaut/titreBas par `migrer_titres()` à la lecture, jamais écrit
    /// par l'UI. None en mémoire post-migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titre: Option<String>,
    /// LEGACY — position du titre legacy, source de migration uniquement.
    /// Voir `titre`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titrePosition: Option<String>,
    /// Couleur de début du dégradé du texte. None → dégradé repris du cadre
    /// SVG actif (comportement automatique par défaut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreCouleur: Option<String>,
    /// Couleur de fin du dégradé du texte. None → couleur unie (reprend
    /// titreCouleur) ou fin du dégradé du cadre si titreCouleur absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreCouleurFin: Option<String>,
    /// Identifiant de la police Google Font (ex: "BebasNeue"). Voir fonts.ts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titrePolice: Option<String>,
    /// Taille du titre en pixels (12-72). Défaut 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreTaille: Option<u32>,
    /// Titre en gras. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreGras: Option<bool>,
    /// Titre en italique. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreItalique: Option<bool>,
    /// Titre souligné. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreSouligne: Option<bool>,
    /// Espacement des lettres en px (-2 à 20). Défaut 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreEspacement: Option<f64>,
    /// Couleur du contour des lettres du titre (hex, ex: "#ffffff"). Défaut blanc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreContourCouleur: Option<String>,
    /// Épaisseur du contour des lettres en px (0-6 ; 0 = aucun contour). Défaut 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titreContourEpaisseur: Option<u32>,
    /// Rect de restauration du mode « plein écran » (clic droit →
    /// « Mettre/Enlever le plein écran »). Some = plein écran actif, le rect
    /// est restauré à la désactivation ; None = état normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pleinEcran: Option<PleinEcranRect>,
}

/// Mapping des touches manette pour un widget input-viewer.
/// `buttons` : tableau de 16 entrées, index = bouton affiché, valeur = index
/// physique lu par gilrs/XInput. Défaut = identité [0,1,...,15].
/// `dpadInvertY`/`dpadInvertX` : inverser l'axe Y/X du D-pad quand la manette
/// reporte le D-pad sur les axes (8BitDo rétro en D-Input).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[allow(non_snake_case)]
pub struct InputMapping {
    pub buttons: Vec<u32>,
    pub dpadInvertY: bool,
    pub dpadInvertX: bool,
}

/// Ligne d'un module « Objectifs du stream » : soit une métrique mesurée
/// (`plateforme` + `metrique`, mêmes valeurs que le widget goal — frames
/// `communaute`/`viewers`), soit un Creator Goal Twitch lié (`goalId`,
/// frames `goal-twitch`). `cible` = valeur cible du mode manuel (None =
/// compteur seul). `label` = libellé affiché (None = libellé auto de la
/// métrique côté rendu).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LigneObjectif {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plateforme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrique: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cible: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goalId: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Un module HUD de la scène (horloge / musique en cours / objectifs du
/// stream). Couche d'affichage SÉPARÉE des widgets : pas de cadre de scène
/// imposé, pas de snap, pas de titres — un rectangle libre en px canvas
/// + options de mise en forme. Struct UNIQUE pour les trois modules.
///
/// Les options spécifiques sont optionnelles et ignorées par les autres
/// modules → un seul patch générique côté frontend.
/// NB : champs camelCase VOLONTAIRES — contrat JSON direct avec scene.ts.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleHud {
    /// Module affiché (dashboard + diffusion). false = configuré mais caché.
    #[serde(default)]
    pub actif: bool,
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
    /// Identifiant de police Google Font (fonts.ts). None = police système.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub police: Option<String>,
    /// Taille de police de base en px. None = dérivée de la hauteur.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taille: Option<u32>,
    /// Cadre SVG de la scène dessiné autour du module. None/false = aucun
    /// (rendu minimaliste par défaut).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadre: Option<bool>,
    /// Opacité du fond translucide (0–1). None = 0.45.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacite: Option<f64>,
    /// Horloge : afficher la date à gauche du trait. None = true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afficherDate: Option<bool>,
    /// Horloge : afficher les secondes. None = false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondes: Option<bool>,
    /// Horloge : format 12 h (AM/PM). None = false (24 h).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format12h: Option<bool>,
    /// Musique : afficher la pastille temps/progression. None = true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub afficherTemps: Option<bool>,
    /// Objectifs : titre du bloc. None = libellé par défaut côté rendu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titre: Option<String>,
    /// Objectifs : lignes affichées, dans l'ordre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lignes: Option<Vec<LigneObjectif>>,
    /// Équaliseur : inclure le micro (périphérique d'entrée par défaut)
    /// dans le spectre en plus de la sortie (loopback). None = true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eqMicro: Option<bool>,
    /// Équaliseur : bulles montantes au sommet des barres. None = true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eqBulles: Option<bool>,
    /// Équaliseur : couleur primaire des barres (hex #rrggbb, extrémité
    /// BASSE du dégradé). None = résolution automatique (couleurs du cadre
    /// de scène, garde de lisibilité → défaut si trop sombres).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eqCouleur: Option<String>,
    /// Équaliseur : couleur de fin du dégradé (extrémité HAUTE). None =
    /// dérivée de eqCouleur (assombrie 25 %) si eqCouleur est posé, sinon
    /// résolution automatique.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eqCouleurFin: Option<String>,
    /// Spots : multiplicateur de vitesse d'animation (balayage des
    /// faisceaux + cycle des couleurs). None = 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotsVitesse: Option<f64>,
    /// Spots : pulser l'intensité sur le spectre audio (frames
    /// `eq-spectrum` — nécessite le module équaliseur actif pour que la
    /// capture tourne ; sinon balayage d'ambiance normal). None = true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotsReagir: Option<bool>,
}

/// Modules HUD d'une scène — un emplacement fixe par module (singletons).
/// None = module jamais configuré sur cette scène.
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ScenesModules {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horloge: Option<ModuleHud>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musique: Option<ModuleHud>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objectifs: Option<ModuleHud>,
    /// Équaliseur audio plein écran : couche dédiée `#eq-root` en diffusion
    /// (au-dessus du fond, SOUS les widgets — pas #modules-root z 9000).
    /// Spectre calculé côté Rust (equaliseur.rs, capture WASAPI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equaliseur: Option<ModuleHud>,
    /// Spots lumineux : deux faisceaux « moving head » animés sur le fond
    /// (couche canvas `#spots-canvas` dans #bg, SOUS les widgets, trous
    /// évidés comme le fond — la lumière ne passe jamais sur les widgets).
    /// x/y/largeur/hauteur inutilisés : effet plein canvas ; `opacite`
    /// sert d'intensité lumineuse. Zéro state Rust — rendu 100 % diffusion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spots: Option<ModuleHud>,
}

/// Scène unique (source de vérité). canvasW/canvasH = résolution OBS lue
/// au connect (fallback 1920×1080 si GetVideoSettings échoue ou config absente).
/// Les widgets existants ne sont JAMAIS rescalés quand ces dims changent.
/// Fond de scène : calque sous les widgets (bgMedia = chemin relatif medias/...).
/// NB : champs camelCase VOLONTAIRES — contrat JSON direct avec le frontend
/// (mêmes noms que scene.ts). D'où le allow(non_snake_case).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub widgets: Vec<Widget>,
    #[serde(default = "default_canvas_w")]
    pub canvasW: u32,
    #[serde(default = "default_canvas_h")]
    pub canvasH: u32,
    #[serde(default)]
    pub bgMedia: String,
    /// Carrousel d'images du fond : liste non vide = ACTIF et prioritaire
    /// sur bgMedia (qui reste stocké). bgObsSource reste prioritaire sur
    /// tout (page transparente). Images uniquement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgSlides: Option<Vec<MediaSlide>>,
    /// Intervalle du carrousel de fond en secondes. None → 5 s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgSlidesSec: Option<f64>,
    #[serde(default = "default_bg_kind")]
    pub bgKind: String,
    #[serde(default = "default_bg_fit")]
    pub bgFit: String,
    #[serde(default = "default_bg_zoom")]
    pub bgZoom: f64,
    #[serde(default = "default_bg_rot")]
    pub bgRot: f64,
    /// Inclinaison 3D du fond (gizmo, ±90° max) : perspective(800px)
    /// rotateX/rotateY appliquée sur l'élément #bg en diffusion et sur
    /// .bg-layer en dashboard. 0 = fond plat. Distinct de bgRot (rotation
    /// 2D du média dans le canvas).
    #[serde(default = "default_zero_f64")]
    pub bgRotX: f64,
    #[serde(default = "default_zero_f64")]
    pub bgRotY: f64,
    #[serde(default = "default_bg_offset")]
    pub bgOffsetX: f64,
    #[serde(default = "default_bg_offset")]
    pub bgOffsetY: f64,
    /// Miroir du fond : retournement horizontal/vertical du média de fond
    /// (scale ±1 combiné au zoom dans le rendu). false = pas de miroir.
    #[serde(default)]
    pub bgMiroirH: bool,
    #[serde(default)]
    pub bgMiroirV: bool,
    /// Effets visuels du fond (0 = neutre) : luminosité (-100..100),
    /// contraste (-100..100), teinte (0..360 deg), flou (0..20 px),
    /// pixelisation (0..50, 0 = désactivé).
    #[serde(default = "default_zero_f64")]
    pub bgLum: f64,
    #[serde(default = "default_zero_f64")]
    pub bgContraste: f64,
    #[serde(default = "default_zero_f64")]
    pub bgTeinte: f64,
    #[serde(default = "default_zero_f64")]
    pub bgFlou: f64,
    #[serde(default = "default_zero_f64")]
    pub bgPixel: f64,
    #[serde(default = "default_true")]
    pub bgPaused: bool,
    #[serde(default = "default_zero_f64")]
    pub bgTime: f64,
    /// Boucle A–B (secondes) sur la vidéo de fond — même règle que
    /// mediaLoopA/mediaLoopB des widgets. None = boucle native complète.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgLoopA: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgLoopB: Option<f64>,
    /// Nom de la source OBS de capture liée au FOND (fenêtre/jeu plein canvas,
    /// ex: "SOS-Fond-ab12cd34" — une par scène). None = fond normal (bgMedia
    /// ou transparent). Présence = PRIORITAIRE sur bgMedia : la diffusion ne
    /// peint PAS le fond (page transparente → la capture en bas de pile OBS
    /// apparaît). Retirer la capture restaure le média stocké.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgObsSource: Option<String>,
    /// Cadre unifié de la scène : appliqué à CHAQUE widget (clip-path +
    /// overlay SVG) ET au bord extérieur du canvas. None = aucun cadre.
    /// (Un seul système depuis 2026-09-26 — avant, le bord avait son propre
    /// cadre `cadreApp`.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadreWidget: Option<CadreConfig>,
    /// LEGACY — ancien « cadre du bord de l'application », remplacé par le
    /// cadre unifié `cadreWidget`. Conservé uniquement pour désérialiser les
    /// vieilles scènes : migré vers cadreWidget par `migrer_cadres()` à la
    /// lecture, jamais écrit par l'UI. None en mémoire post-migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadreApp: Option<CadreConfig>,
    /// Chaîne de morphs appliquée au FOND de scène (bulge/pinch), dans l'ordre.
    /// Coordonnées normalisées (0-1) du canvas. None/vide = rendu natif.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgMorphs: Option<Vec<MorphPoint>>,
    /// Texte du titre affiché en haut à l'intérieur du fond de l'application.
    /// None/vide = aucun titre à cette position. Jusqu'à 4 titres simultanés
    /// (haut/bas/gauche/droite — côtés en texte vertical), style partagé
    /// (bgTitrePolice, bgTitreTaille, …) et gradient reprenant le cadre
    /// unifié (cadreWidget).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreHaut: Option<String>,
    /// Texte du titre affiché en bas à l'intérieur du fond.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreBas: Option<String>,
    /// Texte du titre affiché le long du bord gauche du fond (vertical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreGauche: Option<String>,
    /// Texte du titre affiché le long du bord droit du fond (vertical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreDroite: Option<String>,
    /// LEGACY — ancien titre de fond à position unique (« haut »/« bas »).
    /// Conservé uniquement pour désérialiser les vieilles scènes : migré vers
    /// bgTitreHaut/bgTitreBas par `migrer_titres()` à la lecture, jamais
    /// écrit par l'UI. None en mémoire post-migration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitre: Option<String>,
    /// LEGACY — position du titre de fond legacy, source de migration
    /// uniquement. Voir `bgTitre`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitrePosition: Option<String>,
    /// Couleur de début du dégradé du texte. None → dégradé repris du cadre
    /// unifié (cadreWidget), comportement automatique par défaut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreCouleur: Option<String>,
    /// Couleur de fin du dégradé du texte. None → couleur unie (reprend
    /// bgTitreCouleur) ou fin du dégradé du cadre si bgTitreCouleur absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreCouleurFin: Option<String>,
    /// Identifiant de la police Google Font (ex: "BebasNeue"). Voir fonts.ts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitrePolice: Option<String>,
    /// Taille du titre du fond en pixels (12-72). Défaut 24.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreTaille: Option<u32>,
    /// Titre du fond en gras. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreGras: Option<bool>,
    /// Titre du fond en italique. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreItalique: Option<bool>,
    /// Titre du fond souligné. Défaut false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreSouligne: Option<bool>,
    /// Espacement des lettres du titre du fond en px (-2 à 20). Défaut 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreEspacement: Option<f64>,
    /// Couleur du contour des lettres du titre du fond (hex, ex: "#ffffff").
    /// Défaut blanc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreContourCouleur: Option<String>,
    /// Épaisseur du contour des lettres du titre du fond en px (0-6 ; 0 =
    /// aucun contour). Défaut 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitreContourEpaisseur: Option<u32>,
    /// Aide contextuelle « fond vide » (presets guidés — ex. gaming plein
    /// écran) : encadré jaune-orangé centré sur le canvas du DASHBOARD
    /// uniquement (jamais en diffusion), auto-masqué dès qu'un média de fond
    /// ou une capture OBS (bgObsSource) est lié. Texte i18n côté UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aideFond: Option<bool>,
    /// Aide contextuelle « connecte-toi » au-dessus des widgets goal :
    /// auto-masquée quand toutes les plateformes des goals sont connectées.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aideObjectifs: Option<bool>,
    /// Aide contextuelle « personnalise tes cadres » (presets guidés) :
    /// encadré centré empilé avec aideFond, auto-masquée à la première
    /// ouverture de la modale Cadres ou via la croix (dismiss persistant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aideCadres: Option<bool>,
    /// Modules HUD par scène (horloge / musique en cours / objectifs du
    /// stream) — couche d'affichage séparée des widgets, transportée par le
    /// snapshot WS comme le reste de la scène. None = aucun module
    /// (vieilles scènes : rétro-compat totale).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modules: Option<ScenesModules>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
            canvasW: DEFAULT_CANVAS_W,
            canvasH: DEFAULT_CANVAS_H,
            bgMedia: String::new(),
            bgSlides: None,
            bgSlidesSec: None,
            bgKind: DEFAULT_BG_KIND.to_string(),
            bgFit: DEFAULT_BG_FIT.to_string(),
            bgZoom: DEFAULT_BG_ZOOM,
            bgRot: DEFAULT_BG_ROT,
            bgRotX: 0.0,
            bgRotY: 0.0,
            bgOffsetX: DEFAULT_BG_OFFSET,
            bgOffsetY: DEFAULT_BG_OFFSET,
            bgMiroirH: false,
            bgMiroirV: false,
            bgLum: 0.0,
            bgContraste: 0.0,
            bgTeinte: 0.0,
            bgFlou: 0.0,
            bgPixel: 0.0,
            bgPaused: true,
            bgTime: 0.0,
            bgLoopA: None,
            bgLoopB: None,
            bgObsSource: None,
            // Cadre par défaut à la naissance : MGStyle noir/violet actif
            // (choix produit — une nouvelle sauvegarde sans cadre choisi
            // obtient le cadre marque). Les vieilles sauvegardes ne sont
            // PAS touchées : #[serde(default)] → None pour le champ absent.
            cadreWidget: Some(CadreConfig::defaut_scene()),
            cadreApp: None,
            bgMorphs: None,
            bgTitreHaut: None,
            bgTitreBas: None,
            bgTitreGauche: None,
            bgTitreDroite: None,
            bgTitre: None,
            bgTitrePosition: None,
            bgTitreCouleur: None,
            bgTitreCouleurFin: None,
            bgTitrePolice: None,
            bgTitreTaille: None,
            bgTitreGras: None,
            bgTitreItalique: None,
            bgTitreSouligne: None,
            bgTitreEspacement: None,
            bgTitreContourCouleur: None,
            bgTitreContourEpaisseur: None,
            aideFond: None,
            aideObjectifs: None,
            aideCadres: None,
            modules: None,
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Migration cadre unifié (2026-09-26) : UN SEUL CadreConfig pilote les
    /// DEUX rendus (widgets + bord du canvas). `cadreWidget` est le champ
    /// canonique ; `cadreApp` n'est plus qu'une source de migration lue au
    /// chargement des vieilles scènes — jamais écrite par l'UI.
    /// Règle (choix produit) : cadreWidget actif → il gagne ; sinon cadreApp
    /// (même inactif) est repris pour ne pas perdre le réglage. cadreApp est
    /// vidé dans tous les cas → plus de vérité séparée, le JSON se nettoie à
    /// la prochaine sauvegarde.
    pub fn migrer_cadres(&mut self) {
        if let Some(app) = self.cadreApp.take() {
            let widget_gagne = self.cadreWidget.as_ref().is_some_and(|c| c.actif);
            if !widget_gagne {
                self.cadreWidget = Some(app);
            }
        }
    }

    /// Migration titres multi-positions (2026-09-28) : le titre à position
    /// unique (« dessus »/« dessous » widget, « haut »/« bas » fond) devient
    /// jusqu'à 4 textes par position (haut/bas/gauche/droite), style partagé.
    /// Les champs legacy `titre`/`titrePosition`/`bgTitre`/`bgTitrePosition`
    /// ne sont plus que des sources de migration lues au chargement — jamais
    /// écrits par l'UI. Un champ nouveau déjà présent gagne (le legacy est
    /// vidé dans tous les cas → le JSON se nettoie à la prochaine sauvegarde).
    pub fn migrer_titres(&mut self) {
        for w in self.widgets.iter_mut() {
            let pos = w.titrePosition.take();
            if let Some(t) = w.titre.take() {
                let bas = pos.as_deref() == Some("dessous");
                let cible = if bas { &mut w.titreBas } else { &mut w.titreHaut };
                if cible.is_none() {
                    *cible = Some(t);
                }
            }
        }
        let pos = self.bgTitrePosition.take();
        if let Some(t) = self.bgTitre.take() {
            let bas = pos.as_deref() == Some("bas");
            let cible = if bas { &mut self.bgTitreBas } else { &mut self.bgTitreHaut };
            if cible.is_none() {
                *cible = Some(t);
            }
        }
    }
}
