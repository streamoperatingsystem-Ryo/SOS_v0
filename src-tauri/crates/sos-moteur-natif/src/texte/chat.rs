//! Widgets chat : mise en page DirectWrite des messages (bloc mis en cache
//! par version du buffer, pseudo en gras/coloré) puis dessin des lignes
//! zébrées (compact) ou des bulles — miroir des règles CSS de
//! `diffusion.html`.
//!
//! Extrait de `texte.rs` (découpage 2026-10-29) : ces fonctions restent
//! des méthodes de `DessinateurTexte` — l'`impl` est scindé, le module
//! enfant voyant les champs privés du parent.

use std::hash::{Hash, Hasher};

use windows::core::{Interface, IUnknown, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_WRAP,
};

use crate::chat::{ChatCfg, FileChat};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte};

/// Un message mis en page : layout DirectWrite + hauteur rendue.
struct MessageRendu {
    layout: IDWriteTextLayout,
    hauteur: f32,
    /// `nth-child` impair (index pair) — zébrage de `diffusion.html`.
    pair: bool,
    /// Brosse de la couleur du pseudo : le layout la référence.
    _brosse: Option<ID2D1SolidColorBrush>,
}

/// Bloc de chat complet (tous les messages d'un widget), mis en page une fois
/// par version du buffer.
///
/// La clé de cache ne contient **pas** la version : le bloc est réutilisé
/// tant que la configuration du widget ne change pas, et reconstruit quand
/// `version` diffère. La table reste donc bornée par le nombre de widgets.
pub(super) struct BlocChat {
    /// Version du buffer au moment de la mise en page.
    version: u64,
    /// Le layout DirectWrite ne possède pas le format : il doit vivre ici.
    _format: IDWriteTextFormat,
    messages: Vec<MessageRendu>,
    /// Hauteur totale, interlignes de bulles comprises.
    total: f32,
}

impl DessinateurTexte {
    /// Dessine les widgets chat — dans une passe ouverte.
    ///
    /// Chaque widget a son bloc mis en page (cache par version du buffer) :
    /// un nouveau message ne reconfigure **rien**, il invalide le bloc.
    /// Le plus récent est en bas, on pousse vers le haut ; ce qui dépasse est
    /// rogné au rect du widget (défilement automatique, pas de scroll manuel).
    pub fn dessiner_chat(&mut self, cfgs: &[ChatCfg], file: &FileChat) -> Result<(), ErreurTexte> {
        if cfgs.is_empty() || file.version() == 0 {
            return Ok(());
        }
        let blanc = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 255]), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse chat : {}", e.message())))?;
        let clair = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.07,
        };
        let fond = unsafe { self.cible_rt.CreateSolidColorBrush(&clair, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fond chat : {}", e.message())))?;
        let noir = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d([0, 0, 0, 255]), None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse fond chat : {}", e.message())))?;

        for cfg in cfgs {
            let g = geometrie_chat(cfg);
            let cle = cle_bloc(cfg, &g);
            // `is_none_or` est 1.82 : MSRV du dépôt = 1.77.
            let perime = match self.blocs.get(&cle) {
                Some(bloc) => bloc.version != file.version(),
                None => true,
            };
            if perime {
                self.construire_bloc(cfg, file, &g, cle)?;
            }
            let Some(bloc) = self.blocs.get(&cle) else {
                continue;
            };
            let (x, y, w, h) = cfg.rect;
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
            let clip = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };

            let mut curseur = y + h - g.pad_bas - bloc.total;
            for message in &bloc.messages {
                let rect_fond = D2D_RECT_F {
                    left: x + g.pad_x,
                    top: curseur,
                    right: x + w - g.pad_x,
                    bottom: curseur + message.hauteur,
                };
                // Bulles : fond uniforme clair (zébrage off, cf. CSS) ;
                // compact : une ligne sur deux en noir plein.
                let brosse = if cfg.bulles || message.pair {
                    &fond
                } else {
                    &noir
                };
                unsafe { self.cible_rt.FillRectangle(&rect_fond, brosse) };
                let origine = D2D_POINT_2F {
                    x: x + g.pad_x + g.marge_gauche,
                    y: curseur + g.marge_haut,
                };
                unsafe {
                    self.cible_rt.DrawTextLayout(
                        origine,
                        &message.layout,
                        &blanc,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
                curseur += message.hauteur + g.gap;
            }
            unsafe { self.cible_rt.PopAxisAlignedClip() };
        }
        Ok(())
    }

    /// Met en page tous les messages d'un widget (une fois par version).
    fn construire_bloc(
        &mut self,
        cfg: &ChatCfg,
        file: &FileChat,
        g: &ChatGeometrie,
        cle: u64,
    ) -> Result<(), ErreurTexte> {
        let format = self.format_chat(g.police)?;
        let mut messages = Vec::new();
        let mut total = 0.0f32;
        for (index, message) in file.pour_filtre(&cfg.filtre).enumerate() {
            let contenu = if message.pseudo.is_empty() {
                message.texte.clone()
            } else {
                format!("{}: {}", message.pseudo, message.texte)
            };
            let utf16: Vec<u16> = contenu.encode_utf16().collect();
            let layout = unsafe {
                self.dwrite
                    .CreateTextLayout(&utf16, &format, g.largeur_texte, 100_000.0)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout chat : {}", e.message())))?;
            unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP) }
                .map_err(|e| ErreurTexte::Systeme(format!("wrap chat : {}", e.message())))?;

            let longueur = message.pseudo.encode_utf16().count() as u32;
            let mut brosse = None;
            if longueur > 0 {
                let plage = DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: longueur,
                };
                unsafe { layout.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("gras chat : {}", e.message())))?;
                let couleur = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(message.couleur_pseudo()), None)
                }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse pseudo : {}", e.message())))?;
                let inconnu: IUnknown = couleur
                    .cast()
                    .map_err(|e| ErreurTexte::Systeme(format!("IUnknown pseudo : {}", e.message())))?;
                unsafe { layout.SetDrawingEffect(&inconnu, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("couleur pseudo : {}", e.message())))?;
                brosse = Some(couleur);
            }

            let mut metriques = DWRITE_TEXT_METRICS::default();
            unsafe { layout.GetMetrics(&mut metriques) }
                .map_err(|e| ErreurTexte::Systeme(format!("métriques chat : {}", e.message())))?;
            let hauteur = metriques.height.ceil() + g.marge_haut + g.marge_bas;
            total += hauteur;
            messages.push(MessageRendu {
                layout,
                hauteur,
                pair: index % 2 == 0,
                _brosse: brosse,
            });
        }
        total += g.gap * messages.len().saturating_sub(1) as f32;
        self.blocs.insert(
            cle,
            BlocChat {
                version: file.version(),
                _format: format,
                messages,
                total,
            },
        );
        Ok(())
    }

    /// `IDWriteTextFormat` du chat : police système, interligne natif.
    fn format_chat(&self, police: f32) -> Result<IDWriteTextFormat, ErreurTexte> {
        let nom: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                police.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat chat : {}", e.message())))
    }
}

/// Métriques de mise en page d'un widget chat (px canvas).
///
/// Miroir des règles CSS de `diffusion.html` :
/// - `compact` : `padding: 4px`, `padding-top: 5%` (pourcentage de la
///   **largeur** en CSS), pas d'interligne, message `4px 6px 4px 10px` ;
/// - `bulles` : `padding: 1.4em`, `gap: 0.6em`, message en `1.3em` avec
///   `padding: 6px 10px 6px 12px`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatGeometrie {
    pub police: f32,
    pub pad_x: f32,
    pub pad_haut: f32,
    pub pad_bas: f32,
    pub gap: f32,
    pub largeur_texte: f32,
    pub marge_gauche: f32,
    pub marge_droite: f32,
    pub marge_haut: f32,
    pub marge_bas: f32,
}

/// Calcule les métriques d'un widget chat (pure, testable sans GPU).
pub fn geometrie_chat(cfg: &ChatCfg) -> ChatGeometrie {
    let (_, _, largeur, _) = cfg.rect;
    let w = largeur as f32;
    let (police, pad_x, pad_haut, pad_bas, gap, mg, md, mh, mb) = if cfg.bulles {
        let pad = 1.4 * cfg.taille_police;
        (
            1.3 * cfg.taille_police,
            pad,
            pad,
            pad,
            0.6 * cfg.taille_police,
            12.0,
            10.0,
            6.0,
            6.0,
        )
    } else {
        (
            cfg.taille_police,
            4.0,
            0.05 * w,
            4.0,
            0.0,
            10.0,
            6.0,
            4.0,
            4.0,
        )
    };
    ChatGeometrie {
        police,
        pad_x,
        pad_haut,
        pad_bas,
        gap,
        largeur_texte: (w - 2.0 * pad_x - mg - md).max(1.0),
        marge_gauche: mg,
        marge_droite: md,
        marge_haut: mh,
        marge_bas: mb,
    }
}

/// Clé de cache d'un bloc de chat : tout ce qui change la **mise en page**
/// (la version du buffer est comparée séparément, pour borner la table).
fn cle_bloc(cfg: &ChatCfg, g: &ChatGeometrie) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.filtre.hash(&mut h);
    cfg.bulles.hash(&mut h);
    g.police.to_bits().hash(&mut h);
    g.largeur_texte.to_bits().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(bulles: bool) -> ChatCfg {
        let mut cfg = ChatCfg::depuis_widget((0.0, 0.0, 692.0, 88.0), 14);
        cfg.taille_police = 11.0;
        cfg.bulles = bulles;
        cfg
    }

    #[test]
    fn chat_compact_reprend_le_padding_css() {
        let g = geometrie_chat(&chat(false));
        assert_eq!(g.police, 11.0);
        assert_eq!(g.pad_x, 4.0);
        // `padding-top: 5%` en CSS = 5 % de la LARGEUR du bloc.
        assert!((g.pad_haut - 34.6).abs() < 1e-3);
        assert_eq!(g.gap, 0.0);
        // 692 − 2×4 − 10 (gauche) − 6 (droite)
        assert!((g.largeur_texte - 668.0).abs() < 1e-3);
    }

    #[test]
    fn chat_bulles_grandit_police_padding_et_interligne() {
        let g = geometrie_chat(&chat(true));
        assert!((g.police - 14.3).abs() < 1e-4, "1.3em");
        assert!((g.pad_x - 15.4).abs() < 1e-4, "1.4em");
        assert!((g.pad_haut - 15.4).abs() < 1e-4);
        assert!((g.gap - 6.6).abs() < 1e-4, "0.6em");
        // 692 − 2×15,4 − 12 − 10
        assert!((g.largeur_texte - 639.2).abs() < 1e-3);
    }

    #[test]
    fn cle_de_bloc_ne_depend_que_de_la_mise_en_page() {
        // La version du buffer est comparée à part : deux appels identiques
        // doivent produire la même clé, sinon la table grossirait à chaque
        // message.
        let cfg = chat(false);
        let g = geometrie_chat(&cfg);
        assert_eq!(cle_bloc(&cfg, &g), cle_bloc(&cfg, &g));
        let mut autre = chat(true);
        autre.filtre = "kick".to_string();
        assert_ne!(cle_bloc(&autre, &geometrie_chat(&autre)), cle_bloc(&cfg, &g));
    }
}
