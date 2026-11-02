//! Module HUD « objectifs » (`scene.modules.objectifs`) — titre + lignes
//! de progression (label, compteur live / cible, barre accent).
//!
//! Deuxième module porté en natif (après l'horloge) : les compteurs
//! arrivent par [`EtatGoals`] (frames `communaute`/`viewers`/`goal-twitch`
//! du pont app, comme le widget goal) — aucune reconfiguration de scène.
//!
//! Miroir de `.module.objectifs` (ModuleHud.svelte + diffusion.html) :
//! - fond arrondi 10 px `rgba(10,10,14,opacite)` (défaut 0,45) ;
//! - `.mo-titre` : 0,6 em, uppercase, espacement 0,3 em, #b9b9c2 ;
//! - `.mo-ligne` : label gras uppercase 0,72 em #f4f4f6 à gauche +
//!   « actuel / cible » 0,72 em à droite (actuel en `--mod-accent`, le
//!   reste #c9c9d0 — actuel absent → « N/A », cible 0 → « — ») ;
//! - `.mo-bar`/`.mo-fill` : piste 2 px rgba(255,255,255,.22), remplissage
//!   accent borné 0-100 % (actuel/cible — `pct` du DOM) ;
//! - z 9000 (`Z_MODULES`), contenu rogné au rect + polygone du cadre
//!   opt-in (`m.cadre`).
//!
//! Limites assumées : `text-shadow` non porté (comme l'horloge), pas de
//! halo sur la barre (`box-shadow` ≈ remplissage plein), label lié
//! (`goalId`) = `description` du Creator Goal ou repli `goal_type`.

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

use super::goal::EtatGoals;
use super::{couleur_d2d, DessinateurTexte, ErreurTexte, StyleTexte};

/// Une ligne du bloc objectifs — `LigneObjectif` résolue (défauts appliqués).
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectifLigneCfg {
    /// Plateforme de la métrique (`"twitch"` par défaut).
    pub plateforme: String,
    /// Métrique (`"followers"` par défaut).
    pub metrique: String,
    /// Cible manuelle (0 = non définie → « — »).
    pub cible: u64,
    /// Libellé libre du streamer (`None` → repli automatique).
    pub label: Option<String>,
    /// Creator Goal Twitch lié : valeur/cible lues dans `EtatGoals`.
    pub goal_id: Option<String>,
}

/// Configuration du module objectifs, extraite de `scene.modules.objectifs`.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectifCfg {
    /// Rect du module en px canvas.
    pub rect: (f64, f64, f64, f64),
    /// Ordre de composition (9000 — `#modules-root`).
    pub z: i32,
    /// Titre optionnel du bloc (`.mo-titre` ; `None` = absent).
    pub titre: Option<String>,
    /// Lignes dans l'ordre persistant.
    pub lignes: Vec<ObjectifLigneCfg>,
    /// Id de police `fonts.ts` ; `None` = Segoe UI.
    pub police: Option<String>,
    /// `--mod-fs` résolu en px.
    pub taille: f32,
    /// `--mod-opacite` du fond translucide (0–1).
    pub opacite: f32,
    /// Accent `--mod-accent` (compteur + barre).
    pub accent: [u8; 4],
    /// Cadre de scène opt-in (`m.cadre`) : voile et contenu clippés au
    /// polygone, le trait étant tracé par le calque cadre au même z.
    pub cadre: Option<CadreCfg>,
}

/// Texte mis en page caché par (style, contenu) — contrat « texte statique ».
pub(super) struct LigneObj {
    contenu: String,
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
}

/// Cache des mises en page : les valeurs changent (compteurs live), la
/// clé inclut le contenu → recréation seulement au changement réel.
pub(super) type BlocsObjectifs = HashMap<u64, LigneObj>;

fn cle_ligne(sig: u64, contenu: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sig.hash(&mut h);
    contenu.hash(&mut h);
    h.finish()
}

/// (plateforme, métrique, cible, actuel, cible_effective, label) résolus
/// d'une ligne — miroir de `ligneValeurs` (ModuleHud.svelte).
fn resoudre(l: &ObjectifLigneCfg, etat: &EtatGoals) -> (String, Option<u64>, u64) {
    if let Some(id) = &l.goal_id {
        if let Some(g) = etat.goal_twitch(id) {
            let label = if g.description.is_empty() {
                g.goal_type.clone()
            } else {
                g.description.clone()
            };
            return (label, Some(g.actuel), g.cible);
        }
    }
    let label = l.label.clone().unwrap_or_else(|| l.metrique.clone());
    (label, etat.valeur(&l.plateforme, &l.metrique), l.cible)
}

impl DessinateurTexte {
    /// Dessine le module objectifs — dans une passe ouverte (clip + masque
    /// opt-in, même motif que `dessiner_horloge`).
    pub fn dessiner_objectifs(
        &mut self,
        cfg: &ObjectifCfg,
        etat: &EtatGoals,
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
            let layer = unsafe { self.cible_rt.CreateLayer(None) }
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer objectifs : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_objectifs_interne(cfg, etat, &zone);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    fn dessiner_objectifs_interne(
        &mut self,
        cfg: &ObjectifCfg,
        etat: &EtatGoals,
        zone: &D2D_RECT_F,
    ) -> Result<(), ErreurTexte> {
        let fond = self.brosse(D2D1_COLOR_F {
            r: 10.0 / 255.0,
            g: 10.0 / 255.0,
            b: 14.0 / 255.0,
            a: cfg.opacite,
        })?;
        unsafe {
            self.cible_rt.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: *zone, radiusX: 10.0, radiusY: 10.0 },
                &fond,
            )
        };

        let fs = cfg.taille;
        let pad_x = 0.9 * fs;
        let mut cur_y = zone.top + (zone.bottom - zone.top - self.hauteur_bloc(cfg, fs)) / 2.0;
        if cur_y < zone.top {
            cur_y = zone.top;
        }
        let gauche = zone.left + pad_x;
        let droite = zone.right - pad_x;

        if let Some(titre) = cfg.titre.as_ref().filter(|t| !t.is_empty()) {
            let style = StyleTexte {
                police: cfg.police.clone().unwrap_or_else(|| "Segoe UI".to_string()),
                taille: 0.6 * fs,
                espacement: 0.3 * 0.6 * fs,
                couleur: [0xb9, 0xb9, 0xc2, 255],
                ..StyleTexte::default()
            };
            self.texte_ligne_obj(titre.to_uppercase().as_str(), &style, gauche, cur_y, droite - gauche)?;
            cur_y += 0.6 * fs * 1.6;
        }

        for l in &cfg.lignes {
            let (label, actuel, cible) = resoudre(l, etat);
            let style_label = StyleTexte {
                police: cfg.police.clone().unwrap_or_else(|| "Segoe UI".to_string()),
                taille: 0.72 * fs,
                gras: true,
                espacement: 0.16 * 0.72 * fs,
                couleur: [0xf4, 0xf4, 0xf6, 255],
                ..StyleTexte::default()
            };
            let style_val = StyleTexte {
                gras: false,
                espacement: 0.08 * 0.72 * fs,
                couleur: [0xc9, 0xc9, 0xd0, 255],
                ..style_label.clone()
            };
            let lig_h = 0.72 * fs * 1.35;
            self.texte_ligne_obj(&label.to_uppercase(), &style_label, gauche, cur_y, droite - gauche)?;
            let val = format!(
                "{} / {}",
                actuel.map(|v| v.to_string()).unwrap_or_else(|| "N/A".to_string()),
                if cible > 0 { cible.to_string() } else { "—".to_string() }
            );
            // Valeur alignée à droite sur la même ligne.
            let (lval, lval_w) = self.layout_obj(&val, &style_val)?;
            let b_val = self.brosse(couleur_d2d(style_val.couleur))?;
            unsafe {
                self.cible_rt.DrawTextLayout(
                    D2D_POINT_2F { x: droite - lval_w, y: cur_y },
                    &lval,
                    &b_val,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            };
            cur_y += lig_h;
            // Barre : piste + remplissage accent (pct borné, même que le DOM).
            let piste = D2D_RECT_F { left: gauche, top: cur_y, right: droite, bottom: cur_y + 2.0 };
            unsafe {
                self.cible_rt.FillRectangle(
                    &piste,
                    &self.brosse(D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.22 })?,
                )
            };
            if cible > 0 {
                if let Some(a) = actuel {
                    let pct = ((a as f32 / cible as f32) * 100.0).min(100.0) / 100.0;
                    let fill = D2D_RECT_F { right: gauche + (droite - gauche) * pct, ..piste };
                    unsafe {
                        self.cible_rt
                            .FillRectangle(&fill, &self.brosse(couleur_d2d(cfg.accent))?)
                    };
                }
            }
            cur_y += 2.0 + 0.55 * fs;
        }
        Ok(())
    }

    /// Hauteur totale du contenu (titre + lignes) pour le centrage vertical.
    fn hauteur_bloc(&self, cfg: &ObjectifCfg, fs: f32) -> f32 {
        let mut h = 0.0f32;
        if cfg.titre.as_ref().is_some_and(|t| !t.is_empty()) {
            h += 0.6 * fs * 1.6;
        }
        h + cfg.lignes.len() as f32 * (0.72 * fs * 1.35 + 2.0 + 0.55 * fs)
    }

    /// Brosse D2D à la demande (courte durée de vie — le rendu objectifs
    /// crée peu de brosses par frame).
    fn brosse(&self, c: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush, ErreurTexte> {
        unsafe { self.cible_rt.CreateSolidColorBrush(&c, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse objectifs : {}", e.message())))
    }

    /// Layout caché d'une ligne de texte, renvoyé **cloné** (handle COM —
    /// copie comptée, coupe le borrow du cache) + sa largeur mesurée.
    fn layout_obj(
        &mut self,
        texte: &str,
        style: &StyleTexte,
    ) -> Result<(IDWriteTextLayout, f32), ErreurTexte> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        style.police.hash(&mut h);
        style.taille.to_bits().hash(&mut h);
        style.gras.hash(&mut h);
        style.espacement.to_bits().hash(&mut h);
        style.couleur.hash(&mut h);
        let cle = cle_ligne(h.finish(), texte);
        if self.blocs_objectifs.len() > 64 {
            self.blocs_objectifs.clear();
        }
        if let Some(l) = self
            .blocs_objectifs
            .get(&cle)
            .filter(|l| l.contenu == texte)
        {
            return Ok((l.layout.clone(), l.w));
        }
        let format = self.format(style)?;
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        let layout = unsafe { self.dwrite.CreateTextLayout(&contenu, &format, 8192.0, 2048.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout objectifs : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap objectifs : {}", e.message())))?;
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques objectifs : {}", e.message())))?;
        let retour = (layout.clone(), metriques.width);
        self.blocs_objectifs.insert(
            cle,
            LigneObj {
                contenu: texte.to_string(),
                layout,
                _format: format,
                w: metriques.width,
            },
        );
        Ok(retour)
    }

    /// Dessine une ligne de texte rognée à `largeur` (hauteur ~1,35 em).
    fn texte_ligne_obj(
        &mut self,
        texte: &str,
        style: &StyleTexte,
        x: f32,
        y: f32,
        _largeur: f32,
    ) -> Result<(), ErreurTexte> {
        let (l, _w) = self.layout_obj(texte, style)?;
        let b = self.brosse(couleur_d2d(style.couleur))?;
        unsafe {
            self.cible_rt.DrawTextLayout(
                D2D_POINT_2F { x, y },
                &l,
                &b,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        };
        Ok(())
    }
}
