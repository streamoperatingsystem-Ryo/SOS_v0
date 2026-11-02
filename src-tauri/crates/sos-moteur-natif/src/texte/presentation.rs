//! Overlay « présentation streamer » natif — tranches E6a (décor) + E6b (carte).
//!
//! Miroir de `initPresentationOverlay` / `playPresentationShow` /
//! `appliquerModePresentation` / `stopPresentation` (`diffusion.html`
//! 10615-11118) et du bloc CSS `.pres-*` (2600-3142). Overlay plein écran
//! **autonome** (pas un widget de scène), au-dessus de toute la pile
//! existante : écran d'attente 9990 → welcome 9999.
//!
//! - **E6a — décor** : voile `z 10000` (`radial-gradient` de fond, bannière de
//!   chaîne `cover` + scrim, 3 orbes animés, sol de scène, 3 faisceaux de
//!   spot, 20 grains de poussière), rideaux `z 10001` (deux rideaux froncés
//!   `translateX ±27 %` + `scaleX 0,5` en 1,2 s, cantonné festonné permanent,
//!   embrasses dorées) et public `z 10003` (3 rangées de fauteuils + têtes) ;
//! - **E6b — carte** `z 10002` : avatar rond, nom/login, badges
//!   PARTNER/AFFILIATE, bio (5 lignes), jeu courant, lien `twitch.tv/<login>`,
//!   bandeau EN DIRECT + viewers, titre live, jeux récents.
//!
//! - **E6c — clips** : grille 2×2 de vignettes dans la carte (hôte
//!   `clips-media-assets2.twitch.tv`, documenté au contrat vie privée) et
//!   lecteur plein écran `z 10004` (MP4 **préchargé sur disque** par l'app,
//!   le crate ne lit qu'un fichier local) ;
//! - **E6d — audio** : 2 pistes (`voix` one-shot, `musique` en boucle) sur
//!   `medias/…` — fichiers **locaux**, aucune sortie réseau.
//!
//! Canal : frames `chat_tx` **`presentation-show`** (payload `mode` +
//! `donnees`), **`presentation-hide`** et **`presentation-mode`** (bascule à
//! chaud). ⚠️ L'event Tauri `presentation:etat` est l'état **dashboard** —
//! jamais le canal du rendu (même leçon que E1→E5). `presentation-show`
//! n'arrive **qu'une fois** : toute la chorégraphie est détenue par l'horloge
//! locale de ce module (`debut` / `debut_chore` / `fin`).
//!
//! Chorégraphie (miroir du CSS) :
//! - voile : `opacity 0 → 1` en 0,5 s `ease` ;
//! - théâtral : valance et public en 0,6 s, rideaux ouverts en 1,2 s
//!   `cubic-bezier(.4,0,.2,1)` (après le double `requestAnimationFrame` du
//!   DOM, soit ~33 ms), embrasses après 0,75 s puis 0,4 s ;
//! - carte : `opacity 0 → 1` et `translateY(26px) scale(.94) → none` en
//!   0,55 s, **après 0,65 s** en théâtral (0 s en `card`) — la carte apparaît
//!   pendant l'ouverture des rideaux ;
//! - sortie : tout se referme puis masquage à 1 250 ms (théâtral) / 500 ms
//!   (card) — le DOM retire les nœuds à ce moment-là ;
//! - mode `card` : **le voile reste affiché** (le code du DOM fait foi, pas
//!   son commentaire) ; rideaux, valance, embrasses et public sont absents,
//!   la carte passe en colonne (`max-width 760`, gap 22, textes centrés) ;
//! - bascule `presentation-mode` : la chorégraphie est rejouée **sans**
//!   re-fondre le voile (`debut` conservé, `debut_chore` remis à zéro).
//!
//! Limites assumées :
//! - `filter: blur()` non porté — les orbes sont des disques à **dégradé
//!   radial** (opaque jusqu'à `r − 90 px`, transparent à `r + 90 px`), les
//!   faisceaux gardent un bord net (le `clip-path` l'emporte sur le flou) et
//!   la poussière n'est pas adoucie ;
//! - `box-shadow` / `inset` non portés (ombre portée intérieure des rideaux,
//!   du cantonné et des fauteuils, halo du badge, ombre de la carte et des
//!   textes) — **sauf** l'anneau de l'avatar (`0 0 0 6px` = ombre à
//!   débordement seul, sans flou : rendue comme un trait) ;
//! - `text-shadow` des textes de la carte non porté (même limite qu'E3) ;
//! - `-webkit-line-clamp` de la bio (5 lignes) et des noms de jeux (2 lignes)
//!   = **rognage** à la hauteur de N lignes, sans ellipse de fin ;
//! - `word-break: break-word` du nom non appliqué (DirectWrite coupe aux
//!   espaces ; un pseudo sans espace déborde au lieu d'être coupé) ;
//! - `Math.random()` du DOM → **PRNG déterministe** (xorshift, graine fixe) :
//!   le public et la poussière sont stables d'une session à l'autre, mais pas
//!   identiques au tirage du DOM (irreproductible par nature) ;
//! - le public suit le `1920` **codé en dur** du DOM (`for (x = offset;
//!   x < 1920; …)`) : sur un canvas plus large, les rangées s'arrêtent à
//!   1 920 px comme dans la diffusion ;
//! - `border-radius` à rayons différents du fauteuil (`46 % 46 % 8 % 8 % /
//!   32 % 32 % 5 % 5 %`) est construit en **géométrie** (deux rayons par
//!   coin), pas approché par un arrondi uniforme ;
//! - images distantes (bannière, avatar, boîtes de jeux) : **cache disque de
//!   l'app** (lot G). Tant que le fichier manque, l'élément est omis ; une
//!   nouvelle tentative a lieu toutes les 500 ms (le `<img>` du DOM se répare
//!   seul).

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;
use std::time::Instant;

use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_GRADIENT_STOP, D2D_POINT_2F,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1EllipseGeometry, ID2D1Geometry, ID2D1GradientStopCollection, ID2D1Layer,
    ID2D1PathGeometry, ID2D1RoundedRectangleGeometry, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE, D2D1_EXTEND_MODE_CLAMP, D2D1_EXTEND_MODE_WRAP, D2D1_GAMMA_2_2,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteInlineObject, IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1,
    DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_EXTRA_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_LINE_SPACING_METHOD_UNIFORM, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, TraceMedia};
use crate::media::{EffetsCfg, Fit, LectureCfg, MediaCfg, Transform2D};

/// z du voile et de tout le décor (`.pres-voile`, z-index 10000).
pub const Z_PRES_VOILE: i32 = 10000;
/// z des rideaux, du cantonné et des embrasses (z-index 10001).
pub const Z_PRES_RIDEAUX: i32 = 10001;
/// z de la carte (`.pres-root`, z-index 10002) — **sous** le public (10003).
pub const Z_PRES_CARTE: i32 = 10002;
/// z du public (`.pres-public`, z-index 10003) — **devant** la carte, comme
/// dans un vrai théâtre, sous le lecteur clip (10004).
pub const Z_PRES_PUBLIC: i32 = 10003;
/// z du lecteur clip plein écran (`.pres-clip-root`, z-index 10004) — au-dessus
/// de tout, public compris.
pub const Z_PRES_CLIP: i32 = 10004;

/// Couche peinte par `dessiner_presentation` (une entrée de plan par couche).
pub const COUCHE_VOILE: usize = 0;
pub const COUCHE_RIDEAUX: usize = 1;
pub const COUCHE_PUBLIC: usize = 2;
pub const COUCHE_CARTE: usize = 3;
pub const COUCHE_CLIP: usize = 4;

/// `.pres-voile { transition: opacity 0.5s ease }`.
const FONDU_VOILE_S: f32 = 0.5;
/// `.pres-valance` / `.pres-public { transition: opacity 0.6s ease }`.
const FONDU_DECOR_S: f32 = 0.6;
/// `.pres-embrasse { transition: opacity .4s ease, transform .4s ease }`.
const FONDU_EMBRASSE_S: f32 = 0.4;
/// `.pres-embrasse.open { transition-delay: 0.75s }`.
const DELAI_EMBRASSE_S: f32 = 0.75;
/// `.pres-rideau { transition: transform 1.2s cubic-bezier(.4,0,.2,1) }`.
const OUVERTURE_S: f32 = 1.2;
/// Le DOM ajoute `.open` dans un **double `requestAnimationFrame`** : deux
/// frames à 60 Hz avant que la transition ne démarre.
const DELAI_OUVERTURE_S: f32 = 2.0 / 60.0;
/// `setTimeout(…, presMode === "theatrical" ? 1250 : 500)`.
const SORTIE_THEATRALE_S: f32 = 1.25;
const SORTIE_CARTE_S: f32 = 0.5;
/// `ease` CSS = `cubic-bezier(0.25, 0.1, 0.25, 1)`.
const EASE: (f32, f32, f32, f32) = (0.25, 0.1, 0.25, 1.0);
/// `cubic-bezier(0.4, 0, 0.2, 1)` de l'ouverture des rideaux.
const EASE_RIDEAU: (f32, f32, f32, f32) = (0.4, 0.0, 0.2, 1.0);
/// `ease-in-out` CSS = `cubic-bezier(0.42, 0, 0.58, 1)`.
const EASE_IN_OUT: (f32, f32, f32, f32) = (0.42, 0.0, 0.58, 1.0);

/// `.pres-voile` — `radial-gradient(ellipse at 50% 42%, #1b1b38, #05050a 74%)`.
const VOILE_CENTRE: (f32, f32) = (0.50, 0.42);
const VOILE_CLAIR: [u8; 4] = [0x1b, 0x1b, 0x38, 255];
const VOILE_SOMBRE: [u8; 4] = [0x05, 0x05, 0x0a, 255];
const VOILE_ARRET: f32 = 0.74;

/// `.pres-fond::after` — scrim de lisibilité sur la bannière.
const SCRIM_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [5, 5, 10, 140]),
    (0.45, [5, 5, 10, 82]),
    (1.0, [5, 5, 10, 184]),
];

/// `.pres-orbe { filter: blur(90px); opacity: .45 }` — rayon de flou simulé
/// par le dégradé radial.
const ORBE_FLOU: f32 = 90.0;
const ORBE_OPACITE: f32 = 0.45;
/// `@keyframes pres-orbe` : 14 s `ease-in-out` `alternate`.
const ORBE_CYCLE_S: f32 = 14.0;
const ORBE_DEPLACEMENT: (f32, f32) = (60.0, -40.0);
const ORBE_ECHELLE: f32 = 0.12;

/// Un orbe de fond (`.pres-orbe.o1/o2/o3`).
struct Orbe {
    taille: f32,
    /// `left` (px) ou `34 %` de la largeur quand `x_relatif`.
    x: f32,
    /// `top` (px) ou `12 %` de la hauteur quand `y_relatif`.
    y: f32,
    x_relatif: bool,
    y_relatif: bool,
    /// `right: -120px` / `bottom: -240px` : la valeur est une marge négative.
    depuis_droite: bool,
    depuis_bas: bool,
    couleur: [u8; 4],
    delai_s: f32,
}

const ORBES: [Orbe; 3] = [
    // `left: -140px; top: -160px`
    Orbe {
        taille: 620.0,
        x: -140.0,
        y: -160.0,
        x_relatif: false,
        y_relatif: false,
        depuis_droite: false,
        depuis_bas: false,
        couleur: [140, 60, 255, 89],
        delai_s: 0.0,
    },
    // `right: -120px; top: 12%`
    Orbe {
        taille: 560.0,
        x: -120.0,
        y: 0.12,
        x_relatif: false,
        y_relatif: true,
        depuis_droite: true,
        depuis_bas: false,
        couleur: [255, 60, 120, 71],
        delai_s: 5.0,
    },
    // `left: 34%; bottom: -240px`
    Orbe {
        taille: 520.0,
        x: 0.34,
        y: -240.0,
        x_relatif: true,
        y_relatif: false,
        depuis_droite: false,
        depuis_bas: true,
        couleur: [60, 110, 255, 77],
        delai_s: 9.0,
    },
];

/// `.pres-sol { height: 24% }` + dégradés.
const SOL_HAUTEUR: f32 = 0.24;
const SOL_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [20, 11, 7, 0]),
    (0.38, [20, 11, 7, 199]),
    (1.0, [11, 5, 3, 255]),
];
const SOL_LUEUR: [u8; 4] = [255, 214, 150, 26];
const SOL_LISERE: [u8; 4] = [255, 220, 160, 46];

/// `.pres-spot` : 420 px (520 pour le centre), `top -60px`, `height 126 %`.
const SPOT_HAUTEUR: f32 = 1.26;
const SPOT_TOP: f32 = -60.0;
const SPOT_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [255, 238, 200, 41]),
    (0.55, [255, 238, 200, 13]),
    (0.88, [255, 238, 200, 0]),
];
/// `@keyframes pres-spot-bal` : ±1,4° sur 11 s `ease-in-out` `alternate`.
const SPOT_CYCLE_S: f32 = 11.0;
const SPOT_BALANCE: f32 = 1.4;
/// `polygon(47% 0, 53% 0, 100% 100%, 0 100%)`.
const SPOT_TRAPEZE: [(f32, f32); 4] = [(0.47, 0.0), (0.53, 0.0), (1.0, 1.0), (0.0, 1.0)];

/// Poussière : 20 grains, `@keyframes pres-poussiere-drift`.
const POUSSIERE_NB: usize = 20;
const POUSSIERE_COULEUR: [u8; 4] = [255, 240, 210, 140];
const POUSSIERE_DEPLACEMENT: (f32, f32) = (26.0, -42.0);

/// Plis des rideaux et du cantonné : période 88 px.
const PLI_PERIODE: f32 = 88.0;
/// `repeating-linear-gradient(90deg, …)` — positions en px de la période.
const PLIS_STOPS: [(f32, [u8; 4]); 7] = [
    (0.0, [0x3a, 0x06, 0x0d, 255]),
    (15.0, [0x5c, 0x0c, 0x18, 255]),
    (34.0, [0x8f, 0x15, 0x26, 255]),
    (44.0, [0xb0, 0x1c, 0x30, 255]),
    (54.0, [0x8f, 0x15, 0x26, 255]),
    (73.0, [0x5c, 0x0c, 0x18, 255]),
    (88.0, [0x3a, 0x06, 0x0d, 255]),
];
/// Ombre verticale des rideaux (`linear-gradient(180deg, …)`).
const RIDEAU_OMBRE_STOPS: [(f32, [u8; 4]); 4] = [
    (0.0, [0, 0, 0, 140]),
    (0.16, [0, 0, 0, 31]),
    (0.42, [0, 0, 0, 0]),
    (1.0, [0, 0, 0, 97]),
];
/// `border: 6px solid #c9a13b` du bord intérieur.
const RIDEAU_BORD: f32 = 6.0;
const RIDEAU_BORD_COULEUR: [u8; 4] = [0xc9, 0xa1, 0x3b, 255];
/// Ourlet doré : `::after { height: 14px }`.
const OURLET_HAUTEUR: f32 = 14.0;
const OURLET_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [0x8a, 0x6c, 0x25, 255]),
    (0.45, [0xe8, 0xc5, 0x5c, 255]),
    (1.0, [0xb8, 0x93, 0x3a, 255]),
];
/// Ouverture : `translateX(∓27 %)` + `scaleX(0.5)`.
const RIDEAU_TRANSLATE: f32 = 0.27;
const RIDEAU_ECHELLE: f32 = 0.5;

/// Cantonné : `height: 110px`, ombre du haut (`55 %`).
const VALANCE_HAUTEUR: f32 = 110.0;
const VALANCE_OMBRE_STOPS: [(f32, [u8; 4]); 2] = [(0.0, [0, 0, 0, 128]), (0.55, [0, 0, 0, 0])];
/// Festons : tuile de 192 px, deux cercles (rouge dessus, doré dessous).
const FESTON_PERIODE: f32 = 192.0;
const FESTON_HAUTEUR: f32 = 46.0;
const FESTON_ROUGE: [(f32, [u8; 4]); 4] = [
    (0.0, [0xa0, 0x17, 0x28, 255]),
    (0.60, [0x7a, 0x12, 0x20, 255]),
    (0.95, [0x4a, 0x08, 0x10, 255]),
    (1.0, [0x4a, 0x08, 0x10, 0]),
];
const FESTON_DORE: [(f32, [u8; 4]); 3] = [
    (0.0, [0xe8, 0xc5, 0x5c, 255]),
    (0.68, [0xb8, 0x93, 0x3a, 255]),
    (1.0, [0xb8, 0x93, 0x3a, 0]),
];

/// Embrasses : 290 × 30, `top 57 %`, `left/right -55px`, `rotate(∓24deg)`.
const EMBRASSE_LARGEUR: f32 = 290.0;
const EMBRASSE_HAUTEUR: f32 = 30.0;
const EMBRASSE_TOP: f32 = 0.57;
const EMBRASSE_MARGE: f32 = -55.0;
const EMBRASSE_ANGLE: f32 = 24.0;
const EMBRASSE_DECALAGE: f32 = -16.0;
const EMBRASSE_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [0xf0, 0xd0, 0x70, 255]),
    (0.55, [0xc9, 0xa1, 0x3b, 255]),
    (1.0, [0x8a, 0x6c, 0x25, 255]),
];

/// Public : conteneur `height: 240px`, largeur de référence du DOM.
const PUBLIC_HAUTEUR: f32 = 240.0;
const PUBLIC_LARGEUR_REF: f32 = 1920.0;

/// Une rangée du public — miroir du tableau `rangees` de `construirePublic`.
struct Rangee {
    bas: f32,
    siege_l: f32,
    siege_h: f32,
    tete: f32,
    ecart: f32,
    decalage: f32,
    couleur: [u8; 4],
}

const RANGEES: [Rangee; 3] = [
    Rangee {
        bas: 118.0,
        siege_l: 88.0,
        siege_h: 74.0,
        tete: 44.0,
        ecart: 126.0,
        decalage: 24.0,
        couleur: [0x18, 0x11, 0x16, 255],
    },
    Rangee {
        bas: 16.0,
        siege_l: 116.0,
        siege_h: 94.0,
        tete: 60.0,
        ecart: 160.0,
        decalage: 92.0,
        couleur: [0x0b, 0x07, 0x09, 255],
    },
    Rangee {
        bas: -26.0,
        siege_l: 0.0,
        siege_h: 0.0,
        tete: 86.0,
        ecart: 215.0,
        decalage: 158.0,
        couleur: [0x04, 0x02, 0x03, 255],
    },
];

/// `linear-gradient(180deg, #7a1622 0%, #4d0a12 68%, #33060c 100%)`.
const SIEGE_STOPS: [(f32, [u8; 4]); 3] = [
    (0.0, [0x7a, 0x16, 0x22, 255]),
    (0.68, [0x4d, 0x0a, 0x12, 255]),
    (1.0, [0x33, 0x06, 0x0c, 255]),
];
/// `border-radius: 46% 46% 8% 8% / 32% 32% 5% 5%`.
const SIEGE_RAYON_HAUT: (f32, f32) = (0.46, 0.32);
const SIEGE_RAYON_BAS: (f32, f32) = (0.08, 0.05);

/// `@keyframes pres-tete-sway` : ±1,5° + `translateY(-2px)`.
const TETE_BALANCE: f32 = 1.5;
const TETE_SOULEVE: f32 = 2.0;

/// Graine du tirage déterministe (public + poussière). Le DOM tire avec
/// `Math.random()` **une seule fois** à l'init : le moteur doit produire un
/// décor stable entre deux sessions, d'où une graine fixe.
const GRAINE_DECOR: u64 = 0x5f37_a1c9_2d6b_e401;

// ===== Carte (E6b) =====

/// `.pres-carte { transition: opacity .55s ease .65s, transform .55s … }`.
const CARTE_FONDU_S: f32 = 0.55;
/// Retard de la carte en théâtral (elle apparaît **pendant** l'ouverture) ;
/// `transition-delay: 0s` en mode `card`.
const CARTE_DELAI_S: f32 = 0.65;
/// `cubic-bezier(0.2, 0.9, 0.3, 1.15)` — la courbe **dépasse 1** (rebond).
const CARTE_EASE: (f32, f32, f32, f32) = (0.2, 0.9, 0.3, 1.15);
/// `transform: translateY(26px) scale(0.94)` au repos.
const CARTE_DECALAGE: f32 = 26.0;
const CARTE_ECHELLE: f32 = 0.94;

/// `.pres-carte { gap: 56px; max-width: 73% }` ; `mode-card` → 22 px / 760 px.
const CARTE_GAP: f32 = 56.0;
const CARTE_GAP_CARD: f32 = 22.0;
const CARTE_MAX_RATIO: f32 = 0.73;
const CARTE_MAX_CARD: f32 = 760.0;

/// `.pres-col-gauche { gap: 14px; max-width: 460px }`.
const COL_G_MAX: f32 = 460.0;
const COL_G_GAP: f32 = 14.0;
/// `.pres-col-droite { gap: 18px; max-width: 620px }`.
const COL_D_MAX: f32 = 620.0;
const COL_D_GAP: f32 = 18.0;

/// `.pres-avatar { 190px; border: 5px; box-shadow: 0 0 0 6px … }`.
const AVATAR: f32 = 190.0;
const AVATAR_CARD: f32 = 150.0;
const AVATAR_BORD: f32 = 5.0;
const AVATAR_ANNEAU: f32 = 6.0;
const AVATAR_BORD_COULEUR: [u8; 4] = [255, 255, 255, 230];
const AVATAR_ANNEAU_COULEUR: [u8; 4] = [145, 70, 255, 140];

/// `.pres-nom { 800 54px/1.05 }` ; `mode-card` → 42 px.
const NOM_TAILLE: f32 = 54.0;
const NOM_TAILLE_CARD: f32 = 42.0;
const NOM_INTERLIGNE: f32 = 1.05;
const NOM_COULEUR: [u8; 4] = [255, 255, 255, 255];
/// `.pres-login { 600 24px/1.2; rgba(255,255,255,.65) }`.
const LOGIN_TAILLE: f32 = 24.0;
const LOGIN_INTERLIGNE: f32 = 1.2;
const LOGIN_COULEUR: [u8; 4] = [255, 255, 255, 166];
/// `.pres-badge { 700 18px/1; padding 8px 16px; radius 999px }`.
const BADGE_TAILLE: f32 = 18.0;
const BADGE_PAD_H: f32 = 16.0;
const BADGE_PAD_V: f32 = 8.0;
const BADGE_PARTNER: ([u8; 4], [u8; 4]) = ([0x77, 0x2c, 0xe8, 255], [0x91, 0x46, 0xff, 255]);
const BADGE_AFFILIATE: ([u8; 4], [u8; 4]) = ([0x1f, 0x6f, 0xeb, 255], [0x3b, 0x82, 0xf6, 255]);
/// `.pres-bio { 400 23px/1.45; rgba(255,255,255,.88) }`, 5 lignes.
const BIO_TAILLE: f32 = 23.0;
const BIO_INTERLIGNE: f32 = 1.45;
const BIO_LIGNES: f32 = 5.0;
const BIO_COULEUR: [u8; 4] = [255, 255, 255, 224];
/// `.pres-jeu-courant { 700 22px/1.2; #ffd77a }`.
const JEU_TAILLE: f32 = 22.0;
const JEU_INTERLIGNE: f32 = 1.2;
const JEU_COULEUR: [u8; 4] = [0xff, 0xd7, 0x7a, 255];
/// `.pres-lien { 600 20px/1; rgba(255,255,255,.55); letter-spacing .04em }`.
const LIEN_TAILLE: f32 = 20.0;
const LIEN_ESPACEMENT: f32 = 0.04 * LIEN_TAILLE;
const LIEN_COULEUR: [u8; 4] = [255, 255, 255, 140];
/// `.pres-live { 800 22px/1; padding 10px 18px; radius 10px; gap 12px }`.
const LIVE_TAILLE: f32 = 22.0;
const LIVE_PAD_H: f32 = 18.0;
const LIVE_PAD_V: f32 = 10.0;
const LIVE_RAYON: f32 = 10.0;
const LIVE_PASTILLE: f32 = 12.0;
const LIVE_GAP: f32 = 12.0;
const LIVE_FOND: [u8; 4] = [0xe9, 0x19, 0x16, 255];
/// `.pres-titre-live { 600 22px/1.35; rgba(255,255,255,.8) }`.
const TITRE_TAILLE: f32 = 22.0;
const TITRE_INTERLIGNE: f32 = 1.35;
const TITRE_COULEUR: [u8; 4] = [255, 255, 255, 204];
/// `.pres-jeu { width: 96px; gap: 6px }` + `img { 96×128; radius 10px }`.
const BOITE_L: f32 = 96.0;
const BOITE_H: f32 = 128.0;
const BOITE_RAYON: f32 = 10.0;
const BOITE_GAP: f32 = 12.0;
const BOITE_NOM_GAP: f32 = 6.0;
/// `.pres-jeu span { 600 15px/1.2; rgba(255,255,255,.75) }`, 2 lignes.
const BOITE_NOM_TAILLE: f32 = 15.0;
const BOITE_NOM_INTERLIGNE: f32 = 1.2;
const BOITE_NOM_LIGNES: f32 = 2.0;
const BOITE_NOM_COULEUR: [u8; 4] = [255, 255, 255, 191];

/// `.pres-clips { grid 2 colonnes; gap 12px }` + `.pres-clip { radius 12 }`.
const CLIPS_GAP: f32 = 12.0;
const CLIP_RAYON: f32 = 12.0;
const CLIP_RATIO: f32 = 16.0 / 9.0;
/// `.pres-clip .duree { 700 15px/1; padding 4×8; radius 6; right/bottom 8 }`.
const CLIP_DUREE_TAILLE: f32 = 15.0;
const CLIP_DUREE_PAD_H: f32 = 8.0;
const CLIP_DUREE_PAD_V: f32 = 4.0;
const CLIP_DUREE_RAYON: f32 = 6.0;
const CLIP_DUREE_MARGE: f32 = 8.0;
/// `.pres-clip .titre { 600 15px/1.3; padding 18px 10px 8px }`.
const CLIP_TITRE_TAILLE: f32 = 15.0;
const CLIP_TITRE_INTERLIGNE: f32 = 1.3;
const CLIP_TITRE_PAD_H: f32 = 10.0;
const CLIP_TITRE_PAD_HAUT: f32 = 18.0;
const CLIP_TITRE_PAD_BAS: f32 = 8.0;
const CLIP_TITRE_FOND: [u8; 4] = [0, 0, 0, 217];

/// `.pres-clip-root { background: rgba(0,0,0,.9) }`.
const CLIP_VOILE: [u8; 4] = [0, 0, 0, 230];
/// `.pres-clip-video { max-width: 88%; max-height: 82%; radius 14px }`.
const CLIP_VIDEO_MAX_L: f32 = 0.88;
const CLIP_VIDEO_MAX_H: f32 = 0.82;
const CLIP_VIDEO_RAYON: f32 = 14.0;
/// `.pres-clip-titre { top: 44px; left/right: 8%; 700 30px/1.3 }`.
const CLIP_TITRE_LECTEUR_TAILLE: f32 = 30.0;
const CLIP_TITRE_LECTEUR_TOP: f32 = 44.0;
const CLIP_TITRE_LECTEUR_MARGE: f32 = 0.08;

/// Les 2 pistes audio de la présentation (`const PISTES = ["voix","musique"]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PistePresentation {
    /// One-shot : `audio.loop = false`.
    Voix,
    /// Boucle : `audio.loop = true` (accompagne la présentation).
    Musique,
}

impl PistePresentation {
    /// `None` si le nom n'est pas une piste connue (frame ignorée).
    pub fn depuis_nom(nom: &str) -> Option<Self> {
        match nom {
            "voix" => Some(PistePresentation::Voix),
            "musique" => Some(PistePresentation::Musique),
            _ => None,
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            PistePresentation::Voix => "voix",
            PistePresentation::Musique => "musique",
        }
    }

    pub fn index(self) -> usize {
        match self {
            PistePresentation::Voix => 0,
            PistePresentation::Musique => 1,
        }
    }

    /// Index des 2 pistes, dans l'ordre du DOM.
    pub const TOUTES: [PistePresentation; 2] =
        [PistePresentation::Voix, PistePresentation::Musique];

    /// `audio.loop = piste === "musique"`.
    pub fn boucle(self) -> bool {
        matches!(self, PistePresentation::Musique)
    }
}

/// `presFormatViewers` : `1234` → `"1 234"` (espace fine du DOM).
pub fn formater_viewers(n: u32) -> String {
    let chiffres = n.to_string();
    let mut sortie = String::with_capacity(chiffres.len() + chiffres.len() / 3);
    for (i, c) in chiffres.chars().enumerate() {
        if i > 0 && (chiffres.len() - i) % 3 == 0 {
            sortie.push(' ');
        }
        sortie.push(c);
    }
    sortie
}

/// URL de boîte de jeu : les `box_art_url` Helix portent des placeholders
/// `{width}x{height}` que le DOM remplace par `96x128`
/// (`g.box_art_url.replace("{width}", "96").replace("{height}", "128")`).
///
/// ⚠️ Miroir de l'app (`moteur/mod.rs`, préchargement) : les deux côtés
/// doivent produire la **même** URL, sinon le moteur ne retrouve jamais le
/// fichier écrit dans `medias/cache/`.
pub fn url_box_art(url: &str) -> String {
    url.replace("{width}", "96").replace("{height}", "128")
}

/// Chemin du fichier local d'un clip (`base` = dossier de données de l'app).
///
/// ⚠️ **Miroir** de `cache_clips::chemin` (app) : même empreinte FNV-1a 64
/// (`chat::empreinte_url`), même sous-dossier `medias/clips/`, même extension
/// `.mp4`. L'app télécharge l'URL **signée** à cet endroit, le moteur y lit un
/// fichier local (§13.8) — la signature Twitch peut expirer sans conséquence.
pub fn chemin_clip(base: &std::path::Path, mp4_url: &str) -> std::path::PathBuf {
    base.join("medias")
        .join("clips")
        .join(format!("{}.mp4", crate::chat::empreinte_url(mp4_url)))
}

// =============================================================================
// FRAME — payload de `presentation-show`, décodé par le pont.
// =============================================================================

/// Mode d'affichage de la présentation (`msg.mode` du DOM).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ModePresentation {
    /// Voile + décor + rideaux + carte 2 colonnes.
    #[default]
    Theatrical,
    /// Voile + carte compacte (sans rideaux, valance, embrasses ni public).
    Card,
}

impl ModePresentation {
    /// `msg.mode === "card" ? "card" : "theatrical"` — tout le reste = théâtral.
    pub fn depuis_nom(nom: &str) -> Self {
        if nom == "card" {
            ModePresentation::Card
        } else {
            ModePresentation::Theatrical
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            ModePresentation::Theatrical => "theatrical",
            ModePresentation::Card => "card",
        }
    }
}

/// Un jeu récent (`CategoryEntry` Helix) — boîte du DOM `.pres-jeu`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JeuRecent {
    pub id: String,
    pub name: String,
    /// `https://static-cdn.jtvnw.net/…{width}x{height}.jpg` (placeholders).
    pub box_art_url: String,
}

/// Un clip récent (`ClipInfo` de `twitch_clips`) — vignette de la grille.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipPresentation {
    pub id: String,
    pub title: String,
    pub thumbnail_url: String,
    /// Durée en secondes (le DOM affiche `Math.round(duration) + "s"`).
    pub duration: f64,
}

/// Données complètes d'une présentation (`donnees` de la frame) — miroir 1:1
/// de `PresentationDonnees` (`presentation.rs` de l'app).
///
/// La tranche E6a n'en rend que `banniere` ; les autres champs sont le
/// **contrat** de la frame et servent aux tranches E6b (carte) et E6c (clips).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PresentationDonnees {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    /// Avatar Twitch (CDN → cache disque).
    pub avatar: String,
    pub bio: String,
    /// Bannière/entête de chaîne (`offline_image_url`) — vide si absente.
    pub banniere: String,
    /// `"partner"` | `"affiliate"` | `""`.
    pub broadcaster_type: String,
    pub en_live: bool,
    pub viewers_live: Option<u32>,
    pub titre_live: String,
    pub jeu_courant: String,
    pub tags: Vec<String>,
    pub jeux_recents: Vec<JeuRecent>,
    pub clips: Vec<ClipPresentation>,
    pub mode: String,
}

/// Mutation de la présentation — frames `chat_tx`.
///
/// `donnees` est **boxé** : la frame arrive une fois par présentation (jamais
/// dans une boucle de rendu), et le variant `Show` pèse ~330 octets à lui seul
/// contre 1 octet pour les deux autres.
#[derive(Debug, Clone, PartialEq)]
pub enum MajPresentation {
    /// `presentation-show` : nouvelles données + chorégraphie d'entrée.
    Show {
        mode: ModePresentation,
        donnees: Box<PresentationDonnees>,
    },
    /// `presentation-hide` : fermeture (rideaux 1,2 s, masquage à 1,25 s).
    Hide,
    /// `presentation-mode` : bascule à chaud (rejoue la chorégraphie).
    Mode(ModePresentation),
    /// `presentation-clip-play` : lecteur plein écran (`z 10004`).
    ///
    /// `mp4_url` est une URL **signée distante** : le crate ne fait aucun
    /// réseau (§13.8), il lit le fichier préchargé par l'app
    /// (`medias/clips/<fnv16(url)>.mp4`).
    ClipPlay {
        mp4_url: String,
        titre: String,
        /// Durée du clip en ms (0 = inconnue : le lecteur attend `-clip-stop`).
        duree_ms: u64,
    },
    /// `presentation-clip-stop` : retour à la carte (fin naturelle comprise).
    ClipStop,
    /// `presentation-audio-play` : une piste (`src` = `medias/…`, **local**).
    AudioPlay {
        piste: PistePresentation,
        src: String,
    },
    /// `presentation-audio-stop` : une piste, ou les deux (`None` = « toutes »).
    AudioStop(Option<PistePresentation>),
}

// =============================================================================
// ÉTAT LIVE
// =============================================================================

/// Opacités et avancement d'une chorégraphie à un instant donné.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Phases {
    /// Opacité du voile **et de tous ses enfants** (fond, orbes, sol, spots,
    /// poussière) — le fondu est porté par une couche.
    pub voile: f32,
    pub valance: f32,
    pub public: f32,
    pub embrasses: f32,
    /// `translateY` des embrasses (−16 px → 0).
    pub embrasses_decalage: f32,
    /// 0 = rideaux fermés, 1 = ouverts (`translateX ±27 % + scaleX 0,5`).
    pub rideaux: f32,
    /// Opacité de la carte (`transition: opacity .55s`).
    pub carte_opacite: f32,
    /// Avancement du transform de la carte (courbe **à dépassement** :
    /// `translateY(26px) scale(.94)` à 0, `none` à 1, léger rebond au-delà).
    pub carte_avance: f32,
}

/// Clip en cours de lecture (`presentation-clip-play`).
#[derive(Debug, Clone, PartialEq)]
pub struct ClipEnCours {
    /// URL signée Twitch — sert de **clé** au fichier local préchargé.
    pub mp4_url: String,
    pub titre: String,
    /// Durée en ms (`0` = inconnue → le lecteur attend `-clip-stop`).
    pub duree_ms: u64,
    debut: Instant,
}

impl ClipEnCours {
    /// Le clip est allé au bout : le DOM reçoit `onended` → `clipStop`.
    pub fn est_termine(&self, maintenant: Instant) -> bool {
        self.duree_ms > 0
            && maintenant.saturating_duration_since(self.debut).as_millis() as u64 >= self.duree_ms
    }
}

/// État vivant de la présentation : données + mode + chorégraphie.
#[derive(Debug, Default)]
pub struct EtatPresentation {
    donnees: Option<PresentationDonnees>,
    mode: ModePresentation,
    /// Une présentation est affichée (le fondu de sortie ne la remet pas).
    actif: bool,
    /// Début du fondu du voile (conservé par une bascule de mode).
    debut: Option<Instant>,
    /// Début de la chorégraphie (rideaux, valance, embrasses, carte) —
    /// remis à zéro par `presentation-mode`, comme `appliquerModePresentation`.
    debut_chore: Option<Instant>,
    /// `presentation-hide` reçu — base du fondu de sortie.
    fin: Option<Instant>,
    /// Phases au début de la transition en cours : le CSS part de la valeur
    /// calculée, jamais de 0 (un `show` pendant la sortie ne rejoue pas un
    /// fondu depuis le noir).
    depart: Phases,
    /// Clip plein écran en cours (`presentation-clip-play`) — le DOM le coupe
    /// à chaque `show` et à chaque `hide`.
    clip: Option<ClipEnCours>,
    /// Pistes audio actives (`presentation-audio-play`) — `src` = `medias/…`.
    audio: [Option<String>; 2],
    version: u64,
}

/// Ce que la composition peint ce tick-ci (phases déjà résolues).
pub struct RenduPresentation<'a> {
    pub donnees: &'a PresentationDonnees,
    pub mode: ModePresentation,
    pub phases: Phases,
    /// Clip en cours, `None` s'il est fini ou absent.
    pub clip: Option<&'a ClipEnCours>,
    /// `src` des 2 pistes audio actives (index [`PistePresentation::index`]).
    pub audio: [Option<&'a str>; 2],
}

impl EtatPresentation {
    pub fn nouveau() -> Self {
        Self::default()
    }

    pub fn appliquer(&mut self, maj: MajPresentation) {
        self.appliquer_at(maj, Instant::now());
    }

    fn appliquer_at(&mut self, maj: MajPresentation, maintenant: Instant) {
        match maj {
            MajPresentation::Show { mode, donnees } => {
                let deja = self.actif;
                let change_de_mode = deja && mode != self.mode;
                if !deja || change_de_mode {
                    // Entrée, ou reprise pendant le fondu de sortie : le CSS
                    // repart de l'opacité courante (jamais du noir).
                    self.depart = self.phases_at(maintenant);
                    if !deja {
                        self.debut = Some(maintenant);
                    }
                    self.debut_chore = Some(maintenant);
                }
                self.actif = true;
                self.fin = None;
                self.mode = mode;
                self.donnees = Some(*donnees);
                // `playPresentationShow` coupe le clip et les pistes audio.
                self.clip = None;
                self.audio = [None, None];
                self.version = self.version.wrapping_add(1);
            }
            MajPresentation::Hide => {
                if self.actif {
                    self.depart = self.phases_at(maintenant);
                    self.actif = false;
                    self.fin = Some(maintenant);
                    // `stopPresentation` coupe le clip et les pistes tout de
                    // suite (pas à la fin des rideaux).
                    self.clip = None;
                    self.audio = [None, None];
                    self.version = self.version.wrapping_add(1);
                }
            }
            MajPresentation::Mode(mode) => {
                if self.actif && mode != self.mode {
                    self.depart = self.phases_at(maintenant);
                    self.mode = mode;
                    self.debut_chore = Some(maintenant);
                    self.version = self.version.wrapping_add(1);
                }
            }
            MajPresentation::ClipPlay {
                mp4_url,
                titre,
                duree_ms,
            } => {
                self.clip = Some(ClipEnCours {
                    mp4_url,
                    titre,
                    duree_ms,
                    debut: maintenant,
                });
                self.version = self.version.wrapping_add(1);
            }
            MajPresentation::ClipStop => {
                if self.clip.take().is_some() {
                    self.version = self.version.wrapping_add(1);
                }
            }
            MajPresentation::AudioPlay { piste, src } => {
                self.audio[piste.index()] = Some(src);
                self.version = self.version.wrapping_add(1);
            }
            MajPresentation::AudioStop(piste) => {
                let change = match piste {
                    Some(p) => self.audio[p.index()].take().is_some(),
                    None => {
                        let change = self.audio[0].is_some() || self.audio[1].is_some();
                        self.audio = [None, None];
                        change
                    }
                };
                if change {
                    self.version = self.version.wrapping_add(1);
                }
            }
        }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Une présentation est affichée (hors fondu de sortie).
    pub fn actif(&self) -> bool {
        self.actif
    }

    pub fn mode(&self) -> ModePresentation {
        self.mode
    }

    pub fn donnees(&self) -> Option<&PresentationDonnees> {
        self.donnees.as_ref()
    }

    /// Phases au temps courant — `None` quand l'overlay est entièrement sorti.
    pub fn rendu(&self) -> Option<RenduPresentation<'_>> {
        self.rendu_at(Instant::now())
    }

    fn rendu_at(&self, maintenant: Instant) -> Option<RenduPresentation<'_>> {
        let donnees = self.donnees.as_ref()?;
        match self.fin {
            Some(fin) => {
                let duree = match self.mode {
                    ModePresentation::Theatrical => SORTIE_THEATRALE_S,
                    ModePresentation::Card => SORTIE_CARTE_S,
                };
                if maintenant.saturating_duration_since(fin).as_secs_f32() >= duree {
                    return None;
                }
            }
            None if !self.actif => return None,
            None => {}
        }
        Some(RenduPresentation {
            donnees,
            mode: self.mode,
            phases: self.phases_at(maintenant),
            clip: self
                .clip
                .as_ref()
                .filter(|c| !c.est_termine(maintenant)),
            audio: [
                self.audio[0].as_deref(),
                self.audio[1].as_deref(),
            ],
        })
    }

    /// Gain du clip en millièmes — 1000 pendant la lecture, 0 sinon (le DOM
    /// joue le son du `<video>` ; seul `PREVIEW` le coupe, notion absente du
    /// moteur).
    pub fn gain_clip_milliemes(&self) -> u32 {
        self.gain_clip_milliemes_at(Instant::now())
    }

    /// Gain d'une piste audio en millièmes — 1000 tant qu'elle est active
    /// (`muted` n'existe pas : le DOM ne coupe le son qu'en `PREVIEW`).
    pub fn gain_piste_milliemes(&self, piste: PistePresentation) -> u32 {
        if self.audio[piste.index()].is_some() {
            1000
        } else {
            0
        }
    }

    fn gain_clip_milliemes_at(&self, maintenant: Instant) -> u32 {
        match self.clip.as_ref() {
            Some(clip) if !clip.est_termine(maintenant) => 1000,
            _ => 0,
        }
    }

    /// Phases à un instant donné (entrée, sortie, ou figées si rien ne bouge).
    fn phases_at(&self, maintenant: Instant) -> Phases {
        let Some(fin) = self.fin else {
            return self.phases_entree(maintenant);
        };
        let base = self.depart;
        let t = maintenant.saturating_duration_since(fin).as_secs_f32();
        let k_voile = bezier_css(EASE, (t / FONDU_VOILE_S).clamp(0.0, 1.0));
        let voile = base.voile * (1.0 - k_voile);
        // Carte : `.visible` retiré → fondu 0,55 s après 0,65 s (0 s en card).
        let delai_carte = match self.mode {
            ModePresentation::Theatrical => CARTE_DELAI_S,
            ModePresentation::Card => 0.0,
        };
        let k_carte = ((t - delai_carte).max(0.0) / CARTE_FONDU_S).clamp(0.0, 1.0);
        let carte_opacite = base.carte_opacite * (1.0 - bezier_css(EASE, k_carte));
        let carte_avance = base.carte_avance * (1.0 - bezier_css(CARTE_EASE, k_carte));
        if self.mode == ModePresentation::Card {
            // Mode card : le décor n'existe pas, seuls le voile et la carte
            // s'éteignent.
            return Phases {
                voile,
                carte_opacite,
                carte_avance,
                ..Phases::default()
            };
        }
        let k_decor = bezier_css(EASE, (t / FONDU_DECOR_S).clamp(0.0, 1.0));
        let k_embrasse = bezier_css(EASE, (t / FONDU_EMBRASSE_S).clamp(0.0, 1.0));
        let k_rideau = bezier_css(EASE_RIDEAU, (t / OUVERTURE_S).clamp(0.0, 1.0));
        Phases {
            voile,
            valance: base.valance * (1.0 - k_decor),
            public: base.public * (1.0 - k_decor),
            embrasses: base.embrasses * (1.0 - k_embrasse),
            // Le `.open` retiré ramène l'embrasse à son repos (`-16 px`).
            embrasses_decalage: base.embrasses_decalage
                + (EMBRASSE_DECALAGE - base.embrasses_decalage) * k_embrasse,
            rideaux: base.rideaux * (1.0 - k_rideau),
            carte_opacite,
            carte_avance,
        }
    }

    /// Phases d'une entrée (ou d'une reprise) au temps courant.
    fn phases_entree(&self, maintenant: Instant) -> Phases {
        let Some(debut) = self.debut else {
            return Phases::default();
        };
        let base = self.depart;
        let t = maintenant.saturating_duration_since(debut).as_secs_f32();
        let k_voile = bezier_css(EASE, (t / FONDU_VOILE_S).clamp(0.0, 1.0));
        let voile = base.voile + (1.0 - base.voile) * k_voile;
        let tc = self
            .debut_chore
            .map(|d| maintenant.saturating_duration_since(d).as_secs_f32())
            .unwrap_or(0.0);
        // Carte : retard 0,65 s en théâtral, immédiat en mode `card`.
        let delai_carte = match self.mode {
            ModePresentation::Theatrical => CARTE_DELAI_S,
            ModePresentation::Card => 0.0,
        };
        let k_carte = ((tc - delai_carte).max(0.0) / CARTE_FONDU_S).clamp(0.0, 1.0);
        let carte_opacite =
            base.carte_opacite + (1.0 - base.carte_opacite) * bezier_css(EASE, k_carte);
        let carte_avance =
            base.carte_avance + (1.0 - base.carte_avance) * bezier_css(CARTE_EASE, k_carte);
        if self.mode == ModePresentation::Card {
            return Phases {
                voile,
                carte_opacite,
                carte_avance,
                ..Phases::default()
            };
        }
        let k_decor = bezier_css(EASE, (tc / FONDU_DECOR_S).clamp(0.0, 1.0));
        let k_embrasse = bezier_css(
            EASE,
            ((tc - DELAI_EMBRASSE_S).max(0.0) / FONDU_EMBRASSE_S).clamp(0.0, 1.0),
        );
        let k_rideau = bezier_css(
            EASE_RIDEAU,
            ((tc - DELAI_OUVERTURE_S).max(0.0) / OUVERTURE_S).clamp(0.0, 1.0),
        );
        // Au repos l'embrasse est 16 px plus haut (`translateY(-16px)`) : une
        // entrée partie du noir démarre de cette position, pas de 0.
        let depart_embrasse = if base.embrasses > 0.0 {
            base.embrasses_decalage
        } else {
            EMBRASSE_DECALAGE
        };
        Phases {
            voile,
            valance: base.valance + (1.0 - base.valance) * k_decor,
            public: base.public + (1.0 - base.public) * k_decor,
            embrasses: base.embrasses + (1.0 - base.embrasses) * k_embrasse,
            embrasses_decalage: depart_embrasse + (0.0 - depart_embrasse) * k_embrasse,
            rideaux: base.rideaux + (1.0 - base.rideaux) * k_rideau,
            carte_opacite,
            carte_avance,
        }
    }
}

/// `cubic-bezier(x1,y1,x2,y2)` CSS à `x` ∈ [0,1] (dichotomie).
fn bezier_css((x1, y1, x2, y2): (f32, f32, f32, f32), x: f32) -> f32 {
    fn courbe(a1: f32, a2: f32, t: f32) -> f32 {
        let u = 1.0 - t;
        3.0 * u * u * t * a1 + 3.0 * u * t * t * a2 + t * t * t
    }
    let x = x.clamp(0.0, 1.0);
    let (mut bas, mut haut) = (0.0_f32, 1.0_f32);
    let mut t = x;
    for _ in 0..32 {
        let v = courbe(x1, x2, t);
        if (v - x).abs() < 1e-5 {
            break;
        }
        if v < x {
            bas = t;
        } else {
            haut = t;
        }
        t = (bas + haut) / 2.0;
    }
    courbe(y1, y2, t)
}

fn ease_in_out(t: f32) -> f32 {
    bezier_css(EASE_IN_OUT, t)
}

/// Progression `alternate` d'une animation CSS infinie : `p` dans [0,1],
/// `delai` négatif = animation déjà commencée (comme `animation-delay`).
fn progression_alternee(temps_s: f32, cycle_s: f32, delai_s: f32) -> f32 {
    let p = ((temps_s + delai_s) / cycle_s).rem_euclid(2.0);
    if p > 1.0 {
        2.0 - p
    } else {
        p
    }
}

// =============================================================================
// BLOC DE RENDU (géométries + tirage déterministe du décor)
// =============================================================================

/// Tirage pseudo-aléatoire déterministe (xorshift64) — remplace les
/// `Math.random()` du DOM, qui ne sont pas reproductibles.
struct Alea(u64);

impl Alea {
    fn nouveau(graine: u64) -> Self {
        Self(graine | 1)
    }

    /// `[0, 1)` — miroir de `Math.random()`.
    fn suivant(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 11) as f64 / (1u64 << 53) as f64) as f32
    }
}

/// Un grain de poussière (`--pd` / `--pdl` tirés à l'init du DOM).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grain {
    x: f32,
    y: f32,
    taille: f32,
    duree_s: f32,
    delai_s: f32,
}

/// Silhouette d'une place (tête + épaules).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tete {
    largeur: f32,
    hauteur: f32,
    /// `bottom` en px depuis l'ancre de la rangée.
    bas: f32,
    /// `left: (50 + (Math.random()-0.5)*14) %` de la largeur de la place.
    decalage_pct: f32,
    duree_s: f32,
    delai_s: f32,
}

/// Une place du public (position **canvas**, calculée une fois).
///
/// La hauteur de la boîte du DOM n'intervient dans aucun dessin (les places
/// sont ancrées par `bottom: 0` sur la ligne de rangée) : elle n'est pas
/// conservée.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Place {
    x: f32,
    largeur: f32,
    /// Ancre de la rangée (y du bas de la place).
    ancre: f32,
    tete: Option<Tete>,
}

pub(super) struct BlocPresentation {
    cle: u64,
    /// Trapèzes des 3 faisceaux, en coordonnées canvas (rotation au dessin).
    spots: Vec<ID2D1PathGeometry>,
    /// Fauteuil de chaque rangée en coordonnées **locales** (0,0 → l×h),
    /// `None` pour la rangée du premier plan (sans fauteuil).
    sieges: Vec<Option<ID2D1PathGeometry>>,
    /// Places par rangée (0 = fond, 2 = premier plan).
    public: Vec<Vec<Place>>,
    poussiere: Vec<Grain>,
}

fn cle_presentation(version: u64, mode: ModePresentation, canvas: (u32, u32)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    mode.hash(&mut h);
    canvas.hash(&mut h);
    h.finish()
}

/// Une ligne de texte mise en page de la carte (les handles COM sont
/// clonables : `clone()` ne fait qu'incrémenter leur compteur).
#[derive(Clone)]
pub(super) struct LigneC {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    largeur: f32,
    hauteur: f32,
    /// `line-height` en px — sert au rognage `-webkit-line-clamp`.
    interligne: f32,
}

impl LigneC {
    /// Hauteur visible après `-webkit-line-clamp: N` (rognage, sans ellipse).
    fn clampe(&self, lignes: f32) -> f32 {
        self.hauteur.min(lignes * self.interligne)
    }
}

/// Une primitive de la carte, dans l'ordre de dessin.
#[derive(Clone)]
enum Elem {
    /// Texte déjà positionné (boîte = largeur de colonne, alignement inclus).
    Texte {
        ligne: LigneC,
        x: f32,
        y: f32,
        couleur: [u8; 4],
    },
    /// Pastille arrondie (badge PARTNER/AFFILIATE, bandeau EN DIRECT).
    Pastille {
        rect: (f32, f32, f32, f32),
        rayon: f32,
        /// `(début, fin)` — identiques = couleur unie.
        fond: ([u8; 4], [u8; 4]),
        /// Texte + position absolue (coin haut-gauche de la boîte de texte).
        texte: Option<(LigneC, f32, f32)>,
    },
    /// Pastille ronde (`::before` du bandeau EN DIRECT).
    Cercle {
        cx: f32,
        cy: f32,
        r: f32,
        couleur: [u8; 4],
    },
    /// Avatar rond (`cover` + bord blanc + anneau violet).
    Avatar {
        rect: (f32, f32, f32, f32),
        url: String,
    },
    /// Image arrondie (boîte d'un jeu récent).
    Image {
        rect: (f32, f32, f32, f32),
        rayon: f32,
        url: String,
    },
    /// Vignette de clip (grille 2×2) : image `cover` + durée + titre, le tout
    /// rogné au rectangle arrondi de la cellule (`overflow: hidden`).
    Clip {
        rect: (f32, f32, f32, f32),
        url: String,
        /// `Math.round(duration) + "s"` (mise en page une fois).
        duree: LigneC,
        /// Coin haut-gauche de la pastille de durée.
        dx: f32,
        dy: f32,
        /// Titre mis en page sur la largeur utile (`nowrap`, rogné avant la
        /// pastille) + position.
        titre: LigneC,
        tx: f32,
        ty: f32,
    },
}

impl Elem {
    /// Décale une primitive (la carte est centrée après coup).
    fn decaler(&mut self, dx: f32, dy: f32) {
        match self {
            Elem::Texte { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Elem::Pastille { rect, texte, .. } => {
                rect.0 += dx;
                rect.1 += dy;
                if let Some((_, tx, ty)) = texte {
                    *tx += dx;
                    *ty += dy;
                }
            }
            Elem::Cercle { cx, cy, .. } => {
                *cx += dx;
                *cy += dy;
            }
            Elem::Avatar { rect, .. } | Elem::Image { rect, .. } => {
                rect.0 += dx;
                rect.1 += dy;
            }
            Elem::Clip {
                rect, tx, ty, dx: px, dy: py, ..
            } => {
                rect.0 += dx;
                rect.1 += dy;
                *tx += dx;
                *ty += dy;
                *px += dx;
                *py += dy;
            }
        }
    }
}

fn decaler(elements: Vec<Elem>, dx: f32, dy: f32) -> Vec<Elem> {
    elements
        .into_iter()
        .map(|mut e| {
            e.decaler(dx, dy);
            e
        })
        .collect()
}

/// Paramètres d'une vignette de clip (regroupement clippy
/// `too_many_arguments`, convention §37.4.5).
struct CelluleClip<'a> {
    rect: (f32, f32, f32, f32),
    url: &'a str,
    /// Durée déjà mise en page (`"30s"`).
    duree: &'a LigneC,
    /// Coin haut-gauche de la pastille de durée.
    pastille: (f32, f32),
    titre: &'a LigneC,
    /// Position du titre.
    titre_pos: (f32, f32),
}

/// Spécification d'une ligne de la carte.
struct SpecLigne<'a> {
    texte: &'a str,
    taille: f32,
    poids: DWRITE_FONT_WEIGHT,
    espacement: f32,
    largeur: f32,
    wrap: bool,
    interligne: f32,
    centre: bool,
    /// `text-overflow: ellipsis` (titres `nowrap` de la grille et du lecteur).
    tronquer: bool,
}

#[derive(Clone)]
pub(super) struct BlocCarte {
    cle: u64,
    /// Boîte de la carte (centre = point de pivot du transform).
    boite: (f32, f32, f32, f32),
    elements: Vec<Elem>,
}

/// Répartition des deux colonnes de la carte — `flex` du DOM résolu à la main.
///
/// En mode `card` les colonnes sont empilées : chacune garde sa largeur
/// naturelle bornée par la carte (`max-width: 760px`). En théâtral elles sont
/// côte à côte : si l'ensemble dépasse la largeur de la carte
/// (`max-width: 73 %`), les deux sont réduites **proportionnellement** — ce que
/// fait `flex-shrink: 1` avec des bases de contenu.
fn repartition_colonnes(
    card: bool,
    max_carte: f32,
    gauche_nat: f32,
    droite_nat: f32,
) -> (f32, f32) {
    if card {
        return (gauche_nat.min(max_carte), droite_nat.min(max_carte));
    }
    let total = gauche_nat + CARTE_GAP + droite_nat;
    if total > max_carte && gauche_nat + droite_nat > 0.0 {
        let k = ((max_carte - CARTE_GAP) / (gauche_nat + droite_nat)).max(0.05);
        (gauche_nat * k, droite_nat * k)
    } else {
        (gauche_nat, droite_nat)
    }
}

fn cle_carte(version: u64, mode: ModePresentation, canvas: (u32, u32)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    version.hash(&mut h);
    mode.hash(&mut h);
    canvas.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine une **couche** de la présentation — dans une passe ouverte.
    ///
    /// `couche` = [`COUCHE_VOILE`] (z 10000), [`COUCHE_RIDEAUX`] (10001) ou
    /// [`COUCHE_PUBLIC`] (10003) : le plan de composition porte une entrée par
    /// couche, la carte (10002, tranche E6b) s'intercale entre les deux
    /// dernières.
    pub fn dessiner_presentation(
        &mut self,
        etat: &EtatPresentation,
        couche: usize,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(rendu) = etat.rendu() else {
            // Entièrement sorti : décor, bannière et géométries libérés
            // (le DOM retire les nœuds après le `setTimeout`).
            self.bloc_presentation = None;
            self.bloc_carte = None;
            self.pres_images_tentative.clear();
            self.pres_clip_media = None;
            self.pres_clip_url = None;
            self.bloc_clip = None;
            self.pres_audios = [None, None];
            self.pres_audio_rel = [None, None];
            return Ok(());
        };
        // Pistes audio : chargées/avancées une fois par frame, quelle que soit
        // la couche peinte.
        self.synchroniser_audios(&rendu.audio, temps_s);
        self.synchroniser_bloc_presentation(etat.version(), rendu.mode, canvas)?;
        match couche {
            COUCHE_VOILE => self.peindre_voile(&rendu, canvas, temps_s),
            COUCHE_RIDEAUX => self.peindre_rideaux(&rendu, canvas),
            COUCHE_PUBLIC => self.peindre_public(&rendu, canvas, temps_s),
            COUCHE_CARTE => self.peindre_carte(&rendu, etat.version(), canvas),
            COUCHE_CLIP => self.peindre_clip(&rendu, etat.version(), canvas, temps_s),
            _ => Ok(()),
        }
    }

    /// (Re)construit le décor quand la clé change (version, mode, canvas).
    fn synchroniser_bloc_presentation(
        &mut self,
        version: u64,
        mode: ModePresentation,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let cle = cle_presentation(version, mode, canvas);
        if self.bloc_presentation.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let mut spots = Vec::with_capacity(3);
        for (x, largeur) in positions_spots(w) {
            let points: Vec<(f64, f64)> = SPOT_TRAPEZE
                .iter()
                .map(|(fx, fy)| {
                    (
                        (x + fx * largeur) as f64,
                        (SPOT_TOP + fy * SPOT_HAUTEUR * h) as f64,
                    )
                })
                .collect();
            spots.push(
                crate::cadres::geometrie_fermee(&self.d2d, &points)
                    .map_err(|e| ErreurTexte::Systeme(e.to_string()))?,
            );
        }
        let mut sieges = Vec::with_capacity(RANGEES.len());
        for r in RANGEES.iter() {
            if r.siege_l <= 0.0 || r.siege_h <= 0.0 {
                sieges.push(None);
                continue;
            }
            sieges.push(Some(geometrie_fauteuil(
                &self.d2d,
                r.siege_l,
                r.siege_h,
            )?));
        }
        let (public, poussiere) = tirer_decor(w, h);
        self.bloc_presentation = Some(BlocPresentation {
            cle,
            spots,
            sieges,
            public,
            poussiere,
        });
        Ok(())
    }

    /// Image distante de la présentation (bannière, avatar, boîte de jeu) —
    /// lue dans le **cache disque** de l'app (`medias/cache/<fnv16(url)>`,
    /// lot G), re-testée toutes les 500 ms tant que le fichier n'existe pas.
    ///
    /// Le crate ne fait aucun réseau (§13.8) : c'est l'app qui télécharge, sur
    /// la frame `presentation-show`. L'équivalent natif du `<img>` du DOM qui
    /// se répare tout seul.
    fn image_presentation(&mut self, url: &str) -> Option<super::BitmapChat> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        let chemin = crate::chat::chemin_cache(&self.base, url);
        if let Some(deja) = self.images_chat.get(&chemin) {
            return Some(deja.clone());
        }
        let relance = self
            .pres_images_tentative
            .get(url)
            .map(|t: &Instant| t.elapsed().as_millis() >= 500)
            .unwrap_or(true);
        if !relance {
            return None;
        }
        self.pres_images_tentative.insert(url.to_string(), Instant::now());
        let image = self.image_cache(url)?;
        self.pres_images_tentative.remove(url);
        Some(image)
    }

    /// Couche d'opacité + décor complet du voile (`z 10000`).
    fn peindre_voile(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let opacite = rendu.phases.voile.clamp(0.0, 1.0);
        if opacite <= 0.0 {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer présentation : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = self.peindre_decor(rendu, canvas, temps_s);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Fond, bannière, orbes, sol, faisceaux et poussière (dans cet ordre).
    fn peindre_decor(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let plein = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };

        // `.pres-voile` : `radial-gradient(ellipse at 50% 42%, …)` — taille par
        // défaut `farthest-corner` : ellipse au ratio de la boîte passant par
        // le coin le plus éloigné du centre.
        let (cx, cy) = (VOILE_CENTRE.0 * w, VOILE_CENTRE.1 * h);
        let mut etendue = 0.0_f32;
        for (dx, dy) in [(cx, cy), (w - cx, cy), (cx, h - cy), (w - cx, h - cy)] {
            etendue = etendue.max(((dx / w).powi(2) + (dy / h).powi(2)).sqrt());
        }
        let stops = [
            (0.0, VOILE_CLAIR),
            (VOILE_ARRET, VOILE_SOMBRE),
            (1.0, VOILE_SOMBRE),
        ];
        let fond = self.pres_brosse_radiale(&stops, (cx, cy), (etendue * w, etendue * h))?;
        unsafe { self.cible_rt.FillRectangle(&plein, &fond) };

        // `.pres-fond` (1er enfant du voile) : bannière `cover` + scrim.
        if let Some((_, bitmap, (iw, ih))) = self.image_presentation(&rendu.donnees.banniere) {
            if iw > 0.0 && ih > 0.0 {
                let e = (w / iw).max(h / ih);
                let (dw, dh) = (iw * e, ih * e);
                let dst = D2D_RECT_F {
                    left: (w - dw) / 2.0,
                    top: (h - dh) / 2.0,
                    right: (w + dw) / 2.0,
                    bottom: (h + dh) / 2.0,
                };
                unsafe {
                    self.cible_rt.DrawBitmap(
                        &bitmap,
                        Some(&dst),
                        1.0,
                        windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
            }
            let scrim = self.pres_brosse_verticale(&SCRIM_STOPS, 0.0, h)?;
            unsafe { self.cible_rt.FillRectangle(&plein, &scrim) };
        }

        // Les 3 orbes (`@keyframes pres-orbe` : 14 s alterné).
        for orbe in ORBES.iter() {
            self.peindre_orbe(orbe, canvas, temps_s)?;
        }

        self.peindre_sol(canvas)?;
        self.peindre_spots(canvas, temps_s)?;
        self.peindre_poussiere(temps_s)?;
        Ok(())
    }

    /// Un orbe : disque à dégradé radial (approximation du `blur(90px)`).
    fn peindre_orbe(
        &mut self,
        orbe: &Orbe,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let x = if orbe.depuis_droite {
            w - orbe.taille - orbe.x
        } else if orbe.x_relatif {
            orbe.x * w
        } else {
            orbe.x
        };
        let y = if orbe.depuis_bas {
            h - orbe.taille - orbe.y
        } else if orbe.y_relatif {
            orbe.y * h
        } else {
            orbe.y
        };
        let p = progression_alternee(temps_s as f32, ORBE_CYCLE_S, -orbe.delai_s);
        let k = ease_in_out(p);
        let (dx, dy) = (
            ORBE_DEPLACEMENT.0 * k,
            ORBE_DEPLACEMENT.1 * k,
        );
        let echelle = 1.0 + ORBE_ECHELLE * k;
        let (cx, cy) = (x + orbe.taille / 2.0, y + orbe.taille / 2.0);
        let mtx = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle) + dx,
            M32: cy * (1.0 - echelle) + dy,
        };
        // Rayon du dégradé : le disque plus l'étendue du flou.
        let rayon = orbe.taille / 2.0 + ORBE_FLOU;
        let opaque = (orbe.taille / 2.0 - ORBE_FLOU).max(0.0) / rayon;
        let a = (orbe.couleur[3] as f32 / 255.0) * ORBE_OPACITE;
        let couleurs = [
            (
                0.0,
                [
                    orbe.couleur[0],
                    orbe.couleur[1],
                    orbe.couleur[2],
                    (a * 255.0).round() as u8,
                ],
            ),
            (
                opaque,
                [
                    orbe.couleur[0],
                    orbe.couleur[1],
                    orbe.couleur[2],
                    (a * 255.0).round() as u8,
                ],
            ),
            (
                1.0,
                [orbe.couleur[0], orbe.couleur[1], orbe.couleur[2], 0],
            ),
        ];
        let brosse = self.pres_brosse_radiale(&couleurs, (cx, cy), (rayon, rayon))?;
        unsafe { self.cible_rt.SetTransform(&mtx) };
        unsafe {
            self.cible_rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: rayon,
                    radiusY: rayon,
                },
                &brosse,
            )
        };
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    /// Sol de scène : dégradé vertical + lueur rasante + liseré de lumière.
    fn peindre_sol(&mut self, canvas: (u32, u32)) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let hauteur = SOL_HAUTEUR * h;
        let sol = D2D_RECT_F {
            left: 0.0,
            top: h - hauteur,
            right: w,
            bottom: h,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&sol, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let base = self.pres_brosse_verticale(&SOL_STOPS, sol.top, sol.bottom)?;
        unsafe { self.cible_rt.FillRectangle(&sol, &base) };
        // `radial-gradient(ellipse 62% 95% at 50% 112%, …)`.
        let lueur = self.pres_brosse_radiale(
            &[(0.0, SOL_LUEUR), (0.60, [SOL_LUEUR[0], SOL_LUEUR[1], SOL_LUEUR[2], 0]), (1.0, [SOL_LUEUR[0], SOL_LUEUR[1], SOL_LUEUR[2], 0])],
            (0.5 * w, sol.top + 1.12 * hauteur),
            (0.62 * w, 0.95 * hauteur),
        )?;
        unsafe { self.cible_rt.FillRectangle(&sol, &lueur) };
        // `::before` — liseré de 2 px à 8 % des bords.
        let liseré = D2D_RECT_F {
            left: 0.08 * w,
            top: sol.top,
            right: 0.92 * w,
            bottom: sol.top + 2.0,
        };
        let brosse = self.pres_brosse_horizontale(
            &[
                (0.0, [SOL_LISERE[0], SOL_LISERE[1], SOL_LISERE[2], 0]),
                (0.5, SOL_LISERE),
                (1.0, [SOL_LISERE[0], SOL_LISERE[1], SOL_LISERE[2], 0]),
            ],
            liseré.left,
            liseré.right,
        )?;
        unsafe { self.cible_rt.FillRectangle(&liseré, &brosse) };
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        Ok(())
    }

    /// Les 3 faisceaux : trapèze + rotation (parent) + balancement (enfant).
    fn peindre_spots(&mut self, canvas: (u32, u32), temps_s: f64) -> Result<(), ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(bloc) = self.bloc_presentation.as_ref() else {
            return Ok(());
        };
        let positions = positions_spots(w);
        let angles = [17.0_f32, -17.0, 0.0];
        let delais = [0.0_f32, 4.0, 7.0];
        let hauteur = SPOT_HAUTEUR * h;
        for (i, geo) in bloc.spots.iter().enumerate() {
            let (x, largeur) = positions[i];
            let p = progression_alternee(temps_s as f32, SPOT_CYCLE_S, -delais[i]);
            let balancement = -SPOT_BALANCE + 2.0 * SPOT_BALANCE * ease_in_out(p);
            // Le parent porte l'angle de base, l'enfant le balancement : même
            // origine (`50 % 0`), donc une seule rotation composée.
            let cx = x + largeur / 2.0;
            let mtx = Matrix3x2::rotation(angles[i] + balancement, cx, SPOT_TOP);
            let brosse = self.pres_brosse_verticale(&SPOT_STOPS, SPOT_TOP, SPOT_TOP + hauteur)?;
            unsafe { self.cible_rt.SetTransform(&mtx) };
            unsafe { self.cible_rt.FillGeometry(geo, &brosse, None) };
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        }
        Ok(())
    }

    /// Poussière : 20 grains qui dérivent (`@keyframes pres-poussiere-drift`).
    fn peindre_poussiere(&mut self, temps_s: f64) -> Result<(), ErreurTexte> {
        let Some(bloc) = self.bloc_presentation.as_ref() else {
            return Ok(());
        };
        let grains = bloc.poussiere.clone();
        let brosse: ID2D1SolidColorBrush =
            unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(POUSSIERE_COULEUR), None) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse poussière : {}", e.message()))
                })?;
        let t = temps_s as f32;
        for grain in &grains {
            let u = progression_alternee(t, grain.duree_s, -grain.delai_s);
            let e = ease_in_out(u);
            // Keyframes d'opacité : 0 → 0,12 ; 50 % → 0,5 ; 100 % → 0,22.
            let k = if u < 0.5 {
                0.12 + (0.5 - 0.12) * ease_in_out(u * 2.0)
            } else {
                0.5 + (0.22 - 0.5) * ease_in_out((u - 0.5) * 2.0)
            };
            let alpha = (POUSSIERE_COULEUR[3] as f32 / 255.0) * k;
            unsafe {
                brosse.SetColor(&couleur_d2d([
                    POUSSIERE_COULEUR[0],
                    POUSSIERE_COULEUR[1],
                    POUSSIERE_COULEUR[2],
                    (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
                ]))
            };
            let cx = grain.x + POUSSIERE_DEPLACEMENT.0 * e;
            let cy = grain.y + POUSSIERE_DEPLACEMENT.1 * e;
            let r = grain.taille / 2.0;
            unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cx, y: cy },
                        radiusX: r,
                        radiusY: r,
                    },
                    &brosse,
                )
            };
        }
        Ok(())
    }

    /// Rideaux, cantonné festonné et embrasses (`z 10001`).
    fn peindre_rideaux(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        if rendu.mode == ModePresentation::Card {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let k = rendu.phases.rideaux.clamp(0.0, 1.0);
        let demi = w / 2.0;
        let echelle = 1.0 - RIDEAU_ECHELLE * k;
        let translation = RIDEAU_TRANSLATE * demi * k;

        // Gauche : `transform-origin: left center` → origine (0, h/2).
        let mtx_g = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: 1.0,
            M31: -translation,
            M32: 0.0,
        };
        unsafe { self.cible_rt.SetTransform(&mtx_g) };
        self.peindre_rideau(
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: demi,
                bottom: h,
            },
            true,
        )?;
        // Droite : `transform-origin: right center` → origine (w, h/2).
        let mtx_d = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: 1.0,
            M31: w * (1.0 - echelle) + translation,
            M32: 0.0,
        };
        unsafe { self.cible_rt.SetTransform(&mtx_d) };
        self.peindre_rideau(
            D2D_RECT_F {
                left: demi,
                top: 0.0,
                right: w,
                bottom: h,
            },
            false,
        )?;
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };

        self.peindre_valance(rendu, canvas)?;
        self.peindre_embrasses(rendu, canvas)?;
        Ok(())
    }

    /// Un rideau : plis (brosse `WRAP`), ombre verticale, ourlet, bord doré.
    fn peindre_rideau(&mut self, rect: D2D_RECT_F, gauche: bool) -> Result<(), ErreurTexte> {
        let plis = self.pres_brosse_periodique(&PLIS_STOPS, rect.left, PLI_PERIODE)?;
        unsafe { self.cible_rt.FillRectangle(&rect, &plis) };
        let ombre = self.pres_brosse_verticale(&RIDEAU_OMBRE_STOPS, rect.top, rect.bottom)?;
        unsafe { self.cible_rt.FillRectangle(&rect, &ombre) };
        // `::after` — ourlet doré de 14 px en bas.
        let ourlet = D2D_RECT_F {
            left: rect.left,
            top: rect.bottom - OURLET_HAUTEUR,
            right: rect.right,
            bottom: rect.bottom,
        };
        let brosse = self.pres_brosse_verticale(&OURLET_STOPS, ourlet.top, ourlet.bottom)?;
        unsafe { self.cible_rt.FillRectangle(&ourlet, &brosse) };
        // `border-right` (gauche) / `border-left` (droite) — 6 px dorés.
        let bord = D2D_RECT_F {
            left: if gauche {
                rect.right - RIDEAU_BORD
            } else {
                rect.left
            },
            top: rect.top,
            right: if gauche {
                rect.right
            } else {
                rect.left + RIDEAU_BORD
            },
            bottom: rect.bottom,
        };
        let brosse: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(RIDEAU_BORD_COULEUR), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse bord rideau : {}", e.message())))?;
        unsafe { self.cible_rt.FillRectangle(&bord, &brosse) };
        Ok(())
    }

    /// Cantonné : drapé horizontal + festons (deux cercles par tuile de 192 px).
    fn peindre_valance(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let opacite = rendu.phases.valance.clamp(0.0, 1.0);
        if opacite <= 0.0 {
            return Ok(());
        }
        let (w, _h) = (canvas.0 as f32, canvas.1 as f32);
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: VALANCE_HAUTEUR + FESTON_HAUTEUR,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer valance : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = self.peindre_valance_corps(w);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Corps du cantonné — dans la couche d'opacité (toujours dépilée).
    fn peindre_valance_corps(&mut self, w: f32) -> Result<(), ErreurTexte> {
        let rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: VALANCE_HAUTEUR,
        };
        let plis = self.pres_brosse_periodique(&PLIS_STOPS, 0.0, PLI_PERIODE)?;
        unsafe { self.cible_rt.FillRectangle(&rect, &plis) };
        let ombre = self.pres_brosse_verticale(&VALANCE_OMBRE_STOPS, 0.0, VALANCE_HAUTEUR)?;
        unsafe { self.cible_rt.FillRectangle(&rect, &ombre) };

        // `::after` — festons : cercle doré (dessous) puis rouge (dessus).
        let mut x = 0.0_f32;
        while x < w {
            let dore = D2D_RECT_F {
                left: x,
                top: VALANCE_HAUTEUR + 4.0,
                right: x + FESTON_PERIODE,
                bottom: VALANCE_HAUTEUR + 4.0 + 44.0,
            };
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&dore, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            let brosse = self.pres_brosse_radiale(
                &FESTON_DORE,
                (x + FESTON_PERIODE / 2.0, VALANCE_HAUTEUR + 4.0),
                (104.0, 104.0),
            )?;
            unsafe { self.cible_rt.FillRectangle(&dore, &brosse) };
            unsafe { self.cible_rt.PopAxisAlignedClip() };

            let rouge = D2D_RECT_F {
                left: x,
                top: VALANCE_HAUTEUR,
                right: x + FESTON_PERIODE,
                bottom: VALANCE_HAUTEUR + 40.0,
            };
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&rouge, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            let brosse = self.pres_brosse_radiale(
                &FESTON_ROUGE,
                (x + FESTON_PERIODE / 2.0, VALANCE_HAUTEUR),
                (100.0, 100.0),
            )?;
            unsafe { self.cible_rt.FillRectangle(&rouge, &brosse) };
            unsafe { self.cible_rt.PopAxisAlignedClip() };
            x += FESTON_PERIODE;
        }
        Ok(())
    }

    /// Embrasses dorées : bandes arrondies retenant les rideaux froncés.
    fn peindre_embrasses(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let opacite = rendu.phases.embrasses.clamp(0.0, 1.0);
        if opacite <= 0.0 {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let brosse = self.pres_brosse_verticale(
            &EMBRASSE_STOPS,
            0.0,
            EMBRASSE_HAUTEUR,
        )?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer embrasses : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let resultat = self.peindre_embrasses_corps(rendu, w, h, brosse);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Corps des embrasses — dans la couche d'opacité (toujours dépilée).
    fn peindre_embrasses_corps(
        &mut self,
        rendu: &RenduPresentation<'_>,
        w: f32,
        h: f32,
        brosse: ID2D1Brush,
    ) -> Result<(), ErreurTexte> {
        let top = EMBRASSE_TOP * h;
        for (gauche, angle) in [(true, -EMBRASSE_ANGLE), (false, EMBRASSE_ANGLE)] {
            let x = if gauche {
                EMBRASSE_MARGE
            } else {
                w - EMBRASSE_MARGE - EMBRASSE_LARGEUR
            };
            let (cx, cy) = (x + EMBRASSE_LARGEUR / 2.0, top + EMBRASSE_HAUTEUR / 2.0);
            // CSS `rotate(∓24deg) translateY(dy)` : le décalage est appliqué
            // **avant** la rotation (transformée avec elle).
            let mtx = Matrix3x2::translation(0.0, rendu.phases.embrasses_decalage)
                * Matrix3x2::rotation(angle, cx, cy);
            unsafe { self.cible_rt.SetTransform(&mtx) };
            let rect = D2D_RECT_F {
                left: x,
                top,
                right: x + EMBRASSE_LARGEUR,
                bottom: top + EMBRASSE_HAUTEUR,
            };
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                        rect,
                        radiusX: EMBRASSE_HAUTEUR / 2.0,
                        radiusY: EMBRASSE_HAUTEUR / 2.0,
                    },
                    &brosse,
                )
            };
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        }
        Ok(())
    }

    /// Public : 3 rangées de fauteuils + silhouettes (z 10003).
    fn peindre_public(
        &mut self,
        rendu: &RenduPresentation<'_>,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        if rendu.mode == ModePresentation::Card {
            return Ok(());
        }
        let opacite = rendu.phases.public.clamp(0.0, 1.0);
        if opacite <= 0.0 {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let Some(bloc) = self.bloc_presentation.as_ref() else {
            return Ok(());
        };
        let public = bloc.public.clone();
        let sieges = bloc.sieges.clone();

        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer public : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        // `overflow: hidden` du conteneur (240 px depuis le bas).
        let clip = D2D_RECT_F {
            left: 0.0,
            top: h - PUBLIC_HAUTEUR,
            right: w,
            bottom: h,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };

        let resultat = self.peindre_public_corps(&public, &sieges, temps_s);
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Corps du public — dans la couche et le clip du conteneur.
    fn peindre_public_corps(
        &mut self,
        public: &[Vec<Place>],
        sieges: &[Option<ID2D1PathGeometry>],
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        for (i, places) in public.iter().enumerate() {
            let rangee = &RANGEES[i];
            unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
            let siege = if rangee.siege_l > 0.0 {
                Some(self.pres_brosse_verticale(&SIEGE_STOPS, 0.0, rangee.siege_h)?)
            } else {
                None
            };
            let tete: ID2D1SolidColorBrush = unsafe {
                self.cible_rt
                    .CreateSolidColorBrush(&couleur_d2d(rangee.couleur), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse tête : {}", e.message())))?;

            for place in places {
                if let (Some(brosse), Some(geo)) = (siege.as_ref(), sieges[i].as_ref()) {
                    let sx = place.x + place.largeur / 2.0 - rangee.siege_l / 2.0;
                    let sy = place.ancre - rangee.siege_h;
                    let mtx = Matrix3x2::translation(sx, sy);
                    unsafe { self.cible_rt.SetTransform(&mtx) };
                    unsafe { self.cible_rt.FillGeometry(geo, brosse, None) };
                }
                let Some(t) = place.tete.as_ref() else {
                    continue;
                };
                let cx = place.x + place.largeur * (0.5 + t.decalage_pct / 100.0);
                let (bw, bh) = (t.largeur, t.hauteur);
                let bas = place.ancre - t.bas;
                let (haut, gauche) = (bas - bh, cx - bw / 2.0);
                // `@keyframes pres-tete-sway` : rotation autour du bas.
                let u = progression_alternee(temps_s as f32, t.duree_s, -t.delai_s);
                let e = ease_in_out(u);
                let angle = -TETE_BALANCE + 2.0 * TETE_BALANCE * e;
                let mtx = Matrix3x2::translation(0.0, -TETE_SOULEVE * e)
                    * Matrix3x2::rotation(angle, cx, bas);
                unsafe { self.cible_rt.SetTransform(&mtx) };
                unsafe {
                    self.cible_rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: D2D_POINT_2F {
                                x: cx,
                                y: haut + bh / 2.0,
                            },
                            radiusX: bw / 2.0,
                            radiusY: bh / 2.0,
                        },
                        &tete,
                    )
                };
                // `::after` — épaules : demi-ellipse de 1,64 × la largeur,
                // `top: 80 %`, `height: 55 %` de la tête.
                let ep_h = 0.55 * bh;
                let ep_top = haut + 0.80 * bh;
                let ep_w = 1.64 * bw;
                let clip_epaules = D2D_RECT_F {
                    left: gauche - 0.32 * bw,
                    top: ep_top,
                    right: gauche + bw + 0.32 * bw,
                    bottom: ep_top + ep_h,
                };
                unsafe {
                    self.cible_rt.PushAxisAlignedClip(
                        &clip_epaules,
                        D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    )
                };
                unsafe {
                    self.cible_rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: D2D_POINT_2F {
                                x: cx,
                                y: ep_top + ep_h,
                            },
                            radiusX: ep_w / 2.0,
                            radiusY: ep_h,
                        },
                        &tete,
                    )
                };
                unsafe { self.cible_rt.PopAxisAlignedClip() };
            }
        }
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        Ok(())
    }

    // ===== Carte (E6b) =====

    /// Peint la carte (`z 10002`) — couche d'opacité + transform d'entrée.
    fn peindre_carte(
        &mut self,
        rendu: &RenduPresentation<'_>,
        version: u64,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let opacite = rendu.phases.carte_opacite.clamp(0.0, 1.0);
        if opacite <= 0.0 {
            return Ok(());
        }
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        self.synchroniser_bloc_carte(version, rendu.donnees, rendu.mode, canvas)?;
        let Some(bloc) = self.bloc_carte.as_ref() else {
            return Ok(());
        };
        let (bx, by, bw, bh) = bloc.boite;
        let avance = rendu.phases.carte_avance;
        // `translateY(26px) scale(0.94)` → `none`, origine = centre de la carte.
        let echelle = (CARTE_ECHELLE + (1.0 - CARTE_ECHELLE) * avance).max(0.01);
        let decalage = CARTE_DECALAGE * (1.0 - avance);
        let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
        let mtx = Matrix3x2 {
            M11: echelle,
            M12: 0.0,
            M21: 0.0,
            M22: echelle,
            M31: cx * (1.0 - echelle),
            M32: cy * (1.0 - echelle) + decalage,
        };

        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w,
                bottom: h,
            },
            geometricMask: ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: opacite,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer carte : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        unsafe { self.cible_rt.SetTransform(&mtx) };
        let resultat = self.peindre_elements_carte();
        unsafe { self.cible_rt.SetTransform(&Matrix3x2::identity()) };
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        resultat
    }

    /// Peint la liste d'éléments de la carte, dans l'ordre de construction.
    fn peindre_elements_carte(&mut self) -> Result<(), ErreurTexte> {
        let Some(bloc) = self.bloc_carte.as_ref() else {
            return Ok(());
        };
        let elements = bloc.elements.clone();
        for elem in &elements {
            match elem {
                Elem::Texte {
                    ligne,
                    x,
                    y,
                    couleur,
                } => self.dessiner_ligne_c(ligne, *x, *y, *couleur)?,
                Elem::Pastille {
                    rect,
                    rayon,
                    fond,
                    texte,
                } => {
                    let r = D2D_RECT_F {
                        left: rect.0,
                        top: rect.1,
                        right: rect.0 + rect.2,
                        bottom: rect.1 + rect.3,
                    };
                    let brosse = if fond.0 == fond.1 {
                        let b: ID2D1SolidColorBrush = unsafe {
                            self.cible_rt.CreateSolidColorBrush(&couleur_d2d(fond.0), None)
                        }
                        .map_err(|e| {
                            ErreurTexte::Systeme(format!("brosse pastille : {}", e.message()))
                        })?;
                        b.into()
                    } else {
                        self.pres_brosse_css(&[(0.0, fond.0), (1.0, fond.1)], r, 135.0)?
                    };
                    unsafe {
                        self.cible_rt
                            .FillRoundedRectangle(
                                &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                                    rect: r,
                                    radiusX: *rayon,
                                    radiusY: *rayon,
                                },
                                &brosse,
                            )
                    };
                    if let Some((ligne, x, y)) = texte {
                        self.dessiner_ligne_c(ligne, *x, *y, [255, 255, 255, 255])?;
                    }
                }
                Elem::Cercle { cx, cy, r, couleur } => {
                    let brosse: ID2D1SolidColorBrush = unsafe {
                        self.cible_rt.CreateSolidColorBrush(&couleur_d2d(*couleur), None)
                    }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse pastille live : {}", e.message()))
                    })?;
                    unsafe {
                        self.cible_rt.FillEllipse(
                            &D2D1_ELLIPSE {
                                point: D2D_POINT_2F { x: *cx, y: *cy },
                                radiusX: *r,
                                radiusY: *r,
                            },
                            &brosse,
                        )
                    };
                }
                Elem::Avatar { rect, url } => self.dessiner_avatar(*rect, url)?,
                Elem::Image { rect, rayon, url } => self.dessiner_image(*rect, *rayon, url)?,
                Elem::Clip {
                    rect,
                    url,
                    duree,
                    dx,
                    dy,
                    titre,
                    tx,
                    ty,
                } => self.dessiner_clip(&CelluleClip {
                    rect: *rect,
                    url,
                    duree,
                    pastille: (*dx, *dy),
                    titre,
                    titre_pos: (*tx, *ty),
                })?,
            }
        }
        Ok(())
    }

    /// Avatar rond : image `cover` clippée au cercle, bord blanc 5 px, anneau
    /// violet 6 px (`box-shadow: 0 0 0 6px` — sans flou, donc porté).
    fn dessiner_avatar(
        &mut self,
        rect: (f32, f32, f32, f32),
        url: &str,
    ) -> Result<(), ErreurTexte> {
        let (x, y, d, _) = rect;
        let (cx, cy) = (x + d / 2.0, y + d / 2.0);
        let rayon_image = (d - 2.0 * AVATAR_BORD) / 2.0;
        if let Some((_, bitmap, (iw, ih))) = self.image_presentation(url) {
            if iw > 0.0 && ih > 0.0 {
                let geo: ID2D1EllipseGeometry = unsafe {
                    self.d2d.CreateEllipseGeometry(&D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: cx, y: cy },
                        radiusX: rayon_image,
                        radiusY: rayon_image,
                    })
                }
                .map_err(|e| ErreurTexte::Gpu(format!("masque avatar : {}", e.message())))?;
                let masque: ID2D1Geometry = geo
                    .cast()
                    .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry avatar : {}", e.message())))?;
                let mut params = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + d,
                        bottom: y + d,
                    },
                    geometricMask: ManuallyDrop::new(Some(masque)),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.0,
                    opacityBrush: ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                };
                let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
                    .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer avatar : {}", e.message())))?;
                unsafe { self.ctx.PushLayer(&params, &layer) };
                // `object-fit: cover` dans le cercle intérieur.
                let e = (2.0 * rayon_image / iw).max(2.0 * rayon_image / ih);
                let (dw, dh) = (iw * e, ih * e);
                let dst = D2D_RECT_F {
                    left: cx - dw / 2.0,
                    top: cy - dh / 2.0,
                    right: cx + dw / 2.0,
                    bottom: cy + dh / 2.0,
                };
                unsafe {
                    self.cible_rt.DrawBitmap(
                        &bitmap,
                        Some(&dst),
                        1.0,
                        windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
                unsafe { self.ctx.PopLayer() };
                unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
            }
        }
        // `border: 5px solid rgba(255,255,255,.9)` — trait centré sur le bord
        // extérieur de l'image (border-box).
        let brosse: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(AVATAR_BORD_COULEUR), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse bord avatar : {}", e.message())))?;
        unsafe {
            self.cible_rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: rayon_image + AVATAR_BORD / 2.0,
                    radiusY: rayon_image + AVATAR_BORD / 2.0,
                },
                &brosse,
                AVATAR_BORD,
                None,
            )
        };
        let brosse: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(AVATAR_ANNEAU_COULEUR), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse anneau avatar : {}", e.message())))?;
        let rayon_anneau = d / 2.0 + AVATAR_ANNEAU / 2.0;
        unsafe {
            self.cible_rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: rayon_anneau,
                    radiusY: rayon_anneau,
                },
                &brosse,
                AVATAR_ANNEAU,
                None,
            )
        };
        Ok(())
    }

    /// Image rectangulaire arrondie (`object-fit: cover` + `border-radius`).
    fn dessiner_image(
        &mut self,
        rect: (f32, f32, f32, f32),
        rayon: f32,
        url: &str,
    ) -> Result<(), ErreurTexte> {
        let (x, y, w, h) = rect;
        let Some((_, bitmap, (iw, ih))) = self.image_presentation(url) else {
            return Ok(());
        };
        if iw <= 0.0 || ih <= 0.0 {
            return Ok(());
        }
        let geo: ID2D1RoundedRectangleGeometry = unsafe {
            self.d2d.CreateRoundedRectangleGeometry(
                &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: rayon,
                    radiusY: rayon,
                },
            )
        }
        .map_err(|e| ErreurTexte::Gpu(format!("masque image : {}", e.message())))?;
        let masque: ID2D1Geometry = geo
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry image : {}", e.message())))?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer image : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        let e = (w / iw).max(h / ih);
        let (dw, dh) = (iw * e, ih * e);
        let dst = D2D_RECT_F {
            left: x + (w - dw) / 2.0,
            top: y + (h - dh) / 2.0,
            right: x + (w + dw) / 2.0,
            bottom: y + (h + dh) / 2.0,
        };
        unsafe {
            self.cible_rt.DrawBitmap(
                &bitmap,
                Some(&dst),
                1.0,
                windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            )
        };
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
    }

    /// Vignette de clip : image `cover`, bandeau de titre en dégradé, durée.
    ///
    /// Tout est **rogné** au rectangle arrondi de la cellule
    /// (`overflow: hidden` du DOM) — le titre `nowrap` est coupé net, sans
    /// ellipse (`text-overflow: ellipsis` non porté).
    fn dessiner_clip(&mut self, c: &CelluleClip<'_>) -> Result<(), ErreurTexte> {
        let CelluleClip {
            rect,
            url,
            duree,
            pastille,
            titre,
            titre_pos,
        } = c;
        let (x, y, w, h) = *rect;
        let (dx, dy) = *pastille;
        let (tx, ty) = *titre_pos;
        let geo: ID2D1RoundedRectangleGeometry = unsafe {
            self.d2d.CreateRoundedRectangleGeometry(
                &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: CLIP_RAYON,
                    radiusY: CLIP_RAYON,
                },
            )
        }
        .map_err(|e| ErreurTexte::Gpu(format!("masque clip : {}", e.message())))?;
        let masque: ID2D1Geometry = geo
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry clip : {}", e.message())))?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer clip : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };

        // `.pres-clip { background: rgba(255,255,255,.06) }` — visible tant
        // que la vignette n'est pas dans le cache.
        let fond: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 15]), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse clip : {}", e.message())))?;
        unsafe {
            self.cible_rt.FillRectangle(
                &D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                &fond,
            )
        };

        if let Some((_, bitmap, (iw, ih))) = self.image_presentation(url) {
            if iw > 0.0 && ih > 0.0 {
                let e = (w / iw).max(h / ih);
                let (dw, dh) = (iw * e, ih * e);
                let dst = D2D_RECT_F {
                    left: x + (w - dw) / 2.0,
                    top: y + (h - dh) / 2.0,
                    right: x + (w + dw) / 2.0,
                    bottom: y + (h + dh) / 2.0,
                };
                unsafe {
                    self.cible_rt.DrawBitmap(
                        &bitmap,
                        Some(&dst),
                        1.0,
                        windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
            }
        }

        // `.titre` — bandeau en dégradé (transparent → noir 85 %) + texte.
        let bande_h = CLIP_TITRE_PAD_HAUT + CLIP_TITRE_PAD_BAS + CLIP_TITRE_TAILLE * CLIP_TITRE_INTERLIGNE;
        let bande = D2D_RECT_F {
            left: x,
            top: y + h - bande_h,
            right: x + w,
            bottom: y + h,
        };
        let brosse = self.pres_brosse_verticale(
            &[(0.0, [0, 0, 0, 0]), (1.0, CLIP_TITRE_FOND)],
            bande.top,
            bande.bottom,
        )?;
        unsafe { self.cible_rt.FillRectangle(&bande, &brosse) };
        self.dessiner_ligne_c(titre, tx, ty, [255, 255, 255, 255])?;

        // `.duree` — pastille noire 75 % en bas à droite.
        //
        // ⚠️ Écart assumé : le DOM place la durée **avant** le titre (donc sous
        // son bandeau dégradé) et laisse le titre courir dessous — les deux
        // deviennent illisibles. Le moteur peint la pastille **au-dessus** et
        // rogne le titre avant elle (`construire_carte`).
        let bw = duree.largeur + 2.0 * CLIP_DUREE_PAD_H;
        let bh = duree.hauteur.max(CLIP_DUREE_TAILLE) + 2.0 * CLIP_DUREE_PAD_V;
        let brosse: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d([0, 0, 0, 191]), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse durée : {}", e.message())))?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: dx,
                        top: dy,
                        right: dx + bw,
                        bottom: dy + bh,
                    },
                    radiusX: CLIP_DUREE_RAYON,
                    radiusY: CLIP_DUREE_RAYON,
                },
                &brosse,
            )
        };
        self.dessiner_ligne_c(
            duree,
            dx + CLIP_DUREE_PAD_H,
            dy + CLIP_DUREE_PAD_V,
            [255, 255, 255, 255],
        )?;

        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
    }

    /// Texte de la carte — `ID2D1DeviceContext` + `ENABLE_COLOR_FONT` (les
    /// pseudos, titres et bios Twitch contiennent des emojis).
    fn dessiner_ligne_c(
        &self,
        ligne: &LigneC,
        x: f32,
        y: f32,
        couleur: [u8; 4],
    ) -> Result<(), ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(couleur), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse carte : {}", e.message())))?;
        unsafe {
            self.ctx.DrawTextLayout(
                D2D_POINT_2F { x, y },
                &ligne.layout,
                &brosse,
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        };
        Ok(())
    }

    /// (Re)met en page la carte quand la version, le mode ou le canvas change.
    fn synchroniser_bloc_carte(
        &mut self,
        version: u64,
        donnees: &PresentationDonnees,
        mode: ModePresentation,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let cle = cle_carte(version, mode, canvas);
        if self.bloc_carte.as_ref().map(|b| b.cle) == Some(cle) {
            return Ok(());
        }
        self.bloc_carte = None;
        let bloc = self.construire_carte(cle, donnees, mode, canvas)?;
        self.bloc_carte = Some(bloc);
        Ok(())
    }

    /// Mise en page de la carte — `flex` du DOM résolu à la main.
    ///
    /// Les deux colonnes sont dimensionnées à leur contenu (`max-width` 460 et
    /// 620), puis réduites proportionnellement si l'ensemble dépasse la
    /// largeur de la carte (`max-width: 73 %`, ou 760 px en mode `card`) —
    /// c'est ce que fait `flex-shrink: 1` avec des bases de contenu.
    fn construire_carte(
        &mut self,
        cle: u64,
        donnees: &PresentationDonnees,
        mode: ModePresentation,
        canvas: (u32, u32),
    ) -> Result<BlocCarte, ErreurTexte> {
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        let card = mode == ModePresentation::Card;
        let centre = card;
        let max_carte = if card {
            CARTE_MAX_CARD.min(w)
        } else {
            CARTE_MAX_RATIO * w
        };
        let nom = if !donnees.display_name.trim().is_empty() {
            donnees.display_name.trim()
        } else {
            donnees.login.trim()
        };
        let login = if donnees.login.trim().is_empty() {
            String::new()
        } else {
            format!("@{}", donnees.login.trim())
        };
        let badge = match donnees.broadcaster_type.as_str() {
            "partner" => Some(("PARTNER", BADGE_PARTNER)),
            "affiliate" => Some(("AFFILIATE", BADGE_AFFILIATE)),
            _ => None,
        };
        let bio = donnees.bio.trim();
        let jeu = if donnees.jeu_courant.trim().is_empty() {
            String::new()
        } else {
            format!("Joue à : {}", donnees.jeu_courant.trim())
        };
        let lien = if donnees.login.trim().is_empty() {
            String::new()
        } else {
            format!("twitch.tv/{}", donnees.login.trim())
        };
        let live = if donnees.en_live {
            let mut texte = String::from("EN DIRECT");
            if let Some(v) = donnees.viewers_live {
                texte.push_str(" · ");
                texte.push_str(&formater_viewers(v));
            }
            Some(texte)
        } else {
            None
        };
        let titre_live = if donnees.en_live {
            donnees.titre_live.trim().to_string()
        } else {
            String::new()
        };

        let taille_nom = if card { NOM_TAILLE_CARD } else { NOM_TAILLE };
        let avatar_d = if card { AVATAR_CARD } else { AVATAR };

        // --- Largeurs naturelles (contenu, sans retour à la ligne) ---
        let mut gauche_nat = if donnees.avatar.trim().is_empty() {
            0.0
        } else {
            avatar_d
        };
        let mesure = |texte: &str, taille: f32, poids, interligne: f32, espacement: f32| {
            if texte.is_empty() {
                return 0.0;
            }
            self.mesurer_texte(texte, taille, poids, interligne, espacement)
                .unwrap_or(0.0)
        };
        let nat_nom = mesure(nom, taille_nom, DWRITE_FONT_WEIGHT_EXTRA_BOLD, NOM_INTERLIGNE, 0.0);
        gauche_nat = gauche_nat.max(nat_nom);
        gauche_nat = gauche_nat.max(mesure(
            &login,
            LOGIN_TAILLE,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            LOGIN_INTERLIGNE,
            0.0,
        ));
        if let Some((texte, _)) = badge {
            gauche_nat = gauche_nat.max(
                self.mesurer_texte(texte, BADGE_TAILLE, DWRITE_FONT_WEIGHT_BOLD, 1.0, 0.0)
                    .unwrap_or(0.0)
                    + 2.0 * BADGE_PAD_H,
            );
        }
        gauche_nat = gauche_nat.max(mesure(
            bio,
            BIO_TAILLE,
            DWRITE_FONT_WEIGHT_NORMAL,
            BIO_INTERLIGNE,
            0.0,
        ));
        gauche_nat = gauche_nat.max(mesure(
            &jeu,
            JEU_TAILLE,
            DWRITE_FONT_WEIGHT_BOLD,
            JEU_INTERLIGNE,
            0.0,
        ));
        gauche_nat = gauche_nat.max(mesure(
            &lien,
            LIEN_TAILLE,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            1.0,
            LIEN_ESPACEMENT,
        ));
        gauche_nat = gauche_nat.min(COL_G_MAX);

        let mut droite_nat = 0.0_f32;
        if let Some(texte) = live.as_deref() {
            droite_nat = droite_nat.max(
                self.mesurer_texte(texte, LIVE_TAILLE, DWRITE_FONT_WEIGHT_EXTRA_BOLD, 1.0, 0.0)
                    .unwrap_or(0.0)
                    + 2.0 * LIVE_PAD_H
                    + LIVE_PASTILLE
                    + LIVE_GAP,
            );
        }
        droite_nat = droite_nat.max(mesure(
            &titre_live,
            TITRE_TAILLE,
            DWRITE_FONT_WEIGHT_SEMI_BOLD,
            TITRE_INTERLIGNE,
            0.0,
        ));
        let nb_jeux = donnees.jeux_recents.len().min(4) as f32;
        if nb_jeux > 0.0 {
            droite_nat = droite_nat.max(nb_jeux * BOITE_L + (nb_jeux - 1.0) * BOITE_GAP);
        }
        // La grille de clips n'a aucune largeur intrinsèque (vignettes en
        // `width: 100%` + `aspect-ratio`) : sans titre ni jeux la colonne
        // s'effondrerait, on garde une largeur utile pour une grille 2×2.
        if !donnees.clips.is_empty() {
            droite_nat = droite_nat.max(2.0 * 156.0 + CLIPS_GAP);
        }
        droite_nat = droite_nat.min(COL_D_MAX);

        // --- Répartition (row : deux colonnes côte à côte ; card : empilées) ---
        let (gauche_w, droite_w) =
            repartition_colonnes(card, max_carte, gauche_nat, droite_nat);

        // --- Colonne gauche ---
        let mut elems_g: Vec<Elem> = Vec::new();
        let mut y = 0.0_f32;
        let mut pose = false;
        let gap = |y: &mut f32, pose: &mut bool, valeur: f32| {
            if *pose {
                *y += valeur;
            }
            *pose = true;
            *y
        };
        if !donnees.avatar.trim().is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            elems_g.push(Elem::Avatar {
                rect: ((gauche_w - avatar_d) / 2.0, dy, avatar_d, avatar_d),
                url: donnees.avatar.clone(),
            });
            y += avatar_d;
        }
        if !nom.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: nom,
                taille: taille_nom,
                poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                espacement: 0.0,
                largeur: gauche_w,
                wrap: true,
                interligne: NOM_INTERLIGNE,
                centre: true,
                tronquer: false,
            })?;
            let hh = ligne.hauteur;
            elems_g.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: NOM_COULEUR,
            });
            y += hh;
        }
        if !login.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: &login,
                taille: LOGIN_TAILLE,
                poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                espacement: 0.0,
                largeur: gauche_w,
                wrap: true,
                interligne: LOGIN_INTERLIGNE,
                centre: true,
                tronquer: false,
            })?;
            let hh = ligne.hauteur;
            elems_g.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: LOGIN_COULEUR,
            });
            y += hh;
        }
        if let Some((texte, couleurs)) = badge {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte,
                taille: BADGE_TAILLE,
                poids: DWRITE_FONT_WEIGHT_BOLD,
                espacement: 0.0,
                largeur: 4096.0,
                wrap: false,
                interligne: 1.0,
                centre: false,
                tronquer: false,
            })?;
            let bw = ligne.largeur + 2.0 * BADGE_PAD_H;
            let bh = ligne.hauteur.max(BADGE_TAILLE) + 2.0 * BADGE_PAD_V;
            elems_g.push(Elem::Pastille {
                rect: ((gauche_w - bw) / 2.0, dy, bw, bh),
                rayon: bh / 2.0,
                fond: couleurs,
                texte: Some((ligne, (gauche_w - bw) / 2.0 + BADGE_PAD_H, dy + BADGE_PAD_V)),
            });
            y += bh;
        }
        if !bio.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: bio,
                taille: BIO_TAILLE,
                poids: DWRITE_FONT_WEIGHT_NORMAL,
                espacement: 0.0,
                largeur: gauche_w,
                wrap: true,
                interligne: BIO_INTERLIGNE,
                centre: true,
                tronquer: false,
            })?;
            let hh = ligne.clampe(BIO_LIGNES);
            elems_g.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: BIO_COULEUR,
            });
            y += hh;
        }
        if !jeu.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: &jeu,
                taille: JEU_TAILLE,
                poids: DWRITE_FONT_WEIGHT_BOLD,
                espacement: 0.0,
                largeur: gauche_w,
                wrap: true,
                interligne: JEU_INTERLIGNE,
                centre: true,
                tronquer: false,
            })?;
            let hh = ligne.hauteur;
            elems_g.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: JEU_COULEUR,
            });
            y += hh;
        }
        if !lien.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_G_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: &lien,
                taille: LIEN_TAILLE,
                poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                espacement: LIEN_ESPACEMENT,
                largeur: gauche_w,
                wrap: true,
                interligne: 1.0,
                centre: true,
                tronquer: false,
            })?;
            let hh = ligne.hauteur;
            elems_g.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: LIEN_COULEUR,
            });
            y += hh;
        }
        let gauche_h = y;

        // --- Colonne droite ---
        let mut elems_d: Vec<Elem> = Vec::new();
        let mut y = 0.0_f32;
        let mut pose = false;
        if let Some(texte) = live.as_deref() {
            let dy = gap(&mut y, &mut pose, COL_D_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte,
                taille: LIVE_TAILLE,
                poids: DWRITE_FONT_WEIGHT_EXTRA_BOLD,
                espacement: 0.0,
                largeur: 4096.0,
                wrap: false,
                interligne: 1.0,
                centre: false,
                tronquer: false,
            })?;
            let bh = ligne.hauteur.max(LIVE_TAILLE) + 2.0 * LIVE_PAD_V;
            let bw = ligne.largeur + 2.0 * LIVE_PAD_H + LIVE_PASTILLE + LIVE_GAP;
            // `align-self: flex-start` ; centré en mode `card`.
            let bx = if centre { (droite_w - bw) / 2.0 } else { 0.0 };
            elems_d.push(Elem::Cercle {
                cx: bx + LIVE_PAD_H + LIVE_PASTILLE / 2.0,
                cy: dy + bh / 2.0,
                r: LIVE_PASTILLE / 2.0,
                couleur: [255, 255, 255, 255],
            });
            let tx = bx + LIVE_PAD_H + LIVE_PASTILLE + LIVE_GAP;
            elems_d.push(Elem::Pastille {
                rect: (bx, dy, bw, bh),
                rayon: LIVE_RAYON,
                fond: (LIVE_FOND, LIVE_FOND),
                texte: Some((ligne, tx, dy + LIVE_PAD_V)),
            });
            y += bh;
        }
        if !titre_live.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_D_GAP);
            let ligne = self.ligne_carte(SpecLigne {
                texte: &titre_live,
                taille: TITRE_TAILLE,
                poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                espacement: 0.0,
                largeur: droite_w,
                wrap: true,
                interligne: TITRE_INTERLIGNE,
                centre,
                tronquer: false,
            })?;
            let hh = ligne.hauteur;
            elems_d.push(Elem::Texte {
                ligne,
                x: 0.0,
                y: dy,
                couleur: TITRE_COULEUR,
            });
            y += hh;
        }
        if nb_jeux > 0.0 {
            let dy = gap(&mut y, &mut pose, COL_D_GAP);
            let total = nb_jeux * BOITE_L + (nb_jeux - 1.0) * BOITE_GAP;
            let x0 = if centre { (droite_w - total) / 2.0 } else { 0.0 };
            let mut hh = 0.0_f32;
            for (i, j) in donnees.jeux_recents.iter().take(4).enumerate() {
                let bx = x0 + i as f32 * (BOITE_L + BOITE_GAP);
                let url = url_box_art(&j.box_art_url);
                if !url.trim().is_empty() {
                    elems_d.push(Elem::Image {
                        rect: (bx, dy, BOITE_L, BOITE_H),
                        rayon: BOITE_RAYON,
                        url,
                    });
                }
                let mut bas = BOITE_H;
                if !j.name.trim().is_empty() {
                    let ligne = self.ligne_carte(SpecLigne {
                        texte: j.name.trim(),
                        taille: BOITE_NOM_TAILLE,
                        poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        espacement: 0.0,
                        largeur: BOITE_L,
                        wrap: true,
                        interligne: BOITE_NOM_INTERLIGNE,
                        centre: true,
                        tronquer: false,
                    })?;
                    let nh = ligne.clampe(BOITE_NOM_LIGNES);
                    elems_d.push(Elem::Texte {
                        ligne,
                        x: bx,
                        y: dy + BOITE_H + BOITE_NOM_GAP,
                        couleur: BOITE_NOM_COULEUR,
                    });
                    bas = BOITE_H + BOITE_NOM_GAP + nh;
                }
                hh = hh.max(bas);
            }
            y += hh;
        }
        // `.pres-clips` — grille 2 colonnes, 4 premiers clips. La grille prend
        // toujours la largeur de la colonne : en mode `card`, le
        // `align-items: center` du DOM laisse la largeur d'une grille
        // `repeat(2, 1fr)` indéterminée (aucun contenu intrinsèque).
        let clips: Vec<&ClipPresentation> = donnees.clips.iter().take(4).collect();
        if !clips.is_empty() {
            let dy = gap(&mut y, &mut pose, COL_D_GAP);
            let cell_w = ((droite_w - CLIPS_GAP) / 2.0).max(1.0);
            let cell_h = cell_w / CLIP_RATIO;
            for (i, clip) in clips.iter().enumerate() {
                let (col, ligne_idx) = (i % 2, i / 2);
                let bx = col as f32 * (cell_w + CLIPS_GAP);
                let by = dy + ligne_idx as f32 * (cell_h + CLIPS_GAP);
                // Durée : `Math.round(duration) + "s"`, pastille en bas à
                // droite — le titre est rogné **avant** elle (écart assumé,
                // voir `dessiner_clip`).
                let duree = self.ligne_carte(SpecLigne {
                    texte: &format!("{}s", clip.duration.round().max(0.0) as u64),
                    taille: CLIP_DUREE_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_BOLD,
                    espacement: 0.0,
                    largeur: 4096.0,
                    wrap: false,
                    interligne: 1.0,
                    centre: false,
                    tronquer: false,
                })?;
                let pastille_w = duree.largeur + 2.0 * CLIP_DUREE_PAD_H;
                let pastille_h = duree.hauteur.max(CLIP_DUREE_TAILLE) + 2.0 * CLIP_DUREE_PAD_V;
                let dx = bx + cell_w - CLIP_DUREE_MARGE - pastille_w;
                let dy_p = by + cell_h - CLIP_DUREE_MARGE - pastille_h;
                let titre_w = (cell_w - 2.0 * CLIP_TITRE_PAD_H - pastille_w - CLIP_DUREE_MARGE)
                    .max(1.0);
                let titre = self.ligne_carte(SpecLigne {
                    texte: clip.title.trim(),
                    taille: CLIP_TITRE_TAILLE,
                    poids: DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    espacement: 0.0,
                    largeur: titre_w,
                    wrap: false,
                    interligne: CLIP_TITRE_INTERLIGNE,
                    centre: false,
                    tronquer: true,
                })?;
                elems_d.push(Elem::Clip {
                    rect: (bx, by, cell_w, cell_h),
                    url: clip.thumbnail_url.clone(),
                    duree,
                    dx,
                    dy: dy_p,
                    tx: bx + CLIP_TITRE_PAD_H,
                    ty: by + cell_h - CLIP_TITRE_PAD_BAS - CLIP_TITRE_TAILLE * CLIP_TITRE_INTERLIGNE,
                    titre,
                });
            }
            let lignes = clips.len().div_ceil(2) as f32;
            y += lignes * cell_h + (lignes - 1.0) * CLIPS_GAP;
        }
        let droite_h = y;

        // --- Boîte de la carte, centrée dans le canvas ---
        let (carte_w, carte_h) = if card {
            (
                gauche_w.max(droite_w),
                gauche_h + CARTE_GAP_CARD + droite_h,
            )
        } else {
            (gauche_w + CARTE_GAP + droite_w, gauche_h.max(droite_h))
        };
        let x0 = (w - carte_w) / 2.0;
        let y0 = (h - carte_h) / 2.0;
        let (gx, gy, dx, dy) = if card {
            (
                x0 + (carte_w - gauche_w) / 2.0,
                y0,
                x0 + (carte_w - droite_w) / 2.0,
                y0 + gauche_h + CARTE_GAP_CARD,
            )
        } else {
            (
                x0,
                y0 + (carte_h - gauche_h) / 2.0,
                x0 + gauche_w + CARTE_GAP,
                y0 + (carte_h - droite_h) / 2.0,
            )
        };
        let mut elements = Vec::with_capacity(elems_g.len() + elems_d.len());
        elements.extend(decaler(elems_g, gx, gy));
        elements.extend(decaler(elems_d, dx, dy));

        Ok(BlocCarte {
            cle,
            boite: (x0, y0, carte_w, carte_h),
            elements,
        })
    }

    /// Largeur naturelle d'une ligne (sans retour à la ligne).
    fn mesurer_texte(
        &self,
        texte: &str,
        taille: f32,
        poids: DWRITE_FONT_WEIGHT,
        interligne: f32,
        espacement: f32,
    ) -> Result<f32, ErreurTexte> {
        Ok(self
            .ligne_carte(SpecLigne {
                texte,
                taille,
                poids,
                espacement,
                largeur: 8192.0,
                wrap: false,
                interligne,
                centre: false,
                tronquer: false,
            })?
            .largeur)
    }

    /// Met en page une ligne de la carte — `system-ui` du body, donc famille
    /// système (`Segoe UI`), poids et interligne explicites (hors du
    /// vocabulaire `StyleTexte`).
    fn ligne_carte(&self, spec: SpecLigne<'_>) -> Result<LigneC, ErreurTexte> {
        let famille: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(famille.as_ptr()),
                None,
                spec.poids,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                spec.taille,
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat carte : {}", e.message())))?;
        unsafe {
            format.SetTextAlignment(if spec.centre {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("alignement carte : {}", e.message())))?;
        // `line-height` CSS : hauteur de ligne exacte (DirectWrite prendrait
        // sinon ~1,2 em et la carte grandirait).
        unsafe {
            format.SetLineSpacing(
                DWRITE_LINE_SPACING_METHOD_UNIFORM,
                spec.taille * spec.interligne,
                spec.taille * 0.8,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("interligne carte : {}", e.message())))?;
        let contenu: Vec<u16> = spec.texte.encode_utf16().collect();
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, spec.largeur.max(1.0), 4096.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("layout carte : {}", e.message())))?;
        unsafe {
            layout.SetWordWrapping(if spec.wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            })
        }
        .map_err(|e| ErreurTexte::Systeme(format!("wrap carte : {}", e.message())))?;
        if spec.tronquer {
            // `text-overflow: ellipsis` : sans ça DirectWrite laisse le texte
            // `NO_WRAP` déborder de sa boîte (le titre passait sous la
            // pastille de durée).
            let signe: IDWriteInlineObject = unsafe { self.dwrite.CreateEllipsisTrimmingSign(&layout) }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("ellipse carte : {}", e.message()))
                })?;
            unsafe {
                layout.SetTrimming(
                    &DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    },
                    Some(&signe),
                )
            }
            .map_err(|e| ErreurTexte::Systeme(format!("trimming carte : {}", e.message())))?;
        }
        if spec.espacement != 0.0 {
            let layout1: IDWriteTextLayout1 = layout.cast().map_err(|e| {
                ErreurTexte::Systeme(format!("IDWriteTextLayout1 carte : {}", e.message()))
            })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    spec.espacement,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| ErreurTexte::Systeme(format!("espacement carte : {}", e.message())))?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques carte : {}", e.message())))?;
        Ok(LigneC {
            layout,
            _format: format,
            largeur: metriques.widthIncludingTrailingWhitespace,
            hauteur: metriques.height,
            interligne: spec.taille * spec.interligne,
        })
    }

    // ===== Lecteur clip (E6c) =====

    /// Peint le lecteur clip plein écran (`z 10004`).
    ///
    /// `mp4_url` est une URL **signée distante** : le crate lit le fichier
    /// préchargé par l'app (`medias/clips/<fnv16(url)>.mp4`). Tant qu'il
    /// manque, le voile et le titre restent affichés — c'est le « buffering »
    /// du `<video>` du DOM — et la lecture démarre dès que le fichier arrive
    /// (re-tentative 500 ms).
    fn peindre_clip(
        &mut self,
        rendu: &RenduPresentation<'_>,
        version: u64,
        canvas: (u32, u32),
        temps_s: f64,
    ) -> Result<(), ErreurTexte> {
        let Some(clip) = rendu.clip else {
            // Retour à la carte : média libéré (le DOM retire le `<video>`).
            self.pres_clip_media = None;
            self.pres_clip_url = None;
            self.bloc_clip = None;
            self.pres_audios = [None, None];
            self.pres_audio_rel = [None, None];
            return Ok(());
        };
        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
        self.synchroniser_clip(clip, temps_s)?;

        // `.pres-clip-root { background: rgba(0,0,0,.9) }`.
        let plein = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: w,
            bottom: h,
        };
        let voile: ID2D1SolidColorBrush = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(CLIP_VOILE), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse voile clip : {}", e.message())))?;
        unsafe { self.cible_rt.FillRectangle(&plein, &voile) };

        // Vidéo : taille intrinsèque bornée à 88 % × 82 % (jamais agrandie),
        // centrée, `border-radius: 14px`.
        // Handles clonés : `bitmap_media` emprunte `&mut self`.
        let clip_video = self
            .pres_clip_media
            .as_ref()
            .map(|m| (m.texture.clone(), m.taille, m.alpha));
        if let Some((texture, taille, alpha)) = clip_video {
            let (vw, vh) = (taille.0 as f32, taille.1 as f32);
            if vw > 0.0 && vh > 0.0 {
                let e = 1.0_f32
                    .min(CLIP_VIDEO_MAX_L * w / vw)
                    .min(CLIP_VIDEO_MAX_H * h / vh);
                let (dw, dh) = (vw * e, vh * e);
                let dst = D2D_RECT_F {
                    left: (w - dw) / 2.0,
                    top: (h - dh) / 2.0,
                    right: (w + dw) / 2.0,
                    bottom: (h + dh) / 2.0,
                };
                let geo: ID2D1RoundedRectangleGeometry = unsafe {
                    self.d2d.CreateRoundedRectangleGeometry(
                        &windows::Win32::Graphics::Direct2D::D2D1_ROUNDED_RECT {
                            rect: dst,
                            radiusX: CLIP_VIDEO_RAYON,
                            radiusY: CLIP_VIDEO_RAYON,
                        },
                    )
                }
                .map_err(|e| ErreurTexte::Gpu(format!("masque vidéo clip : {}", e.message())))?;
                let masque: ID2D1Geometry = geo.cast().map_err(|e| {
                    ErreurTexte::Gpu(format!("ID2D1Geometry vidéo clip : {}", e.message()))
                })?;
                let mut params = D2D1_LAYER_PARAMETERS1 {
                    contentBounds: dst,
                    geometricMask: ManuallyDrop::new(Some(masque)),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: 1.0,
                    opacityBrush: ManuallyDrop::new(None),
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                };
                let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("CreateLayer vidéo clip : {}", e.message()))
                })?;
                unsafe { self.ctx.PushLayer(&params, &layer) };
                let bitmap = self.bitmap_media(&texture, alpha)?;
                let trace = TraceMedia {
                    destination: windows::Win32::Foundation::RECT {
                        left: dst.left as i32,
                        top: dst.top as i32,
                        right: dst.right as i32,
                        bottom: dst.bottom as i32,
                    },
                    source: windows::Win32::Foundation::RECT {
                        left: 0,
                        top: 0,
                        right: vw as i32,
                        bottom: vh as i32,
                    },
                    boite: windows::Win32::Foundation::RECT {
                        left: dst.left as i32,
                        top: dst.top as i32,
                        right: dst.right as i32,
                        bottom: dst.bottom as i32,
                    },
                    transforme: None,
                    clip: None,
                    effets: &crate::media::EffetsCfg::NEUTRE,
                    opacite: 1.0,
                };
                self.dessiner_media(&bitmap, &trace)?;
                unsafe { self.ctx.PopLayer() };
                unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
            }
        }

        // Titre du clip : `top 44px`, `left/right 8%`, 30 px gras, centré.
        self.synchroniser_bloc_clip(version, &clip.titre, canvas)?;
        if let Some((_, ligne, x, y)) = self.bloc_clip.as_ref() {
            self.dessiner_ligne_c(ligne, *x, *y, [255, 255, 255, 255])?;
        }
        Ok(())
    }

    /// Charge la vidéo locale du clip (une tentative toutes les 500 ms tant
    /// que l'app n'a pas fini de télécharger).
    fn synchroniser_clip(&mut self, clip: &ClipEnCours, temps_s: f64) -> Result<(), ErreurTexte> {
        if self.pres_clip_url.as_deref() != Some(clip.mp4_url.as_str()) {
            self.pres_clip_media = None;
            self.pres_clip_url = Some(clip.mp4_url.clone());
            self.pres_clip_tentative = None;
        }
        if self.pres_clip_media.is_some() {
            let temps_us = (temps_s * 1e6) as i64;
            if let Some(media) = self.pres_clip_media.as_mut() {
                if let Err(e) = media.avancer(temps_us) {
                    log::debug!("présentation : clip vidéo ({e})");
                }
                if let Err(e) = media.avancer_audio(temps_us) {
                    log::debug!("présentation : clip audio ({e})");
                }
            }
            return Ok(());
        }
        let relance = self
            .pres_clip_tentative
            .map(|t: Instant| t.elapsed().as_millis() >= 500)
            .unwrap_or(true);
        if !relance {
            return Ok(());
        }
        self.pres_clip_tentative = Some(Instant::now());
        let chemin = chemin_clip(&self.base, &clip.mp4_url);
        if !chemin.is_file() {
            return Ok(());
        }
        let media_cfg = MediaCfg {
            chemin: chemin.clone(),
            // Rect/effets neutres : le lecteur peint la vidéo lui-même.
            rect: (0.0, 0.0, 0.0, 0.0),
            z: Z_PRES_CLIP,
            fit: Fit::Remplir,
            transform: Transform2D::NEUTRE,
            effets: EffetsCfg::NEUTRE,
            cadre: None,
            morphs: Vec::new(),
            pixel_morph: 0.0,
            // Un clip ne boucle pas (`<video>` sans `loop`).
            lecture: LectureCfg {
                paused: false,
                boucle: None,
                boucle_infinie: false,
            },
            carrousel: None,
        };
        match crate::media::charger(&self.contexte, &media_cfg) {
            Ok(mut media) => {
                // Chargé en cours de session : la première frame est due
                // maintenant (sinon le clip sauterait à `temps_s`).
                if let Err(e) = media.recaler((temps_s * 1e6) as i64) {
                    log::debug!("présentation : recalage clip ({e})");
                }
                self.pres_clip_media = Some(media);
            }
            Err(e) => log::debug!("présentation : clip {} ignoré ({e})", chemin.display()),
        }
        Ok(())
    }

    /// Met en page le titre du lecteur (cache : version + canvas).
    fn synchroniser_bloc_clip(
        &mut self,
        version: u64,
        titre: &str,
        canvas: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        version.hash(&mut h);
        canvas.hash(&mut h);
        let cle = h.finish();
        if self.bloc_clip.as_ref().map(|b| b.0) == Some(cle) {
            return Ok(());
        }
        self.bloc_clip = None;
        if titre.trim().is_empty() {
            return Ok(());
        }
        let (w, _h) = (canvas.0 as f32, canvas.1 as f32);
        let largeur = (w * (1.0 - 2.0 * CLIP_TITRE_LECTEUR_MARGE)).max(1.0);
        let ligne = self.ligne_carte(SpecLigne {
            texte: titre.trim(),
            taille: CLIP_TITRE_LECTEUR_TAILLE,
            poids: DWRITE_FONT_WEIGHT_BOLD,
            espacement: 0.0,
            largeur,
            wrap: false,
            interligne: 1.3,
            centre: true,
            tronquer: true,
        })?;
        self.bloc_clip = Some((cle, ligne, w * CLIP_TITRE_LECTEUR_MARGE, CLIP_TITRE_LECTEUR_TOP));
        Ok(())
    }

    /// Anneau audio du clip (le mixeur y puise le son du lecteur).
    pub(crate) fn anneau_audio_clip(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<crate::audio::live::AnneauAudio>>> {
        self.pres_clip_media.as_ref()?.anneau_audio()
    }

    /// Anneau audio d'une piste de la présentation (`voix` / `musique`).
    pub(crate) fn anneau_audio_piste(
        &self,
        piste: PistePresentation,
    ) -> Option<std::sync::Arc<std::sync::Mutex<crate::audio::live::AnneauAudio>>> {
        self.pres_audios[piste.index()]
            .as_ref()
            .map(|a| a.anneau_audio())
    }

    /// Charge / avance / reboucle les 2 pistes audio (miroir de
    /// `presentationAudioPlay` / `presentationAudioStop`).
    ///
    /// `src` = `medias/…` : **fichier local**, le crate ne fait aucun réseau.
    /// `musique` reboucle (`audio.loop = true`) : le fichier est réouvert en
    /// fin de flux ; `voix` joue **une fois** puis se tait.
    fn synchroniser_audios(&mut self, actives: &[Option<&str>; 2], temps_s: f64) {
        let temps_us = (temps_s * 1e6) as i64;
        for piste in PistePresentation::TOUTES {
            let i = piste.index();
            let src = actives[i];
            if self.pres_audio_rel[i].as_deref() != src {
                self.pres_audios[i] = None;
                self.pres_audio_rel[i] = src.map(|s| s.to_string());
                if let Some(src) = src {
                    let chemin = self.base.join(src);
                    match crate::media::AudioFichier::ouvrir(&chemin, temps_us) {
                        Ok(audio) => self.pres_audios[i] = Some(audio),
                        Err(e) => {
                            log::debug!("présentation : piste {} ignorée ({e})", chemin.display())
                        }
                    }
                }
            }
            let Some(audio) = self.pres_audios[i].as_mut() else {
                continue;
            };
            if audio.est_termine() {
                if piste.boucle() {
                    // `audio.loop = true` : on repart du début du fichier.
                    if let Some(src) = src {
                        let chemin = self.base.join(src);
                        match crate::media::AudioFichier::ouvrir(&chemin, temps_us) {
                            Ok(nouveau) => {
                                self.pres_audios[i] = Some(nouveau);
                                continue;
                            }
                            Err(e) => {
                                log::debug!("présentation : boucle {} ({e})", chemin.display());
                                self.pres_audios[i] = None;
                                continue;
                            }
                        }
                    }
                }
                continue;
            }
            if let Err(e) = audio.avancer(temps_us) {
                log::debug!("présentation : piste audio ({e})");
            }
        }
    }

    // ===== Brosses =====
    /// Brosse d'un `linear-gradient(<angle>deg, …)` CSS : la ligne de dégradé
    /// traverse la boîte (`|w·sin θ| + |h·cos θ|`), angle mesuré depuis le
    /// haut, sens horaire (badges `135deg`).
    fn pres_brosse_css(
        &self,
        stops: &[(f32, [u8; 4])],
        boite: D2D_RECT_F,
        angle_deg: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let (w, h) = (boite.right - boite.left, boite.bottom - boite.top);
        let rad = angle_deg.to_radians();
        let (dx, dy) = (rad.sin(), -rad.cos());
        let longueur = ((w * rad.sin()).abs() + (h * rad.cos()).abs()).max(1.0);
        let (cx, cy) = (
            (boite.left + boite.right) / 2.0,
            (boite.top + boite.bottom) / 2.0,
        );
        self.pres_brosse_lineaire(
            stops,
            (cx - dx * longueur / 2.0, cy - dy * longueur / 2.0),
            (cx + dx * longueur / 2.0, cy + dy * longueur / 2.0),
            D2D1_EXTEND_MODE_CLAMP,
        )
    }

    /// Brosse linéaire verticale (`linear-gradient(180deg, …)`).
    fn pres_brosse_verticale(
        &self,
        stops: &[(f32, [u8; 4])],
        haut: f32,
        bas: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        self.pres_brosse_lineaire(stops, (0.0, haut), (0.0, bas), D2D1_EXTEND_MODE_CLAMP)
    }

    /// Brosse linéaire horizontale (`linear-gradient(90deg, …)`).
    fn pres_brosse_horizontale(
        &self,
        stops: &[(f32, [u8; 4])],
        gauche: f32,
        droite: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        self.pres_brosse_lineaire(stops, (gauche, 0.0), (droite, 0.0), D2D1_EXTEND_MODE_CLAMP)
    }

    /// Brosse d'un `repeating-linear-gradient(90deg, …)` : une période de
    /// [`PLI_PERIODE`] px répétée par `D2D1_EXTEND_MODE_WRAP`.
    fn pres_brosse_periodique(
        &self,
        stops_px: &[(f32, [u8; 4])],
        gauche: f32,
        periode: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let stops: Vec<(f32, [u8; 4])> = stops_px
            .iter()
            .map(|(px, couleur)| ((px / periode).clamp(0.0, 1.0), *couleur))
            .collect();
        self.pres_brosse_lineaire(
            &stops,
            (gauche, 0.0),
            (gauche + periode, 0.0),
            D2D1_EXTEND_MODE_WRAP,
        )
    }

    fn pres_brosse_lineaire(
        &self,
        stops: &[(f32, [u8; 4])],
        debut: (f32, f32),
        fin: (f32, f32),
        mode: D2D1_EXTEND_MODE,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, couleur)| D2D1_GRADIENT_STOP {
                position: *position,
                color: couleur_d2d(*couleur),
            })
            .collect();
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, mode)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé présentation : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F {
                x: debut.0,
                y: debut.1,
            },
            endPoint: D2D_POINT_2F { x: fin.0, y: fin.1 },
        };
        let brosse = unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("brosse dégradé présentation : {}", e.message()))
            })?;
        Ok(brosse.into())
    }

    /// Brosse radiale elliptique (orbes, lueur du sol, festons).
    fn pres_brosse_radiale(
        &self,
        stops: &[(f32, [u8; 4])],
        centre: (f32, f32),
        rayons: (f32, f32),
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, couleur)| D2D1_GRADIENT_STOP {
                position: *position,
                color: couleur_d2d(*couleur),
            })
            .collect();
        let collection: ID2D1GradientStopCollection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("dégradé radial : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F {
                x: centre.0,
                y: centre.1,
            },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: rayons.0.max(1.0),
            radiusY: rayons.1.max(1.0),
        };
        let brosse = unsafe { self.cible_rt.CreateRadialGradientBrush(&props, None, &collection) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse radiale : {}", e.message())))?;
        Ok(brosse.into())
    }
}

/// Positions `(x, largeur)` des 3 faisceaux (`.pres-spot.s1/s2/s3`).
fn positions_spots(w: f32) -> [(f32, f32); 3] {
    [
        (0.06 * w, 420.0),
        (w - 0.06 * w - 420.0, 420.0),
        (0.5 * w - 260.0, 520.0),
    ]
}

/// Fauteuil d'une rangée : rectangle à rayons **elliptiques** distincts en
/// haut et en bas (`border-radius: 46% 46% 8% 8% / 32% 32% 5% 5%`).
fn geometrie_fauteuil(
    factory: &windows::Win32::Graphics::Direct2D::ID2D1Factory1,
    largeur: f32,
    hauteur: f32,
) -> Result<ID2D1PathGeometry, ErreurTexte> {
    use windows::Win32::Graphics::Direct2D::ID2D1GeometrySink;
    let rx_h = SIEGE_RAYON_HAUT.0 * largeur;
    let ry_h = SIEGE_RAYON_HAUT.1 * hauteur;
    let rx_b = SIEGE_RAYON_BAS.0 * largeur;
    let ry_b = SIEGE_RAYON_BAS.1 * hauteur;
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry() }
        .and_then(|g| g.cast())
        .map_err(|e| ErreurTexte::Gpu(format!("CreatePathGeometry fauteuil : {}", e.message())))?;
    let sink: ID2D1GeometrySink = unsafe { geo.Open() }
        .map_err(|e| ErreurTexte::Gpu(format!("Open fauteuil : {}", e.message())))?;
    let arc = |x: f32, y: f32, rx: f32, ry: f32| D2D1_ARC_SEGMENT {
        point: D2D_POINT_2F { x, y },
        size: D2D_SIZE_F {
            width: rx,
            height: ry,
        },
        rotationAngle: 0.0,
        // Le contour est parcouru dans le sens **anti-horaire** (bas → droite →
        // haut → gauche, y vers le bas) : les quatre coins sont donc des arcs
        // anti-horaires, sinon D2D prend le grand arc (270°) et le fauteuil
        // s'évide au milieu (bug vu au rendu, 2026-11-01).
        sweepDirection: D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        arcSize: D2D1_ARC_SIZE_SMALL,
    };
    unsafe {
        sink.SetFillMode(windows::Win32::Graphics::Direct2D::Common::D2D1_FILL_MODE_ALTERNATE);
        sink.BeginFigure(
            D2D_POINT_2F {
                x: rx_b,
                y: hauteur,
            },
            D2D1_FIGURE_BEGIN_FILLED,
        );
        sink.AddLine(D2D_POINT_2F {
            x: largeur - rx_b,
            y: hauteur,
        });
        sink.AddArc(&arc(largeur, hauteur - ry_b, rx_b, ry_b));
        sink.AddLine(D2D_POINT_2F {
            x: largeur,
            y: ry_h,
        });
        sink.AddArc(&arc(largeur - rx_h, 0.0, rx_h, ry_h));
        sink.AddLine(D2D_POINT_2F { x: rx_h, y: 0.0 });
        sink.AddArc(&arc(0.0, ry_h, rx_h, ry_h));
        sink.AddLine(D2D_POINT_2F {
            x: 0.0,
            y: hauteur - ry_b,
        });
        sink.AddArc(&arc(rx_b, hauteur, rx_b, ry_b));
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    }
    unsafe { sink.Close() }
        .map_err(|e| ErreurTexte::Gpu(format!("Close fauteuil : {}", e.message())))?;
    Ok(geo)
}

/// Tirage du décor (public + poussière) — miroir de `construirePublic` et de
/// la boucle `pres-poussiere` du DOM, avec le PRNG déterministe.
fn tirer_decor(w: f32, h: f32) -> (Vec<Vec<Place>>, Vec<Grain>) {
    let mut alea = Alea::nouveau(GRAINE_DECOR);
    let mut public = Vec::with_capacity(RANGEES.len());
    for rangee in RANGEES.iter() {
        let mut places = Vec::new();
        // ⚠️ Le DOM boucle jusqu'à **1920 px** en dur, pas la largeur canvas.
        let mut x = rangee.decalage;
        while x < PUBLIC_LARGEUR_REF {
            let largeur = if rangee.siege_l > 0.0 {
                rangee.siege_l + 24.0
            } else {
                rangee.tete + 60.0
            };
            let tete = if rangee.siege_l <= 0.0 || alea.suivant() >= 0.15 {
                let t = (rangee.tete * (0.88 + alea.suivant() * 0.24)).round();
                Some(Tete {
                    largeur: t,
                    hauteur: (t * 1.08).round(),
                    bas: (rangee.siege_h * 0.55).round(),
                    decalage_pct: (alea.suivant() - 0.5) * 14.0,
                    duree_s: 4.0 + alea.suivant() * 3.5,
                    delai_s: alea.suivant() * 8.0,
                })
            } else {
                None
            };
            places.push(Place {
                x,
                largeur,
                ancre: h - rangee.bas,
                tete,
            });
            x += rangee.ecart + ((alea.suivant() - 0.5) * 30.0).round();
        }
        public.push(places);
    }
    let mut poussiere = Vec::with_capacity(POUSSIERE_NB);
    for _ in 0..POUSSIERE_NB {
        let taille = 2.0 + alea.suivant() * 3.0;
        poussiere.push(Grain {
            x: (0.22 + alea.suivant() * 0.56) * w,
            y: (0.08 + alea.suivant() * 0.78) * h,
            taille,
            duree_s: 8.0 + alea.suivant() * 9.0,
            delai_s: alea.suivant() * 14.0,
        });
    }
    (public, poussiere)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn donnees() -> PresentationDonnees {
        PresentationDonnees {
            login: "streamer".to_string(),
            display_name: "Streamer".to_string(),
            banniere: "https://static-cdn.jtvnw.net/banner.jpg".to_string(),
            ..PresentationDonnees::default()
        }
    }

    fn show(mode: ModePresentation) -> MajPresentation {
        MajPresentation::Show {
            mode,
            donnees: Box::new(donnees()),
        }
    }

    #[test]
    fn entree_theatrale_voile_rideaux_embrasses() {
        let mut etat = EtatPresentation::nouveau();
        assert!(etat.rendu().is_none(), "rien avant le premier show");

        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        assert_eq!(etat.version(), 1);
        let depart = etat.rendu_at(t0).expect("voile affiché");
        assert!(depart.phases.voile.abs() < 1e-3, "opacité 0 au départ");
        assert!(depart.phases.rideaux.abs() < 1e-3, "rideaux fermés");

        // Voile : 0,5 s.
        let demi = etat.rendu_at(t0 + Duration::from_millis(250)).expect("en fondu");
        assert!(demi.phases.voile > 0.3 && demi.phases.voile < 1.0);
        let voile = etat.rendu_at(t0 + Duration::from_millis(600)).expect("voile ouvert");
        assert!((voile.phases.voile - 1.0).abs() < 1e-3);

        // Embrasses : retard 0,75 s puis 0,4 s → nulles à 700 ms.
        let avant = etat.rendu_at(t0 + Duration::from_millis(700)).expect("visible");
        assert!(avant.phases.embrasses.abs() < 1e-3, "embrasses encore cachées");
        assert!(
            (avant.phases.embrasses_decalage - EMBRASSE_DECALAGE).abs() < 1e-3,
            "décalage au repos"
        );
        let apres = etat.rendu_at(t0 + Duration::from_millis(1250)).expect("visible");
        assert!(apres.phases.embrasses > 0.9);
        assert!(apres.phases.embrasses_decalage.abs() < 0.5);

        // Rideaux : 1,2 s (après le double rAF) → encore en cours à 1 s.
        let milieu = etat.rendu_at(t0 + Duration::from_millis(1000)).expect("visible");
        assert!(milieu.phases.rideaux > 0.3 && milieu.phases.rideaux < 1.0);
        let ouverts = etat.rendu_at(t0 + Duration::from_millis(1400)).expect("visible");
        assert!((ouverts.phases.rideaux - 1.0).abs() < 1e-3);

        // Valance et public : 0,6 s.
        assert!((ouverts.phases.valance - 1.0).abs() < 1e-3);
        assert!((ouverts.phases.public - 1.0).abs() < 1e-3);
    }

    #[test]
    fn mode_carte_garde_le_voile_sans_decor() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Card), t0);
        let stable = etat.rendu_at(t0 + Duration::from_millis(800)).expect("visible");
        assert!(
            (stable.phases.voile - 1.0).abs() < 1e-3,
            "le voile reste affiché en mode card (le code du DOM fait foi)"
        );
        assert_eq!(stable.phases.rideaux, 0.0);
        assert_eq!(stable.phases.valance, 0.0);
        assert_eq!(stable.phases.public, 0.0);
        assert_eq!(stable.phases.embrasses, 0.0);
    }

    #[test]
    fn sortie_theatrale_puis_carte() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        etat.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(2000));
        assert_eq!(etat.version(), 2);
        assert!(!etat.actif());

        let en_cours = etat
            .rendu_at(t0 + Duration::from_millis(2600))
            .expect("sortie en cours");
        assert!(en_cours.phases.voile < 1.0);
        assert!(en_cours.phases.rideaux < 1.0, "les rideaux se referment");
        assert!(
            etat.rendu_at(t0 + Duration::from_millis(3250)).is_none(),
            "masqué après 1,25 s"
        );

        // Mode card : masquage à 500 ms.
        let mut carte = EtatPresentation::nouveau();
        carte.appliquer_at(show(ModePresentation::Card), t0);
        carte.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(1000));
        assert!(carte.rendu_at(t0 + Duration::from_millis(1400)).is_some());
        assert!(carte.rendu_at(t0 + Duration::from_millis(1600)).is_none());
    }

    #[test]
    fn show_pendant_la_sortie_repart_des_phases_courantes() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        etat.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(1000));
        // Interruption à mi-fondu : le voile ne doit pas retomber à 0.
        let interrompu = t0 + Duration::from_millis(1250);
        let avant = etat.rendu_at(interrompu).expect("sortie en cours").phases.voile;
        etat.appliquer_at(show(ModePresentation::Theatrical), interrompu);
        let apres = etat.rendu_at(interrompu).expect("repris");
        assert!(
            (apres.phases.voile - avant).abs() < 1e-3,
            "reprise à l'opacité courante"
        );
        assert!(apres.phases.voile > 0.0);
        let stable = etat.rendu_at(interrompu + Duration::from_millis(600)).expect("visible");
        assert!((stable.phases.voile - 1.0).abs() < 1e-3);
    }

    #[test]
    fn bascule_de_mode_rejoue_la_choregraphie_sans_refondre_le_voile() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        let ouvert = etat.rendu_at(t0 + Duration::from_millis(2000)).expect("visible");
        assert!((ouvert.phases.rideaux - 1.0).abs() < 1e-3);

        // → card : décor retiré instantanément, voile conservé.
        let bascule = t0 + Duration::from_millis(2100);
        etat.appliquer_at(MajPresentation::Mode(ModePresentation::Card), bascule);
        assert_eq!(etat.version(), 2);
        let carte = etat.rendu_at(bascule).expect("visible");
        assert!((carte.phases.voile - 1.0).abs() < 1e-3);
        assert_eq!(carte.phases.rideaux, 0.0);

        // → théâtral : la chorégraphie repart de zéro.
        let retour = t0 + Duration::from_millis(2200);
        etat.appliquer_at(MajPresentation::Mode(ModePresentation::Theatrical), retour);
        let rejoue = etat.rendu_at(retour).expect("visible");
        assert!(rejoue.phases.rideaux.abs() < 1e-3, "rideaux refermés");
        assert!((rejoue.phases.voile - 1.0).abs() < 1e-3, "voile jamais re-fondu");
        let fini = etat.rendu_at(retour + Duration::from_millis(1400)).expect("visible");
        assert!((fini.phases.rideaux - 1.0).abs() < 1e-3);

        // Mode identique : aucune version (le DOM ne rejoue rien).
        etat.appliquer_at(MajPresentation::Mode(ModePresentation::Theatrical), fini_debut(&etat));
        assert_eq!(etat.version(), 3);
    }

    /// Instant « courant » d'un état déjà lancé — utilitaire de test.
    fn fini_debut(etat: &EtatPresentation) -> Instant {
        etat.debut.unwrap_or_else(Instant::now) + Duration::from_millis(5000)
    }

    #[test]
    fn rideaux_ouverts_laissent_onze_pour_cent_de_l_ecran() {
        // `translateX(-27%) scaleX(0.5)` sur un rideau de 50 % : la largeur
        // visible après ouverture est 0,5 × 50 % = 25 % du canvas, décalée de
        // −13,5 % → 11,5 % restent visibles (commentaire du CSS : « ~11 % »).
        let w = 1920.0_f32;
        let demi = w / 2.0;
        let echelle = 1.0 - RIDEAU_ECHELLE;
        let translation = -RIDEAU_TRANSLATE * demi;
        let gauche = translation;
        let droite = gauche + demi * echelle;
        assert!((gauche - (-259.2)).abs() < 0.5);
        assert!((droite / w - 0.115).abs() < 1e-3);
    }

    #[test]
    fn decor_deterministe_et_public_borne_a_1920() {
        let (a, grains_a) = tirer_decor(1920.0, 1080.0);
        let (b, grains_b) = tirer_decor(1920.0, 1080.0);
        assert_eq!(a, b, "même graine → même décor");
        assert_eq!(grains_a, grains_b);
        assert_eq!(grains_a.len(), POUSSIERE_NB);
        assert_eq!(a.len(), 3);
        for (i, places) in a.iter().enumerate() {
            assert!(!places.is_empty(), "rangée {i} vide");
            for place in places {
                assert!(
                    place.x < PUBLIC_LARGEUR_REF,
                    "le DOM s'arrête à 1920 px, pas à la largeur du canvas"
                );
                if RANGEES[i].siege_l <= 0.0 {
                    assert!(place.tete.is_some(), "le premier plan n'a pas de fauteuil");
                }
            }
        }
        // Rangée du premier plan : aucune place vide (pas de fauteuil).
        assert!(a[2].iter().all(|p| p.tete.is_some()));
    }

    #[test]
    fn tete_absente_sur_quinze_pour_cent_des_fauteuils() {
        let (public, _) = tirer_decor(1920.0, 1080.0);
        let total: usize = public[0].len() + public[1].len();
        let vides: usize = public[0]
            .iter()
            .chain(public[1].iter())
            .filter(|p| p.tete.is_none())
            .count();
        assert!(vides > 0, "quelques fauteuils restent vides");
        let part = vides as f32 / total as f32;
        assert!((0.03..0.30).contains(&part), "part de vides inattendue : {part}");
    }

    #[test]
    fn bezier_css_borne_et_monotone() {
        assert_eq!(bezier_css(EASE, 0.0), 0.0);
        assert!((bezier_css(EASE, 1.0) - 1.0).abs() < 1e-6);
        let mut precedent = -1.0;
        for i in 0..=20 {
            let v = bezier_css(EASE_RIDEAU, i as f32 / 20.0);
            assert!(v >= precedent, "bézier non monotone à {i}");
            precedent = v;
        }
    }

    #[test]
    fn progression_alternee_boucle_et_miroir() {
        assert!((progression_alternee(0.0, 14.0, 0.0) - 0.0).abs() < 1e-6);
        assert!((progression_alternee(14.0, 14.0, 0.0) - 1.0).abs() < 1e-6);
        // Deuxième moitié du cycle : miroir.
        assert!((progression_alternee(21.0, 14.0, 0.0) - 0.5).abs() < 1e-6);
        // `animation-delay` négatif = animation déjà commencée.
        assert!((progression_alternee(0.0, 14.0, 5.0) - 5.0 / 14.0).abs() < 1e-6);
    }

    #[test]
    fn mode_depuis_nom_et_inconnu() {
        assert_eq!(ModePresentation::depuis_nom("card"), ModePresentation::Card);
        assert_eq!(
            ModePresentation::depuis_nom("theatrical"),
            ModePresentation::Theatrical
        );
        assert_eq!(
            ModePresentation::depuis_nom("inconnu"),
            ModePresentation::Theatrical,
            "tout le reste = théâtral, comme le DOM"
        );
    }

    #[test]
    fn positions_spots_suivent_le_css() {
        let (w, _h) = (1920.0_f32, 1080.0_f32);
        let p = positions_spots(w);
        assert!((p[0].0 - 115.2).abs() < 0.01);
        assert!((p[1].0 - 1384.8).abs() < 0.01);
        assert!((p[2].0 - 700.0).abs() < 0.01);
        assert_eq!(p[2].1, 520.0);
    }

    #[test]
    fn carte_entre_apres_les_rideaux_et_disparait_avant_le_masquage() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        // Retard CSS 0,65 s : la carte est encore invisible à 600 ms.
        let avant = etat.rendu_at(t0 + Duration::from_millis(600)).expect("visible");
        assert!(avant.phases.carte_opacite.abs() < 1e-3);
        assert!(
            (avant.phases.carte_avance).abs() < 1e-3,
            "translateY(26px) scale(.94) au départ"
        );
        // En cours d'ouverture : opacité partielle, transform déjà avancé.
        let milieu = etat.rendu_at(t0 + Duration::from_millis(950)).expect("visible");
        assert!(milieu.phases.carte_opacite > 0.1 && milieu.phases.carte_opacite < 1.0);
        // Stable après 0,65 + 0,55 = 1,2 s.
        let stable = etat.rendu_at(t0 + Duration::from_millis(1300)).expect("visible");
        assert!((stable.phases.carte_opacite - 1.0).abs() < 1e-3);
        assert!((stable.phases.carte_avance - 1.0).abs() < 1e-3);
    }

    #[test]
    fn carte_mode_card_entre_sans_retard() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Card), t0);
        // `transition-delay: 0s` en mode card : déjà en fondu à 300 ms.
        let milieu = etat.rendu_at(t0 + Duration::from_millis(300)).expect("visible");
        assert!(milieu.phases.carte_opacite > 0.3);
        let stable = etat.rendu_at(t0 + Duration::from_millis(700)).expect("visible");
        assert!((stable.phases.carte_opacite - 1.0).abs() < 1e-3);
    }

    #[test]
    fn carte_reste_visible_pendant_la_fermeture_des_rideaux() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        etat.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(2000));
        // Le fondu de la carte ne démarre qu'après 0,65 s (le CSS garde la
        // carte pendant que les rideaux se referment).
        let tient = etat.rendu_at(t0 + Duration::from_millis(2500)).expect("sortie");
        assert!((tient.phases.carte_opacite - 1.0).abs() < 1e-3);
        let fin = etat.rendu_at(t0 + Duration::from_millis(3100)).expect("sortie");
        assert!(fin.phases.carte_opacite < 1.0);
    }

    #[test]
    fn formater_viewers_espace_les_milliers() {
        assert_eq!(formater_viewers(0), "0");
        assert_eq!(formater_viewers(999), "999");
        assert_eq!(formater_viewers(1000), "1 000");
        assert_eq!(formater_viewers(1234), "1 234");
        assert_eq!(formater_viewers(1234567), "1 234 567");
    }

    #[test]
    fn url_box_art_remplace_les_placeholders() {
        // Miroir exact de `g.box_art_url.replace("{width}", "96")…` du DOM.
        assert_eq!(
            url_box_art("https://cdn/{width}x{height}.jpg"),
            "https://cdn/96x128.jpg"
        );
        assert_eq!(url_box_art("https://cdn/fiche.jpg"), "https://cdn/fiche.jpg");
    }

    #[test]
    fn colonnes_reduites_proportionnellement_sous_73_pour_cent() {
        // 1280 × 0,73 = 934,4 < 460 + 56 + 620 = 1136 → réduction.
        let max = CARTE_MAX_RATIO * 1280.0;
        let (g, d) = repartition_colonnes(false, max, 460.0, 620.0);
        assert!((g + CARTE_GAP + d - max).abs() < 0.01, "total = 73 %");
        assert!((g / 460.0 - d / 620.0).abs() < 1e-4, "réduction proportionnelle");
        // 1920 : 1401,6 > 1136 → aucune réduction.
        let (g, d) = repartition_colonnes(false, CARTE_MAX_RATIO * 1920.0, 460.0, 620.0);
        assert!((g - 460.0).abs() < 0.01);
        assert!((d - 620.0).abs() < 0.01);
    }

    #[test]
    fn mode_card_empile_sans_reduire() {
        let (g, d) = repartition_colonnes(true, CARTE_MAX_CARD, 460.0, 620.0);
        assert!((g - 460.0).abs() < 0.01);
        assert!((d - 620.0).abs() < 0.01);
        // Carte plus étroite que les colonnes : chacune est bornée à la carte.
        let (g, d) = repartition_colonnes(true, 400.0, 460.0, 620.0);
        assert!((g - 400.0).abs() < 0.01);
        assert!((d - 400.0).abs() < 0.01);
    }

    #[test]
    fn clip_joue_puis_se_termine_seul() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        assert!(etat.rendu_at(t0).expect("visible").clip.is_none());

        etat.appliquer_at(
            MajPresentation::ClipPlay {
                mp4_url: "https://clips-media-assets2.twitch.tv/abc-1080.mp4?sig=x".to_string(),
                titre: "Le clip".to_string(),
                duree_ms: 5000,
            },
            t0 + Duration::from_millis(1500),
        );
        let en_cours = etat
            .rendu_at(t0 + Duration::from_millis(2000))
            .expect("visible");
        let clip = en_cours.clip.expect("clip en cours");
        assert_eq!(clip.titre, "Le clip");
        assert_eq!(
            etat.gain_clip_milliemes_at(t0 + Duration::from_millis(2000)),
            1000
        );

        // `duree_ms` dépassée → le DOM reçoit `onended` et revient à la carte.
        let fini = etat
            .rendu_at(t0 + Duration::from_millis(7000))
            .expect("présentation toujours visible");
        assert!(fini.clip.is_none(), "clip terminé");
        assert_eq!(
            etat.gain_clip_milliemes_at(t0 + Duration::from_millis(7000)),
            0,
            "son coupé en fin de clip"
        );

        // `-clip-stop` : retour immédiat, même avant la fin.
        etat.appliquer_at(
            MajPresentation::ClipPlay {
                mp4_url: "https://x/y.mp4".to_string(),
                titre: String::new(),
                duree_ms: 0,
            },
            t0 + Duration::from_millis(8000),
        );
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(8100))
            .expect("visible")
            .clip
            .is_some());
        etat.appliquer_at(MajPresentation::ClipStop, t0 + Duration::from_millis(8200));
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(8300))
            .expect("visible")
            .clip
            .is_none());
        // Durée inconnue (0) : le clip reste jusqu'au stop.
        etat.appliquer_at(
            MajPresentation::ClipPlay {
                mp4_url: "https://x/z.mp4".to_string(),
                titre: String::new(),
                duree_ms: 0,
            },
            t0 + Duration::from_millis(9000),
        );
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(60000))
            .expect("visible")
            .clip
            .is_some());
    }

    #[test]
    fn show_et_hide_coupent_le_clip() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        etat.appliquer_at(
            MajPresentation::ClipPlay {
                mp4_url: "https://x/y.mp4".to_string(),
                titre: "T".to_string(),
                duree_ms: 0,
            },
            t0 + Duration::from_millis(100),
        );
        // `playPresentationShow` coupe le clip (il remplace la présentation).
        etat.appliquer_at(show(ModePresentation::Theatrical), t0 + Duration::from_millis(200));
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(300))
            .expect("visible")
            .clip
            .is_none());

        etat.appliquer_at(
            MajPresentation::ClipPlay {
                mp4_url: "https://x/y.mp4".to_string(),
                titre: "T".to_string(),
                duree_ms: 0,
            },
            t0 + Duration::from_millis(400),
        );
        // `stopPresentation` coupe le clip tout de suite (pas à la fin des
        // rideaux).
        etat.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(500));
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(600))
            .expect("sortie en cours")
            .clip
            .is_none());
    }

    #[test]
    fn chemin_clip_sous_medias_clips() {
        let base = std::path::Path::new("C:/data");
        let url = "https://clips-media-assets2.twitch.tv/AT-cm%7Cabc-1080.mp4?sig=x";
        assert_eq!(
            chemin_clip(base, url),
            base.join("medias")
                .join("clips")
                .join(format!("{}.mp4", crate::chat::empreinte_url(url)))
        );
    }

    #[test]
    fn pistes_audio_jouent_et_s_arretent() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);

        etat.appliquer_at(
            MajPresentation::AudioPlay {
                piste: PistePresentation::Musique,
                src: "medias/musique.mp3".to_string(),
            },
            t0 + Duration::from_millis(100),
        );
        etat.appliquer_at(
            MajPresentation::AudioPlay {
                piste: PistePresentation::Voix,
                src: "medias/voix.mp3".to_string(),
            },
            t0 + Duration::from_millis(150),
        );
        let r = etat.rendu_at(t0 + Duration::from_millis(200)).expect("visible");
        assert_eq!(r.audio[PistePresentation::Voix.index()], Some("medias/voix.mp3"));
        assert_eq!(
            r.audio[PistePresentation::Musique.index()],
            Some("medias/musique.mp3")
        );
        assert_eq!(etat.gain_piste_milliemes(PistePresentation::Voix), 1000);
        assert_eq!(etat.gain_piste_milliemes(PistePresentation::Musique), 1000);

        // `-audio-stop` d'une seule piste : l'autre continue.
        etat.appliquer_at(
            MajPresentation::AudioStop(Some(PistePresentation::Voix)),
            t0 + Duration::from_millis(300),
        );
        let r = etat.rendu_at(t0 + Duration::from_millis(350)).expect("visible");
        assert!(r.audio[0].is_none());
        assert!(r.audio[1].is_some());
        assert_eq!(etat.gain_piste_milliemes(PistePresentation::Voix), 0);

        // « toutes » : les deux s'arrêtent.
        etat.appliquer_at(MajPresentation::AudioStop(None), t0 + Duration::from_millis(400));
        let r = etat.rendu_at(t0 + Duration::from_millis(450)).expect("visible");
        assert!(r.audio.iter().all(Option::is_none));
        assert_eq!(etat.gain_piste_milliemes(PistePresentation::Musique), 0);
    }

    #[test]
    fn show_et_hide_arretent_les_pistes() {
        let mut etat = EtatPresentation::nouveau();
        let t0 = Instant::now();
        etat.appliquer_at(show(ModePresentation::Theatrical), t0);
        etat.appliquer_at(
            MajPresentation::AudioPlay {
                piste: PistePresentation::Musique,
                src: "medias/musique.mp3".to_string(),
            },
            t0 + Duration::from_millis(100),
        );
        // `playPresentationShow` → `presentationAudioStop("toutes")`.
        etat.appliquer_at(show(ModePresentation::Theatrical), t0 + Duration::from_millis(200));
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(250))
            .expect("visible")
            .audio
            .iter()
            .all(Option::is_none));

        etat.appliquer_at(
            MajPresentation::AudioPlay {
                piste: PistePresentation::Voix,
                src: "medias/voix.mp3".to_string(),
            },
            t0 + Duration::from_millis(300),
        );
        // `stopPresentation` → idem, tout de suite (pas à la fin des rideaux).
        etat.appliquer_at(MajPresentation::Hide, t0 + Duration::from_millis(400));
        assert!(etat
            .rendu_at(t0 + Duration::from_millis(450))
            .expect("sortie en cours")
            .audio
            .iter()
            .all(Option::is_none));
    }

    #[test]
    fn piste_presentation_suit_les_noms_du_dom() {
        assert_eq!(
            PistePresentation::depuis_nom("voix"),
            Some(PistePresentation::Voix)
        );
        assert_eq!(
            PistePresentation::depuis_nom("musique"),
            Some(PistePresentation::Musique)
        );
        assert_eq!(PistePresentation::depuis_nom("basse"), None);
        assert_eq!(PistePresentation::depuis_nom("toutes"), None);
        assert_eq!(PistePresentation::Voix.nom(), "voix");
        // Seule `musique` boucle (`audio.loop = piste === "musique"`).
        assert!(!PistePresentation::Voix.boucle());
        assert!(PistePresentation::Musique.boucle());
        assert_eq!(PistePresentation::Voix.index(), 0);
        assert_eq!(PistePresentation::Musique.index(), 1);
    }
}


