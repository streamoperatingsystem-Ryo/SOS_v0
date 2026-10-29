//! Input viewer natif — état, dispositions et géométrie des touches.
//!
//! Tranche 1.6c : ce module ne porte que des **données pures** (testables sans
//! GPU). Le dessin est dans `texte.rs` (`dessiner_input`), le câblage dans
//! `moteur.rs`.
//!
//! Miroir de `diffusion.html` : `ivGenererClavierHTML` / `ivGenererNumpadHTML`
//! et des tables `IV_CLAVIER_AZERTY` / `IV_CLAVIER_QWERTY`. Toutes les
//! dimensions sont en `cqw` (= 1 % de la **largeur** du widget), comme le CSS.
//! La souris (c2) et les 3 skins manette (c3 : `ivGenererManette*SVG`) sont
//! des viewBox `xMidYMid meet` mis à l'échelle du widget.
//!
//! Limite de la tranche : le `backdrop-filter` du widget n'est pas porté.

use std::collections::BTreeSet;

use crate::cadres::CadreCfg;

/// Mode d'affichage du widget (`inputMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInput {
    Clavier,
    Souris,
    Numpad,
    Manette,
}

impl ModeInput {
    /// `inputMode` de la scène → mode ; inconnu = clavier (défaut du dashboard).
    pub fn depuis_nom(nom: &str) -> Self {
        match nom {
            "souris" => ModeInput::Souris,
            "numpad" => ModeInput::Numpad,
            "manette" => ModeInput::Manette,
            _ => ModeInput::Clavier,
        }
    }
}

/// Disposition des touches (`inputLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Azerty,
    Qwerty,
}

impl Disposition {
    pub fn depuis_nom(nom: &str) -> Self {
        if nom == "qwerty" {
            Disposition::Qwerty
        } else {
            Disposition::Azerty
        }
    }
}

/// Skin de la manette (`inputSkin`) — miroir de `ivGenererManetteHTML`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinManette {
    Xbox,
    Ps,
    /// « 8BitDo » — seul skin avec le fallback D-pad sur axes.
    Bitdo,
}

impl SkinManette {
    /// `inputSkin` → skin ; inconnu = xbox (défaut du dispatch JS).
    pub fn depuis_nom(nom: &str) -> Self {
        match nom {
            "8bitdo" => SkinManette::Bitdo,
            "ps" => SkinManette::Ps,
            _ => SkinManette::Xbox,
        }
    }
}

/// Remap des boutons de la manette (`inputMapping`).
///
/// Miroir de `sos_scene::InputMapping` : `boutons[affiché]` = index physique
/// lu dans `manette.boutons`. La table n'a que 16 entrées — les boutons 16
/// (home) et 17 (touchpad PS) passent par l'identité, comme le JS. Les
/// inversions ne s'appliquent **que** dans le fallback 8bitdo du D-pad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappingManette {
    pub boutons: Vec<u32>,
    pub dpad_invert_x: bool,
    pub dpad_invert_y: bool,
}

/// Un widget input-viewer de la scène.
#[derive(Debug, Clone, PartialEq)]
pub struct InputCfg {
    pub rect: (f64, f64, f64, f64),
    pub z: i32,
    pub mode: ModeInput,
    pub disposition: Disposition,
    /// Skin manette (`xbox` | `ps` | `8bitdo`) — lu en mode `Manette`.
    pub skin: String,
    /// Couleur de presse (`--inputviewer-couleur-presse`), RGBA.
    pub couleur: [u8; 4],
    /// Remap des boutons + inversions D-pad (`inputMapping` de la scène).
    pub mapping: Option<MappingManette>,
    /// Cadre du widget : `Some` = le contenu est clippé au polygone.
    pub cadre: Option<CadreCfg>,
}

/// État de la souris (frame `input-viewer-etat`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EtatSouris {
    pub gauche: bool,
    pub milieu: bool,
    pub droite: bool,
    pub x: i32,
    pub y: i32,
}

/// État de la manette (sous-objet `manette` de la frame `input-viewer-etat`).
///
/// Index boutons (mapping standard Gamepad) : 0=A/✕, 1=B/○, 2=X/□, 3=Y/△,
/// 4=LB/L1, 5=RB/R1, 6=LT/L2, 7=RT/R2, 8=Select/Share, 9=Start/Options,
/// 10=L3, 11=R3, 12-15 = D-pad haut/bas/gauche/droite, 16 = home,
/// 17 = touchpad PS, 18-19 libres.
/// Axes : 0-1 = stick gauche X/Y, 2-3 = stick droit X/Y, 4-5 = gâchettes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EtatManette {
    /// Porté par le contrat de frame ; `majInputViewerEtat` ne le lit pas —
    /// aucun rendu dédié en natif non plus (débranchée = tout relâché).
    pub connectee: bool,
    pub boutons: [bool; 20],
    pub axes: [f32; 6],
}

/// État reçu de l'app (frame `input-viewer-etat`, 30 Hz).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EtatInput {
    /// Codes `KeyboardEvent.code` pressés (`KeyW`, `ShiftLeft`, …).
    pub touches: Vec<String>,
    pub souris: EtatSouris,
    pub manette: EtatManette,
}

/// État vivant du widget + version (invalidation du rendu).
///
/// L'app pousse l'état pendant l'enregistrement, comme le chat : aucun
/// redémarrage, aucune reconfiguration de scène.
pub struct FileInput {
    touches: BTreeSet<String>,
    souris: EtatSouris,
    manette: EtatManette,
    version: u64,
}

impl Default for FileInput {
    fn default() -> Self {
        Self::nouveau()
    }
}

impl FileInput {
    pub fn nouveau() -> Self {
        Self {
            touches: BTreeSet::new(),
            souris: EtatSouris::default(),
            manette: EtatManette::default(),
            version: 0,
        }
    }

    /// Remplace l'état ; la version ne bouge que s'il change (l'app publie à
    /// 30 Hz, le rendu ne doit pas s'invalider pour rien).
    pub fn remplacer(&mut self, etat: EtatInput) {
        let nouveau: BTreeSet<String> = etat.touches.into_iter().collect();
        if nouveau == self.touches && etat.souris == self.souris && etat.manette == self.manette
        {
            return;
        }
        self.touches = nouveau;
        self.souris = etat.souris;
        self.manette = etat.manette;
        self.version += 1;
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn est_pressee(&self, code: &str) -> bool {
        self.touches.contains(code)
    }

    /// `true` si au moins un des codes est pressé (touches multi-codes du
    /// numpad : `Digit7` ou `Numpad7`).
    pub fn est_pressee_parmi(&self, codes: &[&str]) -> bool {
        codes.iter().any(|code| self.est_pressee(code))
    }

    pub fn souris(&self) -> EtatSouris {
        self.souris
    }

    pub fn manette(&self) -> EtatManette {
        self.manette
    }

    pub fn len(&self) -> usize {
        self.touches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.touches.is_empty()
    }
}

/// Rect en pixels canvas (coins flottants, comme le CSS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub largeur: f32,
    pub hauteur: f32,
}

/// Une touche placée : codes qui l'allument, libellé, rect.
#[derive(Debug, Clone, PartialEq)]
pub struct TouchePlacee {
    pub codes: &'static [&'static str],
    pub label: &'static str,
    pub rect: RectF,
}

/// Une LED du châssis (CAPS / NUM / SCRL).
#[derive(Debug, Clone, PartialEq)]
pub struct Led {
    /// Code qui l'allume (`CapsLock`, `NumLock`, `ScrollLock`).
    pub code: &'static str,
    pub centre: (f32, f32),
    pub rayon: f32,
    pub label: &'static str,
    pub taille: f32,
}

/// Châssis du clavier/numpad (`ivChassisHTML`) : plaque, câble, plateau, LEDs.
#[derive(Debug, Clone, PartialEq)]
pub struct Chassis {
    pub plaque: RectF,
    pub cable: RectF,
    pub plateau: RectF,
    pub rayon: f32,
    pub leds: Vec<Led>,
}

/// Résultat de [`disposition`] : ce que le dessinateur doit peindre.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GeometrieInput {
    pub touches: Vec<TouchePlacee>,
    pub chassis: Option<Chassis>,
    pub souris: Option<SourisGeometrie>,
    pub manette: Option<ManetteGeometrie>,
}

/// Point en pixels canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Commande d'un chemin Direct2D (déjà mise à l'échelle du widget).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Debut(Point),
    Ligne(Point),
    Cubique(Point, Point, Point),
    Fermer,
}

/// Chemin fermé ou ouvert, en pixels canvas.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chemin {
    pub segments: Vec<Segment>,
}

/// Reflet elliptique tourné (`ellipse … transform="rotate(a cx cy)"`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reflet {
    pub centre: Point,
    pub rayon_x: f32,
    pub rayon_y: f32,
    /// Rotation en degrés.
    pub angle: f32,
}

/// Souris (`ivGenererSourisHTML`, viewBox 360 × 520 en `xMidYMid meet`).
#[derive(Debug, Clone, PartialEq)]
pub struct SourisGeometrie {
    pub corps: Chemin,
    pub zone_gauche: Chemin,
    pub zone_droite: Chemin,
    pub couture_verticale: (Point, Point),
    pub couture_horizontale: (Point, Point),
    pub loge: RectF,
    pub molette: RectF,
    pub crans: Vec<(Point, Point)>,
    pub dpi: (Point, f32),
    pub reflet: Reflet,
    /// Ancre du texte de coordonnées (centre, ligne de base) + taille.
    pub coordonnees: (Point, f32),
    /// Échelle appliquée au viewBox (pour les épaisseurs de trait).
    pub echelle: f32,
}

// =============================================================================
// MANETTE — figure SVG portée en données pures (`ivGenererManette*SVG`)
// =============================================================================

/// Direction d'un élément `data-dpad` : 4 bras + 4 diagonales.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Haut,
    Bas,
    Gauche,
    Droite,
    HautGauche,
    HautDroite,
    BasGauche,
    BasDroite,
}

impl Direction {
    /// État allumé : une diagonale est vraie quand ses **deux** bras le sont
    /// (`majInputViewerEtat`, switch `data-dpad`).
    pub fn activee(self, dpad: DpadEtat) -> bool {
        match self {
            Direction::Haut => dpad.haut,
            Direction::Bas => dpad.bas,
            Direction::Gauche => dpad.gauche,
            Direction::Droite => dpad.droite,
            Direction::HautGauche => dpad.haut && dpad.gauche,
            Direction::HautDroite => dpad.haut && dpad.droite,
            Direction::BasGauche => dpad.bas && dpad.gauche,
            Direction::BasDroite => dpad.bas && dpad.droite,
        }
    }
}

/// Bras du D-pad résolus — `majInputViewerEtat` lignes `dpad` + fallback 8bitdo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DpadEtat {
    pub haut: bool,
    pub bas: bool,
    pub gauche: bool,
    pub droite: bool,
}

/// Résout les 4 bras du D-pad : boutons 12-15 **bruts** (jamais remappés),
/// puis — skin 8bitdo seulement — fallback sur `axes[0]`/`axes[1]` (stick
/// gauche, seuil 0,5, `axes[1] < 0` = haut) quand le D-pad est inactif.
/// `dpadInvertX/Y` ne s'appliquent que dans ce fallback.
pub fn resoudre_dpad(
    etat: &EtatManette,
    skin: SkinManette,
    mapping: Option<&MappingManette>,
) -> DpadEtat {
    let mut dpad = DpadEtat {
        haut: etat.boutons[12],
        bas: etat.boutons[13],
        gauche: etat.boutons[14],
        droite: etat.boutons[15],
    };
    if skin == SkinManette::Bitdo && dpad == DpadEtat::default() {
        const SEUIL: f32 = 0.5;
        let (inv_x, inv_y) = mapping
            .map(|m| (m.dpad_invert_x, m.dpad_invert_y))
            .unwrap_or_default();
        let ax = if inv_x { -etat.axes[0] } else { etat.axes[0] };
        let ay = if inv_y { -etat.axes[1] } else { etat.axes[1] };
        dpad.haut = ay < -SEUIL;
        dpad.bas = ay > SEUIL;
        dpad.gauche = ax < -SEUIL;
        dpad.droite = ax > SEUIL;
    }
    dpad
}

/// `data-bouton` affiché → index physique dans `manette.boutons` (remap
/// `inputMapping.buttons`) ; sans mapping ou hors table → identité.
pub fn bouton_physique(idx: u8, mapping: Option<&MappingManette>) -> usize {
    mapping
        .and_then(|m| m.boutons.get(idx as usize).copied())
        .unwrap_or(idx as u32) as usize
}

/// Forme élémentaire d'une primitive manette, en pixels canvas.
#[derive(Debug, Clone, PartialEq)]
pub enum Forme {
    Cercle { centre: Point, rayon: f32 },
    Ellipse { centre: Point, rayon_x: f32, rayon_y: f32 },
    /// Rectangle arrondi (`rx` du SVG, déjà mis à l'échelle).
    Rect { rect: RectF, rayon: f32 },
    /// Tracé libre (corps, liserés de coque) — ouvert ou fermé.
    Chemin(Chemin),
}

/// Style d'une forme — miroir des classes `iv-fig-*` du CSS (couleurs et
/// épaisseurs résolues côté `texte.rs`, par skin).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleManette {
    /// `iv-fig-corps` : dégradé de coque, alpha 0,62 + filet blanc 0,12.
    Corps,
    /// `iv-fig-corps-inner` (8bitdo) : voile blanc 0,03 + filet 0,05.
    CorpsInner,
    /// `iv-fig-face` / `iv-fig-shoulder` / `iv-fig-pill` : dégradé bouton +
    /// filet 0,15 ; pressé = couleur unie + halo.
    Bouton,
    /// `iv-fig-zone` : transparent au repos ; pressée = couleur × 0,85 + halo.
    Zone,
    /// `iv-fig-diag` : point blanc 0,12 ; pressé = couleur + halo.
    Diag,
    /// `iv-fig-home` : dégradé bouton + filet 0,10 ; pressé = couleur + halo.
    Home,
    /// `iv-fig-pad` (touchpad PS) : dégradé dish + filet 0,10 ; pressé idem.
    Pad,
    /// `iv-fig-dish` : dégradé radial + filet 0,08, alpha 0,75.
    Dish,
    /// `iv-fig-cross` : dégradé croix + filet 0,10.
    Cross,
    /// `iv-fig-hub` : dégradé cross (8bitdo/xbox) ou bouton (ps) + filet 0,10.
    Hub,
    /// `iv-fig-hub-dot` : blanc 0,08.
    HubDot,
    /// `iv-fig-stick-base` : dégradé radial + filet 0,08, alpha 0,75.
    StickBase,
    /// `iv-fig-stick-cap` : dégradé bouton + filet 0,16 ; éclairé `actif` ou
    /// pressé (L3/R3) — couleur unie + halo.
    StickCap,
    /// `iv-fig-stick-ring` : anneau filet 0,08, jamais rempli.
    StickRing,
    /// `iv-fig-btn-shadow` : noir 0,35.
    Ombre,
    /// `iv-fig-sheen` : blanc 0,12.
    Sheen,
    /// `iv-fig-sheen-line` : filet blanc 0,07 (trait ~3 px viewBox).
    SheenLine,
    /// `iv-fig-ico` : pictogramme gris `#aeb4c2`.
    Ico,
    /// `iv-fig-deco` : blanc 0,10.
    Deco,
}

/// Style d'un `<text>` — miroir `iv-fig-lbl*` ; la taille est sur la
/// primitive. Dans un groupe pressé tout texte passe en blanc (`g.presse
/// text`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StyleLabel {
    /// `iv-fig-lbl` : `#dfe3ec`.
    Lbl,
    /// `iv-fig-lbl-sm` : `#aeb4c2`.
    LblSm,
    /// `iv-fig-arrow` : `rgba(223,227,236,0.40)`.
    Arrow,
    /// Lettre Xbox (`lbl-a/b/x/y`) ou symbole PS (`lbl-tri/squ/cir/cro`) —
    /// couleur littérale RGB.
    Couleur([u8; 3]),
}

/// Une forme ou un texte d'un groupe manette.
#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Forme { forme: Forme, style: StyleManette },
    /// `text-anchor: middle` + `dominant-baseline: central` → ancre centrée.
    Texte { ancre: Point, taille: f32, contenu: &'static str, style: StyleLabel },
}

/// Ce qui allume un groupe — miroir de `data-bouton` / `data-dpad` /
/// `data-stick-indicateur` (+ la classe `actif` des sticks).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LienManette {
    /// Jamais lié à l'état (coque, croix de base, décorations).
    Statique,
    /// `data-bouton="n"` : pressé quand le bouton **physique** (remappé) est vrai.
    Bouton(u8),
    /// `data-dpad="dir"` : bras ou diagonale (cf. [`Direction::activee`]).
    Dpad(Direction),
    /// `data-stick-indicateur` + `data-bouton` : cap mobile — translate
    /// `axes × 26` px viewBox, éclairé `actif` (|ax|>0,15) ou pressé (L3/R3).
    Stick { gauche: bool, bouton: u8 },
}

/// Un `<g>` du SVG : son lien d'état + ses primitives, dans l'ordre.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupeManette {
    pub lien: LienManette,
    pub primitives: Vec<Primitive>,
}

/// Figure complète d'un skin, en pixels canvas (viewBox en `meet`).
///
/// Les `groupes` gardent l'**ordre de document** du SVG = ordre de peinture
/// (gâchettes derrière la coque, cap de stick par-dessus sa base…).
#[derive(Debug, Clone, PartialEq)]
pub struct ManetteGeometrie {
    pub skin: SkinManette,
    pub groupes: Vec<GroupeManette>,
    /// Échelle viewBox → widget : épaisseurs, translate des sticks (×26),
    /// tailles de texte déjà multipliées dans les primitives.
    pub echelle: f32,
}

/// Une définition de touche : codes, libellé, poids `flex`.
type ToucheDef = (&'static [&'static str], &'static str, f32);

const AZERTY: &[&[ToucheDef]] = &[
    &[
        (&["Escape"], "Esc", 1.0),
        (&[], "", 0.0),
        (&["F1"], "F1", 1.0),
        (&["F2"], "F2", 1.0),
        (&["F3"], "F3", 1.0),
        (&["F4"], "F4", 1.0),
        (&[], "", 0.0),
        (&["F5"], "F5", 1.0),
        (&["F6"], "F6", 1.0),
        (&["F7"], "F7", 1.0),
        (&["F8"], "F8", 1.0),
        (&[], "", 0.0),
        (&["F9"], "F9", 1.0),
        (&["F10"], "F10", 1.0),
        (&["F11"], "F11", 1.0),
        (&["F12"], "F12", 1.0),
    ],
    &[
        (&["Backquote"], "²", 1.0),
        (&["Digit1"], "&", 1.0),
        (&["Digit2"], "é", 1.0),
        (&["Digit3"], "\"", 1.0),
        (&["Digit4"], "'", 1.0),
        (&["Digit5"], "(", 1.0),
        (&["Digit6"], "-", 1.0),
        (&["Digit7"], "è", 1.0),
        (&["Digit8"], "_", 1.0),
        (&["Digit9"], "ç", 1.0),
        (&["Digit0"], "à", 1.0),
        (&["Minus"], ")", 1.0),
        (&["Equal"], "=", 1.0),
        (&["Backspace"], "←", 1.6),
    ],
    &[
        (&["Tab"], "Tab", 1.6),
        (&["KeyA"], "A", 1.0),
        (&["KeyZ"], "Z", 1.0),
        (&["KeyE"], "E", 1.0),
        (&["KeyR"], "R", 1.0),
        (&["KeyT"], "T", 1.0),
        (&["KeyY"], "Y", 1.0),
        (&["KeyU"], "U", 1.0),
        (&["KeyI"], "I", 1.0),
        (&["KeyO"], "O", 1.0),
        (&["KeyP"], "P", 1.0),
        (&["BracketLeft"], "^", 1.0),
        (&["BracketRight"], "$", 1.0),
        (&["Enter"], "↵", 1.6),
    ],
    &[
        (&["CapsLock"], "Verr", 1.6),
        (&["KeyQ"], "Q", 1.0),
        (&["KeyS"], "S", 1.0),
        (&["KeyD"], "D", 1.0),
        (&["KeyF"], "F", 1.0),
        (&["KeyG"], "G", 1.0),
        (&["KeyH"], "H", 1.0),
        (&["KeyJ"], "J", 1.0),
        (&["KeyK"], "K", 1.0),
        (&["KeyL"], "L", 1.0),
        (&["Semicolon"], "M", 1.0),
        (&["Quote"], "ù", 1.0),
        (&["Backslash"], "*", 1.0),
    ],
    &[
        (&["ShiftLeft"], "⇧", 1.6),
        (&["IntlBackslash"], "<", 1.0),
        (&["KeyW"], "W", 1.0),
        (&["KeyX"], "X", 1.0),
        (&["KeyC"], "C", 1.0),
        (&["KeyV"], "V", 1.0),
        (&["KeyB"], "B", 1.0),
        (&["KeyN"], "N", 1.0),
        (&["Comma"], ",", 1.0),
        (&["Period"], ";", 1.0),
        (&["Slash"], ":", 1.0),
        (&["IntlRo"], "!", 1.0),
        (&["ShiftRight"], "⇧", 1.6),
    ],
    &[
        (&["ControlLeft"], "Ctrl", 1.0),
        (&["MetaLeft"], "⊞", 1.0),
        (&["AltLeft"], "Alt", 1.0),
        (&["Space"], "Espace", 5.0),
        (&["AltRight"], "Alt Gr", 1.4),
        (&["MetaRight"], "⊞", 1.0),
        (&["ContextMenu"], "☰", 1.0),
        (&["ControlRight"], "Ctrl", 1.0),
    ],
];

const QWERTY: &[&[ToucheDef]] = &[
    &[
        (&["Escape"], "Esc", 1.0),
        (&[], "", 0.0),
        (&["F1"], "F1", 1.0),
        (&["F2"], "F2", 1.0),
        (&["F3"], "F3", 1.0),
        (&["F4"], "F4", 1.0),
        (&[], "", 0.0),
        (&["F5"], "F5", 1.0),
        (&["F6"], "F6", 1.0),
        (&["F7"], "F7", 1.0),
        (&["F8"], "F8", 1.0),
        (&[], "", 0.0),
        (&["F9"], "F9", 1.0),
        (&["F10"], "F10", 1.0),
        (&["F11"], "F11", 1.0),
        (&["F12"], "F12", 1.0),
    ],
    &[
        (&["Backquote"], "`", 1.0),
        (&["Digit1"], "1", 1.0),
        (&["Digit2"], "2", 1.0),
        (&["Digit3"], "3", 1.0),
        (&["Digit4"], "4", 1.0),
        (&["Digit5"], "5", 1.0),
        (&["Digit6"], "6", 1.0),
        (&["Digit7"], "7", 1.0),
        (&["Digit8"], "8", 1.0),
        (&["Digit9"], "9", 1.0),
        (&["Digit0"], "0", 1.0),
        (&["Minus"], "-", 1.0),
        (&["Equal"], "=", 1.0),
        (&["Backspace"], "←", 1.6),
    ],
    &[
        (&["Tab"], "Tab", 1.6),
        (&["KeyQ"], "Q", 1.0),
        (&["KeyW"], "W", 1.0),
        (&["KeyE"], "E", 1.0),
        (&["KeyR"], "R", 1.0),
        (&["KeyT"], "T", 1.0),
        (&["KeyY"], "Y", 1.0),
        (&["KeyU"], "U", 1.0),
        (&["KeyI"], "I", 1.0),
        (&["KeyO"], "O", 1.0),
        (&["KeyP"], "P", 1.0),
        (&["BracketLeft"], "[", 1.0),
        (&["BracketRight"], "]", 1.0),
        (&["Enter"], "↵", 1.6),
    ],
    &[
        (&["CapsLock"], "Caps", 1.8),
        (&["KeyA"], "A", 1.0),
        (&["KeyS"], "S", 1.0),
        (&["KeyD"], "D", 1.0),
        (&["KeyF"], "F", 1.0),
        (&["KeyG"], "G", 1.0),
        (&["KeyH"], "H", 1.0),
        (&["KeyJ"], "J", 1.0),
        (&["KeyK"], "K", 1.0),
        (&["KeyL"], "L", 1.0),
        (&["Semicolon"], ";", 1.0),
        (&["Quote"], "'", 1.0),
        (&["Backslash"], "\\", 1.0),
        (&["Enter"], "↵", 1.2),
    ],
    &[
        (&["ShiftLeft"], "⇧", 2.0),
        (&["KeyZ"], "Z", 1.0),
        (&["KeyX"], "X", 1.0),
        (&["KeyC"], "C", 1.0),
        (&["KeyV"], "V", 1.0),
        (&["KeyB"], "B", 1.0),
        (&["KeyN"], "N", 1.0),
        (&["KeyM"], "M", 1.0),
        (&["Comma"], ",", 1.0),
        (&["Period"], ".", 1.0),
        (&["Slash"], "/", 1.0),
        (&["ShiftRight"], "⇧", 2.0),
    ],
    &[
        (&["ControlLeft"], "Ctrl", 1.0),
        (&["MetaLeft"], "Win", 1.0),
        (&["AltLeft"], "Alt", 1.0),
        (&["Space"], "Space", 5.0),
        (&["AltRight"], "Alt", 1.0),
        (&["MetaRight"], "Win", 1.0),
        (&["ContextMenu"], "☰", 1.0),
        (&["ControlRight"], "Ctrl", 1.0),
    ],
];

/// Une case du pavé numérique : ligne, colonne, spans, codes, libellé.
type CaseNumpad = (u32, u32, u32, u32, &'static [&'static str], &'static str);

/// Grille 4 × 5 du pavé numérique, dans l'ordre de `grid-auto-flow: row`.
const NUMPAD: &[CaseNumpad] = &[
    (0, 0, 1, 1, &["Escape"], "Esc"),
    (0, 1, 1, 1, &["NumpadDivide"], "/"),
    (0, 2, 1, 1, &["NumpadMultiply"], "*"),
    (0, 3, 1, 1, &["NumpadSubtract"], "−"),
    (1, 0, 1, 1, &["Digit7", "Numpad7"], "7"),
    (1, 1, 1, 1, &["Digit8", "Numpad8"], "8"),
    (1, 2, 1, 1, &["Digit9", "Numpad9"], "9"),
    (1, 3, 2, 1, &["NumpadAdd"], "+"),
    (2, 0, 1, 1, &["Digit4", "Numpad4"], "4"),
    (2, 1, 1, 1, &["Digit5", "Numpad5"], "5"),
    (2, 2, 1, 1, &["Digit6", "Numpad6"], "6"),
    (3, 0, 1, 1, &["Digit1", "Numpad1"], "1"),
    (3, 1, 1, 1, &["Digit2", "Numpad2"], "2"),
    (3, 2, 1, 1, &["Digit3", "Numpad3"], "3"),
    (3, 3, 2, 1, &["NumpadEnter"], "↵"),
    (4, 0, 1, 2, &["Digit0", "Numpad0"], "0"),
    (4, 2, 1, 1, &["NumpadDecimal"], "."),
];

/// Géométrie complète d'un widget input-viewer, en px canvas.
pub fn disposition(cfg: &InputCfg) -> GeometrieInput {
    let rect = RectF {
        x: cfg.rect.0 as f32,
        y: cfg.rect.1 as f32,
        largeur: cfg.rect.2 as f32,
        hauteur: cfg.rect.3 as f32,
    };
    if rect.largeur < 1.0 || rect.hauteur < 1.0 {
        return GeometrieInput::default();
    }
    let cqw = rect.largeur / 100.0;
    match cfg.mode {
        ModeInput::Clavier => clavier(cfg.disposition, rect, cqw),
        ModeInput::Numpad => numpad(rect, cqw),
        ModeInput::Souris => souris(rect),
        ModeInput::Manette => manette(rect, SkinManette::depuis_nom(&cfg.skin)),
    }
}

/// Souris : corps, deux zones de clic, molette crantée, DPI, coordonnées.
///
/// Les chemins sont **déjà mis à l'échelle** du widget (`xMidYMid meet` sur un
/// viewBox 360 × 520) : le dessinateur n'applique aucune transformation.
fn souris(rect: RectF) -> GeometrieInput {
    let echelle = (rect.largeur / 360.0).min(rect.hauteur / 520.0);
    let ox = rect.x + (rect.largeur - 360.0 * echelle) / 2.0;
    let oy = rect.y + (rect.hauteur - 520.0 * echelle) / 2.0;
    let p = |x: f32, y: f32| Point {
        x: ox + x * echelle,
        y: oy + y * echelle,
    };
    let r = |x: f32, y: f32, largeur: f32, hauteur: f32| RectF {
        x: ox + x * echelle,
        y: oy + y * echelle,
        largeur: largeur * echelle,
        hauteur: hauteur * echelle,
    };

    let corps = Chemin {
        segments: vec![
            Segment::Debut(p(180.0, 32.0)),
            Segment::Cubique(p(92.0, 32.0), p(56.0, 128.0), p(56.0, 236.0)),
            Segment::Ligne(p(56.0, 296.0)),
            Segment::Cubique(p(56.0, 412.0), p(104.0, 490.0), p(180.0, 490.0)),
            Segment::Cubique(p(256.0, 490.0), p(304.0, 412.0), p(304.0, 296.0)),
            Segment::Ligne(p(304.0, 236.0)),
            Segment::Cubique(p(304.0, 128.0), p(268.0, 32.0), p(180.0, 32.0)),
            Segment::Fermer,
        ],
    };
    let zone_gauche = Chemin {
        segments: vec![
            Segment::Debut(p(174.0, 46.0)),
            Segment::Cubique(p(104.0, 50.0), p(72.0, 134.0), p(72.0, 214.0)),
            Segment::Ligne(p(174.0, 214.0)),
            Segment::Fermer,
        ],
    };
    let zone_droite = Chemin {
        segments: vec![
            Segment::Debut(p(186.0, 46.0)),
            Segment::Cubique(p(256.0, 50.0), p(288.0, 134.0), p(288.0, 214.0)),
            Segment::Ligne(p(186.0, 214.0)),
            Segment::Fermer,
        ],
    };

    GeometrieInput {
        touches: Vec::new(),
        chassis: None,
        souris: Some(SourisGeometrie {
            corps,
            zone_gauche,
            zone_droite,
            couture_verticale: (p(180.0, 44.0), p(180.0, 216.0)),
            couture_horizontale: (p(72.0, 214.0), p(288.0, 214.0)),
            loge: r(158.0, 62.0, 44.0, 92.0),
            molette: r(166.0, 74.0, 28.0, 68.0),
            crans: [94.0_f32, 105.0, 116.0, 127.0]
                .iter()
                .map(|y| (p(172.0, *y), p(188.0, *y)))
                .collect(),
            dpi: (p(180.0, 186.0), 7.0 * echelle),
            reflet: Reflet {
                centre: p(150.0, 90.0),
                rayon_x: 60.0 * echelle,
                rayon_y: 22.0 * echelle,
                angle: -16.0,
            },
            coordonnees: (p(180.0, 512.0), 16.0 * echelle),
            echelle,
        }),
        manette: None,
    }
}

/// Passage viewBox → pixels canvas (`xMidYMid meet`), même calcul que
/// `souris()` ; les formes/textes ressortent déjà à l'échelle du widget.
struct Echelle {
    ox: f32,
    oy: f32,
    e: f32,
}

impl Echelle {
    fn meet(rect: RectF, vw: f32, vh: f32) -> Self {
        let e = (rect.largeur / vw).min(rect.hauteur / vh);
        Self {
            ox: rect.x + (rect.largeur - vw * e) / 2.0,
            oy: rect.y + (rect.hauteur - vh * e) / 2.0,
            e,
        }
    }

    fn p(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.ox + x * self.e,
            y: self.oy + y * self.e,
        }
    }

    fn cercle(&self, cx: f32, cy: f32, rayon: f32) -> Forme {
        Forme::Cercle {
            centre: self.p(cx, cy),
            rayon: rayon * self.e,
        }
    }

    fn ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32) -> Forme {
        Forme::Ellipse {
            centre: self.p(cx, cy),
            rayon_x: rx * self.e,
            rayon_y: ry * self.e,
        }
    }

    /// Rect arrondi : `x/y/w/h/rx` du SVG, déjà mis à l'échelle.
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, rayon: f32) -> Forme {
        Forme::Rect {
            rect: RectF {
                x: self.ox + x * self.e,
                y: self.oy + y * self.e,
                largeur: w * self.e,
                hauteur: h * self.e,
            },
            rayon: rayon * self.e,
        }
    }

    fn forme(&self, forme: Forme, style: StyleManette) -> Primitive {
        Primitive::Forme { forme, style }
    }

    /// `<text>` : `taille` en px viewBox → px canvas (reste multipliée).
    fn txt(
        &self,
        x: f32,
        y: f32,
        taille: f32,
        contenu: &'static str,
        style: StyleLabel,
    ) -> Primitive {
        Primitive::Texte {
            ancre: self.p(x, y),
            taille: taille * self.e,
            contenu,
            style,
        }
    }

    fn groupe(&self, lien: LienManette, primitives: Vec<Primitive>) -> GroupeManette {
        GroupeManette { lien, primitives }
    }

    /// `<g>` statique à une seule forme (coque, dish, décorations).
    fn statique(&self, forme: Forme, style: StyleManette) -> GroupeManette {
        self.groupe(LienManette::Statique, vec![self.forme(forme, style)])
    }

    /// `<g>` statique à plusieurs primitives (décorations groupées).
    fn bloc(&self, primitives: Vec<Primitive>) -> GroupeManette {
        self.groupe(LienManette::Statique, primitives)
    }
}

/// Manette : l'un des 3 skins (`ivGenererManette*SVG`), viewBox `meet`.
///
/// L'ordre des `groupes` suit le document SVG : gâchettes/triggers **avant**
/// la coque (elles dépassent derrière), cap de stick après sa base.
fn manette(rect: RectF, skin: SkinManette) -> GeometrieInput {
    // viewBox : 8bitdo 760×420, xbox/ps 760×430 — tous `xMidYMid meet`.
    let (vw, vh) = if skin == SkinManette::Bitdo {
        (760.0, 420.0)
    } else {
        (760.0, 430.0)
    };
    let e = Echelle::meet(rect, vw, vh);
    let groupes = match skin {
        SkinManette::Bitdo => manette_8bitdo(&e),
        SkinManette::Xbox => manette_xbox(&e),
        SkinManette::Ps => manette_ps(&e),
    };
    GeometrieInput {
        touches: Vec::new(),
        chassis: None,
        souris: None,
        manette: Some(ManetteGeometrie {
            skin,
            groupes,
            echelle: e.e,
        }),
    }
}

/// Skin « 8BitDo ProFight » (`ivGenererManette8BitDoSVG`, viewBox 760×420).
/// Pas de sticks : croix en creux + zones pressables invisibles, 6 boutons
/// face, gâchettes ZL/ZR derrière la coque.
fn manette_8bitdo(e: &Echelle) -> Vec<GroupeManette> {
    let face = |bouton: u8, cx: f32, cy: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.cercle(cx, cy + 4.0, 28.0), StyleManette::Ombre),
                e.forme(e.cercle(cx, cy, 27.0), StyleManette::Bouton),
                e.forme(e.ellipse(cx, cy - 12.0, 15.0, 6.0), StyleManette::Sheen),
                e.txt(cx, cy, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    let zone = |dir: Direction, x: f32, y: f32, w: f32, h: f32| {
        e.groupe(
            LienManette::Dpad(dir),
            vec![e.forme(e.rect(x, y, w, h, 10.0), StyleManette::Zone)],
        )
    };
    let diag = |dir: Direction, cx: f32, cy: f32| {
        e.groupe(
            LienManette::Dpad(dir),
            vec![e.forme(e.cercle(cx, cy, 7.0), StyleManette::Diag)],
        )
    };
    let shoulder = |bouton: u8, x: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.rect(x, 16.0, 150.0, 80.0, 32.0), StyleManette::Bouton),
                e.forme(e.rect(x + 12.0, 24.0, 126.0, 20.0, 10.0), StyleManette::Sheen),
                e.txt(x + 75.0, 46.0, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    let pill = |bouton: u8, x: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.cercle(x + 31.0, 310.0, 17.0), StyleManette::Ombre),
                e.forme(e.rect(x, 288.0, 62.0, 26.0, 13.0), StyleManette::Bouton),
                e.txt(x + 31.0, 301.0, 11.0, lbl, StyleLabel::LblSm),
            ],
        )
    };
    vec![
        shoulder(6, 118.0, "ZL"),
        shoulder(7, 492.0, "ZR"),
        e.statique(corps_8bitdo(e), StyleManette::Corps),
        e.statique(corps_inner_8bitdo(e), StyleManette::CorpsInner),
        e.bloc(vec![
            e.forme(e.cercle(352.0, 108.0, 7.0), StyleManette::Deco),
            e.forme(e.cercle(408.0, 108.0, 7.0), StyleManette::Deco),
        ]),
        e.groupe(
            LienManette::Bouton(16),
            vec![e.forme(e.cercle(380.0, 140.0, 11.0), StyleManette::Home)],
        ),
        e.statique(e.cercle(178.0, 220.0, 92.0), StyleManette::Dish),
        e.bloc(vec![
            e.forme(e.rect(154.0, 152.0, 48.0, 136.0, 10.0), StyleManette::Cross),
            e.forme(e.rect(110.0, 196.0, 136.0, 48.0, 10.0), StyleManette::Cross),
        ]),
        diag(Direction::HautGauche, 130.0, 172.0),
        diag(Direction::HautDroite, 226.0, 172.0),
        diag(Direction::BasGauche, 130.0, 268.0),
        diag(Direction::BasDroite, 226.0, 268.0),
        zone(Direction::Haut, 156.0, 154.0, 44.0, 60.0),
        zone(Direction::Bas, 156.0, 226.0, 44.0, 60.0),
        zone(Direction::Gauche, 112.0, 198.0, 60.0, 44.0),
        zone(Direction::Droite, 184.0, 198.0, 60.0, 44.0),
        // Flèches gravées : HORS des <g data-dpad> — elles ne blanchissent
        // pas quand le bras est pressé (le skin PS les a dans le <g>).
        e.bloc(vec![
            e.txt(178.0, 163.0, 17.0, "▲", StyleLabel::Arrow),
            e.txt(178.0, 277.0, 17.0, "▼", StyleLabel::Arrow),
            e.txt(121.0, 220.0, 17.0, "◄", StyleLabel::Arrow),
            e.txt(235.0, 220.0, 17.0, "►", StyleLabel::Arrow),
        ]),
        e.bloc(vec![
            e.forme(e.cercle(178.0, 220.0, 30.0), StyleManette::Hub),
            e.forme(e.cercle(178.0, 220.0, 9.0), StyleManette::HubDot),
        ]),
        face(2, 468.0, 166.0, "X"),
        face(3, 558.0, 160.0, "Y"),
        face(4, 648.0, 166.0, "L"),
        face(0, 468.0, 254.0, "A"),
        face(1, 558.0, 260.0, "B"),
        face(5, 648.0, 254.0, "R"),
        pill(8, 318.0, "SELECT"),
        pill(9, 400.0, "START"),
    ]
}

/// Coque fightpad (`ivg-corps`) : haut plat, flancs arrondis, bas aplati.
fn corps_8bitdo(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(58.0, 200.0)),
            Segment::Cubique(e.p(58.0, 108.0), e.p(138.0, 58.0), e.p(243.0, 58.0)),
            Segment::Ligne(e.p(517.0, 58.0)),
            Segment::Cubique(e.p(622.0, 58.0), e.p(702.0, 108.0), e.p(702.0, 200.0)),
            Segment::Cubique(e.p(702.0, 252.0), e.p(674.0, 294.0), e.p(614.0, 322.0)),
            Segment::Cubique(e.p(550.0, 352.0), e.p(466.0, 362.0), e.p(380.0, 362.0)),
            Segment::Cubique(e.p(294.0, 362.0), e.p(210.0, 352.0), e.p(146.0, 322.0)),
            Segment::Cubique(e.p(86.0, 294.0), e.p(58.0, 252.0), e.p(58.0, 200.0)),
            Segment::Fermer,
        ],
    })
}

/// Panneau interne en retrait (`iv-fig-corps-inner`).
fn corps_inner_8bitdo(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(78.0, 202.0)),
            Segment::Cubique(e.p(78.0, 124.0), e.p(150.0, 76.0), e.p(250.0, 76.0)),
            Segment::Ligne(e.p(510.0, 76.0)),
            Segment::Cubique(e.p(610.0, 76.0), e.p(682.0, 124.0), e.p(682.0, 202.0)),
            Segment::Cubique(e.p(682.0, 246.0), e.p(656.0, 282.0), e.p(604.0, 306.0)),
            Segment::Cubique(e.p(546.0, 332.0), e.p(464.0, 342.0), e.p(380.0, 342.0)),
            Segment::Cubique(e.p(296.0, 342.0), e.p(214.0, 332.0), e.p(156.0, 306.0)),
            Segment::Cubique(e.p(104.0, 282.0), e.p(78.0, 246.0), e.p(78.0, 202.0)),
            Segment::Fermer,
        ],
    })
}

/// Skin Xbox (`ivGenererManetteXboxSVG`, viewBox 760×430) : grips
/// asymétriques, sticks décalés, face en losange A/B/X/Y colorée.
fn manette_xbox(e: &Echelle) -> Vec<GroupeManette> {
    let face = |bouton: u8, cx: f32, cy: f32, lbl: &'static str, sty: StyleLabel| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.cercle(cx, cy + 4.0, 26.0), StyleManette::Ombre),
                e.forme(e.cercle(cx, cy, 25.0), StyleManette::Bouton),
                e.forme(e.ellipse(cx, cy - 11.0, 14.0, 5.0), StyleManette::Sheen),
                e.txt(cx, cy, 22.0, lbl, sty),
            ],
        )
    };
    let zone = |dir: Direction, x: f32, y: f32, w: f32, h: f32| {
        e.groupe(
            LienManette::Dpad(dir),
            vec![e.forme(e.rect(x, y, w, h, 10.0), StyleManette::Zone)],
        )
    };
    let diag = |dir: Direction, cx: f32, cy: f32| {
        e.groupe(
            LienManette::Dpad(dir),
            vec![e.forme(e.cercle(cx, cy, 5.0), StyleManette::Diag)],
        )
    };
    let trigger = |bouton: u8, cx: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.rect(cx - 70.0, 6.0, 140.0, 80.0, 30.0), StyleManette::Bouton),
                e.txt(cx, 36.0, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    let bumper = |bouton: u8, cx: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.rect(cx - 82.0, 50.0, 164.0, 42.0, 21.0), StyleManette::Bouton),
                e.forme(e.rect(cx - 70.0, 56.0, 140.0, 12.0, 6.0), StyleManette::Sheen),
                e.txt(cx, 72.0, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    vec![
        // LT/RT derrière la coque, puis bumpers sur le bord supérieur.
        trigger(6, 170.0, "LT"),
        trigger(7, 590.0, "RT"),
        e.statique(corps_xbox(e), StyleManette::Corps),
        e.statique(sheen_line_xbox(e), StyleManette::SheenLine),
        bumper(4, 170.0, "LB"),
        bumper(5, 590.0, "RB"),
        e.groupe(
            LienManette::Bouton(16),
            vec![
                e.forme(e.cercle(380.0, 109.0, 20.0), StyleManette::Ombre),
                e.forme(e.cercle(380.0, 105.0, 19.0), StyleManette::Bouton),
                e.txt(380.0, 105.0, 18.0, "✕", StyleLabel::Lbl),
            ],
        ),
        e.groupe(
            LienManette::Bouton(8),
            vec![
                e.forme(e.cercle(335.0, 148.0, 13.0), StyleManette::Bouton),
                e.forme(e.rect(329.0, 143.0, 7.0, 8.0, 1.0), StyleManette::Ico),
                e.forme(e.rect(334.0, 146.0, 7.0, 8.0, 1.0), StyleManette::Ico),
            ],
        ),
        e.groupe(
            LienManette::Bouton(9),
            vec![
                e.forme(e.cercle(425.0, 148.0, 13.0), StyleManette::Bouton),
                e.forme(e.rect(419.0, 141.0, 12.0, 2.4, 1.0), StyleManette::Ico),
                e.forme(e.rect(419.0, 146.0, 12.0, 2.4, 1.0), StyleManette::Ico),
                e.forme(e.rect(419.0, 151.0, 12.0, 2.4, 1.0), StyleManette::Ico),
            ],
        ),
        e.statique(e.cercle(195.0, 175.0, 48.0), StyleManette::StickBase),
        e.groupe(
            LienManette::Stick { gauche: true, bouton: 10 },
            vec![
                e.forme(e.cercle(195.0, 179.0, 31.0), StyleManette::Ombre),
                e.forme(e.cercle(195.0, 175.0, 30.0), StyleManette::StickCap),
                e.forme(e.cercle(195.0, 175.0, 18.0), StyleManette::StickRing),
            ],
        ),
        e.statique(e.cercle(195.0, 300.0, 56.0), StyleManette::Dish),
        e.bloc(vec![
            e.forme(e.rect(173.0, 262.0, 44.0, 76.0, 10.0), StyleManette::Cross),
            e.forme(e.rect(157.0, 278.0, 76.0, 44.0, 10.0), StyleManette::Cross),
        ]),
        diag(Direction::HautGauche, 161.0, 266.0),
        diag(Direction::HautDroite, 229.0, 266.0),
        diag(Direction::BasGauche, 161.0, 334.0),
        diag(Direction::BasDroite, 229.0, 334.0),
        zone(Direction::Haut, 175.0, 264.0, 40.0, 32.0),
        zone(Direction::Bas, 175.0, 304.0, 40.0, 32.0),
        zone(Direction::Gauche, 159.0, 280.0, 32.0, 40.0),
        zone(Direction::Droite, 199.0, 280.0, 32.0, 40.0),
        e.statique(e.cercle(195.0, 300.0, 14.0), StyleManette::Hub),
        face(3, 560.0, 122.0, "Y", StyleLabel::Couleur([0xe0, 0xc4, 0x5e])),
        face(2, 507.0, 170.0, "X", StyleLabel::Couleur([0x5e, 0x9d, 0xe0])),
        face(1, 613.0, 170.0, "B", StyleLabel::Couleur([0xe0, 0x68, 0x5e])),
        face(0, 560.0, 218.0, "A", StyleLabel::Couleur([0x7b, 0xc4, 0x7f])),
        e.statique(e.cercle(560.0, 300.0, 48.0), StyleManette::StickBase),
        e.groupe(
            LienManette::Stick { gauche: false, bouton: 11 },
            vec![
                e.forme(e.cercle(560.0, 304.0, 31.0), StyleManette::Ombre),
                e.forme(e.cercle(560.0, 300.0, 30.0), StyleManette::StickCap),
                e.forme(e.cercle(560.0, 300.0, 18.0), StyleManette::StickRing),
            ],
        ),
    ]
}

/// Coque Xbox (`ivgx-corps`) : arcade haute + deux grips aux coins.
fn corps_xbox(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(380.0, 64.0)),
            Segment::Cubique(e.p(215.0, 64.0), e.p(80.0, 114.0), e.p(70.0, 198.0)),
            Segment::Cubique(e.p(62.0, 268.0), e.p(80.0, 336.0), e.p(120.0, 372.0)),
            Segment::Cubique(e.p(150.0, 400.0), e.p(200.0, 388.0), e.p(216.0, 346.0)),
            Segment::Cubique(e.p(226.0, 318.0), e.p(254.0, 300.0), e.p(287.0, 300.0)),
            Segment::Ligne(e.p(473.0, 300.0)),
            Segment::Cubique(e.p(506.0, 300.0), e.p(534.0, 318.0), e.p(544.0, 346.0)),
            Segment::Cubique(e.p(560.0, 388.0), e.p(610.0, 400.0), e.p(640.0, 372.0)),
            Segment::Cubique(e.p(680.0, 336.0), e.p(698.0, 268.0), e.p(690.0, 198.0)),
            Segment::Cubique(e.p(680.0, 114.0), e.p(545.0, 64.0), e.p(380.0, 64.0)),
            Segment::Fermer,
        ],
    })
}

/// Liseré lumineux de coque Xbox (`iv-fig-sheen-line`) — tracé ouvert.
fn sheen_line_xbox(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(108.0, 152.0)),
            Segment::Cubique(e.p(195.0, 96.0), e.p(565.0, 96.0), e.p(652.0, 152.0)),
        ],
    })
}

/// Skin PlayStation (`ivGenererManettePsSVG`, viewBox 760×430) : DualSense,
/// sticks symétriques en bas, D-pad à 4 bras séparés, touchpad cliquable.
fn manette_ps(e: &Echelle) -> Vec<GroupeManette> {
    let face = |bouton: u8, cx: f32, cy: f32, lbl: &'static str, sty: StyleLabel| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.cercle(cx, cy + 4.0, 26.0), StyleManette::Ombre),
                e.forme(e.cercle(cx, cy, 25.0), StyleManette::Bouton),
                e.forme(e.ellipse(cx, cy - 11.0, 14.0, 5.0), StyleManette::Sheen),
                e.txt(cx, cy, 22.0, lbl, sty),
            ],
        )
    };
    // Bras séparés : la flèche est DANS le <g data-dpad> → elle blanchit
    // quand le bras est pressé (contrairement aux skins 8bitdo/xbox).
    let dbtn = |dir: Direction, x: f32, y: f32, w: f32, h: f32, arrow: &'static str, tx: f32, ty: f32| {
        e.groupe(
            LienManette::Dpad(dir),
            vec![
                e.forme(e.rect(x, y, w, h, 12.0), StyleManette::Bouton),
                e.txt(tx, ty, 17.0, arrow, StyleLabel::Arrow),
            ],
        )
    };
    let trigger = |bouton: u8, cx: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.rect(cx - 70.0, 6.0, 140.0, 80.0, 30.0), StyleManette::Bouton),
                e.txt(cx, 36.0, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    let bumper = |bouton: u8, cx: f32, lbl: &'static str| {
        e.groupe(
            LienManette::Bouton(bouton),
            vec![
                e.forme(e.rect(cx - 82.0, 50.0, 164.0, 42.0, 21.0), StyleManette::Bouton),
                e.forme(e.rect(cx - 70.0, 56.0, 140.0, 12.0, 6.0), StyleManette::Sheen),
                e.txt(cx, 72.0, 22.0, lbl, StyleLabel::Lbl),
            ],
        )
    };
    let stick = |cx: f32, cy: f32, gauche: bool, bouton: u8| {
        e.groupe(
            LienManette::Stick { gauche, bouton },
            vec![
                e.forme(e.cercle(cx, cy + 4.0, 31.0), StyleManette::Ombre),
                e.forme(e.cercle(cx, cy, 30.0), StyleManette::StickCap),
                e.forme(e.cercle(cx, cy, 18.0), StyleManette::StickRing),
            ],
        )
    };
    vec![
        trigger(6, 160.0, "L2"),
        trigger(7, 600.0, "R2"),
        e.statique(corps_ps(e), StyleManette::Corps),
        e.statique(sheen_line_ps(e), StyleManette::SheenLine),
        bumper(4, 160.0, "L1"),
        bumper(5, 600.0, "R1"),
        e.groupe(
            LienManette::Bouton(17),
            vec![e.forme(e.rect(305.0, 92.0, 150.0, 88.0, 12.0), StyleManette::Pad)],
        ),
        e.statique(sheen_line_pad_ps(e), StyleManette::SheenLine),
        e.groupe(
            LienManette::Bouton(8),
            vec![
                e.forme(e.cercle(288.0, 102.0, 10.0), StyleManette::Bouton),
                e.forme(e.rect(283.0, 99.0, 10.0, 2.0, 1.0), StyleManette::Ico),
                e.forme(e.rect(283.0, 103.0, 10.0, 2.0, 1.0), StyleManette::Ico),
            ],
        ),
        e.groupe(
            LienManette::Bouton(9),
            vec![
                e.forme(e.cercle(472.0, 102.0, 10.0), StyleManette::Bouton),
                e.forme(e.rect(467.0, 98.0, 10.0, 8.0, 1.0), StyleManette::Ico),
            ],
        ),
        e.groupe(
            LienManette::Bouton(16),
            vec![
                e.forme(e.cercle(380.0, 209.0, 13.0), StyleManette::Ombre),
                e.forme(e.cercle(380.0, 205.0, 12.0), StyleManette::Bouton),
                e.txt(380.0, 205.0, 8.0, "PS", StyleLabel::Lbl),
            ],
        ),
        dbtn(Direction::Haut, 164.0, 112.0, 42.0, 56.0, "▲", 185.0, 132.0),
        dbtn(Direction::Bas, 164.0, 182.0, 42.0, 56.0, "▼", 185.0, 218.0),
        dbtn(Direction::Gauche, 108.0, 154.0, 56.0, 42.0, "◄", 128.0, 175.0),
        dbtn(Direction::Droite, 196.0, 154.0, 56.0, 42.0, "►", 232.0, 175.0),
        e.statique(e.cercle(185.0, 175.0, 12.0), StyleManette::Hub),
        face(3, 575.0, 130.0, "△", StyleLabel::Couleur([0x7b, 0xc4, 0x7f])),
        face(2, 522.0, 177.0, "□", StyleLabel::Couleur([0xd7, 0x8b, 0xb8])),
        face(1, 628.0, 177.0, "○", StyleLabel::Couleur([0xe0, 0x68, 0x5e])),
        face(0, 575.0, 224.0, "✕", StyleLabel::Couleur([0x6f, 0xa8, 0xe8])),
        e.statique(e.cercle(265.0, 285.0, 48.0), StyleManette::StickBase),
        stick(265.0, 285.0, true, 10),
        e.statique(e.cercle(495.0, 285.0, 48.0), StyleManette::StickBase),
        stick(495.0, 285.0, false, 11),
    ]
}

/// Coque DualSense (`ivgp-corps`) : haut large, grips qui plongent.
fn corps_ps(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(380.0, 60.0)),
            Segment::Cubique(e.p(220.0, 60.0), e.p(90.0, 105.0), e.p(80.0, 178.0)),
            Segment::Cubique(e.p(72.0, 240.0), e.p(88.0, 305.0), e.p(118.0, 348.0)),
            Segment::Cubique(e.p(140.0, 390.0), e.p(170.0, 408.0), e.p(200.0, 398.0)),
            Segment::Cubique(e.p(232.0, 387.0), e.p(244.0, 350.0), e.p(248.0, 318.0)),
            Segment::Ligne(e.p(512.0, 318.0)),
            Segment::Cubique(e.p(516.0, 350.0), e.p(528.0, 387.0), e.p(560.0, 398.0)),
            Segment::Cubique(e.p(590.0, 408.0), e.p(620.0, 390.0), e.p(642.0, 348.0)),
            Segment::Cubique(e.p(672.0, 305.0), e.p(688.0, 240.0), e.p(680.0, 178.0)),
            Segment::Cubique(e.p(670.0, 105.0), e.p(540.0, 60.0), e.p(380.0, 60.0)),
            Segment::Fermer,
        ],
    })
}

/// Liseré lumineux de coque PS (`iv-fig-sheen-line`) — tracé ouvert.
fn sheen_line_ps(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(112.0, 140.0)),
            Segment::Cubique(e.p(200.0, 88.0), e.p(560.0, 88.0), e.p(648.0, 140.0)),
        ],
    })
}

/// Liseré du touchpad PS (`M312 184 L448 184`) — trait ouvert.
fn sheen_line_pad_ps(e: &Echelle) -> Forme {
    Forme::Chemin(Chemin {
        segments: vec![
            Segment::Debut(e.p(312.0, 184.0)),
            Segment::Ligne(e.p(448.0, 184.0)),
        ],
    })
}

fn clavier(dispo: Disposition, rect: RectF, cqw: f32) -> GeometrieInput {
    let rangees: &[&[ToucheDef]] = match dispo {
        Disposition::Azerty => AZERTY,
        Disposition::Qwerty => QWERTY,
    };
    // `.iv-kb-keys` : left/right 4,5 %, top 11,5 %, bottom 5,5 %.
    let kx = rect.x + 0.045 * rect.largeur;
    let ky = rect.y + 0.115 * rect.hauteur;
    let kw = 0.91 * rect.largeur;
    let kh = 0.83 * rect.hauteur;
    // `.iv-clavier` : padding 0,5cqw vertical, 1,5cqw horizontal.
    let ix = kx + 1.5 * cqw;
    let iy = ky + 0.5 * cqw;
    let iw = kw - 3.0 * cqw;
    let ih = kh - cqw;

    let gap_rangee = 0.25 * cqw;
    let gap_colonne = 0.3 * cqw;
    let h_touche = 6.0 * cqw;
    // 6 rangées + bloc flèches (2 rangées), `margin-top` 0,3cqw sur le bloc.
    let total = 8.0 * h_touche + 6.0 * gap_colonne + 0.3 * cqw + gap_rangee;
    let mut y = iy + (ih - total) / 2.0;

    let mut touches = Vec::new();
    for rangee in rangees {
        let somme: f32 = rangee.iter().map(|(_, _, flex)| flex).sum();
        let utile = iw - gap_rangee * (rangee.len() as f32 - 1.0);
        let mut x = ix;
        for (codes, label, flex) in rangee.iter() {
            let largeur = if somme > 0.0 { utile * flex / somme } else { 0.0 };
            if !codes.is_empty() {
                touches.push(TouchePlacee {
                    codes,
                    label,
                    rect: RectF {
                        x,
                        y,
                        largeur,
                        hauteur: h_touche,
                    },
                });
            }
            x += largeur + gap_rangee;
        }
        y += h_touche + gap_colonne;
    }

    // Bloc flèches : deux rangées de touches `flex: 0 0 6cqw`, centrées.
    y += 0.3 * cqw;
    let cote = 6.0 * cqw;
    touches.push(TouchePlacee {
        codes: &["ArrowUp"],
        label: "▲",
        rect: RectF {
            x: ix + (iw - cote) / 2.0,
            y,
            largeur: cote,
            hauteur: h_touche,
        },
    });
    y += h_touche + gap_rangee;
    let largeur_fleches = 3.0 * cote + 2.0 * gap_rangee;
    let mut x = ix + (iw - largeur_fleches) / 2.0;
    for (codes, label) in [
        (&["ArrowLeft"][..], "◄"),
        (&["ArrowDown"][..], "▼"),
        (&["ArrowRight"][..], "►"),
    ] {
        touches.push(TouchePlacee {
            codes,
            label,
            rect: RectF {
                x,
                y,
                largeur: cote,
                hauteur: h_touche,
            },
        });
        x += cote + gap_rangee;
    }

    GeometrieInput {
        touches,
        chassis: Some(chassis(rect)),
        souris: None,
        manette: None,
    }
}

fn numpad(rect: RectF, cqw: f32) -> GeometrieInput {
    // `.iv-section-numpad` : padding 4 % vertical, 8 % horizontal.
    let ix = rect.x + 0.08 * rect.largeur;
    let iy = rect.y + 0.04 * rect.hauteur;
    let iw = 0.84 * rect.largeur;
    let ih = 0.92 * rect.hauteur;
    let gap = 1.2 * cqw;
    let h_rangee = ((ih - 4.0 * gap) / 5.0).max(6.0 * cqw);
    let w_colonne = (iw - 3.0 * gap) / 4.0;
    let total_h = 5.0 * h_rangee + 4.0 * gap;
    let y0 = iy + (ih - total_h) / 2.0;

    let touches = NUMPAD
        .iter()
        .map(|(ligne, colonne, span_l, span_c, codes, label)| TouchePlacee {
            codes,
            label,
            rect: RectF {
                x: ix + *colonne as f32 * (w_colonne + gap),
                y: y0 + *ligne as f32 * (h_rangee + gap),
                largeur: *span_c as f32 * w_colonne + (*span_c as f32 - 1.0) * gap,
                hauteur: *span_l as f32 * h_rangee + (*span_l as f32 - 1.0) * gap,
            },
        })
        .collect();

    GeometrieInput {
        touches,
        chassis: Some(chassis(rect)),
        souris: None,
        manette: None,
    }
}

/// Châssis commun clavier/numpad (`ivChassisHTML`, viewBox 100 × 100 étiré).
fn chassis(rect: RectF) -> Chassis {
    let (l, h) = (rect.largeur, rect.hauteur);
    // Le `rx` du SVG est exprimé dans un viewBox étiré : un rayon unique est
    // une approximation (D2D n'a pas d'ellipse de coin par axe).
    let rayon = 3.0 * l.min(h) / 100.0;
    // Barre de LEDs : viewBox 280 × 46 en `xMaxYMid meet` dans une boîte
    // (4 %, 1,5 %, 92 % × 8 %).
    let bx = rect.x + 0.04 * l;
    let by = rect.y + 0.015 * h;
    let bw = 0.92 * l;
    let bh = 0.08 * h;
    let echelle = (bw / 280.0).min(bh / 46.0);
    let ox = bx + bw - 280.0 * echelle;
    let oy = by + (bh - 46.0 * echelle) / 2.0;
    let leds = [
        (176.0_f32, "CAPS", "CapsLock"),
        (226.0, "NUM", "NumLock"),
        (272.0, "SCRL", "ScrollLock"),
    ]
    .iter()
    .map(|(cx, label, code)| Led {
        code,
        centre: (ox + cx * echelle, oy + 16.0 * echelle),
        rayon: 7.0 * echelle,
        label,
        taille: 10.0 * echelle,
    })
    .collect();

    Chassis {
        plaque: RectF {
            x: rect.x + 0.02 * l,
            y: rect.y + 0.015 * h,
            largeur: 0.96 * l,
            hauteur: 0.97 * h,
        },
        cable: RectF {
            x: rect.x + 0.46 * l,
            y: rect.y,
            largeur: 0.08 * l,
            hauteur: 0.02 * h,
        },
        plateau: RectF {
            x: rect.x + 0.04 * l,
            y: rect.y + 0.105 * h,
            largeur: 0.92 * l,
            hauteur: 0.84 * h,
        },
        rayon,
        leds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: ModeInput, dispo: Disposition) -> InputCfg {
        InputCfg {
            rect: (100.0, 50.0, 213.0, 197.0),
            z: 0,
            mode,
            disposition: dispo,
            skin: "xbox".to_string(),
            couleur: [0x8b, 0x5c, 0xf6, 255],
            mapping: None,
            cadre: None,
        }
    }

    #[test]
    fn clavier_azerty_a_les_bonnes_rangees() {
        let g = disposition(&cfg(ModeInput::Clavier, Disposition::Azerty));
        // 13 + 14 + 14 + 13 + 13 + 8 touches + 4 flèches = 79.
        assert_eq!(g.touches.len(), 79);
        assert!(g.chassis.is_some());
        assert!(g.touches.iter().any(|t| t.codes == ["KeyA"]));
        assert!(g.touches.iter().any(|t| t.codes == ["Space"]));
        assert!(
            !g.touches.iter().any(|t| t.codes.is_empty()),
            "les espaceurs ne produisent pas de touche"
        );
    }

    #[test]
    fn clavier_qwerty_place_les_lettres_autrement() {
        let azerty = disposition(&cfg(ModeInput::Clavier, Disposition::Azerty));
        let qwerty = disposition(&cfg(ModeInput::Clavier, Disposition::Qwerty));
        let y = |g: &GeometrieInput, code: &str| {
            g.touches
                .iter()
                .find(|t| t.codes == [code])
                .map(|t| t.rect.y)
                .expect("touche")
        };
        // AZERTY : A sur la rangée du haut, Q en dessous. QWERTY : l'inverse.
        assert!(y(&azerty, "KeyA") < y(&azerty, "KeyQ"));
        assert!(y(&qwerty, "KeyQ") < y(&qwerty, "KeyA"));
    }

    #[test]
    fn touches_dans_la_zone_et_sans_chevauchement() {
        let c = cfg(ModeInput::Clavier, Disposition::Azerty);
        let g = disposition(&c);
        for t in &g.touches {
            assert!(t.rect.x >= c.rect.0 as f32 - 0.01);
            assert!(t.rect.y >= c.rect.1 as f32 - 0.01);
            assert!(t.rect.x + t.rect.largeur <= (c.rect.0 + c.rect.2) as f32 + 0.01);
            assert!(t.rect.y + t.rect.hauteur <= (c.rect.1 + c.rect.3) as f32 + 0.01);
        }
        // Deux touches d'une même rangée ne se chevauchent jamais.
        for (i, a) in g.touches.iter().enumerate() {
            for b in g.touches.iter().skip(i + 1) {
                let meme_rangee = (a.rect.y - b.rect.y).abs() < 0.01;
                if meme_rangee {
                    let disjoint = a.rect.x + a.rect.largeur <= b.rect.x + 0.01
                        || b.rect.x + b.rect.largeur <= a.rect.x + 0.01;
                    assert!(disjoint, "chevauchement {a:?} / {b:?}");
                }
            }
        }
    }

    #[test]
    fn numpad_zero_sur_deux_colonnes_et_plus_sur_deux_lignes() {
        let g = disposition(&cfg(ModeInput::Numpad, Disposition::Azerty));
        assert_eq!(g.touches.len(), 17);
        let zero = g
            .touches
            .iter()
            .find(|t| t.codes == ["Digit0", "Numpad0"])
            .expect("0");
        let un = g
            .touches
            .iter()
            .find(|t| t.codes == ["Digit1", "Numpad1"])
            .expect("1");
        assert!((zero.rect.largeur - (2.0 * un.rect.largeur + 1.2 * 213.0 / 100.0)).abs() < 0.5);
        let plus = g
            .touches
            .iter()
            .find(|t| t.codes == ["NumpadAdd"])
            .expect("+");
        assert!((plus.rect.hauteur - (2.0 * un.rect.hauteur + 1.2 * 213.0 / 100.0)).abs() < 0.5);
    }

    fn cfg_skin(skin: &str) -> InputCfg {
        InputCfg {
            skin: skin.to_string(),
            mode: ModeInput::Manette,
            ..cfg(ModeInput::Manette, Disposition::Azerty)
        }
    }

    #[test]
    fn manette_produit_une_figure_par_skin() {
        for skin in ["8bitdo", "xbox", "ps", "inconnu"] {
            let g = disposition(&cfg_skin(skin));
            let m = g.manette.unwrap_or_else(|| panic!("skin {skin}"));
            assert!(g.touches.is_empty() && g.chassis.is_none() && g.souris.is_none());
            assert!(
                m.groupes.iter().map(|g| g.primitives.len()).sum::<usize>() > 30,
                "skin {skin} : figure trop courte"
            );
            // `meet` : le viewBox 760×… est borné par la **largeur** du
            // widget (213×197 → échelle ~0,28), centré verticalement.
            assert!(m.echelle > 0.27 && m.echelle < 0.29, "skin {skin}");
        }
        // Skin inconnu → xbox (défaut du dispatch JS).
        assert_eq!(
            disposition(&cfg_skin("inconnu")).manette.unwrap().skin,
            SkinManette::Xbox
        );
    }

    #[test]
    fn manette_liens_couvrent_le_contrat_svg() {
        let g = disposition(&cfg_skin("xbox"));
        let m = g.manette.expect("géométrie");
        let boutons: Vec<u8> = m
            .groupes
            .iter()
            .filter_map(|g| match g.lien {
                LienManette::Bouton(b) => Some(b),
                _ => None,
            })
            .collect();
        // Xbox : 4 face + 2 bumpers + 2 triggers + view/menu + home = 12,
        // + les 2 caps de stick (lien Stick, bouton 10/11) hors ce comptage.
        assert_eq!(boutons.len(), 11);
        assert!(boutons.contains(&0) && boutons.contains(&16));
        let dpad: Vec<Direction> = m
            .groupes
            .iter()
            .filter_map(|g| match g.lien {
                LienManette::Dpad(d) => Some(d),
                _ => None,
            })
            .collect();
        assert_eq!(dpad.len(), 8); // 4 zones + 4 diagonales
        let sticks = m
            .groupes
            .iter()
            .filter(|g| matches!(g.lien, LienManette::Stick { .. }))
            .count();
        assert_eq!(sticks, 2);
    }

    #[test]
    fn manette_8bitdo_a_le_fallback_dpad_sur_axes() {
        // Boutons D-pad bruts : jamais remappés ni soumis au fallback.
        let mut etat = EtatManette::default();
        etat.boutons[12] = true;
        let dpad = resoudre_dpad(&etat, SkinManette::Bitdo, None);
        assert!(dpad.haut && !dpad.bas && !dpad.gauche && !dpad.droite);
        // Skin 8bitdo, D-pad inactif → axes LeftStick comme D-pad.
        let mut etat = EtatManette::default();
        etat.axes[1] = -0.8;
        let dpad = resoudre_dpad(&etat, SkinManette::Bitdo, None);
        assert!(dpad.haut && !dpad.bas);
        // Seuil 0,5 strict : sous le seuil rien ne s'allume.
        etat.axes[1] = -0.4;
        assert_eq!(resoudre_dpad(&etat, SkinManette::Bitdo, None), DpadEtat::default());
        // Le fallback n'existe pas sur les autres skins.
        etat.axes[1] = -0.8;
        assert_eq!(resoudre_dpad(&etat, SkinManette::Xbox, None), DpadEtat::default());
        // Inversions uniquement dans le fallback.
        let mapping = MappingManette {
            boutons: Vec::new(),
            dpad_invert_x: false,
            dpad_invert_y: true,
        };
        let dpad = resoudre_dpad(&etat, SkinManette::Bitdo, Some(&mapping));
        assert!(dpad.bas && !dpad.haut, "dpadInvertY inverse haut/bas");
    }

    #[test]
    fn manette_diagonales_exigent_les_deux_bras() {
        let mut etat = EtatManette::default();
        etat.boutons[12] = true;
        let dpad = resoudre_dpad(&etat, SkinManette::Xbox, None);
        assert!(!Direction::HautGauche.activee(dpad));
        etat.boutons[14] = true;
        let dpad = resoudre_dpad(&etat, SkinManette::Xbox, None);
        assert!(Direction::HautGauche.activee(dpad));
        assert!(!Direction::BasGauche.activee(dpad));
    }

    #[test]
    fn manette_remap_vise_le_bouton_physique() {
        // Identité par défaut et hors table (16/17 : home, touchpad).
        assert_eq!(bouton_physique(0, None), 0);
        assert_eq!(bouton_physique(16, None), 16);
        let mapping = MappingManette {
            boutons: vec![4, 5, 6, 7, 0, 1, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15],
            dpad_invert_x: false,
            dpad_invert_y: false,
        };
        assert_eq!(bouton_physique(0, Some(&mapping)), 4);
        // La table a 16 entrées : 16/17 passent par l'identité (comme le JS).
        assert_eq!(bouton_physique(16, Some(&mapping)), 16);
        assert_eq!(bouton_physique(17, Some(&mapping)), 17);
    }

    #[test]
    fn souris_a_un_corps_deux_zones_et_une_molette() {
        let g = disposition(&cfg(ModeInput::Souris, Disposition::Azerty));
        let s = g.souris.expect("géométrie souris");
        assert!(g.touches.is_empty() && g.chassis.is_none());
        assert!(s.corps.segments.len() > 4, "corps en courbes");
        assert_eq!(s.zone_gauche.segments.len(), 4);
        assert_eq!(s.zone_droite.segments.len(), 4);
        assert_eq!(s.crans.len(), 4);
        // Molette dans la loge, loge dans le corps.
        assert!(s.molette.x > s.loge.x && s.molette.y > s.loge.y);
        assert!(s.molette.y + s.molette.hauteur < s.loge.y + s.loge.hauteur);
    }

    #[test]
    fn souris_tient_dans_le_widget_et_garde_son_ratio() {
        let c = cfg(ModeInput::Souris, Disposition::Azerty);
        let g = disposition(&c);
        let s = g.souris.expect("géométrie souris");
        let mut min = (f32::MAX, f32::MAX);
        let mut max = (f32::MIN, f32::MIN);
        for segment in &s.corps.segments {
            let points: &[Point] = match segment {
                Segment::Debut(pt) | Segment::Ligne(pt) => std::slice::from_ref(pt),
                Segment::Cubique(a, b, d) => {
                    min = (min.0.min(a.x).min(b.x).min(d.x), min.1.min(a.y).min(b.y).min(d.y));
                    max = (max.0.max(a.x).max(b.x).max(d.x), max.1.max(a.y).max(b.y).max(d.y));
                    &[]
                }
                Segment::Fermer => &[],
            };
            for pt in points {
                min = (min.0.min(pt.x), min.1.min(pt.y));
                max = (max.0.max(pt.x), max.1.max(pt.y));
            }
        }
        // `meet` : le viewBox 360 × 520 est réduit à la hauteur du widget
        // (213 × 197 → échelle 0,379), centré horizontalement.
        assert!(min.0 >= c.rect.0 as f32 - 0.01 && max.0 <= (c.rect.0 + c.rect.2) as f32 + 0.01);
        assert!(min.1 >= c.rect.1 as f32 - 0.01 && max.1 <= (c.rect.1 + c.rect.3) as f32 + 0.01);
        assert!((s.echelle - 197.0 / 520.0).abs() < 0.01);
    }

    #[test]
    fn file_input_ne_bump_que_si_l_ensemble_change() {
        let mut file = FileInput::nouveau();
        file.remplacer(EtatInput {
            touches: vec!["KeyW".to_string(), "KeyA".to_string()],
            souris: EtatSouris::default(),
            manette: EtatManette::default(),
        });
        assert_eq!(file.version(), 1);
        // Même ensemble, autre ordre : aucune invalidation.
        file.remplacer(EtatInput {
            touches: vec!["KeyA".to_string(), "KeyW".to_string()],
            souris: EtatSouris::default(),
            manette: EtatManette::default(),
        });
        assert_eq!(file.version(), 1);
        // Un clic suffit à invalider.
        file.remplacer(EtatInput {
            touches: vec!["KeyA".to_string(), "KeyW".to_string()],
            souris: EtatSouris {
                gauche: true,
                ..EtatSouris::default()
            },
            manette: EtatManette::default(),
        });
        assert_eq!(file.version(), 2);
        assert!(file.souris().gauche);
        file.remplacer(EtatInput {
            touches: vec!["KeyW".to_string()],
            souris: EtatSouris::default(),
            manette: EtatManette::default(),
        });
        assert_eq!(file.version(), 3);
        assert!(!file.est_pressee("KeyA"));
        assert!(file.est_pressee("KeyW"));
    }

    #[test]
    fn file_input_invalide_sur_changement_manette() {
        let mut file = FileInput::nouveau();
        let mut manette = EtatManette {
            connectee: true,
            ..EtatManette::default()
        };
        manette.boutons[0] = true;
        file.remplacer(EtatInput {
            touches: Vec::new(),
            souris: EtatSouris::default(),
            manette,
        });
        assert_eq!(file.version(), 1);
        assert!(file.manette().boutons[0]);
        // Même état repoussé (30 Hz) : pas de nouvelle version.
        file.remplacer(EtatInput {
            touches: Vec::new(),
            souris: EtatSouris::default(),
            manette,
        });
        assert_eq!(file.version(), 1);
        // Un axe qui bouge invalide aussi.
        manette.axes[0] = 0.5;
        file.remplacer(EtatInput {
            touches: Vec::new(),
            souris: EtatSouris::default(),
            manette,
        });
        assert_eq!(file.version(), 2);
    }

    #[test]
    fn touche_multi_codes_repond_a_l_un_ou_l_autre() {
        let mut file = FileInput::nouveau();
        file.remplacer(EtatInput {
            touches: vec!["Numpad7".to_string()],
            souris: EtatSouris::default(),
            manette: EtatManette::default(),
        });
        assert!(file.est_pressee_parmi(&["Digit7", "Numpad7"]));
        assert!(!file.est_pressee_parmi(&["Digit8", "Numpad8"]));
    }

    #[test]
    fn mode_et_disposition_inconnus_prennent_les_defauts() {
        assert_eq!(ModeInput::depuis_nom("inconnu"), ModeInput::Clavier);
        assert_eq!(Disposition::depuis_nom("inconnu"), Disposition::Azerty);
        assert_eq!(Disposition::depuis_nom("qwerty"), Disposition::Qwerty);
    }
}
