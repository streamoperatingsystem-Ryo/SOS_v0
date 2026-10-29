//! Médias statiques du moteur — décodage WIC (PNG/JPG/BMP) → texture PBGRA.
//!
//! Tranche P3 n°1 : fond de scène + widgets média, sans animation ni texte.
//! Les vidéos (Media Foundation Source Reader) et le rendu de texte arrivent
//! dans les tranches suivantes — ne pas élargir ce module sans plan.

use std::ffi::c_void;
use std::fmt;
use std::path::{Path, PathBuf};

use windows::Foundation::Numerics::Matrix3x2;
use windows::core::{HSTRING, Interface};
use windows::Win32::Foundation::{RECT, GENERIC_READ};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Imaging::{
    IWICImagingFactory, CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::Media::MediaFoundation::{
    IMFSourceReader, IMFSample, MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromURL,
    MFMediaType_Video, MFVideoFormat_RGB32, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM,
};
use windows::core::{GUID, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::cadres::CadreCfg;
use crate::gpu::{ContexteGpu, DescTexture, Format};

/// Erreurs de média. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurMedia {
    Fichier(String),
    Decodage(String),
    Gpu(String),
}

impl fmt::Display for ErreurMedia {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurMedia::Fichier(m) => write!(f, "fichier : {m}"),
            ErreurMedia::Decodage(m) => write!(f, "décodage : {m}"),
            ErreurMedia::Gpu(m) => write!(f, "GPU : {m}"),
        }
    }
}

impl std::error::Error for ErreurMedia {}

/// Comment le média remplit son rect (contrat `mediaFit`/`bgFit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Étirement au rect (historique, aucune déformation évitée).
    Etirer,
    /// Contenu entier, centré (letterbox).
    Ajuster,
    /// Rect entièrement couvert, crop centré (cover).
    Remplir,
    /// Taille naturelle centrée — `object-fit: none` : la destination peut
    /// déborder la boîte, c'est le clip (`overflow: hidden`) qui rogne.
    Centrer,
    /// `scale-down` : min(taille naturelle, contain) — jamais agrandi.
    Vignette,
}

impl Fit {
    /// `mediaFit` d'un widget — vrai `object-fit` (`FIT_CSS`, diffusion.html).
    pub fn depuis_nom(nom: &str) -> Self {
        match nom {
            "ajuster" => Fit::Ajuster,
            "remplir" | "etendre" => Fit::Remplir,
            "centrer" => Fit::Centrer,
            "vignette" => Fit::Vignette,
            _ => Fit::Etirer,
        }
    }

    /// `bgFit` du fond de scène — `fitRects` de `diffusion.html` : `centrer`
    /// n'y est PAS `object-fit: none` (naturel si le média tient dans le
    /// canvas, sinon contain) — soit exactement `scale-down`.
    pub fn depuis_nom_fond(nom: &str) -> Self {
        match nom {
            "centrer" => Fit::Vignette,
            autre => Fit::depuis_nom(autre),
        }
    }
}

/// Transform 2D d'un média — miroir de `applyMediaStyle` (widget) et de
/// `dessinerContenuFond`/`renderBg` (fond) dans `diffusion.html`.
///
/// Chaîne du widget (`transform-origin: center`) :
/// `translate(offset) rotate(rot) scale(zoom·miroir)` ; le fond peint la
/// même chaîne sur son canvas. `zoom_ecran` = le `scale(bgZoom)` posé sur
/// `#bg` entier en espace écran (post-tilt, lot T2) — distinct du zoom du
/// média : il englobe aussi l'offset. 1 partout sauf pour le fond de scène.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    /// Zoom du média dans sa boîte (1 = taille du fit).
    pub zoom: f64,
    /// Rotation 2D en **degrés** (0 = aucune).
    pub rot: f64,
    /// Translation en px canvas, appliquée après la rotation.
    pub offset_x: f64,
    pub offset_y: f64,
    /// Retournement horizontal/vertical (`scale(−1)` combiné au zoom, avant
    /// la rotation — flip de la source, sémantique OBS).
    pub miroir_h: bool,
    pub miroir_v: bool,
    /// Scale externe autour du centre (`bgZoom` sur `#bg`) : 1 = absent.
    pub zoom_ecran: f64,
}

impl Transform2D {
    /// Rien à appliquer : le chemin historique sans transform suffit.
    pub const NEUTRE: Self = Self {
        zoom: 1.0,
        rot: 0.0,
        offset_x: 0.0,
        offset_y: 0.0,
        miroir_h: false,
        miroir_v: false,
        zoom_ecran: 1.0,
    };

    pub fn est_neutre(&self) -> bool {
        *self == Self::NEUTRE
    }

    /// Matrice Direct2D du transform ; `centre` = centre de la boîte du
    /// média (rect du widget, ou le canvas pour le fond de scène).
    ///
    /// Convention `Matrix3x2` (windows-rs) : `p·M` — le facteur de **gauche**
    /// s'applique au point en premier (précédent `rotation * avant`,
    /// texte.rs). La chaîne CSS colonne `T(c)·T(o)·R·S·T(−c)` se lit donc
    /// `T(−c)·S·R·T(o)·T(c)` ici : recentrage → scale(zoom·miroir) →
    /// rotation → offset → retour. `zoom_ecran` enfile ensuite le
    /// `scale(bgZ)` de `#bg` : `…·T(−c)·S(z)·T(c)`, autour du même centre.
    ///
    /// ⚠️ `Matrix3x2::rotation` passe par `D2D1MakeRotateMatrix`, qui attend
    /// des **degrés** (piège documenté dans texte.rs) — `rot` l'est déjà.
    pub fn matrice(&self, centre: (f64, f64)) -> Matrix3x2 {
        let (cx, cy) = (centre.0 as f32, centre.1 as f32);
        let sx = (self.zoom * if self.miroir_h { -1.0 } else { 1.0 }) as f32;
        let sy = (self.zoom * if self.miroir_v { -1.0 } else { 1.0 }) as f32;
        let t_mc = Matrix3x2::translation(-cx, -cy);
        let t_c = Matrix3x2::translation(cx, cy);
        let scale = Matrix3x2 {
            M11: sx,
            M12: 0.0,
            M21: 0.0,
            M22: sy,
            M31: 0.0,
            M32: 0.0,
        };
        let rotation = Matrix3x2::rotation(self.rot as f32, 0.0, 0.0);
        let offset = Matrix3x2::translation(self.offset_x as f32, self.offset_y as f32);
        let mut m = t_mc * scale * rotation * offset * t_c;
        if self.zoom_ecran != 1.0 {
            let z = self.zoom_ecran as f32;
            let zoom = Matrix3x2 {
                M11: z,
                M12: 0.0,
                M21: 0.0,
                M22: z,
                M31: 0.0,
                M32: 0.0,
            };
            m = m * t_mc * zoom * t_c;
        }
        m
    }
}

/// Effets visuels d'un média — miroir de `chaineFilter` (diffusion.html) :
/// `brightness(1+lum/100) contrast(1+contraste/100) hue-rotate(teinte°)
/// blur(flou px)`, appliqués au média DOM (`style.filter`) ou en
/// `ctx.filter` sur les chemins canvas (morph, fond).
///
/// `pixel` = pixelisation downscale→upscale sans lissage — chemins
/// morph/fond **uniquement** (parité DOM : aucun `filter` pixel n'existe
/// côté widget ; le fond en a un — `bgPixel`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffetsCfg {
    /// `brightness(1 + lum/100)` — 0 = neutre.
    pub lum: f64,
    /// `contrast(1 + contraste/100)` — 0 = neutre.
    pub contraste: f64,
    /// `hue-rotate(teinte°)` — 0 = neutre.
    pub teinte: f64,
    /// `blur(flou px)` en px de l'**espace destination** (élément DOM ou
    /// canvas) — converti en écart-type source au dessin (`flou / s_fit`,
    /// approximation isotrope sous étirement non uniforme).
    pub flou: f64,
    /// Facteur de pixelisation (`bgPixel`) — ≤ 1 = inactif.
    pub pixel: f64,
}

impl EffetsCfg {
    /// Tout neutre : le chemin historique `DrawBitmap` suffit.
    pub const NEUTRE: Self = Self {
        lum: 0.0,
        contraste: 0.0,
        teinte: 0.0,
        flou: 0.0,
        pixel: 0.0,
    };

    pub fn est_neutre(&self) -> bool {
        *self == Self::NEUTRE
    }

    /// La partie couleur mérite une `ColorMatrix` (chaîne `""` de
    /// `chaineFilter` quand les trois paramètres sont nuls).
    pub fn couleur_active(&self) -> bool {
        self.lum != 0.0 || self.contraste != 0.0 || self.teinte != 0.0
    }

    /// Pixelisation active (`pf > 1` — DOM borne à `Math.max(1, …)`).
    pub fn pixel_active(&self) -> bool {
        self.pixel > 1.0
    }

    /// Matrice `D2D1_COLOR_MATRIX` (5×4, 80 octets) composant
    /// brightness → contrast → hue-rotate — évaluée en alpha **straight**
    /// (`D2D1_COLORMATRIX_ALPHA_MODE_STRAIGHT`), comme les filtres CSS.
    ///
    /// Convention D2D : `out_c = Σ_r in_r·M[r][c] + M[4][c]` — les lignes
    /// sont les canaux d'entrée (r, g, b, a, 1). La partie linéaire est
    /// `k·H` avec `k = (1+lum/100)·(1+contraste/100)` et `H` la matrice
    /// `hueRotate` des spéc SVG/CSS (luminance 0.213/0.715/0.072,
    /// convention colonne `out = H·v` — transposée ici). L'offset du
    /// contraste `o = 0,5 − 0,5·(1+contraste/100)` traverse H inchangé
    /// (chaque ligne de H somme à 1).
    pub fn matrice_couleur(&self) -> [[f32; 4]; 5] {
        let k = ((1.0 + self.lum / 100.0) * (1.0 + self.contraste / 100.0)) as f32;
        let o = (0.5 - 0.5 * (1.0 + self.contraste / 100.0)) as f32;
        let t = self.teinte.to_radians();
        let (s, c) = (t.sin() as f32, t.cos() as f32);
        let h = [
            [0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.072],
            [0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283],
            [0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072],
        ];
        let mut m = [[0.0f32; 4]; 5];
        for (r, ligne) in m.iter_mut().take(3).enumerate() {
            for (c, case) in ligne.iter_mut().take(3).enumerate() {
                *case = k * h[c][r];
            }
        }
        m[3][3] = 1.0;
        m[4][0] = o;
        m[4][1] = o;
        m[4][2] = o;
        m
    }
}

/// Perspective 3D d'un plan — `perspective(d) rotateX(rx) rotateY(ry)`
/// (`applyWidgetStyle` sur `.widget`, `renderBg` sur `#bg`, diffusion.html).
///
/// Le plan entier (voile + média + bordure + cadre pour un widget ; le média
/// de fond pour la scène) est d'abord rasterisé dans une texture
/// intermédiaire, puis projeté par l'effet `CLSID_D2D13DPerspectiveTransform`
/// — la projection n'existe pas sur `DrawBitmap`. Les entrées du plan de
/// composition sont regroupées par (`rect`, `z`), miroir du `.widget` DOM :
/// tous les calques d'un widget partagent son rect et son z.
#[derive(Debug, Clone, PartialEq)]
pub struct PerspCfg {
    /// Boîte du plan en coords canvas (rect du widget, ou le canvas entier
    /// pour le fond) — clé de regroupement avec `z`.
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    /// `rotateX`/`rotateY` en degrés.
    pub rot_x: f64,
    pub rot_y: f64,
    /// `perspective(d)` en px : 800 pour les widgets (constante
    /// `applyWidgetStyle`), 2·canvasW pour le fond (`renderBg`).
    pub profondeur: f64,
    /// `scale(bgZ)` posé en premier sur `#bg` = appliqué **après** la
    /// projection, en espace écran autour du centre : 1 = absente.
    pub zoom_ecran: f64,
}

/// État de lecture d'une vidéo (`mediaPaused`/`bgPaused`, `mediaLoopA/B`,
/// `bgLoopA/B`) — miroir d'`applyMediaState`/`applyBgState` (diffusion.html).
///
/// `mediaTime`/`bgTime` sont volontairement absents : le DOM seede le tracker
/// à la création (`entry.mediaTime = w.mediaTime`) → aucun seek initial ;
/// l'engine rejoue ce comportement (une session = un rechargement de page).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LectureCfg {
    /// `mediaPaused`/`bgPaused` (défaut true) : vidéo figée sur sa première
    /// frame — `v.pause()` du DOM.
    pub paused: bool,
    /// Boucle A–B en secondes : `Some((a, b))` avec b > a = reseek à A dès
    /// que la position atteint B. `None` = boucle native complète
    /// (`v.loop = !paused && !ab`).
    pub boucle: Option<(f64, f64)>,
}

/// Carrousel d'images (`mediaSlides`/`bgSlides` non vide) — la liste est
/// prioritaire sur le média unique (contrat `carrousel.md`, images
/// uniquement).
#[derive(Debug, Clone, PartialEq)]
pub struct CarrouselCfg {
    /// Chemins résolus des slides.
    pub slides: Vec<PathBuf>,
    /// Intervalle entre deux slides : `slidesSecOf` = `max(1, sec || 5)`.
    pub sec: f64,
    /// `true` = widget : fondu `ease-in-out` (transition CSS des `.slide`) ;
    /// `false` = fond : alpha linéaire (`globalAlpha = kFade` de
    /// `dessinerContenuFond`).
    pub ease: bool,
}

/// Un média statique à composer : chemin résolu + destination + ordre z.
#[derive(Debug, Clone)]
pub struct MediaCfg {
    pub chemin: PathBuf,
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    pub fit: Fit,
    /// Transform 2D du contenu (zoom/rotation/offset/miroir, + `zoom_ecran`
    /// du fond) — [`Transform2D::NEUTRE`] = chemin historique.
    pub transform: Transform2D,
    /// Effets visuels (`chaineFilter` + `bgPixel`) — [`EffetsCfg::NEUTRE`]
    /// = chemin historique.
    pub effets: EffetsCfg,
    /// Cadre du widget porteur : `Some` = le média est **découpé au polygone**
    /// du cadre (un seul polygone, miroir de `pointsClipPathCadre`). `None` =
    /// média sans cadre, composé par le video processor.
    pub cadre: Option<CadreCfg>,
    /// Points de morph bulge/pinch (`w.morphs` / `scene.bgMorphs`) —
    /// non vide = le média passe par le chemin offscreen + warp (T4).
    pub morphs: Vec<sos_scene::MorphPoint>,
    /// Pixelisation **en chemin morph** (`w.mediaPixel` pour un widget —
    /// le fond réutilise `effets.pixel`/`bgPixel` dans les deux chemins).
    /// Ignorée sans morph, parité DOM.
    pub pixel_morph: f64,
    /// Lecture vidéo (pause / boucle A–B) — sans effet sur les images.
    pub lecture: LectureCfg,
    /// Carrousel d'images — `Some` = prioritaire sur `chemin` (le média
    /// unique reste stocké, jamais chargé, comme la pile `.slide` du DOM).
    pub carrousel: Option<CarrouselCfg>,
}

/// Média décodé, prêt pour le compositeur.
pub struct MediaChargee {
    pub texture: ID3D11Texture2D,
    pub taille: (u32, u32),
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    pub fit: Fit,
    /// Transform 2D du contenu (voir [`MediaCfg::transform`]).
    pub transform: Transform2D,
    /// Effets visuels du contenu (voir [`MediaCfg::effets`]).
    pub effets: EffetsCfg,
    /// Cadre du widget porteur (voir [`MediaCfg::cadre`]).
    pub cadre: Option<CadreCfg>,
    /// Points de morph (voir [`MediaCfg::morphs`]).
    pub morphs: Vec<sos_scene::MorphPoint>,
    /// Pixelisation en chemin morph (voir [`MediaCfg::pixel_morph`]).
    pub pixel_morph: f64,
    /// `true` si le média porte de l'alpha **par pixel** (PNG `rgba`).
    ///
    /// Décidé au décodage : les pixels sont en **PBGRA** (prémultiplié) et
    /// l'octet alpha est inspecté une fois. Une vidéo (RGB32/X8, octet alpha
    /// non garanti) et une image opaque valent `false` : leur bitmap
    /// Direct2D reste en `ALPHA_MODE_IGNORE`, jamais un média qui disparaît.
    pub alpha: bool,
    /// Lecture vidéo (voir [`MediaCfg::lecture`]).
    pub lecture: LectureCfg,
    /// Carrousel chargé (voir [`MediaCfg::carrousel`]) — `Some` = le média
    /// de base n'est pas chargé (priorité de la liste, contrat carrousel).
    pub carrousel: Option<Carrousel>,
    video: Option<VideoLecteur>,
}

/// Une slide de carrousel décodée en texture (images uniquement).
pub struct SlideChargee {
    pub texture: ID3D11Texture2D,
    pub taille: (u32, u32),
    /// Alpha par pixel (voir [`MediaChargee::alpha`]).
    pub alpha: bool,
}

/// Carrousel chargé : slides + rythme (miroir `slidesDemarrer` pour les
/// widgets, `startBgLoop`/`dessinerContenuFond` pour le fond).
pub struct Carrousel {
    /// Slides décodées — `empty` = toutes en échec, rien n'est dessiné
    /// (miroir d'une pile de `<img>` cassées).
    pub slides: Vec<SlideChargee>,
    /// Intervalle entre slides (≥ 1 s — `slidesSecOf`).
    pub sec: f64,
    /// Durée du fondu : `min(0,8 s, sec / 2)` — contrat `carrousel.md`
    /// (`fadeDur = min(800, sec×500)` du fond, `transitionDuration` idem
    /// pour les `.slide` du widget).
    pub fondu_s: f64,
    /// `true` = widget (`ease-in-out` CSS) ; `false` = fond (linéaire).
    pub ease: bool,
}

impl Carrousel {
    /// Slide courante (+ précédente pendant le fondu) à l'instant `temps_s`.
    ///
    /// Miroir du `setInterval(sec)` du DOM : à chaque multiple de `sec`,
    /// l'index avance et le fondu court `fondu_s` secondes — la précédente
    /// reste dessous à `1 - k`, la courante entre à `k`. Moins de 2 slides
    /// = statique (`slidesDemarrer` n'arme aucun timer).
    ///
    /// Retour : `(courante, Option<(precedente, k)>)` avec `k` ∈ [0, 1[ —
    /// `k` est la progression du fondu (eased pour un widget, linéaire pour
    /// le fond).
    pub fn fenetre(&self, temps_s: f64) -> (usize, Option<(usize, f32)>) {
        fenetre_carrousel(
            self.slides.len(),
            self.sec,
            self.fondu_s,
            self.ease,
            temps_s,
        )
    }
}

/// Cœur pur de [`Carrousel::fenetre`] (sans les textures, testable).
fn fenetre_carrousel(
    nb_slides: usize,
    sec: f64,
    fondu_s: f64,
    ease: bool,
    temps_s: f64,
) -> (usize, Option<(usize, f32)>) {
    if nb_slides < 2 {
        return (0, None);
    }
    let n = nb_slides as i64;
    let cycle = (temps_s.max(0.0) / sec).floor() as i64;
    let courante = cycle.rem_euclid(n) as usize;
    let k = ((temps_s.max(0.0) - cycle as f64 * sec) / fondu_s) as f32;
    if cycle >= 1 && k < 1.0 {
        let precedente = (cycle - 1).rem_euclid(n) as usize;
        (
            courante,
            Some((precedente, if ease { ease_in_out(k) } else { k })),
        )
    } else {
        (courante, None)
    }
}

/// `ease-in-out` CSS = `cubic-bezier(0,42 ; 0 ; 0,58 ; 1)` — évaluée
/// exactement (Newton sur le paramètre de Bézier), comme les navigateurs
/// l'appliquent à la transition `opacity` des `.slide`.
fn ease_in_out(t: f32) -> f32 {
    let bez = |u: f32, p1: f32, p2: f32| {
        let v = 1.0 - u;
        3.0 * u * v * v * p1 + 3.0 * u * u * v * p2 + u * u * u
    };
    // x(u) : abscisses de contrôle 0,42 et 0,58 — inverser x(u) = t.
    let mut u = t.clamp(0.0, 1.0);
    for _ in 0..4 {
        let x = bez(u, 0.42, 0.58);
        let v = 1.0 - u;
        let dx = 3.0 * v * v * 0.42 + 6.0 * u * v * (0.58 - 0.42) + 3.0 * u * u * (1.0 - 0.58);
        if dx.abs() < 1e-6 {
            break;
        }
        u = (u - (x - t) / dx).clamp(0.0, 1.0);
    }
    // y(u) : ordonnées de contrôle 0 et 1.
    bez(u, 0.0, 1.0)
}

struct VideoLecteur {
    reader: IMFSourceReader,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    /// Taille de la frame vidéo (source `ReadSample`, fixe).
    taille: (u32, u32),
    /// Frame lue mais **pas encore due** (PTS postérieur au temps écoulé).
    /// Le Source Reader ne sait pas remettre un échantillon : on le garde.
    en_attente: Option<(IMFSample, i64)>,
    /// `mediaPaused`/`bgPaused` : la vidéo est figée après sa première
    /// frame (`v.pause()` du DOM).
    paused: bool,
    /// Boucle A–B en µs (`mediaLoopA/B` × 1e6) : reseek à A dès que la
    /// position affichée atteint B.
    boucle: Option<(i64, i64)>,
    /// PTS vidéo de la dernière frame affichée ; −1 = aucune frame montrée.
    position_us: i64,
    /// Décalage horloge moteur → horloge vidéo : un seek à `p` au temps
    /// moteur `t` pose `decalage = t - p`, et la vidéo repart de `p` en
    /// rythme (miroir du `v.currentTime = …` du DOM).
    decalage_us: i64,
}

impl VideoLecteur {
    /// `SetCurrentPosition` à `position_us` (µs → hectonanosecondes) et
    /// recalage de l'horloge vidéo : les frames qui suivent le seek sont
    /// dues quand `temps_moteur - decalage` atteint leur PTS — comme
    /// `v.currentTime = position` dans le DOM.
    fn seeker(&mut self, position_us: i64, temps_us: i64) -> Result<(), ErreurMedia> {
        let prop = PROPVARIANT::from(position_us.saturating_mul(10));
        unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &prop) }
        .map_err(|e| ErreurMedia::Decodage(format!("seek : {}", e.message())))?;
        self.en_attente = None;
        self.position_us = -1;
        self.decalage_us = temps_us - position_us;
        Ok(())
    }

    /// Téléverse un échantillon RGB32 dans `texture` (`UpdateSubresource`).
    fn afficher(
        &mut self,
        texture: &ID3D11Texture2D,
        sample: &IMFSample,
    ) -> Result<(), ErreurMedia> {
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }
            .map_err(|e| ErreurMedia::Decodage(format!("buffer : {}", e.message())))?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut longueur = 0u32;
        unsafe { buffer.Lock(&mut ptr, None, Some(&mut longueur)) }
            .map_err(|e| ErreurMedia::Decodage(format!("Lock : {}", e.message())))?;
        let ressource: windows::Win32::Graphics::Direct3D11::ID3D11Resource = texture
            .cast()
            .map_err(|e| ErreurMedia::Gpu(format!("texture → ressource : {}", e.message())))?;
        unsafe {
            self.context.UpdateSubresource(
                Some(&ressource),
                0,
                None,
                ptr as *const c_void,
                self.taille.0 * 4,
                0,
            )
        };
        unsafe { buffer.Unlock() }
            .map_err(|e| ErreurMedia::Decodage(format!("Unlock : {}", e.message())))?;
        Ok(())
    }
}

impl MediaChargee {
    pub fn est_video(&self) -> bool {
        self.video.is_some()
    }

    /// Taille de la slide `index` (carrousel) — ou du média hors carrousel.
    /// Le fit (`fit_rects`/`geometrie`) se calcule par slide : leurs tailles
    /// peuvent différer.
    pub fn taille_slide(&self, index: usize) -> (u32, u32) {
        self.carrousel
            .as_ref()
            .and_then(|c| c.slides.get(index))
            .map(|s| s.taille)
            .unwrap_or(self.taille)
    }

    /// Avance la vidéo jusqu'à la frame **due** à `temps_us`.
    ///
    /// Une frame lue dont le PTS est postérieur au temps écoulé est conservée
    /// jusqu'à son échéance (`en_attente`) : sans cette attente, chaque appel
    /// du moteur afficherait une frame et toute vidéo serait jouée à la
    /// cadence du moteur — ×2 pour une source 30 fps, ×2,5 pour du 24 fps
    /// (mesuré 2026-10-28 : frame 61 à t = 1 s d'un compteur 24 fps).
    /// Sans effet pour une image.
    ///
    /// Sémantique `applyMediaState`/`applyBgState` (diffusion.html) : une
    /// vidéo en pause est figée après sa première frame ; la boucle A–B
    /// (`mediaLoopA/B`) reseek à A dès que la position affichée atteint B —
    /// le `timeupdate` ~4 Hz du DOM est approximé à la frame près — et la
    /// boucle simple (`v.loop = !paused && !ab`) reseek à 0 en fin de flux.
    /// `mediaTime` n'est **pas** appliqué au démarrage : le tracker DOM est
    /// seedé à la création, une session = un rechargement de page.
    /// Dernière image figée en fin de flux sans boucle. Un seul seek par
    /// appel : un fichier dont le seek retomberait ≥ B bouclerait à l'infini.
    pub fn avancer(&mut self, temps_us: i64) -> Result<(), ErreurMedia> {
        let Some(video) = self.video.as_mut() else {
            return Ok(());
        };
        if video.paused && video.position_us >= 0 {
            return Ok(());
        }
        let texture = self.texture.clone();
        let mut seek_fait = false;
        loop {
            if !seek_fait {
                if let Some((a, b)) = video.boucle {
                    if video.position_us >= b {
                        video.seeker(a, temps_us)?;
                        seek_fait = true;
                    }
                }
            }
            let (sample, pts_us) = match video.en_attente.take() {
                Some(en_attente) => en_attente,
                None => {
                    let mut flags = 0u32;
                    let mut sample: Option<IMFSample> = None;
                    unsafe {
                        video.reader.ReadSample(
                            MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                            0,
                            None,
                            Some(&mut flags),
                            None,
                            Some(&mut sample),
                        )
                    }
                    .map_err(|e| {
                        ErreurMedia::Decodage(format!("ReadSample : {}", e.message()))
                    })?;
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        if seek_fait {
                            return Ok(());
                        }
                        match video.boucle {
                            Some((a, _)) => video.seeker(a, temps_us)?,
                            None if !video.paused => video.seeker(0, temps_us)?,
                            _ => return Ok(()),
                        }
                        seek_fait = true;
                        continue;
                    }
                    let Some(sample) = sample else {
                        return Ok(());
                    };
                    let pts_us = unsafe { sample.GetSampleTime() }.unwrap_or(0) / 10;
                    (sample, pts_us)
                }
            };
            // Frame future : conservée jusqu'à son échéance — sauf la toute
            // première (`position_us < 0`), affichée même en pause (la
            // vignette du DOM montre sa première frame).
            let due = temps_us - video.decalage_us;
            if pts_us > due && video.position_us >= 0 {
                video.en_attente = Some((sample, pts_us));
                return Ok(());
            }
            video.afficher(&texture, &sample)?;
            video.position_us = pts_us;
            if video.paused {
                return Ok(());
            }
        }
    }
}

/// Calcule la fenêtre source et la destination selon le fit, puis rogne la
/// paire au canvas (chemin **sans transform** — voir [`fit_rects`]).
///
/// Le fit se calcule sur la boîte **entière**, comme `object-fit` en CSS :
/// borner la boîte au canvas avant le fit écrasait le média d'un widget qui
/// dépasse le canvas (2026-10-28 : 311 px au lieu de 452 sur `06de1ec7`,
/// boîte à `y = −83`). Le rognage au canvas se fait **après**, en recadrant la
/// source dans la même proportion : le video processor et Direct2D reçoivent
/// donc les mêmes pixels visibles, sans déformation.
///
/// ⚠️ Ne pas passer par ici sous transform : recadrer la destination **avant**
/// la rotation/zoom retire des pixels qui peuvent tourner dans le champ — le
/// chemin transformé dessine la paire de `fit_rects` entière et laisse le
/// clip (boîte ou polygone) rogner.
pub fn geometrie(
    taille: (u32, u32),
    rect: (f64, f64, f64, f64),
    canvas: (u32, u32),
    fit: Fit,
) -> (RECT, RECT) {
    let (source, destination) = fit_rects(taille, rect, fit);
    rogner_au_canvas(source, destination, canvas)
}

/// Fenêtre source et destination du **fit seul**, sans rognage canvas.
///
/// `rect` est la **boîte du widget** ; `taille` = dimensions natives du média.
/// `Etirer` garde toute l'image et le rect ; `Ajuster` réduit la destination
/// au ratio du média (centrée) ; `Remplir` garde le rect et croppe la source ;
/// `Centrer` (`object-fit: none`) pose la taille naturelle au centre — la
/// destination peut déborder la boîte ; `Vignette` (`scale-down`) = `Ajuster`
/// borné à ×1, jamais agrandi.
pub fn fit_rects(taille: (u32, u32), rect: (f64, f64, f64, f64), fit: Fit) -> (RECT, RECT) {
    let (mw, mh) = (taille.0.max(1) as f64, taille.1.max(1) as f64);
    let (x, y) = (rect.0, rect.1);
    let (w, h) = (rect.2.max(1.0), rect.3.max(1.0));

    let source_pleine = RECT {
        left: 0,
        top: 0,
        right: taille.0 as i32,
        bottom: taille.1 as i32,
    };
    let destination = RECT {
        left: x as i32,
        top: y as i32,
        right: (x + w) as i32,
        bottom: (y + h) as i32,
    };
    // Destination centrée dans la boîte ; l'arrondi évite une asymétrie de
    // 1 px du letterbox.
    let centre = |dw: f64, dh: f64| {
        let dx = (x + (w - dw) / 2.0).round();
        let dy = (y + (h - dh) / 2.0).round();
        RECT {
            left: dx as i32,
            top: dy as i32,
            right: (dx + dw).round() as i32,
            bottom: (dy + dh).round() as i32,
        }
    };

    match fit {
        Fit::Etirer => (source_pleine, destination),
        Fit::Ajuster => {
            let echelle = (w / mw).min(h / mh);
            (source_pleine, centre(mw * echelle, mh * echelle))
        }
        Fit::Centrer => (source_pleine, centre(mw, mh)),
        Fit::Vignette => {
            let echelle = (w / mw).min(h / mh).min(1.0);
            (source_pleine, centre(mw * echelle, mh * echelle))
        }
        Fit::Remplir => {
            let ratio_destination = w / h;
            let ratio_media = mw / mh;
            let (sw, sh) = if ratio_media > ratio_destination {
                (mh * ratio_destination, mh)
            } else {
                (mw, mw / ratio_destination)
            };
            let (sx, sy) = ((mw - sw) / 2.0, (mh - sh) / 2.0);
            (
                RECT {
                    left: sx as i32,
                    top: sy as i32,
                    right: (sx + sw) as i32,
                    bottom: (sy + sh) as i32,
                },
                destination,
            )
        }
    }
}

/// Rogne une paire source/destination au canvas, **sans déformer** le média.
///
/// La destination est bornée à la surface ; la source est recadrée dans la
/// même proportion (mapping linéaire identique). Une destination entièrement
/// hors canvas rend une paire vide : rien à peindre, aucune erreur.
fn rogner_au_canvas(source: RECT, destination: RECT, canvas: (u32, u32)) -> (RECT, RECT) {
    let (cw, ch) = (canvas.0 as i32, canvas.1 as i32);
    let visible = RECT {
        left: destination.left.max(0),
        top: destination.top.max(0),
        right: destination.right.min(cw),
        bottom: destination.bottom.min(ch),
    };
    let (dw, dh) = (
        (destination.right - destination.left) as f64,
        (destination.bottom - destination.top) as f64,
    );
    if visible.right <= visible.left || visible.bottom <= visible.top || dw <= 0.0 || dh <= 0.0 {
        let vide = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        return (vide, vide);
    }
    let (sx, sy) = (
        (source.right - source.left) as f64 / dw,
        (source.bottom - source.top) as f64 / dh,
    );
    let recadre = RECT {
        left: source.left + ((visible.left - destination.left) as f64 * sx).round() as i32,
        top: source.top + ((visible.top - destination.top) as f64 * sy).round() as i32,
        right: source.left + ((visible.right - destination.left) as f64 * sx).round() as i32,
        bottom: source.top + ((visible.bottom - destination.top) as f64 * sy).round() as i32,
    };
    (recadre, visible)
}

/// Charge une image ou une vidéo selon l'extension — ou un carrousel
/// (prioritaire sur le média unique, contrat `carrousel.md`).
pub fn charger(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    if let Some(carrousel) = &cfg.carrousel {
        return charger_carrousel(contexte, cfg, carrousel);
    }
    if !cfg.chemin.is_file() {
        return Err(ErreurMedia::Fichier(format!(
            "introuvable : {}",
            cfg.chemin.display()
        )));
    }
    if est_video(&cfg.chemin) {
        charger_video(contexte, cfg)
    } else {
        charger_image(contexte, cfg)
    }
}

/// Charge les slides d'un carrousel — chaque image est décodée en texture
/// (comme la pile `<img class="slide">` du DOM). Une slide illisible est
/// ignorée : le DOM poursuit avec les `<img>` restantes ; jamais d'échec
/// de session pour un fichier manquant.
fn charger_carrousel(
    contexte: &ContexteGpu,
    cfg: &MediaCfg,
    carrousel: &CarrouselCfg,
) -> Result<MediaChargee, ErreurMedia> {
    let mut slides = Vec::with_capacity(carrousel.slides.len());
    for chemin in &carrousel.slides {
        match decoder_pbgra(chemin) {
            Ok((largeur, hauteur, pixels, alpha)) if largeur > 0 && hauteur > 0 => {
                slides.push(SlideChargee {
                    texture: televerser(contexte, largeur, hauteur, &pixels)?,
                    taille: (largeur, hauteur),
                    alpha,
                });
            }
            _ => {
                log::warn!("carrousel : slide ignorée {}", chemin.display());
            }
        }
    }
    // Texture de base = première slide (ou 1×1 transparent si toutes en
    // échec — pile vide, rien n'est dessiné au passage du fondu).
    let (texture, taille, alpha) = match slides.first() {
        Some(s) => (s.texture.clone(), s.taille, s.alpha),
        None => (televerser(contexte, 1, 1, &[0, 0, 0, 0])?, (1, 1), true),
    };
    Ok(MediaChargee {
        texture,
        taille,
        rect: cfg.rect,
        z: cfg.z,
        fit: cfg.fit,
        transform: cfg.transform,
        effets: cfg.effets,
        cadre: cfg.cadre.clone(),
        morphs: cfg.morphs.clone(),
        pixel_morph: cfg.pixel_morph,
        alpha,
        lecture: cfg.lecture,
        carrousel: Some(Carrousel {

            slides,
            sec: carrousel.sec,
            fondu_s: (carrousel.sec * 0.5).min(0.8),
            ease: carrousel.ease,
        }),
        video: None,
    })
}

/// Extensions vidéo prises en charge par le Source Reader.
pub fn est_video(chemin: &Path) -> bool {
    matches!(
        chemin
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "m4v" | "mov" | "avi" | "mkv" | "webm"
    )
}

fn charger_image(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    let (largeur, hauteur, pixels, alpha) = decoder_pbgra(&cfg.chemin)?;
    if largeur == 0 || hauteur == 0 {
        return Err(ErreurMedia::Decodage("image vide".to_string()));
    }
    let texture = televerser(contexte, largeur, hauteur, &pixels)?;
    Ok(MediaChargee {
        texture,
        taille: (largeur, hauteur),
        rect: cfg.rect,
        z: cfg.z,
        fit: cfg.fit,
        transform: cfg.transform,
        effets: cfg.effets,
        cadre: cfg.cadre.clone(),
        morphs: cfg.morphs.clone(),
        pixel_morph: cfg.pixel_morph,
        alpha,
        lecture: cfg.lecture,
        carrousel: None,
        video: None,
    })
}

/// Ouvre une vidéo via Media Foundation Source Reader (sortie RGB32).
fn charger_video(contexte: &ContexteGpu, cfg: &MediaCfg) -> Result<MediaChargee, ErreurMedia> {
    crate::encode::detect::initialiser();
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe {
        let mut attrs: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 2)
            .map_err(|e| ErreurMedia::Decodage(format!("attributs : {}", e.message())))?;
        let attrs = attrs.ok_or_else(|| ErreurMedia::Decodage("attributs nuls".to_string()))?;
        attrs
            .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
            .map_err(|e| ErreurMedia::Decodage(format!("attributs : {}", e.message())))?;

        let reader = MFCreateSourceReaderFromURL(
            &HSTRING::from(cfg.chemin.as_os_str()),
            &attrs,
        )
        .map_err(|e| {
            ErreurMedia::Decodage(format!("ouverture {} : {}", cfg.chemin.display(), e.message()))
        })?;

        let type_sortie = MFCreateMediaType()
            .map_err(|e| ErreurMedia::Decodage(format!("type : {}", e.message())))?;
        type_sortie
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| ErreurMedia::Decodage(format!("major : {}", e.message())))?;
        type_sortie
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|e| ErreurMedia::Decodage(format!("subtype : {}", e.message())))?;
        reader
            .SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &type_sortie)
            .map_err(|e| ErreurMedia::Decodage(format!("type RGB32 : {}", e.message())))?;

        let courant = reader
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
            .map_err(|e| ErreurMedia::Decodage(format!("type courant : {}", e.message())))?;
        let pack = courant
            .GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(|e| ErreurMedia::Decodage(format!("taille vidéo : {}", e.message())))?;
        let largeur = (pack >> 32) as u32;
        let hauteur = pack as u32;
        if largeur == 0 || hauteur == 0 {
            return Err(ErreurMedia::Decodage("vidéo de taille nulle".to_string()));
        }

        let texture = creer_texture_video(contexte, largeur, hauteur)?;
        let mut media = MediaChargee {
            texture,
            taille: (largeur, hauteur),
            rect: cfg.rect,
            z: cfg.z,
            fit: cfg.fit,
            transform: cfg.transform,
            effets: cfg.effets,
            cadre: cfg.cadre.clone(),
            morphs: cfg.morphs.clone(),
            pixel_morph: cfg.pixel_morph,
            alpha: false,
            lecture: cfg.lecture,
            carrousel: None,
            video: Some(VideoLecteur {
                reader,
                context: contexte.context().clone(),
                taille: (largeur, hauteur),
                en_attente: None,
                paused: cfg.lecture.paused,
                boucle: cfg
                    .lecture
                    .boucle
                    .map(|(a, b)| ((a * 1e6) as i64, (b * 1e6) as i64)),
                position_us: -1,
                decalage_us: 0,
            }),
        };
        media.avancer(0)?;
        Ok(media)
    }
}

fn creer_texture_video(
    contexte: &ContexteGpu,
    largeur: u32,
    hauteur: u32,
) -> Result<ID3D11Texture2D, ErreurMedia> {
    // Échantillonnable : une vidéo à cadre est dessinée par Direct2D
    // (découpe au polygone), pas seulement composée par le video processor.
    let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
        .map_err(|e| ErreurMedia::Gpu(e.to_string()))?;
    contexte
        .creer_texture_echantillonnable(&desc)
        .map_err(|e| ErreurMedia::Gpu(format!("texture vidéo : {e}")))
}

/// Décode un fichier image en **PBGRA** (prémultiplié, ligne par ligne, sans
/// padding) et dit si le média porte de l'alpha par pixel.
///
/// WIC rend `32bppBGRA` en alpha **droit** ; Direct2D prémultiplié attend
/// `32bppPBGRA` (piège §30.6). La conversion est faite par WIC lui-même, une
/// seule fois au chargement. L'alpha est ensuite inspecté : un PNG `rgb24` ou
/// un JPG rendent `false` et restent dessinés en `ALPHA_MODE_IGNORE`.
fn decoder_pbgra(chemin: &Path) -> Result<(u32, u32, Vec<u8>, bool), ErreurMedia> {
    // WIC exige COM sur le fil appelant. Le fil moteur est à nous : MTA.
    // S_FALSE (déjà initialisé) et RPC_E_CHANGED_MODE (déjà STA) sont
    // tolérés — WIC fonctionne dans les deux cas.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| ErreurMedia::Decodage(format!("WIC factory : {}", e.message())))?;
        let nom = HSTRING::from(chemin.as_os_str());
        let decodeur = factory
            .CreateDecoderFromFilename(&nom, None, GENERIC_READ, WICDecodeMetadataCacheOnLoad)
            .map_err(|e| {
                ErreurMedia::Decodage(format!("ouverture {} : {}", chemin.display(), e.message()))
            })?;
        let cadre = decodeur
            .GetFrame(0)
            .map_err(|e| ErreurMedia::Decodage(format!("frame 0 : {}", e.message())))?;
        let convertisseur = factory
            .CreateFormatConverter()
            .map_err(|e| ErreurMedia::Decodage(format!("convertisseur : {}", e.message())))?;
        convertisseur
            .Initialize(
                &cadre,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| ErreurMedia::Decodage(format!("conversion PBGRA : {}", e.message())))?;

        let mut largeur = 0u32;
        let mut hauteur = 0u32;
        convertisseur
            .GetSize(&mut largeur, &mut hauteur)
            .map_err(|e| ErreurMedia::Decodage(format!("taille : {}", e.message())))?;
        let pas = largeur
            .checked_mul(4)
            .ok_or_else(|| ErreurMedia::Decodage("largeur débordante".to_string()))?;
        let taille = (pas as usize)
            .checked_mul(hauteur as usize)
            .ok_or_else(|| ErreurMedia::Decodage("image trop grande".to_string()))?;
        let mut pixels = vec![0u8; taille];
        convertisseur
            .CopyPixels(std::ptr::null(), pas, &mut pixels)
            .map_err(|e| ErreurMedia::Decodage(format!("pixels : {}", e.message())))?;
        let alpha = pixels.chunks_exact(4).any(|p| p[3] != 255);
        Ok((largeur, hauteur, pixels, alpha))
    }
}

/// Crée la texture et téléverse les pixels (une seule fois).
fn televerser(
    contexte: &ContexteGpu,
    largeur: u32,
    hauteur: u32,
    pixels: &[u8],
) -> Result<ID3D11Texture2D, ErreurMedia> {
    // Échantillonnable : une image à cadre est dessinée par Direct2D.
    let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
        .map_err(|e| ErreurMedia::Gpu(e.to_string()))?;
    let texture = contexte
        .creer_texture_echantillonnable(&desc)
        .map_err(|e| ErreurMedia::Gpu(format!("texture : {e}")))?;
    let ressource: windows::Win32::Graphics::Direct3D11::ID3D11Resource = texture
        .cast()
        .map_err(|e| ErreurMedia::Gpu(format!("texture → ressource : {}", e.message())))?;
    unsafe {
        contexte.context().UpdateSubresource(
            Some(&ressource),
            0,
            None,
            pixels.as_ptr() as *const c_void,
            largeur * 4,
            0,
        )
    };
    Ok(texture)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichier_absent_refuse_sans_gpu() {
        let cfg = MediaCfg {
            chemin: PathBuf::from("C:/inexistant/sos-test.png"),
            rect: (0.0, 0.0, 10.0, 10.0),
            z: 0,
            fit: Fit::Etirer,
            transform: Transform2D::NEUTRE,
            effets: EffetsCfg::NEUTRE,
            cadre: None,
            morphs: Vec::new(),
            pixel_morph: 0.0,
            lecture: LectureCfg {
                paused: true,
                boucle: None,
            },
            carrousel: None,
        };
        // Pas de contexte GPU nécessaire : l'erreur vient du fichier.
        let contexte = match ContexteGpu::creer(None) {
            Ok(c) => c,
            Err(_) => return,
        };
        assert!(matches!(charger(&contexte, &cfg), Err(ErreurMedia::Fichier(_))));
    }

    #[test]
    fn erreur_affichee() {
        let e = ErreurMedia::Decodage("test".to_string());
        assert!(e.to_string().contains("test"));
    }

    #[test]
    fn extensions_video_reconnues() {
        assert!(est_video(Path::new("a.MP4")));
        assert!(est_video(Path::new("b.webm")));
        assert!(est_video(Path::new("c.mkv")));
        assert!(!est_video(Path::new("d.png")));
        assert!(!est_video(Path::new("e")));
    }

    #[test]
    fn fit_etirer_garde_tout() {
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Etirer);
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 1920, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 0, 100, 100));
    }

    #[test]
    fn fit_ajuster_letterbox_centre() {
        // 16:9 dans un carré : 100×56 centré (22 px en haut et en bas).
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Ajuster);
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 1920, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 22, 100, 78));
    }

    #[test]
    fn fit_remplir_crop_centre() {
        // 16:9 dans un carré : crop 1080×1080 centré horizontalement.
        let (source, dest) = geometrie((1920, 1080), (0.0, 0.0, 100.0, 100.0), (100, 100), Fit::Remplir);
        assert_eq!((source.left, source.top, source.right, source.bottom), (420, 0, 1500, 1080));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 0, 100, 100));
    }

    #[test]
    fn fit_est_borne_au_canvas() {
        let (source, dest) = geometrie((100, 100), (50.0, 50.0, 500.0, 500.0), (100, 100), Fit::Etirer);
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (50, 50, 100, 100));
        // La source est recadrée dans la même proportion : les 50 px visibles
        // de la destination montrent les 10 px de gauche de l'image.
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 10, 10));
    }

    #[test]
    fn fit_calcule_sur_la_boite_entiere_puis_rogne() {
        // Widget `06de1ec7` de la scène de référence : boîte à y = −83,
        // image 668×374, `ajuster`. Le CSS dimensionne l'image sur la BOÎTE
        // (452 px de large) puis rogne le haut ; borner la boîte avant le fit
        // donnait 311 px de large (mesuré 2026-10-28).
        let (source, dest) = geometrie(
            (668, 374),
            (
                454.900_995_992_289_5,
                -83.042_559_117_259_97,
                452.316_907_684_212_25,
                257.193_037_396_685_8,
            ),
            (1360, 768),
            Fit::Ajuster,
        );
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (455, 0, 907, 172));
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 120, 668, 374));
    }

    #[test]
    fn media_entierement_hors_canvas_donne_une_paire_vide() {
        let (source, dest) = geometrie((100, 100), (2000.0, 0.0, 100.0, 100.0), (1360, 768), Fit::Etirer);
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 0, 0, 0));
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 0, 0));
    }

    #[test]
    fn fits_widget_depuis_nom() {
        // `FIT_CSS` de diffusion.html : etendre = cover, centrer = none,
        // vignette = scale-down. Un nom inconnu retombe sur l'étirement.
        assert_eq!(Fit::depuis_nom("ajuster"), Fit::Ajuster);
        assert_eq!(Fit::depuis_nom("remplir"), Fit::Remplir);
        assert_eq!(Fit::depuis_nom("etendre"), Fit::Remplir);
        assert_eq!(Fit::depuis_nom("centrer"), Fit::Centrer);
        assert_eq!(Fit::depuis_nom("vignette"), Fit::Vignette);
        assert_eq!(Fit::depuis_nom("inconnu"), Fit::Etirer);
    }

    #[test]
    fn fit_fond_centrer_vaut_scale_down() {
        // `fitRects` : `centrer` du fond = naturel si ça tient, contain
        // sinon — la même sortie que `vignette`, jamais `object-fit: none`.
        assert_eq!(Fit::depuis_nom_fond("centrer"), Fit::Vignette);
        assert_eq!(Fit::depuis_nom_fond("etendre"), Fit::Remplir);
        assert_eq!(Fit::depuis_nom_fond("inconnu"), Fit::Etirer);
    }

    #[test]
    fn fit_centrer_pose_la_taille_naturelle() {
        // `object-fit: none` : image 200×150 dans une boîte 100×100 → la
        // destination déborde la boîte (le clip widget rogne, pas le fit).
        let (source, dest) = fit_rects((200, 150), (0.0, 0.0, 100.0, 100.0), Fit::Centrer);
        assert_eq!((source.left, source.top, source.right, source.bottom), (0, 0, 200, 150));
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (-50, -25, 150, 125));
    }

    #[test]
    fn fit_vignette_ne_grossit_jamais() {
        // Petite image dans une grande boîte : `scale-down` = taille
        // naturelle (≠ `ajuster`, qui agrandirait).
        let (_, dest) = fit_rects((50, 40), (0.0, 0.0, 400.0, 300.0), Fit::Vignette);
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (175, 130, 225, 170));
        // Grande image : retombe sur `ajuster` (contain).
        let (_, dest) = fit_rects((1600, 900), (0.0, 0.0, 400.0, 300.0), Fit::Vignette);
        assert_eq!((dest.left, dest.top, dest.right, dest.bottom), (0, 38, 400, 263));
    }

    /// Applique une `Matrix3x2` (convention ligne `p·M`) à un point.
    fn applique(m: &Matrix3x2, x: f32, y: f32) -> (f32, f32) {
        (
            x * m.M11 + y * m.M21 + m.M31,
            x * m.M12 + y * m.M22 + m.M32,
        )
    }

    #[test]
    fn matrice_neutre_est_identite() {
        assert!(Transform2D::NEUTRE.est_neutre());
        let m = Transform2D::NEUTRE.matrice((10.0, 10.0));
        assert_eq!(applique(&m, 12.0, 10.0), (12.0, 10.0));
    }

    #[test]
    fn matrice_zoom_agrandit_autour_du_centre() {
        // Zoom ×2, boîte centrée en (10,10) : un point à 2 px du centre
        // passe à 4 px ; le centre est un point fixe.
        let t = Transform2D {
            zoom: 2.0,
            ..Transform2D::NEUTRE
        };
        let m = t.matrice((10.0, 10.0));
        assert_eq!(applique(&m, 10.0, 10.0), (10.0, 10.0));
        assert_eq!(applique(&m, 12.0, 10.0), (14.0, 10.0));
    }

    #[test]
    fn matrice_rotation_puis_offset() {
        // rot 90° (sens horaire écran, comme `rotate()` CSS) puis offset :
        // (2,0) relatif → (0,2) relatif → +offset (5,0) → +centre.
        let t = Transform2D {
            rot: 90.0,
            offset_x: 5.0,
            ..Transform2D::NEUTRE
        };
        let m = t.matrice((10.0, 10.0));
        let (x, y) = applique(&m, 12.0, 10.0);
        assert!((x - 15.0).abs() < 0.001 && (y - 12.0).abs() < 0.001);
    }

    #[test]
    fn matrice_miroir_retourne_autour_du_centre() {
        let t = Transform2D {
            miroir_h: true,
            ..Transform2D::NEUTRE
        };
        let m = t.matrice((10.0, 10.0));
        // (2,0) relatif → (−2,0) relatif : symétrie horizontale au centre.
        assert_eq!(applique(&m, 12.0, 10.0), (8.0, 10.0));
    }

    #[test]
    fn matrice_zoom_ecran_englobe_l_offset() {
        // `scale(bgZ)` de `#bg` agit APRÈS toute la chaîne interne, autour
        // du centre : l'offset est zoomé lui aussi (vérifié contre
        // `T(c)·S(z)·T(−c)` sur `#bg`).
        let t = Transform2D {
            offset_x: 5.0,
            zoom_ecran: 2.0,
            ..Transform2D::NEUTRE
        };
        let m = t.matrice((10.0, 10.0));
        // Centre : interne → c+o = (15,10) ; écran → c + z·o = (20,10).
        assert_eq!(applique(&m, 10.0, 10.0), (20.0, 10.0));
        // Coin (12,10) : interne → (17,10) ; écran → 2·(7,0)+c = (24,10).
        assert_eq!(applique(&m, 12.0, 10.0), (24.0, 10.0));
    }

    /// `out_c = Σ_r in_r·M[r][c] + M[4][c]` — convention ligne du 5×4 D2D.
    fn applique_couleur(m: &[[f32; 4]; 5], c: [f32; 4]) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (c_out, case) in out.iter_mut().enumerate() {
            for (r, &entree) in c.iter().enumerate() {
                *case += entree * m[r][c_out];
            }
            *case += m[4][c_out];
        }
        out
    }

    #[test]
    fn matrice_couleur_neutre_est_identite() {
        let m = EffetsCfg::NEUTRE.matrice_couleur();
        assert_eq!(applique_couleur(&m, [0.4, 0.6, 0.2, 1.0]), [0.4, 0.6, 0.2, 1.0]);
        assert!(!EffetsCfg::NEUTRE.couleur_active());
        assert!(!EffetsCfg::NEUTRE.pixel_active());
    }

    #[test]
    fn matrice_couleur_luminosite_seule() {
        // brightness(1,2) : RGB × 1,2 — alpha et offset inchangés.
        let m = EffetsCfg {
            lum: 20.0,
            ..EffetsCfg::NEUTRE
        }
        .matrice_couleur();
        let out = applique_couleur(&m, [0.5, 0.25, 0.0, 1.0]);
        assert!((out[0] - 0.6).abs() < 1e-6 && (out[1] - 0.3).abs() < 1e-6);
        assert_eq!(out[3], 1.0);
    }

    #[test]
    fn matrice_couleur_contraste_pivote_sur_le_gris() {
        // contrast(1,5) : 0,5 est un point fixe ; 1,0 déborde à 1,25.
        let m = EffetsCfg {
            contraste: 50.0,
            ..EffetsCfg::NEUTRE
        }
        .matrice_couleur();
        let out = applique_couleur(&m, [0.5, 0.5, 0.5, 1.0]);
        assert!((out[0] - 0.5).abs() < 1e-6);
        let out = applique_couleur(&m, [1.0, 1.0, 1.0, 1.0]);
        assert!((out[0] - 1.25).abs() < 1e-6);
    }

    #[test]
    fn matrice_couleur_teinte_120_pousse_le_rouge_vers_le_vert() {
        // hue-rotate(120°) sur du rouge pur : composante verte dominante
        // (la rotation CSS préserve la luminance, pas le canal — R et B
        // sortent négatifs et seront bornés à l'affichage).
        let m = EffetsCfg {
            teinte: 120.0,
            ..EffetsCfg::NEUTRE
        }
        .matrice_couleur();
        let out = applique_couleur(&m, [1.0, 0.0, 0.0, 1.0]);
        assert!(
            (out[0] + 0.365).abs() < 0.01 && (out[1] - 0.443).abs() < 0.01,
            "rouge → ≈(−0,365 ; 0,443 ; −0,362) attendu : {out:?}"
        );
        // La luminance est préservée (pondération 0.213/0.715/0.072) —
        // mais PAS le gris : `hue-rotate` CSS teinte un gris pur.
        let lum = |c: [f32; 4]| 0.213 * c[0] + 0.715 * c[1] + 0.072 * c[2];
        assert!((lum(out) - lum([1.0, 0.0, 0.0, 1.0])).abs() < 1e-3);
        // 360° = identité.
        let tour = EffetsCfg {
            teinte: 360.0,
            ..EffetsCfg::NEUTRE
        }
        .matrice_couleur();
        let out = applique_couleur(&tour, [0.4, 0.6, 0.2, 1.0]);
        assert!(
            (out[0] - 0.4).abs() < 1e-5 && (out[1] - 0.6).abs() < 1e-5,
            "360° ≈ identité : {out:?}"
        );
    }

    #[test]
    fn carrousel_statique_sans_timer() {
        // < 2 slides = statique (le DOM n'arme aucun timer) ; et le cycle 0
        // (t < sec) n'a pas de précédente — pas de fondu au démarrage.
        assert_eq!(fenetre_carrousel(1, 5.0, 0.8, true, 10.0), (0, None));
        assert_eq!(fenetre_carrousel(3, 5.0, 0.8, true, 3.0), (0, None));
    }

    #[test]
    fn carrousel_avance_et_fonde() {
        // sec = 4, fondu = 0,8 : la slide 1 entre à t = 4..4,8 (linéaire
        // pour le fond), puis seule.
        assert_eq!(fenetre_carrousel(3, 4.0, 0.8, false, 4.0), (1, Some((0, 0.0))));
        let (idx, fondu) = fenetre_carrousel(3, 4.0, 0.8, false, 4.4);
        assert_eq!(idx, 1);
        assert_eq!(fondu, Some((0, 0.5)));
        assert_eq!(fenetre_carrousel(3, 4.0, 0.8, false, 4.8), (1, None));
        // Bouclage : 3 slides → cycle 3 = slide 0, précédente = 2.
        let (idx, fondu) = fenetre_carrousel(3, 4.0, 0.8, false, 12.4);
        assert_eq!(idx, 0);
        assert_eq!(fondu, Some((2, 0.5)));
    }

    #[test]
    fn ease_in_out_css_bornes_et_milieu() {
        // cubic-bezier(0,42 ; 0 ; 0,58 ; 1) : bornes exactes, milieu = 0,5,
        // monotone, plus lente au départ qu'une linéaire (pente 0 en 0).
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-4);
        assert!(ease_in_out(0.25) < 0.25, "départ plus lent : {}", ease_in_out(0.25));
        assert!(ease_in_out(0.75) > 0.75, "arrivée plus lente : {}", ease_in_out(0.75));
        let mut avant = 0.0f32;
        for i in 1..=20 {
            let k = ease_in_out(i as f32 / 20.0);
            assert!(k >= avant, "monotone à i={i}");
            avant = k;
        }
    }
}
