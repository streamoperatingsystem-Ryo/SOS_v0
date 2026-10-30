//! Barre « viewers » en tête du widget chat — œil + pastilles plateforme.
//!
//! Miroir de `.chat-viewers` + `fillViewersBar` (diffusion.html) :
//! - ligne centrée en haut du widget chat (`flex-shrink: 0`), `font-size` =
//!   `taillePolice`, gras #fff, gap 0,8 em, padding 0,3 em, liseré
//!   `rgba(255,255,255,0.12)` en bas de barre ;
//! - `unifie` → une pastille par plateforme **live**, ordre figé
//!   `[twitch, youtube, kick, tiktok]` (`PLATS_VIEWERS`) ; filtré → seulement
//!   la plateforme du filtre (lookup exact, comme `viewersParPlateforme[p]`) ;
//! - pastille = logo vectoriel (Twitch 16, Kick 24 — mêmes `d` que
//!   `logoPlateforme`, partagés avec `goal.rs`) ou badge « YT » rouge /
//!   « TT » noir (0,55 em, `line-height: 1.4`, padding 0 0,2 rem) + count
//!   compact « 1,2k » (`formatViewers`) ;
//! - **`:empty → display:none`** : aucune plateforme live → hauteur 0 —
//!   la zone des messages garde tout le rect (zéro régression de layout) ;
//! - **masquée en `bulles`** (`.chat-bulles .chat-viewers` : chrome du
//!   widget, pas un message).
//!
//! Données : [`EtatGoals`] — les frames `viewers` sont déjà décodées par
//! `decoder_frame_goal` côté app (aucun canal nouveau). La ligne
//! `.chat-musique` (piste en cours, sous la barre) relève du widget musique
//! — hors de ce lot.
//!
//! Limites assumées :
//! - l'œil `👁` est dessiné en Segoe UI Emoji (glyphe couleur quand la
//!   police le permet) — `opacity: 0.8` du DOM n'est pas reproduite pour
//!   un glyphe couleur (pleine opacité, écart visuel minime) ;
//! - `font-family: system-ui` ≈ Segoe UI (même repli que le goal).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use windows::core::PCWSTR;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, IDWriteTextLayout, DWRITE_TEXT_METRICS,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::chat::{ChatCfg, FILTRE_UNIFIE};

use super::goal::{couleur_logo, logo_plateforme, viewbox_logo, LogoGoal};
use super::{couleur_d2d, DessinateurTexte, ErreurTexte, EtatGoals, StyleTexte};

/// Ordre d'affichage des pastilles — `PLATS_VIEWERS` de `diffusion.html`.
pub const PLATS_VIEWERS: [&str; 4] = ["twitch", "youtube", "kick", "tiktok"];

/// `formatViewers` (diffusion.html) : `1234` → « 1,2k » — `toFixed(1)`
/// arrondi, `,0` terminal tronqué, `.` → `,` ; sous 1000 → nombre brut.
pub fn format_viewers(n: u64) -> String {
    if n >= 1000 {
        let k = format!("{:.1}", n as f64 / 1000.0);
        let k = k.strip_suffix(".0").unwrap_or(&k);
        return format!("{}k", k.replace('.', ","));
    }
    n.to_string()
}

/// Plateformes éligibles à la pastille — `plats` de `fillViewersBar` :
/// `unifie` → les quatre, sinon le filtre lui-même (une plateforme inconnue
/// n'aura jamais de count → jamais affichée, comme le DOM).
pub fn plateformes_barre(filtre: &str) -> Vec<&str> {
    if filtre == FILTRE_UNIFIE {
        PLATS_VIEWERS.to_vec()
    } else {
        vec![filtre]
    }
}

/// Pastilles live d'un widget : plateformes éligibles dont le count est un
/// nombre (`typeof n === "number"` du DOM — `None` = hors-ligne/inconnu).
pub fn pastilles_live<'a>(etat: &EtatGoals, plats: &[&'a str]) -> Vec<(&'a str, u64)> {
    plats
        .iter()
        .filter_map(|&p| etat.valeur(p, "viewers").map(|n| (p, n)))
        .collect()
}

/// Fond du badge texte — `.plat-badge.youtube` rouge, `.plat-badge.tiktok`
/// noir (les plateformes à logo n'ont pas de badge).
fn fond_badge(plateforme: &str) -> [u8; 4] {
    match plateforme {
        "youtube" => [0xff, 0, 0, 255],
        _ => [0, 0, 0, 255],
    }
}

/// Texte du badge — `logoPlateforme` : youtube « YT », tiktok « TT ».
fn badge_viewers(plateforme: &str) -> Option<&'static str> {
    match plateforme {
        "youtube" => Some("YT"),
        "tiktok" => Some("TT"),
        _ => None,
    }
}

/// Un texte mis en page de la barre (même motif que `LigneGoal`).
struct LigneViewers {
    layout: IDWriteTextLayout,
    _format: IDWriteTextFormat,
    w: f32,
    h: f32,
}

/// Une pastille de la barre : logo vectoriel OU badge + count.
struct Pastille {
    /// Nom de plateforme — sert à la couleur du badge.
    plateforme: String,
    /// Logo vectoriel (twitch/kick) — clé du cache `logos_goal` partagé.
    logo: Option<LogoGoal>,
    /// Badge texte (« YT »/« TT ») — `None` quand un logo existe.
    badge: Option<LigneViewers>,
    /// Count compact (« 1,2k »).
    count: LigneViewers,
    /// Largeur totale de la pastille (pastille + gap interne 0,25 em).
    w: f32,
    /// Hauteur visuelle max (logo 1 em / boîte badge / count).
    h: f32,
}

/// Bloc mis en page de la barre — reconstruit quand `EtatGoals::version`
/// change (frames déjà dédupliquées côté viewers.rs, ~60 s par plateforme).
pub(super) struct BlocViewers {
    version: u64,
    /// Œil `👁` (Segoe UI Emoji) — les formats doivent vivre ici.
    oeil: LigneViewers,
    pastilles: Vec<Pastille>,
    /// Largeur totale du contenu (œil + gaps 0,8 em + pastilles) — centrage.
    largeur: f32,
    /// Hauteur de la ligne de contenu (max des items) — centrage vertical.
    ligne_h: f32,
}

/// Cache des blocs — déclaré ici pour que `mod.rs` ne connaisse pas le détail.
pub(super) type BlocsViewers = HashMap<u64, BlocViewers>;

/// Clé d'un bloc : filtre + taille — le contenu est suivi par `version`.
fn cle_bloc(cfg: &ChatCfg) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cfg.filtre.hash(&mut h);
    cfg.taille_police.to_bits().hash(&mut h);
    h.finish()
}

impl DessinateurTexte {
    /// Dessine la barre viewers en tête du widget chat — **dans une passe
    /// ouverte, sous le clip du widget**.
    ///
    /// Retourne la hauteur réservée (0 quand la barre est vide ou masquée :
    /// `:empty → display:none` + `.chat-bulles .chat-viewers`). La zone des
    /// messages doit être rognée en dessous (`chat-body flex:1`).
    pub fn barre_viewers_chat(
        &mut self,
        cfg: &ChatCfg,
        etat: &EtatGoals,
    ) -> Result<f32, ErreurTexte> {
        // Chrome du widget masqué en mode bulles — pas de barre du tout.
        if cfg.bulles {
            return Ok(0.0);
        }
        let plats = plateformes_barre(&cfg.filtre);
        let pastilles = pastilles_live(etat, &plats);
        if pastilles.is_empty() {
            return Ok(0.0);
        }

        let fs = cfg.taille_police;
        let cle = cle_bloc(cfg);
        let perime = match self.blocs_viewers.get(&cle) {
            Some(bloc) => bloc.version != etat.version(),
            None => true,
        };
        if perime {
            self.construire_bloc_viewers(cfg, etat, &pastilles, cle)?;
        }
        let Some(bloc) = self.blocs_viewers.get(&cle) else {
            return Ok(0.0);
        };

        let (x, y, w, _) = cfg.rect;
        let (x, y, w) = (x as f32, y as f32, w as f32);
        // `padding: 0.3em` + ligne de contenu + liseré 1 px.
        let bar_h = 0.3 * fs + bloc.ligne_h + 0.3 * fs + 1.0;
        let cy = y + 0.3 * fs + bloc.ligne_h / 2.0;

        // Liseré `border-bottom: 1px solid rgba(255,255,255,0.12)`.
        let lisere = unsafe {
            self.cible_rt.CreateSolidColorBrush(
                &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.12,
                },
                None,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse liseré : {}", e.message())))?;
        unsafe {
            self.cible_rt.FillRectangle(
                &D2D_RECT_F {
                    left: x,
                    top: y + bar_h - 1.0,
                    right: x + w,
                    bottom: y + bar_h,
                },
                &lisere,
            )
        };

        let blanc = unsafe {
            self.cible_rt
                .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 255]), None)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("brosse viewers : {}", e.message())))?;
        let blanc: ID2D1Brush = blanc.into();

        // Contenu centré : œil puis pastilles, `gap: 0.8em` entre enfants.
        let mut px = x + (w - bloc.largeur) / 2.0;
        // `.veye` opacity 0.8 — ignoré pour un glyphe couleur (limite doc).
        unsafe {
            self.cible_rt.DrawTextLayout(
                D2D_POINT_2F {
                    x: px,
                    y: cy - bloc.oeil.h / 2.0,
                },
                &bloc.oeil.layout,
                &blanc,
                D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            )
        };
        px += bloc.oeil.w + 0.8 * fs;

        for pastille in &bloc.pastilles {
            if let Some(logo) = pastille.logo {
                // `.logo` : 1 em × 1 em, `opacity: 0.85` ≈ brosse atténuée.
                let mut c = couleur_logo(logo);
                c[3] = (c[3] as f32 * 0.85) as u8;
                let brosse_logo = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(c), None)
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse logo viewers : {}", e.message()))
                })?;
                // Glyphe partagé avec goal (cache `logos_goal` du même
                // dessinateur — emprunt de `bloc` déjà terminé : clone AddRef).
                let geo = match self.logos_goal.get(&logo) {
                    Some(g) => g.clone(),
                    None => {
                        let g = super::goal::creer_logo(&self.d2d, logo)?;
                        self.logos_goal.insert(logo, g.clone());
                        g
                    }
                };
                let echelle = fs / viewbox_logo(logo);
                let transform = Matrix3x2 {
                    M11: echelle,
                    M22: echelle,
                    M31: px,
                    M32: cy - fs / 2.0,
                    ..Matrix3x2::identity()
                };
                unsafe {
                    self.cible_rt.SetTransform(&transform);
                    self.cible_rt.FillGeometry(&geo, &brosse_logo, None);
                    self.cible_rt.SetTransform(&Matrix3x2::identity());
                }
                px += fs;
            } else if let Some(badge) = &pastille.badge {
                // `.plat-badge` : padding 0 0,2 rem (3,2 px), radius 0,15 rem
                // (2,4 px), line-height 1.4 à 0,55 em — même recette que goal.
                let badge_w = badge.w + 6.4;
                let badge_h = 0.55 * fs * 1.4;
                let fond = unsafe {
                    self.cible_rt.CreateSolidColorBrush(
                        &couleur_d2d(fond_badge(&pastille.plateforme)),
                        None,
                    )
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("brosse badge viewers : {}", e.message()))
                })?;
                unsafe {
                    self.cible_rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: px,
                                top: cy - badge_h / 2.0,
                                right: px + badge_w,
                                bottom: cy + badge_h / 2.0,
                            },
                            radiusX: 2.4,
                            radiusY: 2.4,
                        },
                        &fond,
                    );
                    self.cible_rt.DrawTextLayout(
                        D2D_POINT_2F {
                            x: px + (badge_w - badge.w) / 2.0,
                            y: cy - badge.h / 2.0,
                        },
                        &badge.layout,
                        &blanc,
                        windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
                px += badge_w;
            }
            // `.vcount` : gap interne 0,25 em puis count gras.
            px += 0.25 * fs;
            unsafe {
                self.cible_rt.DrawTextLayout(
                    D2D_POINT_2F {
                        x: px,
                        y: cy - pastille.count.h / 2.0,
                    },
                    &pastille.count.layout,
                    &blanc,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            };
            px += pastille.count.w + 0.8 * fs;
        }
        Ok(bar_h)
    }

    /// (Re)construit le bloc de la barre — appelé seulement quand la
    /// version de l'état a changé (jamais par frame, comme `BlocChat`).
    fn construire_bloc_viewers(
        &mut self,
        cfg: &ChatCfg,
        etat: &EtatGoals,
        pastilles: &[(&str, u64)],
        cle: u64,
    ) -> Result<(), ErreurTexte> {
        let fs = cfg.taille_police;
        let style_count = StyleTexte {
            police: "Segoe UI".to_string(),
            taille: fs,
            gras: true,
            couleur: [255, 255, 255, 255],
            contour_epaisseur: 0.0,
            ..StyleTexte::default()
        };
        let style_badge = StyleTexte {
            taille: 0.55 * fs,
            ..style_count.clone()
        };

        let oeil = self.ligne_viewers_emoji("👁", fs)?;
        let mut items = Vec::with_capacity(pastilles.len());
        for &(plateforme, n) in pastilles {
            let count = self.ligne_viewers(&style_count, &format_viewers(n))?;
            let logo = logo_plateforme(plateforme);
            let badge = match logo {
                Some(_) => None,
                None => badge_viewers(plateforme)
                    .map(|t| self.ligne_viewers(&style_badge, t))
                    .transpose()?,
            };
            // `.vplat` : badge (texte + 2×3,2 px) ou logo 1 em, gap 0,25 em.
            let visuel_w = match (&logo, &badge) {
                (Some(_), _) => fs,
                (None, Some(b)) => b.w + 6.4,
                (None, None) => 0.0,
            };
            let visuel_h = match (&logo, &badge) {
                (Some(_), _) => fs,
                (None, Some(_)) => 0.55 * fs * 1.4,
                (None, None) => 0.0,
            };
            items.push(Pastille {
                plateforme: plateforme.to_string(),
                logo,
                badge,
                w: visuel_w + 0.25 * fs + count.w,
                h: visuel_h.max(count.h),
                count,
            });
        }

        // `gap: 0.8em` entre enfants (œil + pastilles).
        let ligne_h = items
            .iter()
            .map(|p| p.h)
            .fold(oeil.h, |a, b| a.max(b));
        let largeur = oeil.w
            + items.iter().map(|p| 0.8 * fs + p.w).sum::<f32>();

        // Table bornée : les clés ne divergent que par (filtre, taille).
        if self.blocs_viewers.len() > 16 {
            self.blocs_viewers.clear();
        }
        self.blocs_viewers.insert(
            cle,
            BlocViewers {
                version: etat.version(),
                oeil,
                pastilles: items,
                largeur,
                ligne_h,
            },
        );
        Ok(())
    }

    /// Ligne de texte classique (Segoe UI, système) — recette `ligne_goal`.
    fn ligne_viewers(
        &self,
        style: &StyleTexte,
        texte: &str,
    ) -> Result<LigneViewers, ErreurTexte> {
        let format = self.format(style)?;
        self.ligne_viewers_format(format, texte)
    }

    /// Ligne de texte emoji (Segoe UI Emoji, système) — l'œil `👁` de
    /// `.veye`. Même recette que `format_chat` : `None` = collection système.
    fn ligne_viewers_emoji(&self, texte: &str, taille: f32) -> Result<LigneViewers, ErreurTexte> {
        let nom: Vec<u16> = "Segoe UI Emoji"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                taille.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| {
            ErreurTexte::Systeme(format!("CreateTextFormat emoji : {}", e.message()))
        })?;
        self.ligne_viewers_format(format, texte)
    }

    /// Met en page un texte de la barre (format fourni), métriques mesurées.
    fn ligne_viewers_format(
        &self,
        format: IDWriteTextFormat,
        texte: &str,
    ) -> Result<LigneViewers, ErreurTexte> {
        let contenu: Vec<u16> = texte.encode_utf16().collect();
        if contenu.is_empty() {
            return Err(ErreurTexte::Systeme("ligne viewers vide".to_string()));
        }
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&contenu, &format, 8192.0, 2048.0)
        }
        .map_err(|e| {
            ErreurTexte::Systeme(format!("CreateTextLayout viewers : {}", e.message()))
        })?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap viewers : {}", e.message())))?;
        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques viewers : {}", e.message())))?;
        Ok(LigneViewers {
            layout,
            _format: format,
            w: metriques.width,
            h: metriques.height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texte::MajGoal;

    fn etat_avec_viewers() -> EtatGoals {
        let mut etat = EtatGoals::nouveau();
        etat.appliquer(MajGoal::Viewers {
            plateforme: "twitch".to_string(),
            count: Some(1234),
        });
        etat.appliquer(MajGoal::Viewers {
            plateforme: "kick".to_string(),
            count: Some(50),
        });
        // Hors-ligne : `count: null` ne doit jamais produire de pastille.
        etat.appliquer(MajGoal::Viewers {
            plateforme: "youtube".to_string(),
            count: None,
        });
        etat
    }

    #[test]
    fn format_viewers_compact_comme_le_dom() {
        assert_eq!(format_viewers(0), "0");
        assert_eq!(format_viewers(999), "999");
        assert_eq!(format_viewers(1000), "1k");
        assert_eq!(format_viewers(1234), "1,2k");
        assert_eq!(format_viewers(10500), "10,5k");
        assert_eq!(format_viewers(1_000_000), "1000k");
    }

    #[test]
    fn plateformes_unifie_quatre_dans_l_ordre_du_dom() {
        assert_eq!(
            plateformes_barre(FILTRE_UNIFIE),
            vec!["twitch", "youtube", "kick", "tiktok"]
        );
        assert_eq!(plateformes_barre("kick"), vec!["kick"]);
    }

    #[test]
    fn pastilles_live_sautent_hors_ligne_et_inconnues() {
        let etat = etat_avec_viewers();
        // unifie : youtube est hors-ligne (count null) → absente, ordre gardé.
        let pastilles = pastilles_live(&etat, &plateformes_barre(FILTRE_UNIFIE));
        assert_eq!(
            pastilles,
            vec![("twitch", 1234), ("kick", 50)],
            "ordre PLATS_VIEWERS, hors-ligne sautée"
        );
        // Filtré : seule la plateforme du filtre ; un filtre arbitraire n'a
        // jamais de count (comme `viewersParPlateforme[p]` du DOM).
        assert_eq!(
            pastilles_live(&etat, &plateformes_barre("kick")),
            vec![("kick", 50)]
        );
        assert!(pastilles_live(&etat, &plateformes_barre("youtube")).is_empty());
        assert!(pastilles_live(&etat, &plateformes_barre("autre")).is_empty());
        assert!(EtatGoals::nouveau()
            .valeur("twitch", "viewers")
            .is_none());
    }
}
