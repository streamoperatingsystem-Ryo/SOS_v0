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
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Identifiant du device caméra (getUserMedia deviceId) pour les widgets
    /// type "camera". None = device par défaut du système.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cameraDeviceId: Option<String>,
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
    /// Chaîne de morphs (bulge/pinch) appliquée au média, dans l'ordre.
    /// None/vide = rendu natif (img/video), zéro coût GL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morphs: Option<Vec<MorphPoint>>,
    /// Titre optionnel affiché au-dessus ou en dessous du widget. None/vide =
    /// pas de titre. Le texte est rendu avec une police Google Font (titrePolice)
    /// et un gradient de couleur reprenant celui du cadre SVG actif de la scène.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titre: Option<String>,
    /// Position du titre : "dessus" (défaut) ou "dessous".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub titrePosition: Option<String>,
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
    #[serde(default = "default_bg_kind")]
    pub bgKind: String,
    #[serde(default = "default_bg_fit")]
    pub bgFit: String,
    #[serde(default = "default_bg_zoom")]
    pub bgZoom: f64,
    #[serde(default = "default_bg_rot")]
    pub bgRot: f64,
    #[serde(default = "default_bg_offset")]
    pub bgOffsetX: f64,
    #[serde(default = "default_bg_offset")]
    pub bgOffsetY: f64,
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
    /// Cadre appliqué à chaque widget (clip-path + overlay SVG). None = aucun.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadreWidget: Option<CadreConfig>,
    /// Cadre appliqué au bord extérieur du canvas. None = aucun.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadreApp: Option<CadreConfig>,
    /// Chaîne de morphs appliquée au FOND de scène (bulge/pinch), dans l'ordre.
    /// Coordonnées normalisées (0-1) du canvas. None/vide = rendu natif.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgMorphs: Option<Vec<MorphPoint>>,
    /// Titre optionnel affiché à l'intérieur du fond de l'application, en haut
    /// ou en bas. None/vide = pas de titre. Le texte est rendu avec une police
    /// Google Font (bgTitrePolice) et un gradient de couleur reprenant celui
    /// du cadre de l'application (cadreApp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitre: Option<String>,
    /// Position du titre du fond : "haut" (défaut) ou "bas" (intérieur du fond).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bgTitrePosition: Option<String>,
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
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            widgets: Vec::new(),
            canvasW: DEFAULT_CANVAS_W,
            canvasH: DEFAULT_CANVAS_H,
            bgMedia: String::new(),
            bgKind: DEFAULT_BG_KIND.to_string(),
            bgFit: DEFAULT_BG_FIT.to_string(),
            bgZoom: DEFAULT_BG_ZOOM,
            bgRot: DEFAULT_BG_ROT,
            bgOffsetX: DEFAULT_BG_OFFSET,
            bgOffsetY: DEFAULT_BG_OFFSET,
            bgLum: 0.0,
            bgContraste: 0.0,
            bgTeinte: 0.0,
            bgFlou: 0.0,
            bgPixel: 0.0,
            bgPaused: true,
            bgTime: 0.0,
            cadreWidget: None,
            cadreApp: None,
            bgMorphs: None,
            bgTitre: None,
            bgTitrePosition: None,
            bgTitrePolice: None,
            bgTitreTaille: None,
            bgTitreGras: None,
            bgTitreItalique: None,
            bgTitreSouligne: None,
            bgTitreEspacement: None,
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
}
