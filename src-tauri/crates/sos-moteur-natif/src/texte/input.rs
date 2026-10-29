//! Widget input-viewer : clavier / numpad (châssis + keycaps), souris
//! (corps, zones, molette crantée, DPI, reflet, coordonnées) et manette
//! (3 skins 8bitdo/xbox/ps — primitives `Forme`/`Texte`, dégradés
//! `objectBoundingBox`, halo de presse) — miroir de `ivGenerer*` /
//! `iv-fig-*` du DOM.
//!
//! Extrait de `texte.rs` (découpage 2026-10-29) : ces fonctions restent
//! des méthodes de `DessinateurTexte` — l'`impl` est scindé, le module
//! enfant voyant les champs privés du parent.

use std::mem::ManuallyDrop;

use windows::core::{Interface, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FILL_MODE_ALTERNATE, D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Geometry, ID2D1GeometrySink, ID2D1PathGeometry,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteTextFormat, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::input::{
    bouton_physique, resoudre_dpad, FileInput, Forme, InputCfg, LienManette, ModeInput,
    Primitive, Segment, SkinManette, StyleLabel, StyleManette,
};

use super::{couleur_d2d, DessinateurTexte, ErreurTexte};

impl DessinateurTexte {
    /// Dessine un widget input-viewer (clavier / numpad / souris / manette) —
    /// dans une passe.
    ///
    /// Le contenu est rogné au rect du widget (`overflow: hidden`) **et** au
    /// polygone de son cadre (`clip-path`), comme le CSS.
    pub fn dessiner_input(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        clip: Option<&ID2D1PathGeometry>,
    ) -> Result<(), ErreurTexte> {
        let geometrie = crate::input::disposition(cfg);
        if geometrie.chassis.is_none()
            && geometrie.touches.is_empty()
            && geometrie.souris.is_none()
            && geometrie.manette.is_none()
        {
            return Ok(());
        }
        let (x, y, w, h) = cfg.rect;
        let zone = D2D_RECT_F {
            left: x as f32,
            top: y as f32,
            right: (x + w) as f32,
            bottom: (y + h) as f32,
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
                .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer input : {}", e.message())))?;
            unsafe { self.ctx.PushLayer(&params, &layer) };
        }
        let resultat = self.dessiner_input_interne(cfg, etat, &geometrie);
        if avec_couche {
            unsafe { self.ctx.PopLayer() };
            unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        }
        unsafe { self.cible_rt.PopAxisAlignedClip() };
        resultat
    }

    fn dessiner_input_interne(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        geometrie: &crate::input::GeometrieInput,
    ) -> Result<(), ErreurTexte> {
        let largeur = cfg.rect.2 as f32;
        let cqw = largeur / 100.0;
        if let Some(chassis) = &geometrie.chassis {
            self.dessiner_chassis(cfg, chassis, cqw, etat)?;
        }
        for touche in &geometrie.touches {
            self.dessiner_touche(cfg, cqw, touche, etat)?;
        }
        if let Some(souris) = &geometrie.souris {
            self.dessiner_souris(cfg, etat, souris)?;
        }
        if let Some(manette) = &geometrie.manette {
            self.dessiner_manette(cfg, etat, manette)?;
        }
        Ok(())
    }

    /// Souris (`ivGenererSourisHTML`) : corps, zones de clic, molette crantée,
    /// bouton DPI, reflet, coordonnées.
    ///
    /// `fill: var(--inputviewer-couleur-presse)` du CSS remplace le dégradé
    /// des zones/molette par la couleur de presse **unie** ; le
    /// `drop-shadow(0 0 12px)` est approché par un liseré large translucide
    /// sous la forme (même convention que le halo des LEDs).
    fn dessiner_souris(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        s: &crate::input::SourisGeometrie,
    ) -> Result<(), ErreurTexte> {
        let e = s.echelle;
        let etat_s = etat.souris();
        let presse = couleur_d2d(cfg.couleur);
        let halo_presse = D2D1_COLOR_F {
            a: 0.3,
            ..presse
        };

        // Corps translucide (fill-opacity 0.62) — le média transparaît.
        let corps = self.chemin_d2d(&s.corps)?;
        let (cx, y0, _, y1) = bornes_chemin(&s.corps);
        let brosse_corps = self.brosse_degrade_stops(
            &[
                (0.0, couleur_d2d_alpha([0x3a, 0x3f, 0x4d, 255], 0.62)),
                (0.5, couleur_d2d_alpha([0x26, 0x2a, 0x33, 255], 0.62)),
                (1.0, couleur_d2d_alpha([0x14, 0x16, 0x1c, 255], 0.62)),
            ],
            (cx, y0),
            (cx, y1),
        )?;
        unsafe { self.cible_rt.FillGeometry(&corps, &brosse_corps, None) };
        let filet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.12,
        })?;
        unsafe {
            self.cible_rt
                .DrawGeometry(&corps, &filet, (2.0 * e).max(0.5), None)
        };

        // Zones de clic gauche/droite (opaques, dégradé `ivs-btn`).
        for (chemin, enfoncee) in [
            (&s.zone_gauche, etat_s.gauche),
            (&s.zone_droite, etat_s.droite),
        ] {
            let geo = self.chemin_d2d(chemin)?;
            let (zx, z0, _, z1) = bornes_chemin(chemin);
            if enfoncee {
                let halo = self.brosse_unie(halo_presse)?;
                unsafe {
                    self.cible_rt
                        .DrawGeometry(&geo, &halo, (6.0 * e).max(1.0), None)
                };
            }
            let remplissage = if enfoncee {
                self.brosse_unie(presse)?
            } else {
                self.brosse_degrade_stops(
                    &[
                        (0.0, couleur_d2d([0x48, 0x4e, 0x5c, 255])),
                        (1.0, couleur_d2d([0x22, 0x25, 0x2d, 255])),
                    ],
                    (zx, z0),
                    (zx, z1),
                )?
            };
            unsafe { self.cible_rt.FillGeometry(&geo, &remplissage, None) };
            let contour = self.brosse_unie(if enfoncee {
                presse
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.10,
                }
            })?;
            unsafe {
                self.cible_rt
                    .DrawGeometry(&geo, &contour, (1.5 * e).max(0.4), None)
            };
        }

        // Coutures (centrale + séparation sous les clics).
        let couture = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.4,
        })?;
        unsafe {
            self.cible_rt.DrawLine(
                pt(s.couture_verticale.0),
                pt(s.couture_verticale.1),
                &couture,
                (2.0 * e).max(0.5),
                None,
            );
            self.cible_rt.DrawLine(
                pt(s.couture_horizontale.0),
                pt(s.couture_horizontale.1),
                &couture,
                (2.0 * e).max(0.5),
                None,
            );
        }

        // Logement de la molette.
        let loge = rounded(s.loge, 22.0 * e);
        let noir = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.25,
        })?;
        unsafe { self.cible_rt.FillRoundedRectangle(&loge, &noir) };

        // Molette (clic milieu) — crantée.
        if etat_s.milieu {
            let halo = self.brosse_unie(halo_presse)?;
            unsafe {
                self.cible_rt
                    .DrawRoundedRectangle(&rounded(s.molette, 14.0 * e), &halo, 6.0 * e, None)
            };
        }
        let molette = rounded(s.molette, 14.0 * e);
        let brosse_molette = if etat_s.milieu {
            self.brosse_unie(presse)?
        } else {
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([0x56, 0x5e, 0x6e, 255])),
                    (1.0, couleur_d2d([0x2a, 0x2e, 0x37, 255])),
                ],
                (s.molette.x, s.molette.y),
                (s.molette.x, s.molette.y + s.molette.hauteur),
            )?
        };
        unsafe { self.cible_rt.FillRoundedRectangle(&molette, &brosse_molette) };
        let bord_molette = self.brosse_unie(if etat_s.milieu {
            presse
        } else {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.18,
            }
        })?;
        unsafe {
            self.cible_rt
                .DrawRoundedRectangle(&molette, &bord_molette, (1.5 * e).max(0.4), None)
        };
        // Les crans ne sont PAS dans le sélecteur `.presse` du CSS : toujours
        // noirs translucides.
        let cran = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        })?;
        for (a, b) in &s.crans {
            unsafe {
                self.cible_rt
                    .DrawLine(pt(*a), pt(*b), &cran, (2.0 * e).max(0.5), None)
            };
        }

        // Bouton DPI.
        let dpi = D2D1_ELLIPSE {
            point: pt(s.dpi.0),
            radiusX: s.dpi.1,
            radiusY: s.dpi.1,
        };
        let remplissage_dpi = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.08,
        })?;
        unsafe { self.cible_rt.FillEllipse(&dpi, &remplissage_dpi) };
        let bord_dpi = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.10,
        })?;
        unsafe {
            self.cible_rt
                .DrawEllipse(&dpi, &bord_dpi, (1.0 * e).max(0.3), None)
        };

        // Reflet sur la coque (ellipse tournée de -16° autour de son centre).
        let mut avant = Matrix3x2::identity();
        unsafe { self.cible_rt.GetTransform(&mut avant) };
        let rotation = Matrix3x2::rotation(
            s.reflet.angle.to_radians(),
            s.reflet.centre.x,
            s.reflet.centre.y,
        );
        unsafe { self.cible_rt.SetTransform(&(rotation * avant)) };
        let reflet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.12,
        })?;
        unsafe {
            self.cible_rt.FillEllipse(
                &D2D1_ELLIPSE {
                    point: pt(s.reflet.centre),
                    radiusX: s.reflet.rayon_x,
                    radiusY: s.reflet.rayon_y,
                },
                &reflet,
            )
        };
        unsafe { self.cible_rt.SetTransform(&avant) };

        // Coordonnées curseur (`X: … · Y: …`, ancre médiane, taille 16 viewBox).
        let (ancre, taille) = s.coordonnees;
        let format = self.format_input(taille, true)?;
        let texte: Vec<u16> = format!("X: {} · Y: {}", etat_s.x, etat_s.y)
            .encode_utf16()
            .collect();
        let rect = D2D_RECT_F {
            left: ancre.x - 80.0 * e,
            top: ancre.y - 0.95 * taille,
            right: ancre.x + 80.0 * e,
            bottom: ancre.y + 0.25 * taille,
        };
        let encre = self.brosse_unie(couleur_d2d([0xae, 0xb4, 0xc2, 255]))?;
        unsafe {
            self.cible_rt.DrawText(
                &texte,
                &format,
                &rect,
                &encre,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    /// Manette (`ivGenererManette*SVG`) : peint les `groupes` dans l'ordre du
    /// document SVG et résout leur état via le `lien`.
    ///
    /// Presse : couleur unie `cfg.couleur` + liseré-halo translucide dessiné
    /// **sous** la forme (approche du `drop-shadow` CSS, même convention que
    /// les zones de la souris). `connectee` n'a pas de rendu dédié :
    /// `majInputViewerEtat` ne le lit pas non plus — une manette débranchée
    /// n'est que « tout relâché ».
    fn dessiner_manette(
        &mut self,
        cfg: &InputCfg,
        etat: &FileInput,
        m: &crate::input::ManetteGeometrie,
    ) -> Result<(), ErreurTexte> {
        let e = m.echelle;
        let etat_m = etat.manette();
        let mapping = cfg.mapping.as_ref();
        let dpad = resoudre_dpad(&etat_m, m.skin, mapping);
        let presse = PresseManette {
            allume: false,
            couleur: couleur_d2d(cfg.couleur),
            halo: D2D1_COLOR_F {
                a: 0.3,
                ..couleur_d2d(cfg.couleur)
            },
        };

        for groupe in &m.groupes {
            let (allume, deplacement) = match groupe.lien {
                LienManette::Statique => (false, (0.0, 0.0)),
                LienManette::Bouton(idx) => {
                    let phys = bouton_physique(idx, mapping);
                    (etat_m.boutons.get(phys).copied().unwrap_or(false), (0.0, 0.0))
                }
                LienManette::Dpad(dir) => (dir.activee(dpad), (0.0, 0.0)),
                LienManette::Stick { gauche, bouton } => {
                    let (ax, ay) = if gauche {
                        (etat_m.axes[0], etat_m.axes[1])
                    } else {
                        (etat_m.axes[2], etat_m.axes[3])
                    };
                    let phys = bouton_physique(bouton, mapping);
                    let enfonce = etat_m.boutons.get(phys).copied().unwrap_or(false);
                    // `.actif` : le cap s'éclaire aussi quand le stick bouge
                    // (`translate(ax*26, ay*26)` px viewBox, seuil 0,15).
                    let actif = ax.abs() > 0.15 || ay.abs() > 0.15;
                    (enfonce || actif, (ax * 26.0 * e, ay * 26.0 * e))
                }
            };

            let deplace = deplacement != (0.0, 0.0);
            let mut avant = Matrix3x2::identity();
            if deplace {
                unsafe { self.cible_rt.GetTransform(&mut avant) };
                let t = Matrix3x2::translation(deplacement.0, deplacement.1);
                unsafe { self.cible_rt.SetTransform(&(t * avant)) };
            }
            let resultat = self.dessiner_primitives_manette(
                groupe,
                m.skin,
                PresseManette { allume, ..presse },
                e,
            );
            if deplace {
                unsafe { self.cible_rt.SetTransform(&avant) };
            }
            resultat?;
        }
        Ok(())
    }

    /// Primitives d'un `<g>` manette — erreurs propagées, transform déjà posé.
    fn dessiner_primitives_manette(
        &mut self,
        groupe: &crate::input::GroupeManette,
        skin: SkinManette,
        presse: PresseManette,
        e: f32,
    ) -> Result<(), ErreurTexte> {
        for primitive in &groupe.primitives {
            match primitive {
                Primitive::Forme { forme, style } => {
                    self.dessiner_forme_manette(skin, *style, forme, presse, e)?;
                }
                Primitive::Texte {
                    ancre,
                    taille,
                    contenu,
                    style,
                } => {
                    self.dessiner_label_manette(*ancre, *taille, contenu, *style, presse.allume)?;
                }
            }
        }
        Ok(())
    }

    /// Une forme de la figure : `style` → fill/stroke du CSS `iv-fig-*` ;
    /// `presse.allume` applique la convention presse (liseré-halo + couleur
    /// unie). Le liseré-halo ≈ moitié du rayon du `drop-shadow` : 12 px → 6e,
    /// 10 px → 5e, 8 px → 4e.
    fn dessiner_forme_manette(
        &mut self,
        skin: SkinManette,
        style: StyleManette,
        forme: &Forme,
        presse: PresseManette,
        e: f32,
    ) -> Result<(), ErreurTexte> {
        let lumiere = |a: f32| D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a,
        };
        // `?` sur le liseré-halo : dessiné sous la forme, largeur en e.
        let allumer = |largeur: f32| -> Result<(), ErreurTexte> {
            let brosse = self.brosse_unie(presse.halo)?;
            self.tracer_forme(forme, &brosse, (largeur * e).max(0.8))
        };
        let allume = presse.allume;
        let couleur = presse.couleur;
        match style {
            StyleManette::Corps => {
                let (x0, y0, x1, y1) = bornes_forme(forme);
                let stops = stops_corps(skin)
                    .map(|(p, rgb)| (p, couleur_d2d_alpha(rgb, 0.62)));
                let brosse =
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.12))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::CorpsInner => {
                let brosse = self.brosse_unie(lumiere(0.03))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.05))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Bouton | StyleManette::Home | StyleManette::StickCap => {
                if allume {
                    allumer(if style == StyleManette::Home { 4.0 } else { 6.0 })?;
                }
                let brosse = if allume {
                    self.brosse_unie(couleur)?
                } else {
                    let (x0, y0, x1, y1) = bornes_forme(forme);
                    let stops = stops_btn(skin).map(|(p, rgb)| (p, couleur_d2d(rgb)));
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?
                };
                self.remplir_forme(forme, &brosse)?;
                // `.iv-fig-home` garde son filet blanc quand pressé ; face/
                // shoulder/pill/cap passent le filet à la couleur de presse.
                let teinte = if allume && style != StyleManette::Home {
                    couleur
                } else {
                    lumiere(if style == StyleManette::StickCap { 0.16 } else { 0.15 })
                };
                let filet = self.brosse_unie(teinte)?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Zone => {
                if !allume {
                    return Ok(()); // `fill: transparent` au repos.
                }
                allumer(5.0)?;
                let brosse = self.brosse_unie(D2D1_COLOR_F {
                    a: 0.85,
                    ..couleur
                })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Diag => {
                if allume {
                    allumer(4.0)?;
                }
                let brosse = self.brosse_unie(if allume { couleur } else { lumiere(0.12) })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Pad => {
                if allume {
                    allumer(6.0)?;
                    let brosse = self.brosse_unie(couleur)?;
                    self.remplir_forme(forme, &brosse)?;
                } else {
                    let brosse = self.brosse_dish(skin, forme)?;
                    self.remplir_forme(forme, &brosse)?;
                }
                // Le filet 0,10 du touchpad est identique pressé ou non.
                let filet = self.brosse_unie(lumiere(0.10))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::Dish | StyleManette::StickBase => {
                let brosse = self.brosse_dish(skin, forme)?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.08))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::Cross | StyleManette::Hub => {
                let (x0, y0, x1, y1) = bornes_forme(forme);
                // Hub : dégradé « cross » (8bitdo/xbox) ou « btn » (ps).
                let stops = if skin == SkinManette::Ps && style == StyleManette::Hub {
                    stops_btn(skin)
                } else {
                    stops_cross(skin)
                };
                let stops = stops.map(|(p, rgb)| (p, couleur_d2d(rgb)));
                let brosse =
                    self.brosse_degrade_stops(&stops, ((x0 + x1) / 2.0, y0), ((x0 + x1) / 2.0, y1))?;
                self.remplir_forme(forme, &brosse)?;
                let filet = self.brosse_unie(lumiere(0.10))?;
                self.tracer_forme(forme, &filet, (1.5 * e).max(0.4))
            }
            StyleManette::HubDot => {
                let brosse = self.brosse_unie(lumiere(0.08))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::StickRing => {
                let filet = self.brosse_unie(lumiere(0.08))?;
                self.tracer_forme(forme, &filet, (2.0 * e).max(0.5))
            }
            StyleManette::Ombre => {
                let brosse = self.brosse_unie(D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35,
                })?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Sheen => {
                let brosse = self.brosse_unie(lumiere(0.12))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::SheenLine => {
                let filet = self.brosse_unie(lumiere(0.07))?;
                self.tracer_forme(forme, &filet, (3.0 * e).max(0.7))
            }
            StyleManette::Ico => {
                let brosse = self.brosse_unie(couleur_d2d([0xae, 0xb4, 0xc2, 255]))?;
                self.remplir_forme(forme, &brosse)
            }
            StyleManette::Deco => {
                let brosse = self.brosse_unie(lumiere(0.10))?;
                self.remplir_forme(forme, &brosse)
            }
        }
    }

    /// `radialGradient` des `iv*-dish` : centre (50 %, 45 %) de la boîte,
    /// rayons 0,62 × boîte — alpha 0,75 comme le `fill-opacity` du CSS.
    fn brosse_dish(&self, skin: SkinManette, forme: &Forme) -> Result<ID2D1Brush, ErreurTexte> {
        let (x0, y0, x1, y1) = bornes_forme(forme);
        let stops = stops_dish(skin).map(|(p, rgb)| (p, couleur_d2d_alpha(rgb, 0.75)));
        self.brosse_radiale(
            &stops,
            ((x0 + x1) / 2.0, y0 + 0.45 * (y1 - y0)),
            0.62 * (x1 - x0),
            0.62 * (y1 - y0),
        )
    }

    /// Brosse dégradée radial (`CreateRadialGradientBrush`, comme le linéaire).
    fn brosse_radiale(
        &self,
        stops: &[(f32, D2D1_COLOR_F)],
        centre: (f32, f32),
        rayon_x: f32,
        rayon_y: f32,
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: *color,
            })
            .collect();
        let collection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
        let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
            center: D2D_POINT_2F {
                x: centre.0,
                y: centre.1,
            },
            gradientOriginOffset: D2D_POINT_2F { x: 0.0, y: 0.0 },
            radiusX: rayon_x,
            radiusY: rayon_y,
        };
        let brosse = unsafe {
            self.cible_rt
                .CreateRadialGradientBrush(&props, None, &collection)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateRadialGradientBrush : {}", e.message())))?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// `<text>` de la figure : `text-anchor: middle` + `dominant-baseline:
    /// central` → boîte centrée sur l'ancre ; blanc si le groupe est allumé.
    fn dessiner_label_manette(
        &mut self,
        ancre: crate::input::Point,
        taille: f32,
        contenu: &str,
        style: StyleLabel,
        allume: bool,
    ) -> Result<(), ErreurTexte> {
        let format = self.format_input(taille, true)?;
        let texte: Vec<u16> = contenu.encode_utf16().collect();
        let rect = D2D_RECT_F {
            left: ancre.x - 4.0 * taille,
            top: ancre.y - 0.7 * taille,
            right: ancre.x + 4.0 * taille,
            bottom: ancre.y + 0.6 * taille,
        };
        let encre = self.brosse_unie(if allume {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }
        } else {
            match style {
                StyleLabel::Lbl => couleur_d2d([0xdf, 0xe3, 0xec, 255]),
                StyleLabel::LblSm => couleur_d2d([0xae, 0xb4, 0xc2, 255]),
                StyleLabel::Arrow => D2D1_COLOR_F {
                    r: 223.0 / 255.0,
                    g: 227.0 / 255.0,
                    b: 236.0 / 255.0,
                    a: 0.40,
                },
                StyleLabel::Couleur(rgb) => couleur_d2d([rgb[0], rgb[1], rgb[2], 255]),
            }
        })?;
        unsafe {
            self.cible_rt.DrawText(
                &texte,
                &format,
                &rect,
                &encre,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
        Ok(())
    }

    /// Remplit une `Forme` manette (ellipse / rect arrondi / chemin).
    fn remplir_forme(&self, forme: &Forme, brosse: &ID2D1Brush) -> Result<(), ErreurTexte> {
        match forme {
            Forme::Cercle { centre, rayon } => unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon,
                        radiusY: *rayon,
                    },
                    brosse,
                )
            },
            Forme::Ellipse {
                centre,
                rayon_x,
                rayon_y,
            } => unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon_x,
                        radiusY: *rayon_y,
                    },
                    brosse,
                )
            },
            Forme::Rect { rect, rayon } => unsafe {
                self.cible_rt
                    .FillRoundedRectangle(&rounded(*rect, *rayon), brosse)
            },
            Forme::Chemin(chemin) => unsafe {
                self.cible_rt
                    .FillGeometry(&self.chemin_d2d(chemin)?, brosse, None)
            },
        }
        Ok(())
    }

    /// Trace le contour d'une `Forme` manette (filet ou liseré-halo).
    fn tracer_forme(&self, forme: &Forme, brosse: &ID2D1Brush, largeur: f32) -> Result<(), ErreurTexte> {
        match forme {
            Forme::Cercle { centre, rayon } => unsafe {
                self.cible_rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon,
                        radiusY: *rayon,
                    },
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Ellipse {
                centre,
                rayon_x,
                rayon_y,
            } => unsafe {
                self.cible_rt.DrawEllipse(
                    &D2D1_ELLIPSE {
                        point: pt(*centre),
                        radiusX: *rayon_x,
                        radiusY: *rayon_y,
                    },
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Rect { rect, rayon } => unsafe {
                self.cible_rt.DrawRoundedRectangle(
                    &rounded(*rect, *rayon),
                    brosse,
                    largeur,
                    None,
                )
            },
            Forme::Chemin(chemin) => unsafe {
                self.cible_rt
                    .DrawGeometry(&self.chemin_d2d(chemin)?, brosse, largeur, None)
            },
        }
        Ok(())
    }

    /// `Chemin` (données pures) → `ID2D1Geometry` Direct2D.
    ///
    /// La géométrie de chemin est transformée en interface de base : les
    /// `Fill`/`Draw` n'utilisent que `ID2D1Geometry`.
    fn chemin_d2d(&self, chemin: &crate::input::Chemin) -> Result<ID2D1Geometry, ErreurTexte> {
        let geo: ID2D1PathGeometry = unsafe { self.d2d.CreatePathGeometry() }
            .and_then(|g| g.cast())
            .map_err(|e| ErreurTexte::Gpu(format!("CreatePathGeometry : {}", e.message())))?;
        let sink: ID2D1GeometrySink = unsafe { geo.Open() }
            .map_err(|e| ErreurTexte::Gpu(format!("GeometrySink : {}", e.message())))?;
        unsafe {
            sink.SetFillMode(D2D1_FILL_MODE_ALTERNATE);
            for segment in &chemin.segments {
                match segment {
                    Segment::Debut(p) => {
                        sink.BeginFigure(pt(*p), D2D1_FIGURE_BEGIN_FILLED)
                    }
                    Segment::Ligne(p) => sink.AddLine(pt(*p)),
                    Segment::Cubique(a, b, d) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: pt(*a),
                        point2: pt(*b),
                        point3: pt(*d),
                    }),
                    Segment::Fermer => sink.EndFigure(D2D1_FIGURE_END_CLOSED),
                }
            }
        }
        unsafe { sink.Close() }
            .map_err(|e| ErreurTexte::Gpu(format!("Close : {}", e.message())))?;
        geo.cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))
    }

    /// Châssis du clavier/numpad : plaque translucide, plateau encaissé, LEDs.
    fn dessiner_chassis(
        &mut self,
        cfg: &InputCfg,
        chassis: &crate::input::Chassis,
        _cqw: f32,
        etat: &FileInput,
    ) -> Result<(), ErreurTexte> {
        let largeur = cfg.rect.2 as f32;
        let plaque = rounded(chassis.plaque, chassis.rayon);
        let brosse = self.brosse_degrade_stops(
            &[
                (0.0, couleur_d2d_alpha([0x33, 0x38, 0x44, 255], 0.55)),
                (0.5, couleur_d2d_alpha([0x23, 0x26, 0x2e, 255], 0.55)),
                (1.0, couleur_d2d_alpha([0x14, 0x16, 0x1b, 255], 0.55)),
            ],
            (chassis.plaque.x, chassis.plaque.y),
            (chassis.plaque.x, chassis.plaque.y + chassis.plaque.hauteur),
        )?;
        unsafe { self.cible_rt.FillRoundedRectangle(&plaque, &brosse) };
        let filet = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.10,
        })?;
        unsafe {
            self.cible_rt.DrawRoundedRectangle(
                &plaque,
                &filet,
                (0.004 * largeur).max(0.4),
                None,
            )
        };

        let cable = rounded(chassis.cable, chassis.rayon * 0.27);
        let noir = self.brosse_unie(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        })?;
        unsafe { self.cible_rt.FillRoundedRectangle(&cable, &noir) };

        let plateau = rounded(chassis.plateau, chassis.rayon * 0.67);
        unsafe { self.cible_rt.FillRoundedRectangle(&plateau, &noir) };
        let filet_plateau = self.brosse_unie(D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.06,
        })?;
        unsafe {
            self.cible_rt.DrawRoundedRectangle(
                &plateau,
                &filet_plateau,
                (0.003 * largeur).max(0.3),
                None,
            )
        };

        for led in &chassis.leds {
            let pressee = etat.est_pressee(led.code);
            let [cr, cg, cb, _] = cfg.couleur;
            let (r, g, b, a) = if pressee {
                (cr as f32 / 255.0, cg as f32 / 255.0, cb as f32 / 255.0, 1.0)
            } else {
                (1.0, 1.0, 1.0, 0.14)
            };
            let centre = D2D_POINT_2F {
                x: led.centre.0,
                y: led.centre.1,
            };
            if pressee {
                // `filter: drop-shadow(0 0 8px couleur)` : halo approximé par
                // un disque translucide plus grand, dessiné **sous** la LED.
                let halo = self.brosse_unie(D2D1_COLOR_F {
                    r,
                    g,
                    b,
                    a: 0.35,
                })?;
                unsafe {
                    self.cible_rt.FillEllipse(
                        &D2D1_ELLIPSE {
                            point: centre,
                            radiusX: led.rayon * 2.2,
                            radiusY: led.rayon * 2.2,
                        },
                        &halo,
                    )
                };
            }
            let disque = self.brosse_unie(D2D1_COLOR_F { r, g, b, a })?;
            unsafe {
                self.cible_rt.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: centre,
                        radiusX: led.rayon,
                        radiusY: led.rayon,
                    },
                    &disque,
                )
            };
            let format = self.format_input(led.taille, true)?;
            let texte: Vec<u16> = led.label.encode_utf16().collect();
            let demi = 3.0 * led.taille;
            let rect = D2D_RECT_F {
                left: led.centre.0 - demi,
                top: led.centre.1 + led.rayon,
                right: led.centre.0 + demi,
                bottom: led.centre.1 + led.rayon + 2.0 * led.taille,
            };
            let encre = self.brosse_unie(if pressee {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            } else {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35,
                }
            })?;
            unsafe {
                self.cible_rt.DrawText(
                    &texte,
                    &format,
                    &rect,
                    &encre,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            };
        }
        Ok(())
    }

    /// Une keycap : dégradé vertical, bordure, presse (couleur + contour + scale).
    fn dessiner_touche(
        &mut self,
        cfg: &InputCfg,
        cqw: f32,
        touche: &crate::input::TouchePlacee,
        etat: &FileInput,
    ) -> Result<(), ErreurTexte> {
        let pressee = etat.est_pressee_parmi(touche.codes);
        let numpad = cfg.mode == ModeInput::Numpad;
        let rayon = if numpad { 1.6 * cqw } else { 0.9 * cqw };
        let mut r = touche.rect;
        if pressee {
            // `transform: scale(0.92) translateY(0.1cqw)` autour du centre.
            let (cx, cy) = (
                r.x + r.largeur / 2.0,
                r.y + r.hauteur / 2.0 + 0.1 * cqw,
            );
            r = crate::input::RectF {
                x: cx - r.largeur * 0.46,
                y: cy - r.hauteur * 0.46,
                largeur: r.largeur * 0.92,
                hauteur: r.hauteur * 0.92,
            };
        }
        let cap = rounded(r, rayon);
        let brosse = if pressee {
            let [cr, cg, cb, _] = cfg.couleur;
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([cr, cg, cb, 255])),
                    (1.0, couleur_d2d(melange_noir([cr, cg, cb, 255], 0.7))),
                ],
                (r.x, r.y),
                (r.x, r.y + r.hauteur),
            )?
        } else {
            self.brosse_degrade_stops(
                &[
                    (0.0, couleur_d2d([0x3d, 0x42, 0x4e, 255])),
                    (0.55, couleur_d2d([0x27, 0x2b, 0x33, 255])),
                    (1.0, couleur_d2d([0x1d, 0x20, 0x27, 255])),
                ],
                (r.x, r.y),
                (r.x, r.y + r.hauteur),
            )?
        };
        unsafe { self.cible_rt.FillRoundedRectangle(&cap, &brosse) };
        let bord = if pressee {
            couleur_d2d(cfg.couleur)
        } else {
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.16,
            }
        };
        let brosse_bord = self.brosse_unie(bord)?;
        unsafe { self.cible_rt.DrawRoundedRectangle(&cap, &brosse_bord, 1.0, None) };

        if pressee {
            // `outline: 3px; outline-offset: 3px` → bande [3, 6] px.
            let dehors = rounded(
                crate::input::RectF {
                    x: r.x - 4.5,
                    y: r.y - 4.5,
                    largeur: r.largeur + 9.0,
                    hauteur: r.hauteur + 9.0,
                },
                rayon + 4.5,
            );
            let brosse_outline = self.brosse_unie(couleur_d2d(cfg.couleur))?;
            unsafe { self.cible_rt.DrawRoundedRectangle(&dehors, &brosse_outline, 3.0, None) };
        }

        if !touche.label.is_empty() {
            let taille = if numpad { 2.0 * cqw } else { 1.9 * cqw };
            let format = self.format_input(taille, numpad)?;
            let texte: Vec<u16> = touche.label.encode_utf16().collect();
            let rect = D2D_RECT_F {
                left: r.x,
                top: r.y,
                right: r.x + r.largeur,
                bottom: r.y + r.hauteur,
            };
            let encre = self.brosse_unie(if pressee {
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            } else {
                couleur_d2d([0xf0, 0xf0, 0xf0, 255])
            })?;
            unsafe {
                self.cible_rt.DrawText(
                    &texte,
                    &format,
                    &rect,
                    &encre,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            };
        }
        Ok(())
    }

    /// `IDWriteTextFormat` centré (deux axes) pour les libellés input-viewer.
    fn format_input(&mut self, taille: f32, gras: bool) -> Result<IDWriteTextFormat, ErreurTexte> {
        let cle = ((taille * 10.0).round().max(1.0) as u32, gras);
        if let Some(format) = self.formats_input.get(&cle) {
            return Ok(format.clone());
        }
        let nom: Vec<u16> = "Segoe UI".encode_utf16().chain(std::iter::once(0)).collect();
        let locale: Vec<u16> = "fr-fr".encode_utf16().chain(std::iter::once(0)).collect();
        let format = unsafe {
            self.dwrite.CreateTextFormat(
                PCWSTR(nom.as_ptr()),
                None,
                if gras {
                    DWRITE_FONT_WEIGHT_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                taille.max(1.0),
                PCWSTR(locale.as_ptr()),
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateTextFormat input : {}", e.message())))?;
        unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER) }
            .map_err(|e| ErreurTexte::Systeme(format!("alignement : {}", e.message())))?;
        unsafe { format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER) }
            .map_err(|e| ErreurTexte::Systeme(format!("paragraphe : {}", e.message())))?;
        unsafe { format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("nowrap : {}", e.message())))?;
        self.formats_input.insert(cle, format.clone());
        Ok(format)
    }

    /// Brosse unie (couleur déjà en flottants).
    fn brosse_unie(&self, couleur: D2D1_COLOR_F) -> Result<ID2D1Brush, ErreurTexte> {
        let brosse = unsafe { self.cible_rt.CreateSolidColorBrush(&couleur, None) }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse : {}", e.message())))?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }

    /// Brosse dégradée linéaire entre deux points, avec arrêts explicites.
    fn brosse_degrade_stops(
        &self,
        stops: &[(f32, D2D1_COLOR_F)],
        debut: (f32, f32),
        fin: (f32, f32),
    ) -> Result<ID2D1Brush, ErreurTexte> {
        let arrets: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: *color,
            })
            .collect();
        let collection = unsafe {
            self.cible_rt
                .CreateGradientStopCollection(&arrets, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
        }
        .map_err(|e| ErreurTexte::Systeme(format!("CreateGradientStopCollection : {}", e.message())))?;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: D2D_POINT_2F {
                x: debut.0,
                y: debut.1,
            },
            endPoint: D2D_POINT_2F { x: fin.0, y: fin.1 },
        };
        let brosse = unsafe { self.cible_rt.CreateLinearGradientBrush(&props, None, &collection) }
            .map_err(|e| {
                ErreurTexte::Systeme(format!("CreateLinearGradientBrush : {}", e.message()))
            })?;
        brosse
            .cast()
            .map_err(|e| ErreurTexte::Systeme(format!("ID2D1Brush : {}", e.message())))
    }
}

/// Couleur RGBA avec une opacité explicite (`fill-opacity` du SVG).
fn couleur_d2d_alpha(rgba: [u8; 4], alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: alpha,
    }
}

/// `color-mix(in srgb, couleur 70%, #000)` : assombrit sans changer l'alpha.
fn melange_noir(rgba: [u8; 4], ratio: f32) -> [u8; 4] {
    [
        (rgba[0] as f32 * ratio).round() as u8,
        (rgba[1] as f32 * ratio).round() as u8,
        (rgba[2] as f32 * ratio).round() as u8,
        rgba[3],
    ]
}

// =============================================================================
// Manette — état allumé d'un groupe + dégradés des 3 skins (`ivg*`/`ivgx*`/`ivgp*`)
// =============================================================================

/// Couleurs de l'état d'un groupe manette : `allume` + couleur de presse et
/// son halo translucide (regroupement pour `too_many_arguments`).
#[derive(Clone, Copy)]
struct PresseManette {
    allume: bool,
    couleur: D2D1_COLOR_F,
    halo: D2D1_COLOR_F,
}

/// Dégradé de coque (`ivg-corps` / `ivgx-corps` / `ivgp-corps`).
fn stops_corps(skin: SkinManette) -> [(f32, [u8; 4]); 3] {
    match skin {
        SkinManette::Bitdo => [
            (0.0, [0x3a, 0x3f, 0x4d, 255]),
            (0.45, [0x26, 0x2a, 0x33, 255]),
            (1.0, [0x14, 0x16, 0x1c, 255]),
        ],
        SkinManette::Xbox => [
            (0.0, [0x33, 0x37, 0x3f, 255]),
            (0.45, [0x21, 0x24, 0x2b, 255]),
            (1.0, [0x12, 0x14, 0x18, 255]),
        ],
        SkinManette::Ps => [
            (0.0, [0x34, 0x3a, 0x46, 255]),
            (0.45, [0x22, 0x26, 0x2e, 255]),
            (1.0, [0x13, 0x15, 0x19, 255]),
        ],
    }
}

/// Dégradé des boutons (`ivg-btn` / `ivgx-btn` / `ivgp-btn`).
fn stops_btn(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x4a, 0x50, 0x60, 255]), (1.0, [0x22, 0x25, 0x2d, 255])],
        SkinManette::Xbox => [(0.0, [0x45, 0x4b, 0x57, 255]), (1.0, [0x1f, 0x22, 0x29, 255])],
        SkinManette::Ps => [(0.0, [0x47, 0x4e, 0x5c, 255]), (1.0, [0x21, 0x24, 0x2c, 255])],
    }
}

/// Dégradé de la croix D-pad (`ivg-cross` / `ivgx-cross`) — pas de `ivgp-cross`
/// (le moyeu PS prend le dégradé bouton).
fn stops_cross(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x2f, 0x33, 0x3e, 255]), (1.0, [0x1c, 0x1f, 0x26, 255])],
        SkinManette::Xbox => [(0.0, [0x2c, 0x30, 0x38, 255]), (1.0, [0x19, 0x1c, 0x21, 255])],
        SkinManette::Ps => stops_btn(skin),
    }
}

/// Dégradé radial des creux (`ivg-dish` / `ivgx-dish` / `ivgp-dish`).
fn stops_dish(skin: SkinManette) -> [(f32, [u8; 4]); 2] {
    match skin {
        SkinManette::Bitdo => [(0.0, [0x20, 0x24, 0x2c, 255]), (1.0, [0x10, 0x13, 0x18, 255])],
        _ => [(0.0, [0x1e, 0x21, 0x28, 255]), (1.0, [0x0e, 0x11, 0x14, 255])],
    }
}

/// Boîte englobante (x0, y0, x1, y1) d'une `Forme` manette — base des
/// dégradés `objectBoundingBox`.
fn bornes_forme(forme: &Forme) -> (f32, f32, f32, f32) {
    match forme {
        Forme::Cercle { centre, rayon } => (
            centre.x - rayon,
            centre.y - rayon,
            centre.x + rayon,
            centre.y + rayon,
        ),
        Forme::Ellipse {
            centre,
            rayon_x,
            rayon_y,
        } => (
            centre.x - rayon_x,
            centre.y - rayon_y,
            centre.x + rayon_x,
            centre.y + rayon_y,
        ),
        Forme::Rect { rect, .. } => (
            rect.x,
            rect.y,
            rect.x + rect.largeur,
            rect.y + rect.hauteur,
        ),
        Forme::Chemin(c) => {
            // `bornes_chemin` rend (x milieu, y0, x1, y1) → x0 = 2·xm − x1.
            let (xm, y0, x1, y1) = bornes_chemin(c);
            (2.0 * xm - x1, y0, x1, y1)
        }
    }
}

/// Rect arrondi D2D d'un rect flottant.
fn rounded(rect: crate::input::RectF, rayon: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: D2D_RECT_F {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.largeur,
            bottom: rect.y + rect.hauteur,
        },
        radiusX: rayon.max(0.0),
        radiusY: rayon.max(0.0),
    }
}

/// Point D2D d'un `Point` de la géométrie input.
fn pt(p: crate::input::Point) -> D2D_POINT_2F {
    D2D_POINT_2F { x: p.x, y: p.y }
}

/// Bornes (x milieu, y haut, x droit, y bas) d'un `Chemin` — pour les
/// dégradés `objectBoundingBox` du SVG (direction verticale sur la boîte
/// englobante).
fn bornes_chemin(chemin: &crate::input::Chemin) -> (f32, f32, f32, f32) {
    let mut min = (f32::MAX, f32::MAX);
    let mut max = (f32::MIN, f32::MIN);
    let mut acc = |p: &crate::input::Point| {
        min = (min.0.min(p.x), min.1.min(p.y));
        max = (max.0.max(p.x), max.1.max(p.y));
    };
    for segment in &chemin.segments {
        match segment {
            Segment::Debut(p) | Segment::Ligne(p) => acc(p),
            Segment::Cubique(a, b, d) => {
                acc(a);
                acc(b);
                acc(d);
            }
            Segment::Fermer => {}
        }
    }
    ((min.0 + max.0) / 2.0, min.1, max.0, max.1)
}
