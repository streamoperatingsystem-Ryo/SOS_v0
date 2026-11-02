//! Module HUD « musique » (`scene.modules.musique`) — carte lecteur
//! (icône ♫ + titre + demandeur) + bloc temps (position, 5 points de
//! progression, reste).
//!
//! Troisième module porté en natif : l'état live arrive par
//! [`EtatMusique`] (events `musique:update` → `pousser_musique`, comme le
//! widget musique) — aucune reconfiguration de scène. La position avance
//! en local entre les snapshots (`EtatMusique::position_s`).
//!
//! Miroir de `.module.musique` (diffusion.html `renderModuleMusique`) :
//! - voile du module + deux cartes `rgba(10,10,14,opacite)` arrondies
//!   10 px (`.mm-carte` flex 1 + `.mm-temps` mesurée au contenu) ;
//! - `.mm-icone` : carré 2,2 em arrondi 8 px, voile blanc 8 %, « ♫ »
//!   en `--mod-accent` ; `.mm-titre` gras 0,95 em #f4f4f6 + `.mm-sous`
//!   0,68 em #c9c9d0 (« Demandé par @x », vide sans demandeur ; « Aucune
//!   musique »/« Lance la musique avec !rs » à l'arrêt) ;
//! - `.mm-temps` : 0,62 em, « MM:SS » courant + 5 points 0,45 em
//!   (allumés = accent, `round(pct × 5)`) + reste « MM:SS » —
//!   `afficherTemps === false` → bloc absent (`.sans-temps`) ;
//! - z 9000 (`Z_MODULES`), contenu rogné au rect + polygone du cadre
//!   opt-in (`m.cadre`).
//!
//! Limites assumées : `text-shadow`/`box-shadow` des points non portés
//! (comme l'horloge), ellipsis = rogné dur (pas de « … »),
//! `tabular-nums` non porté (chiffres déjà tabulaires en pratique).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Geometry, ID2D1PathGeometry, ID2D1SolidColorBrush,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::cadres::CadreCfg;

use super::musique::EtatMusique;
use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// Nombre de points de progression — `MM_NB_POINTS` (diffusion.html).
const NB_POINTS: usize = 5;

/// Configuration du module musique, extraite de `scene.modules.musique`.
#[derive(Debug, Clone, PartialEq)]
pub struct MusiqueModuleCfg {
    /// Rect du module en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (9000 — `#modules-root`).
    pub z: i32,
    /// `afficherTemps !== false` (défaut DOM : visible).
    pub afficher_temps: bool,
    /// Id de police `fonts.ts` ; `None` = Segoe UI.
    pub police: Option<String>,
    /// `--mod-fs` résolu en px.
    pub taille: f32,
    /// `--mod-opacite` du fond translucide (0–1).
    pub opacite: f32,
    /// Accent `--mod-accent` (icône + points allumés).
    pub accent: [u8; 4],
    /// Cadre de scène opt-in (`m.cadre`) : voile et contenu clippés au
    /// polygone, le trait étant tracé par le calque cadre au même z.
    pub cadre: Option<CadreCfg>,
}

/// Texte mis en page caché par (style, contenu) — contrat « texte statique ».
pub(super) struct LigneMusique {
    contenu: String,
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
}

/// Cache des mises en page (titre/sous/temps — recréés au changement réel).
pub(super) type BlocsMusiqueModule = HashMap<u64, LigneMusique>;

/// « MM:SS » zéro-paddé — `muFmtPad` (diffusion.html) : secondes tronquées.
fn mm_ss(sec: f64) -> String {
    let s = sec.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

impl DessinateurTexte {
    /// Dessine le module musique — dans une passe ouverte (clip + masque
    /// opt-in, même motif que `dessiner_horloge`/`dessiner_objectifs`).
    pub fn dessiner_musique_module(
        &mut self,
        cfg: &MusiqueModuleCfg,
        etat: &EtatMusique,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let (x, y, w, h) = cfg.rect;
        let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
        if w <= 0.0 || h <= 0.0 {
            return Ok(());
        }
        let zone = D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h };
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
            let layer = unsafe { self.cible_rt.CreateLayer(None) }.map_err(|e| {
                ErreurTexte::Gpu(format!("CreateLayer module musique : {}", e.message()))
            })?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_musique_module_interne(cfg, etat, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    /// Voile + carte + bloc temps — sous les clips déjà posés.
    fn dessiner_musique_module_interne(
        &mut self,
        cfg: &MusiqueModuleCfg,
        etat: &EtatMusique,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        let fs = cfg.taille;
        let fond = self.brosse_mu(D2D1_COLOR_F {
            r: 10.0 / 255.0,
            g: 10.0 / 255.0,
            b: 14.0 / 255.0,
            a: cfg.opacite,
        })?;
        // Voile du module (`.mod-fond`), puis chaque carte a le sien.
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: *zone, radiusX: 10.0, radiusY: 10.0 },
                &fond,
            )
        };

        // Bloc temps : largeur mesurée au contenu, plaqué à droite.
        let temps_fs = 0.62 * fs;
        let piste = etat.piste();
        let pos_s = etat.position_s();
        let reste = etat
            .duree_s()
            .map(|d| (d - pos_s).max(0.0))
            .unwrap_or(0.0);
        let pos_txt = if piste.is_some() { mm_ss(pos_s) } else { "00:00".to_string() };
        let rest_txt = if piste.is_some() && etat.duree_s().is_some() {
            mm_ss(reste)
        } else {
            "00:00".to_string()
        };
        let style_temps = StyleTexte {
            police: cfg.police.clone().unwrap_or_else(|| "Segoe UI".to_string()),
            taille: temps_fs,
            espacement: 0.05 * temps_fs,
            couleur: [0xc9, 0xc9, 0xd0, 255],
            ..StyleTexte::default()
        };
        let (pos_l, pos_w) = self.ligne_mm(&pos_txt, &style_temps)?;
        let (rest_l, rest_w) = self.ligne_mm(&rest_txt, &style_temps)?;
        let pts_pas = (0.45 + 0.3) * temps_fs;
        let pts_w = NB_POINTS as f32 * pts_pas - 0.3 * temps_fs;
        let temps_w = if cfg.afficher_temps {
            2.0 * 0.7 * temps_fs + pos_w + 0.5 * temps_fs + pts_w + 0.5 * temps_fs + rest_w
        } else {
            0.0
        };

        // `.mm-carte` : de gauche au bloc temps (ou tout le module).
        let gap = 0.7 * fs;
        let carte = D2D_RECT_F {
            left: zone.left,
            top: zone.top,
            right: if cfg.afficher_temps { zone.right - temps_w - gap } else { zone.right },
            bottom: zone.bottom,
        };
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: carte, radiusX: 10.0, radiusY: 10.0 },
                &fond,
            )
        };

        // `.mm-icone` : carré 2,2 em, voile blanc 8 %, « ♫ » accent centré.
        let ic = 2.2 * fs;
        let ic_x = carte.left + 0.35 * fs;
        let ic_y = carte.top + (carte.bottom - carte.top - ic) / 2.0;
        let voile_ic = self.brosse_mu(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 })?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: ic_x, top: ic_y, right: ic_x + ic, bottom: ic_y + ic },
                    radiusX: 8.0,
                    radiusY: 8.0,
                },
                &voile_ic,
            )
        };
        let style_ic = StyleTexte {
            police: cfg.police.clone().unwrap_or_else(|| "Segoe UI".to_string()),
            taille: fs,
            couleur: cfg.accent,
            ..StyleTexte::default()
        };
        let (ic_l, ic_w, ic_h) = self.ligne_mm_h("♫", &style_ic)?;
        let b_accent = self.brosse_mu(couleur_d2d(cfg.accent))?;
        unsafe {
            self.cible_rt.DrawTextLayout(
                D2D_POINT_2F {
                    x: ic_x + (ic - ic_w) / 2.0,
                    y: ic_y + (ic - ic_h) / 2.0,
                },
                &ic_l,
                &b_accent,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        };

        // `.mm-txt` : titre + sous, empilés, centrés verticalement.
        let titre = piste.map(|p| p.titre.as_str()).unwrap_or("Aucune musique");
        let sous = match piste {
            Some(p) if !p.demandeur.is_empty() => format!("Demandé par @{}", p.demandeur),
            Some(_) => String::new(),
            None => "Lance la musique avec !rs".to_string(),
        };
        let style_titre = StyleTexte {
            taille: 0.95 * fs,
            gras: true,
            couleur: [0xf4, 0xf4, 0xf6, 255],
            ..style_ic.clone()
        };
        let style_sous = StyleTexte {
            taille: 0.68 * fs,
            gras: false,
            couleur: [0xc9, 0xc9, 0xd0, 255],
            ..style_ic.clone()
        };
        let (t_l, _t_w, t_h) = self.ligne_mm_h(titre, &style_titre)?;
        let (s_l, _s_w, s_h) = if sous.is_empty() {
            (None, 0.0, 0.0)
        } else {
            let (l, w, h) = self.ligne_mm_h(&sous, &style_sous)?;
            (Some(l), w, h)
        };
        let txt_x = ic_x + ic + 0.6 * fs;
        let bloc_h = t_h + 0.15 * fs + s_h;
        let mut ty = carte.top + (carte.bottom - carte.top - bloc_h) / 2.0;
        let b_titre = self.brosse_mu(couleur_d2d([0xf4, 0xf4, 0xf6, 255]))?;
        unsafe {
            self.cible_rt.DrawTextLayout(
                D2D_POINT_2F { x: txt_x, y: ty },
                &t_l,
                &b_titre,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        };
        if let Some(s_l) = s_l {
            ty += t_h + 0.15 * fs;
            let b_sous = self.brosse_mu(couleur_d2d([0xc9, 0xc9, 0xd0, 255]))?;
            unsafe {
                self.cible_rt.DrawTextLayout(
                    D2D_POINT_2F { x: txt_x, y: ty },
                    &s_l,
                    &b_sous,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            };
        }

        // `.mm-temps` : carte + pos + 5 points + reste.
        if cfg.afficher_temps {
            let temps = D2D_RECT_F { left: zone.right - temps_w, right: zone.right, ..*zone };
            unsafe {
                self.cible_rt.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT { rect: temps, radiusX: 10.0, radiusY: 10.0 },
                    &fond,
                )
            };
            let mut tx = temps.left + 0.7 * temps_fs;
            let b_txt = self.brosse_mu(couleur_d2d([0xc9, 0xc9, 0xd0, 255]))?;
            let (_, _w, pos_h) = self.ligne_mm_h(&pos_txt, &style_temps)?;
            let ty = temps.top + (temps.bottom - temps.top - pos_h) / 2.0;
            unsafe {
                self.cible_rt.DrawTextLayout(
                    D2D_POINT_2F { x: tx, y: ty },
                    &pos_l,
                    &b_txt,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            };
            tx += pos_w + 0.5 * temps_fs;
            // Points : rond 0,45 em — allumés accent, éteints blanc 25 %.
            let pct = match (piste, etat.duree_s()) {
                (Some(_), Some(d)) if d > 0.0 => (pos_s / d).clamp(0.0, 1.0),
                _ => 0.0,
            };
            let allumes = (pct * NB_POINTS as f64).round() as usize;
            let rond = 0.45 * temps_fs / 2.0;
            let cy = temps.top + (temps.bottom - temps.top) / 2.0;
            let b_off = self.brosse_mu(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.25 })?;
            for i in 0..NB_POINTS {
                let cx = tx + rond + i as f32 * pts_pas;
                let ellipse = windows::Win32::Graphics::Direct2D::D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: rond,
                    radiusY: rond,
                };
                let b = if i < allumes { &b_accent } else { &b_off };
                unsafe { self.cible_rt.FillEllipse(&ellipse, b) };
            }
            tx += pts_w + 0.5 * temps_fs;
            unsafe {
                self.cible_rt.DrawTextLayout(
                    D2D_POINT_2F { x: tx, y: ty },
                    &rest_l,
                    &b_txt,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            };
        }
        Ok(())
    }

    /// Brosse D2D à la demande.
    fn brosse_mu(&self, c: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush, ErreurTexte> {
        unsafe { self.cible_rt.CreateSolidColorBrush(&c, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse module musique : {}", e.message())))
    }

    /// Layout caché d'un texte, renvoyé cloné + largeur.
    fn ligne_mm(
        &mut self,
        texte: &str,
        style: &StyleTexte,
    ) -> Result<(IDWriteTextLayout, f32), ErreurTexte> {
        let (l, w, _h) = self.ligne_mm_h(texte, style)?;
        Ok((l, w))
    }

    /// Layout caché d'un texte, renvoyé cloné + largeur + hauteur.
    fn ligne_mm_h(
        &mut self,
        texte: &str,
        style: &StyleTexte,
    ) -> Result<(IDWriteTextLayout, f32, f32), ErreurTexte> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        style.police.hash(&mut h);
        style.taille.to_bits().hash(&mut h);
        style.gras.hash(&mut h);
        style.espacement.to_bits().hash(&mut h);
        style.couleur.hash(&mut h);
        texte.hash(&mut h);
        let cle = h.finish();
        if self.blocs_musique_module.len() > 64 {
            self.blocs_musique_module.clear();
        }
        if let Some(l) = self
            .blocs_musique_module
            .get(&cle)
            .filter(|l| l.contenu == texte)
        {
            return Ok((l.layout.clone(), l.w, l.h));
        }
        let format = self.format(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        let layout = unsafe { self.dwrite.CreateTextLayout(&contenu, &format, 8192.0, 2048.0) }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("CreateTextLayout module musique : {}", e.message()))
            })?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap module musique : {}", e.message())))?;
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques module musique : {}", e.message())))?;
        let retour = (layout.clone(), metriques.width, metriques.height);
        self.blocs_musique_module.insert(
            cle,
            LigneMusique {
                contenu: texte.to_string(),
                layout,
                _format: format,
                w: metriques.width,
                h: metriques.height,
            },
        );
        Ok(retour)
    }
}
