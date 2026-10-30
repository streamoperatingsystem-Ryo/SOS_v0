//! Module HUD « horloge » (`scene.modules.horloge`) — date + heure locale.
//!
//! Premier module HUD porté en natif : le seul contenu métier piloté à
//! 100 % par la scène (options de `ModuleHud`) et l'horloge système —
//! aucun canal de données live à créer (les autres modules et les widgets
//! métier attendent les frames `communaute`/`viewers`/`speedrun-etat`/
//! `musique-*` du pont app).
//!
//! Miroir de `.module.horloge` + `majModuleHorloge` (diffusion.html) :
//! - « JJ / MM / AA » (0,78 em, espacement 0,25 em, #d6d6dc) si
//!   `afficherDate !== false` (défaut DOM : visible) ;
//! - séparateur 2 px × 1,1 em de l'accent `--mod-accent` (extrémité la plus
//!   lumineuse du dégradé du cadre actif — `bpmResoudreCouleurs().p`) ;
//! - « HH : MM[:SS] » (gras, espacement 0,18 em, #f4f4f6) + `AM`/`PM` en
//!   0,5 em (`format12h`, `secondes`) ;
//! - `--mod-fs` = `taille || hauteur × 0,42` (plancher 8 px), gap 0,7 em,
//!   fond arrondi 10 px `rgba(10,10,14,opacite)` (défaut 0,45) ;
//! - z 9000 (`#modules-root`), contenu rogné au rect (`overflow: hidden`)
//!   et au polygone du cadre opt-in (`m.cadre` → `appliquerCadreOverlay`).
//!
//! Limites assumées :
//! - `text-shadow: 0 1px 2px rgba(0,0,0,.6)` ≈ passe noire à +1 px (un vrai
//!   flou exigerait un offscreen dédié) ;
//! - `box-shadow 0 0 6px` du séparateur ≈ halo translucide élargi de 3 px ;
//! - `font-variant-numeric: tabular-nums` non porté (Segoe UI rend déjà des
//!   chiffres tabulaires en pratique) ;
//! - centrage vertical par boîte (flex `align-items: center`) : la ligne de
//!   base peut diverger de ±1 px avec le DOM — AM/PM est, lui, aligné sur
//!   la ligne de base de l'heure, comme dans le DOM.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Geometry, ID2D1PathGeometry, ID2D1RenderTarget,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, DWRITE_LINE_METRICS,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use sos_scene::{ModuleHud, Scene};

use crate::cadres::{couleur_hex, CadreCfg};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

// --- Liaison directe kernel32 -----------------------------------------------
// `Win32_System_SystemInformation` n'est pas dans les features du crate ;
// même motif que `encode/nvenc/ffi.rs` : kernel32 est déjà liée, la fonction
// est déclarée à la main.
extern "system" {
    fn GetLocalTime(lpsystemtime: *mut SYSTEMTIME);
}

/// Date/heure **locales** (fuseau + DST gérés par l'OS, comme `new Date()`).
/// Sans rapport avec l'horloge monotone du moteur (cadence, §6) : c'est le
/// contenu du module, l'heure civile est sa donnée.
fn heure_locale() -> SYSTEMTIME {
    let mut st = SYSTEMTIME::default();
    unsafe { GetLocalTime(&mut st) };
    st
}

/// z de `#modules-root` dans `diffusion.html` (table §9.1 : modules HUD).
pub const Z_MODULES: i32 = 9000;

/// Configuration d'un module horloge, extraite de `scene.modules.horloge`.
#[derive(Debug, Clone, PartialEq)]
pub struct HorlogeCfg {
    /// Rect du module en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (9000 — `#modules-root`).
    pub z: i32,
    /// Id de police `fonts.ts` ; `None` = Segoe UI (défaut `--mod-font`).
    pub police: Option<String>,
    /// `--mod-fs` résolu en px : `taille || hauteur × 0,42`, plancher 8.
    pub taille: f32,
    /// `afficherDate !== false` (défaut DOM : date visible).
    pub date: bool,
    /// `secondes === true` (défaut DOM : masquées).
    pub secondes: bool,
    /// `format12h === true` (défaut DOM : 24 h).
    pub format12h: bool,
    /// `--mod-opacite` du fond translucide (0–1, défaut 0,45).
    pub opacite: f32,
    /// Accent `--mod-accent` (couleur la plus lumineuse du cadre actif).
    pub accent: [u8; 4],
    /// Cadre de scène opt-in (`m.cadre`) : voile et texte clippés au
    /// polygone, le trait étant tracé par le calque cadre au même z.
    pub cadre: Option<CadreCfg>,
}

/// Taille de base du module (`--mod-fs`) : `taille || hauteur × 0,42`,
/// arrondie et bornée à 8 px (`Math.max(8, Math.round(fs))`). `taille = 0`
/// est falsy côté JS → la dérivation de la hauteur s'applique.
pub fn taille_module(m: &ModuleHud) -> f32 {
    m.taille
        .filter(|&t| t > 0)
        .map(|t| t as f32)
        .unwrap_or(m.hauteur as f32 * 0.42)
        .round()
        .max(8.0)
}

/// Opacité du fond (`--mod-opacite`) : `m.opacite` bornée 0–1, défaut 0,45.
pub fn opacite_module(m: &ModuleHud) -> f32 {
    m.opacite.map(|o| o.clamp(0.0, 1.0)).unwrap_or(0.45) as f32
}

/// Accent `--mod-accent` — miroir de `bpmResoudreCouleurs().p` : si le cadre
/// de scène est actif, l'extrémité **la plus lumineuse** (rec.601) de son
/// dégradé ; sinon le repli #e0e0e0. Une `couleur` invalide emporte le
/// repli, `couleurFin` invalide seule laisse `couleur` (ordre du DOM).
pub fn accent_module(scene: &Scene) -> [u8; 4] {
    const DEFAUT: [u8; 4] = [0xe0, 0xe0, 0xe0, 255];
    let Some(cadre) = scene.cadreWidget.as_ref().filter(|c| c.actif) else {
        return DEFAUT;
    };
    match (hex6(&cadre.couleur), hex6(&cadre.couleurFin)) {
        (Some(a), Some(b)) => extremite_lumineuse(a, b),
        (Some(a), None) => a,
        _ => DEFAUT,
    }
}

/// Extrémité la plus lumineuse (rec.601) d'un dégradé de cadre —
/// `bpmResoudreCouleurs().p` du DOM. Partagée par `accent_module` et la
/// couleur de texte de la carte alerte (`texte::alertes`).
pub(crate) fn extremite_lumineuse(a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    if luminance(&a) < luminance(&b) {
        b
    } else {
        a
    }
}

/// Hex strict `#rrggbb` (`bpmHexValide` : exactement 6 chiffres).
fn hex6(hex: &str) -> Option<[u8; 4]> {
    if hex.len() == 7 && hex.starts_with('#') {
        couleur_hex(hex)
    } else {
        None
    }
}

/// Luminance perceptuelle 0–255 (rec.601) — `bpmLuminance`.
fn luminance(c: &[u8; 4]) -> f32 {
    c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114
}

/// « JJ / MM / AA » — `pad2(d) + " / " + pad2(m+1) + " / " + pad2(y%100)`.
pub fn format_date(jour: u16, mois: u16, annee: u16) -> String {
    format!("{jour:02} / {mois:02} / {:02}", annee % 100)
}

/// « HH : MM[:SS] » + suffixe AM/PM éventuel — miroir de `majModuleHorloge` :
/// 12 h → `h % 12 || 12` avec suffixe ; 24 h → heure brute, pas de suffixe.
pub fn format_heure(
    heure: u16,
    minute: u16,
    seconde: u16,
    secondes: bool,
    format12h: bool,
) -> (String, Option<&'static str>) {
    let (h, ampm) = if format12h {
        let ampm = if heure >= 12 { "PM" } else { "AM" };
        let h = match heure % 12 {
            0 => 12,
            h => h,
        };
        (h, Some(ampm))
    } else {
        (heure, None)
    };
    let mut texte = format!("{h:02} : {minute:02}");
    if secondes {
        texte += &format!(" : {seconde:02}");
    }
    (texte, ampm)
}

/// Un texte mis en page du module : format vivant, métriques mesurées.
struct LigneHorloge {
    /// Contenu — sert à détecter le changement (tick seconde).
    texte: String,
    layout: IDWriteTextLayout,
    /// Le layout ne possède pas son format : il doit vivre ici.
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
    /// Ligne de base de la première ligne — alignement AM/PM sur l'heure.
    base: f32,
}

/// Les trois lignes optionnelles du module, clé de config.
#[derive(Default)]
pub(super) struct BlocHorloge {
    date: Option<LigneHorloge>,
    heure: Option<LigneHorloge>,
    ampm: Option<LigneHorloge>,
}

/// Emplacement d'une ligne dans le bloc.
#[derive(Clone, Copy)]
enum Slot {
    Date,
    Heure,
    Ampm,
}

/// Clé de cache d'un bloc : tout ce qui change la mise en page (le rect
/// et l'opacité changent la destination, pas les layouts).
fn cle_bloc(cfg: &HorlogeCfg) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.police.hash(&mut h);
    cfg.taille.to_bits().hash(&mut h);
    cfg.date.hash(&mut h);
    cfg.secondes.hash(&mut h);
    cfg.format12h.hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine un module horloge (date │ heure) — dans une passe ouverte.
    ///
    /// Le contenu est rogné au rect du module (`overflow: hidden` du CSS)
    /// et au polygone de son cadre opt-in (`clip-path`) — même motif que
    /// `dessiner_input`.
    pub fn dessiner_horloge(
        &mut self,
        cfg: &HorlogeCfg,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let (x, y, w, h) = cfg.rect;
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let zone = D2D_RECT_F {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        unsafe {
            self.cible_rt
                .PushAxisAlignedClip(&zone, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
        };
        let masque: Option<ID2D1Geometry> = match clip {
            Some(geo) => Some(
                geo.cast()
                    .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?,
            ),
            None => None,
        };
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: zone,
            geometricMask: ManuallyDrop::new(masque),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let avec_couche = params.geometricMask.is_some();
        if avec_couche {
            let layer = unsafe { self.cible_rt.CreateLayer(None) }
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer horloge : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_horloge_interne(cfg, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    /// Fond arrondi + contenu du module — sous les clips déjà posés.
    fn dessiner_horloge_interne(
        &mut self,
        cfg: &HorlogeCfg,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        // `.mod-fond` : arrondi 10 px, rgba(10,10,14,opacite).
        let fond = unsafe {
            self.cible_rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 10.0 / 255.0,
                    g: 10.0 / 255.0,
                    b: 14.0 / 255.0,
                    a: cfg.opacite,
                },
                None,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse horloge : {}", e.message())))?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: *zone,
                    radiusX: 10.0,
                    radiusY: 10.0,
                },
                &fond,
            )
        };

        let fs = cfg.taille;
        let st = heure_locale();
        let date = cfg.date.then(|| format_date(st.wDay, st.wMonth, st.wYear));
        let (heure, ampm) = format_heure(
            st.wHour,
            st.wMinute,
            st.wSecond,
            cfg.secondes,
            cfg.format12h,
        );
        self.synchroniser_bloc_horloge(cfg, &date, &heure, ampm)?;
        let Some(bloc) = self.blocs_horloge.get(&cle_bloc(cfg)) else {
            return Ok(());
        };

        let ombre = unsafe {
            self.cible_rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.6,
                },
                None,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse ombre : {}", e.message())))?;
        let clair = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d([0xd6, 0xd6, 0xdc, 255]), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse date : {}", e.message())))?;
        let blanc = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d([0xf4, 0xf4, 0xf6, 255]), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse heure : {}", e.message())))?;
        let mut c = cfg.accent;
        c[3] = 0x40;
        let halo = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(c), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse halo : {}", e.message())))?;
        let accent = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d(cfg.accent), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse accent : {}", e.message())))?;

        // Flex `gap: 0.7em` + `justify-content: center` : le groupe est
        // centré même quand il déborde (rogné symétriquement des deux côtés
        // par `overflow: hidden` — le padding 0.8em ne déplace pas le centre).
        let gap = 0.7 * fs;
        let mut total = bloc.heure.as_ref().map(|l| l.w).unwrap_or(0.0);
        if let Some(a) = &bloc.ampm {
            // `margin-left: 0.4em` de l'ampm (taille 0,5 em) = 0,2 em du fs.
            total += 0.2 * fs + a.w;
        }
        if let Some(d) = &bloc.date {
            total += d.w + gap + 2.0 + gap;
        }
        let mut curseur = zone.left + (zone.right - zone.left - total) / 2.0;
        let cy = (zone.top + zone.bottom) / 2.0;

        if let Some(d) = &bloc.date {
            dessiner_ligne(&self.cible_rt, curseur, cy - d.h / 2.0, d, &ombre, &clair);
            curseur += d.w + gap;
            // `.mh-sep` : 2 px × 1,1 em, accent, halo ≈ `box-shadow 0 0 6px`.
            let sep_h = 1.1 * fs;
            let sep = D2D_RECT_F {
                left: curseur,
                top: cy - sep_h / 2.0,
                right: curseur + 2.0,
                bottom: cy + sep_h / 2.0,
            };
            let halo_r = D2D_RECT_F {
                left: sep.left - 3.0,
                top: sep.top - 3.0,
                right: sep.right + 3.0,
                bottom: sep.bottom + 3.0,
            };
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: halo_r,
                        radiusX: 4.0,
                        radiusY: 4.0,
                    },
                    &halo,
                );
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: sep,
                        radiusX: 2.0,
                        radiusY: 2.0,
                    },
                    &accent,
                );
            }
            curseur += 2.0 + gap;
        }
        if let Some(l) = &bloc.heure {
            let top = cy - l.h / 2.0;
            dessiner_ligne(&self.cible_rt, curseur, top, l, &ombre, &blanc);
            curseur += l.w;
            if let Some(a) = &bloc.ampm {
                // Inline dans `.mh-heure` : l'ampm pose sa ligne de base sur
                // celle de l'heure, décalé de `margin-left: 0.4em` (0,2 em).
                let atop = top + l.base - a.base;
                dessiner_ligne(&self.cible_rt, curseur + 0.2 * fs, atop, a, &ombre, &clair);
            }
        }
        Ok(())
    }

    /// (Re)construit les lignes mises en page dont le contenu a changé —
    /// au plus une recréation par seconde (le tick 1 s du DOM).
    fn synchroniser_bloc_horloge(
        &mut self,
        cfg: &HorlogeCfg,
        date: &Option<String>,
        heure: &str,
        ampm: Option<&str>,
    ) -> Result<(), ErreurTexte> {
        let fs = cfg.taille;
        let style_heure = StyleTexte {
            police: cfg
                .police
                .clone()
                .unwrap_or_else(|| "Segoe UI".to_string()),
            taille: fs,
            gras: true,
            espacement: 0.18 * fs,
            couleur: [0xf4, 0xf4, 0xf6, 255],
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let style_date = StyleTexte {
            taille: 0.78 * fs,
            gras: false,
            espacement: 0.25 * 0.78 * fs,
            couleur: [0xd6, 0xd6, 0xdc, 255],
            ..style_heure.clone()
        };
        // L'ampm hérite le letter-spacing calculé de `.mh-heure` (px absolu).
        let style_ampm = StyleTexte {
            taille: 0.5 * fs,
            couleur: [0xd6, 0xd6, 0xdc, 255],
            ..style_heure.clone()
        };

        // Table bornée : au-delà de 16 configs distinctes on repart à zéro
        // (horloge singleton — les clés ne divergent que par options éditées).
        if self.blocs_horloge.len() > 16 {
            self.blocs_horloge.clear();
        }
        let cle = cle_bloc(cfg);
        let mut maj: Vec<(Slot, StyleTexte, String)> = Vec::new();
        {
            let bloc = self.blocs_horloge.entry(cle).or_default();
            if bloc.date.as_ref().map(|l| l.texte.as_str()) != date.as_deref() {
                bloc.date = None;
                if let Some(t) = date {
                    maj.push((Slot::Date, style_date, t.clone()));
                }
            }
            if bloc.heure.as_ref().map(|l| l.texte.as_str()) != Some(heure) {
                bloc.heure = None;
                maj.push((Slot::Heure, style_heure, heure.to_string()));
            }
            if bloc.ampm.as_ref().map(|l| l.texte.as_str()) != ampm {
                bloc.ampm = None;
                if let Some(t) = ampm {
                    maj.push((Slot::Ampm, style_ampm, t.to_string()));
                }
            }
        }
        for (slot, style, texte) in maj {
            let ligne = self.ligne_horloge(&style, &texte)?;
            if let Some(bloc) = self.blocs_horloge.get_mut(&cle) {
                match slot {
                    Slot::Date => bloc.date = Some(ligne),
                    Slot::Heure => bloc.heure = Some(ligne),
                    Slot::Ampm => bloc.ampm = Some(ligne),
                }
            }
        }
        Ok(())
    }

    /// Met en page un texte du module : format, espacement des lettres
    /// (CSS `letter-spacing` = `trailingSpacing`), métriques mesurées.
    fn ligne_horloge(
        &self,
        style: &StyleTexte,
        texte: &str,
    ) -> Result<LigneHorloge, ErreurTexte> {
        let format = self.format(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("ligne horloge vide".to_string()));
        }
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, 8192.0, 2048.0)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout horloge : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap horloge : {}", e.message())))?;
        if style.espacement != 0.0 {
            let layout1: IDWriteTextLayout1 = layout.cast().map_err(|e| {
                ErreurTexte::Systeme(format!("IDWriteTextLayout1 : {}", e.message()))
            })?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    style.espacement,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: 0,
                        length: contenu.len() as u32,
                    },
                )
            }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("SetCharacterSpacing horloge : {}", e.message()))
            })?;
        }
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques horloge : {}", e.message())))?;
        let base = base_ligne(&layout).unwrap_or(metriques.height * 0.8);
        Ok(LigneHorloge {
            texte: texte.to_string(),
            layout,
            _format: format,
            w: metriques.width,
            h: metriques.height,
            base,
        })
    }
}

/// Une ligne + son ombre — approximation du `text-shadow 0 1px` du DOM.
fn dessiner_ligne(
    rt: &ID2D1RenderTarget,
    x: f32,
    y: f32,
    ligne: &LigneHorloge,
    ombre: &ID2D1Brush,
    brosse: &ID2D1Brush,
) {
    unsafe {
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y: y + 1.0 },
            &ligne.layout,
            ombre,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
        rt.DrawTextLayout(
            D2D_POINT_2F { x, y },
            &ligne.layout,
            brosse,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }
}

/// Ligne de base de la première ligne d'un layout (`None` si non mesurable).
fn base_ligne(layout: &IDWriteTextLayout) -> Option<f32> {
    let mut metriques = [DWRITE_LINE_METRICS::default()];
    let mut lignes = 0u32;
    unsafe { layout.GetLineMetrics(Some(&mut metriques), &mut lignes) }.ok()?;
    Some(metriques[0].baseline)
}

/// Cache des blocs d'horloge — déclaré ici pour que `mod.rs` ne connaisse
/// pas le détail (clé = [`cle_bloc`]).
pub(super) type BlocsHorloge = HashMap<u64, BlocHorloge>;

#[cfg(test)]
mod tests {
    use super::*;
    use sos_scene::{CadreConfig, ScenesModules};

    fn scene_json(modules: Option<ScenesModules>, cadre: Option<CadreConfig>) -> Scene {
        Scene {
            cadreWidget: cadre,
            modules,
            ..Default::default()
        }
    }

    fn module(json: &str) -> ModuleHud {
        serde_json::from_str(json).expect("module JSON")
    }

    #[test]
    fn format_date_jj_mm_aa() {
        assert_eq!(format_date(30, 10, 2026), "30 / 10 / 26");
        assert_eq!(format_date(4, 3, 1999), "04 / 03 / 99");
        assert_eq!(format_date(31, 12, 2000), "31 / 12 / 00");
    }

    #[test]
    fn format_heure_24h_et_secondes() {
        assert_eq!(format_heure(21, 45, 7, false, false), ("21 : 45".into(), None));
        assert_eq!(
            format_heure(21, 45, 7, true, false),
            ("21 : 45 : 07".into(), None)
        );
        assert_eq!(format_heure(0, 5, 0, false, false), ("00 : 05".into(), None));
    }

    #[test]
    fn format_heure_12h_avec_suffixe() {
        // `h % 12 || 12` du DOM : 0 → 12 AM, 12 → 12 PM.
        assert_eq!(format_heure(0, 5, 0, false, true), ("12 : 05".into(), Some("AM")));
        assert_eq!(format_heure(12, 0, 0, false, true), ("12 : 00".into(), Some("PM")));
        assert_eq!(format_heure(15, 30, 0, true, true), ("03 : 30 : 00".into(), Some("PM")));
        assert_eq!(format_heure(23, 59, 0, false, true), ("11 : 59".into(), Some("PM")));
    }

    #[test]
    fn taille_module_derive_ou_explicite() {
        // `m.taille || hauteur*0.42` — 0 est falsy en JS, borné à 8 px.
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":200,"hauteur":100}"#);
        assert_eq!(taille_module(&m), 42.0);
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":200,"hauteur":100,"taille":30}"#);
        assert_eq!(taille_module(&m), 30.0);
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":200,"hauteur":100,"taille":0}"#);
        assert_eq!(taille_module(&m), 42.0);
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":200,"hauteur":10}"#);
        assert_eq!(taille_module(&m), 8.0, "plancher 8 px");
    }

    #[test]
    fn opacite_module_defaut_et_bornes() {
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":10,"hauteur":10}"#);
        assert_eq!(opacite_module(&m), 0.45);
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":10,"hauteur":10,"opacite":2.0}"#);
        assert_eq!(opacite_module(&m), 1.0);
        let m = module(r#"{"actif":true,"x":0,"y":0,"largeur":10,"hauteur":10,"opacite":-1.0}"#);
        assert_eq!(opacite_module(&m), 0.0);
    }

    #[test]
    fn accent_module_prend_lextremite_lumineuse() {
        // Cadre noir → violet : la teinte visible est le violet.
        let cadre = CadreConfig {
            actif: true,
            couleur: "#000000".to_string(),
            couleurFin: "#8b5cf6".to_string(),
            ..CadreConfig::default()
        };
        assert_eq!(accent_module(&scene_json(None, Some(cadre))), [0x8b, 0x5c, 0xf6, 255]);
        // Les deux lumineuses : la plus claire gagne.
        let cadre = CadreConfig {
            actif: true,
            couleur: "#ffffff".to_string(),
            couleurFin: "#8b5cf6".to_string(),
            ..CadreConfig::default()
        };
        assert_eq!(accent_module(&scene_json(None, Some(cadre))), [255, 255, 255, 255]);
    }

    #[test]
    fn accent_module_repli_sur_invalide_ou_inactif() {
        // Cadre inactif → repli.
        let cadre = CadreConfig {
            actif: false,
            couleur: "#8b5cf6".to_string(),
            ..CadreConfig::default()
        };
        assert_eq!(accent_module(&scene_json(None, Some(cadre))), [0xe0, 0xe0, 0xe0, 255]);
        // couleurFin invalide → couleur seule ; couleur invalide → repli.
        let cadre = CadreConfig {
            actif: true,
            couleur: "#8b5cf6".to_string(),
            couleurFin: "violet".to_string(),
            ..CadreConfig::default()
        };
        assert_eq!(accent_module(&scene_json(None, Some(cadre))), [0x8b, 0x5c, 0xf6, 255]);
        let cadre = CadreConfig {
            actif: true,
            couleur: "#fff".to_string(), // 3 chiffres : invalide pour bpmHexValide
            couleurFin: "#8b5cf6".to_string(),
            ..CadreConfig::default()
        };
        assert_eq!(accent_module(&scene_json(None, Some(cadre))), [0xe0, 0xe0, 0xe0, 255]);
    }

    #[test]
    fn cle_bloc_suit_les_options_de_mise_en_page() {
        let cfg = HorlogeCfg {
            rect: (0.0, 0.0, 200.0, 60.0),
            z: Z_MODULES,
            police: Some("Rajdhani".to_string()),
            taille: 24.0,
            date: true,
            secondes: false,
            format12h: false,
            opacite: 0.45,
            accent: [0xe0, 0xe0, 0xe0, 255],
            cadre: None,
        };
        assert_eq!(cle_bloc(&cfg), cle_bloc(&cfg));
        let mut autre = cfg.clone();
        autre.rect = (50.0, 60.0, 300.0, 80.0);
        autre.opacite = 0.9;
        assert_eq!(cle_bloc(&cfg), cle_bloc(&autre), "rect/opacité hors clé");
        autre.taille = 30.0;
        assert_ne!(cle_bloc(&cfg), cle_bloc(&autre));
        autre.taille = 24.0;
        autre.secondes = true;
        assert_ne!(cle_bloc(&cfg), cle_bloc(&autre));
    }
}
