//! Widgets chat : mise en page DirectWrite des messages puis dessin des
//! lignes zébrées (compact) ou des bulles — miroir des règles CSS de
//! `diffusion.html` (`creerNoeudChat` / `renderChatMsg`, 2026-11-01).
//!
//! Composition d'un message (ordre du DOM) :
//! `badges emoji` → `logo plateforme` → `pseudo` → `:` → `texte` (emotes
//! remplacées par des images inline), plus l'**avatar** rond à gauche.
//!
//! Extrait de `texte.rs` (découpage 2026-10-29) : ces fonctions restent
//! des méthodes de `DessinateurTexte` — l'`impl` est scindé, le module
//! enfant voyant les champs privés du parent.
//!
//! Limites assumées du lot G :
//! - `backdrop-filter` (verre) et `box-shadow` non portés ;
//! - la bordure `border-image: linear-gradient(135deg, gf, gt)` est rendue en
//!   **teinte médiane unie** (1 px) — un dégradé par message coûterait une
//!   brosse par message et par frame pour un écart invisible à 1 px ;
//! - le cadre SVG par bulle (`decorerCadreMsg`) n'est pas porté : en mode
//!   `bulles` avec cadre actif, la bulle garde sa bordure unie ;
//! - `YT`/`TT` sont rendus en texte coloré, sans la pastille pleine du DOM.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows::core::{implement, Interface, IUnknown, PCWSTR};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_POINT_2F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1Brush, ID2D1EllipseGeometry, ID2D1Geometry, ID2D1Layer, ID2D1PathGeometry,
    ID2D1RenderTarget,
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
    D2D1_ELLIPSE, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteInlineObject, IDWriteInlineObject_Impl, IDWritePixelSnapping_Impl,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTextLayout1, IDWriteTextRenderer,
    IDWriteTextRenderer_Impl,
    DWRITE_BREAK_CONDITION_NEUTRAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_GLYPH_RUN,
    DWRITE_GLYPH_RUN_DESCRIPTION, DWRITE_INLINE_OBJECT_METRICS, DWRITE_MATRIX,
    DWRITE_MEASURING_MODE, DWRITE_OVERHANG_METRICS, DWRITE_STRIKETHROUGH, DWRITE_TEXT_METRICS,
    DWRITE_TEXT_RANGE, DWRITE_UNDERLINE, DWRITE_WORD_WRAPPING_WRAP,
};

use crate::chat::{badges_emoji, ChatCfg, FileChat, MessageChat};

use super::goal::{badge_plateforme, couleur_logo, logo_plateforme, viewbox_logo, LogoGoal};
use super::{couleur_d2d, DessinateurTexte, ErreurTexte, EtatGoals};

/// `vertical-align: middle` d'une image inline : la moitié de la boîte est
/// sous la ligne de base, décalée d'un demi x-height (`Segoe UI` ≈ 0,5 em).
fn base_inline(hauteur: f32, police: f32) -> f32 {
    hauteur / 2.0 + 0.25 * police
}

/// Dessin porté par un objet inline.
enum DessinInline {
    /// Emote (bitmap décodée du cache disque). La texture est possédée par
    /// l'objet : `ID2D1Bitmap1` ne garde pas sa ressource en vie.
    Bitmap {
        _texture: ID3D11Texture2D,
        bitmap: ID2D1Bitmap1,
    },
    /// Logo de plateforme (Twitch/Kick) — viewBox mise à l'échelle au dessin.
    Logo {
        geo: ID2D1PathGeometry,
        viewbox: f32,
        couleur: [u8; 4],
    },
}

/// Objet inline DirectWrite (emote ou logo).
///
/// `DrawTextLayout` passe la **cible Direct2D** en `clientDrawingContext` :
/// c'est là que l'image est peinte, à l'origine calculée par DirectWrite.
/// L'objet est référencé par le layout (`SetInlineObject`), donc son dessin
/// survit au bloc de rendu.
#[implement(IDWriteInlineObject)]
struct ObjetInline {
    dessin: DessinInline,
    largeur: f32,
    hauteur: f32,
    base: f32,
    /// Trace d'appel — vérifie que DirectWrite peint bien l'objet (test GPU).
    dessine: Arc<AtomicBool>,
}

impl IDWriteInlineObject_Impl for ObjetInline_Impl {
    fn Draw(
        &self,
        clientdrawingcontext: *const std::ffi::c_void,
        _renderer: Option<&IDWriteTextRenderer>,
        originx: f32,
        originy: f32,
        _issideways: BOOL,
        _isrighttoleft: BOOL,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        let contexte = clientdrawingcontext as *mut std::ffi::c_void;
        let Some(rt) =
            (unsafe { windows::core::from_raw_borrowed::<ID2D1RenderTarget>(&contexte) })
        else {
            return Ok(());
        };
        match &self.dessin {
            DessinInline::Bitmap { bitmap, .. } => {
                let dst = D2D_RECT_F {
                    left: originx,
                    top: originy,
                    right: originx + self.largeur,
                    bottom: originy + self.hauteur,
                };
                unsafe {
                    rt.DrawBitmap(
                        bitmap,
                        Some(&dst),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        None,
                    )
                };
            }
            DessinInline::Logo {
                geo,
                viewbox,
                couleur,
            } => {
                // ViewBox → boîte : `SetTransform` posée sur la cible puis
                // restaurée (la cible sert au reste du layout).
                let brosse = unsafe { rt.CreateSolidColorBrush(&couleur_d2d(*couleur), None) }?;
                let echelle = self.largeur / viewbox.max(1.0);
                let mut ancien = Matrix3x2::identity();
                unsafe { rt.GetTransform(&mut ancien) };
                let transforme = Matrix3x2 {
                    M11: echelle,
                    M22: echelle,
                    M31: originx,
                    M32: originy,
                    ..Matrix3x2::identity()
                };
                unsafe { rt.SetTransform(&transforme) };
                unsafe { rt.FillGeometry(geo, &brosse, None) };
                unsafe { rt.SetTransform(&ancien) };
            }
        }
        self.dessine.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn GetMetrics(&self) -> windows::core::Result<DWRITE_INLINE_OBJECT_METRICS> {
        Ok(DWRITE_INLINE_OBJECT_METRICS {
            width: self.largeur,
            height: self.hauteur,
            baseline: self.base,
            supportsSideways: BOOL(0),
        })
    }

    fn GetOverhangMetrics(&self) -> windows::core::Result<DWRITE_OVERHANG_METRICS> {
        Ok(DWRITE_OVERHANG_METRICS::default())
    }

    fn GetBreakConditions(
        &self,
        breakconditionbefore: *mut windows::Win32::Graphics::DirectWrite::DWRITE_BREAK_CONDITION,
        breakconditionafter: *mut windows::Win32::Graphics::DirectWrite::DWRITE_BREAK_CONDITION,
    ) -> windows::core::Result<()> {
        unsafe {
            *breakconditionbefore = DWRITE_BREAK_CONDITION_NEUTRAL;
            *breakconditionafter = DWRITE_BREAK_CONDITION_NEUTRAL;
        }
        Ok(())
    }
}

/// Renderer DirectWrite du chat — **indispensable aux objets inline** :
/// `ID2D1RenderTarget::DrawTextLayout` n'appelle jamais
/// `IDWriteInlineObject::Draw` (vérifié par test GPU : le contexte transmis
/// n'est pas la cible). On passe donc notre renderer et **notre** cible en
/// `clientDrawingContext`, que l'objet inline relit pour peindre l'image.
#[implement(IDWriteTextRenderer)]
struct RenduChat {
    cible: ID2D1RenderTarget,
    /// Brosse de repli (texte sans `SetDrawingEffect`).
    blanc: ID2D1Brush,
}

impl IDWritePixelSnapping_Impl for RenduChat_Impl {
    fn IsPixelSnappingDisabled(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
    ) -> windows::core::Result<BOOL> {
        Ok(BOOL(1))
    }

    fn GetCurrentTransform(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        transform: *mut DWRITE_MATRIX,
    ) -> windows::core::Result<()> {
        unsafe {
            *transform = DWRITE_MATRIX {
                m11: 1.0,
                m12: 0.0,
                m21: 0.0,
                m22: 1.0,
                dx: 0.0,
                dy: 0.0,
            };
        }
        Ok(())
    }

    fn GetPixelsPerDip(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
    ) -> windows::core::Result<f32> {
        Ok(1.0)
    }
}

impl IDWriteTextRenderer_Impl for RenduChat_Impl {
    fn DrawGlyphRun(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        baselineoriginx: f32,
        baselineoriginy: f32,
        measuringmode: DWRITE_MEASURING_MODE,
        glyphrun: *const DWRITE_GLYPH_RUN,
        _glyphrundescription: *const DWRITE_GLYPH_RUN_DESCRIPTION,
        clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        // `SetDrawingEffect` porte la brosse de couleur (pseudo, badges,
        // séparateur) ; sans effet, la brosse blanche du chat.
        let brosse: ID2D1Brush = clientdrawingeffect
            .and_then(|effet| effet.cast::<ID2D1Brush>().ok())
            .unwrap_or_else(|| self.blanc.clone());
        unsafe {
            self.cible.DrawGlyphRun(
                D2D_POINT_2F {
                    x: baselineoriginx,
                    y: baselineoriginy,
                },
                glyphrun,
                &brosse,
                measuringmode,
            )
        };
        Ok(())
    }

    fn DrawUnderline(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _underline: *const DWRITE_UNDERLINE,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        // Le chat n'utilise pas de soulignement (aucun style ne le pose).
        Ok(())
    }

    fn DrawStrikethrough(
        &self,
        _clientdrawingcontext: *const std::ffi::c_void,
        _baselineoriginx: f32,
        _baselineoriginy: f32,
        _strikethrough: *const DWRITE_STRIKETHROUGH,
        _clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        Ok(())
    }

    fn DrawInlineObject(
        &self,
        clientdrawingcontext: *const std::ffi::c_void,
        originx: f32,
        originy: f32,
        inlineobject: Option<&IDWriteInlineObject>,
        issideways: BOOL,
        isrighttoleft: BOOL,
        clientdrawingeffect: Option<&IUnknown>,
    ) -> windows::core::Result<()> {
        if let Some(objet) = inlineobject {
            // On **propage notre** contexte : l'objet y relit la cible
            // Direct2D et peint l'image (emote) ou le logo.
            unsafe {
                objet.Draw(
                    Some(clientdrawingcontext),
                    None,
                    originx,
                    originy,
                    issideways,
                    isrighttoleft,
                    clientdrawingeffect,
                )
            }?;
        }
        Ok(())
    }
}

/// Un message mis en page : layout DirectWrite + hauteur rendue.
struct MessageRendu {
    layout: IDWriteTextLayout,
    hauteur: f32,
    /// `nth-child` impair (index pair) — zébrage de `diffusion.html`.
    pair: bool,
    /// Brosse de la couleur du pseudo : le layout la référence.
    _brosse: Option<ID2D1SolidColorBrush>,
    /// Objets inline (logo + emotes) — le layout les référence.
    ///
    /// ⚠️ Une **brosse** n'y a pas sa place : `cast()` un
    /// `ID2D1SolidColorBrush` vers `IDWriteInlineObject` rend
    /// `E_NOINTERFACE` (tout message avec pseudo tombait en erreur, lot G).
    /// Les couleurs passent par `SetDrawingEffect`, qui retient la brosse.
    _inline: Vec<IDWriteInlineObject>,
    /// Avatar : texture (garde la bitmap en vie) + vue Direct2D.
    avatar: Option<(ID3D11Texture2D, ID2D1Bitmap1)>,
    /// Cercle de découpe de l'avatar (rayon = côté/2, centré en 0,0).
    avatar_geo: Option<ID2D1EllipseGeometry>,
    /// Largeur réservée par l'avatar (`1,8em`), 0 = pas d'avatar.
    avatar_largeur: f32,
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

/// `margin-right: 2px` du DOM entre pseudo, `:` et texte.
const MARGE_PSEUDO: f32 = 2.0;

/// Couleur médiane d'un dégradé (bordure de message, 1 px).
fn teinte_mediane(a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    let moy = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
    [
        moy(a[0], b[0]),
        moy(a[1], b[1]),
        moy(a[2], b[2]),
        moy(a[3], b[3]),
    ]
}

impl DessinateurTexte {
    /// Dessine les widgets chat — dans une passe ouverte.
    ///
    /// Chaque widget a son bloc mis en page (cache par version du buffer) :
    /// un nouveau message ne reconfigure **rien**, il invalide le bloc.
    /// Le plus récent est en bas, on pousse vers le haut ; ce qui dépasse est
    /// rogné au corps du widget (défilement automatique, pas de scroll manuel).
    /// La barre viewers (`.chat-viewers`) est dessinée en tête, sous le clip
    /// du widget : chrome indépendant des messages, elle vit même quand le
    /// buffer est vide — d'où l'absence de garde sur `file.version()`.
    pub fn dessiner_chat(
        &mut self,
        cfgs: &[ChatCfg],
        file: &FileChat,
        etat: &EtatGoals,
    ) -> Result<(), ErreurTexte> {
        if cfgs.is_empty() {
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

            // `.chat-viewers` en tête, sous le clip du widget — `bar_h` = 0
            // sans plateforme live ni en bulles (`:empty` → display:none) :
            // le corps garde alors tout le rect, zéro régression de layout.
            // Appel avant l'emprunt de `blocs` : la méthode prend `&mut self`.
            let bar_h = match self.barre_viewers_chat(cfg, etat) {
                Ok(h) => h,
                Err(e) => {
                    unsafe { self.cible_rt.PopAxisAlignedClip() };
                    return Err(e);
                }
            };
            let Some(bloc) = self.blocs.get(&cle) else {
                unsafe { self.cible_rt.PopAxisAlignedClip() };
                continue;
            };
            // `.chat-body flex:1` : les messages sont rognés sous la barre.
            let corps = D2D_RECT_F {
                top: y + bar_h,
                ..clip
            };
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&corps, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };

            // Bordure 1 px (`border-image` du DOM) : teinte médiane du dégradé
            // du cadre, repli `#e0e0e0 → #9a9a9a`.
            let bordure = teinte_mediane(cfg.couleurs.0, cfg.couleurs.1);
            let brosse_bordure =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(bordure), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse bordure chat : {}", e.message()))
                    })?;
            let brosse_anneau =
                unsafe { self.cible_rt.CreateSolidColorBrush(&couleur_d2d(cfg.couleurs.0), None) }
                    .map_err(|e| {
                        ErreurTexte::Systeme(format!("brosse anneau chat : {}", e.message()))
                    })?;

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
                // `border: 1px solid` — trait interne.
                unsafe {
                    self.cible_rt.DrawRectangle(
                        &D2D_RECT_F {
                            left: rect_fond.left + 0.5,
                            top: rect_fond.top + 0.5,
                            right: rect_fond.right - 0.5,
                            bottom: rect_fond.bottom - 0.5,
                        },
                        &brosse_bordure,
                        1.0,
                        None,
                    )
                };

                // Avatar rond (clip au cercle) + anneau 1 px de la couleur du
                // cadre (`avatarRing` : `0 0 0 1px gf`).
                let mut decalage = 0.0_f32;
                if message.avatar_largeur > 0.0 {
                    decalage = 4.0 + message.avatar_largeur + 6.0;
                    let cote = message.avatar_largeur;
                    let ax = x + g.pad_x + g.marge_gauche + 4.0;
                    let ay = curseur + g.marge_haut + 2.0;
                    if let (Some((_, bitmap)), Some(geo)) =
                        (message.avatar.as_ref(), message.avatar_geo.as_ref())
                    {
                        let masque: ID2D1Geometry = geo.cast().map_err(|e| {
                            ErreurTexte::Gpu(format!("ellipse avatar : {}", e.message()))
                        })?;
                        let mut params = D2D1_LAYER_PARAMETERS1 {
                            contentBounds: D2D_RECT_F {
                                left: ax,
                                top: ay,
                                right: ax + cote,
                                bottom: ay + cote,
                            },
                            geometricMask: std::mem::ManuallyDrop::new(Some(masque)),
                            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                            maskTransform: windows::Foundation::Numerics::Matrix3x2 {
                                M31: ax + cote / 2.0,
                                M32: ay + cote / 2.0,
                                ..windows::Foundation::Numerics::Matrix3x2::identity()
                            },
                            opacity: 1.0,
                            opacityBrush: std::mem::ManuallyDrop::new(None),
                            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                        };
                        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
                            .map_err(|e| {
                                ErreurTexte::Gpu(format!("layer avatar : {}", e.message()))
                            })?;
                        unsafe { self.ctx.PushLayer(&params, &layer) };
                        unsafe {
                            self.cible_rt.DrawBitmap(
                                bitmap,
                                Some(&D2D_RECT_F {
                                    left: ax,
                                    top: ay,
                                    right: ax + cote,
                                    bottom: ay + cote,
                                }),
                                1.0,
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                None,
                            )
                        };
                        unsafe { self.ctx.PopLayer() };
                        unsafe { std::mem::ManuallyDrop::drop(&mut params.geometricMask) };
                        unsafe {
                            self.cible_rt.DrawEllipse(
                                &D2D1_ELLIPSE {
                                    point: D2D_POINT_2F {
                                        x: ax + cote / 2.0,
                                        y: ay + cote / 2.0,
                                    },
                                    radiusX: cote / 2.0,
                                    radiusY: cote / 2.0,
                                },
                                &brosse_anneau,
                                1.0,
                                None,
                            )
                        };
                    }
                }

                let origine = D2D_POINT_2F {
                    x: x + g.pad_x + g.marge_gauche + decalage,
                    y: curseur + g.marge_haut,
                };
                self.dessiner_layout_chat(&message.layout, origine, &blanc)?;
                curseur += message.hauteur + g.gap;
            }
            unsafe { self.cible_rt.PopAxisAlignedClip() };
            unsafe { self.cible_rt.PopAxisAlignedClip() };
        }
        Ok(())
    }

    /// Dessine un layout de chat avec [`RenduChat`] — seul chemin qui
    /// peint réellement les objets inline (emotes, logo).
    fn dessiner_layout_chat(
        &self,
        layout: &IDWriteTextLayout,
        origine: D2D_POINT_2F,
        blanc: &ID2D1Brush,
    ) -> Result<(), ErreurTexte> {
        let renderer: IDWriteTextRenderer = RenduChat {
            cible: self.cible_rt.clone(),
            blanc: blanc.clone(),
        }
        .into();
        unsafe {
            layout.Draw(
                Some(self.cible_rt.as_raw() as *const std::ffi::c_void),
                &renderer,
                origine.x,
                origine.y,
            )
        }
        .map_err(|e| ErreurTexte::Systeme(format!("draw chat : {}", e.message())))
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
            let rendu = self.mettre_en_page_message(message, g, &format)?;
            total += rendu.hauteur;
            messages.push(MessageRendu {
                pair: index % 2 == 0,
                ..rendu
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

    /// Met en page **un** message (composition du DOM) :
    /// badges emoji + logo plateforme + pseudo coloré + `:` + texte, emotes
    /// remplacées par des images inline, avatar réservé à gauche.
    fn mettre_en_page_message(
        &mut self,
        message: &MessageChat,
        g: &ChatGeometrie,
        format: &IDWriteTextFormat,
    ) -> Result<MessageRendu, ErreurTexte> {
        // --- Avatar : espace réservé dès qu'une URL existe (comme l'`<img>`
        // du DOM, dont la boîte est dimensionnée avant le chargement).
        let avatar_largeur = if message.avatar.is_some() {
            1.8 * g.police
        } else {
            0.0
        };
        let decalage = if avatar_largeur > 0.0 {
            4.0 + avatar_largeur + 6.0
        } else {
            0.0
        };
        let largeur = (g.largeur_texte - decalage).max(1.0);

        // --- Composition du texte (ordre du DOM) ---------------------------
        let badges: String = badges_emoji(message.badges.as_deref())
            .iter()
            .map(|b| b.emoji())
            .collect();
        let badges_len = badges.encode_utf16().count();
        let logo = logo_plateforme(&message.plateforme);
        let badge_texte = badge_plateforme(&message.plateforme);
        // Twitch/Kick : un caractère de remplacement porteur de l'objet inline ;
        // YouTube/TikTok : leur pastille texte (rendue en gras coloré).
        let puce: &str = if logo.is_some() {
            "\u{FFFC}"
        } else {
            badge_texte.unwrap_or("")
        };
        let puce_len = puce.encode_utf16().count();
        let pseudo = message.pseudo.as_str();
        let pseudo_len = pseudo.encode_utf16().count();
        let contenu = if pseudo.is_empty() {
            format!("{badges}{puce}{}", message.texte)
        } else {
            format!("{badges}{puce}{pseudo}:{}", message.texte)
        };
        let utf16: Vec<u16> = contenu.encode_utf16().collect();
        let layout = unsafe { self.dwrite.CreateTextLayout(&utf16, format, largeur, 100_000.0) }
            .map_err(|e| ErreurTexte::Systeme(format!("CreateTextLayout chat : {}", e.message())))?;
        unsafe { layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP) }
            .map_err(|e| ErreurTexte::Systeme(format!("wrap chat : {}", e.message())))?;

        let mut inline: Vec<IDWriteInlineObject> = Vec::new();

        // Badges : `font-size: 0.9em`.
        if badges_len > 0 {
            let plage = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: badges_len as u32,
            };
            unsafe { layout.SetFontSize(0.9 * g.police, plage) }
                .map_err(|e| ErreurTexte::Systeme(format!("taille badges : {}", e.message())))?;
        }

        // Puce : logo vectoriel inline (Twitch/Kick) ou pastille texte.
        if puce_len > 0 {
            let plage = DWRITE_TEXT_RANGE {
                startPosition: badges_len as u32,
                length: puce_len as u32,
            };
            if let Some(logo) = logo {
                if let Some(objet) = self.objet_logo(logo, g.police)? {
                    unsafe { layout.SetInlineObject(&objet, plage) }.map_err(|e| {
                        ErreurTexte::Systeme(format!("logo inline chat : {}", e.message()))
                    })?;
                    inline.push(objet);
                }
            } else {
                unsafe { layout.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("gras puce : {}", e.message())))?;
                unsafe { layout.SetFontSize(0.55 * g.police, plage) }
                    .map_err(|e| ErreurTexte::Systeme(format!("taille puce : {}", e.message())))?;
                let couleur = match message.plateforme.as_str() {
                    "youtube" => [0xff, 0x00, 0x00, 255],
                    _ => [0x00, 0x00, 0x00, 255],
                };
                let brosse = unsafe {
                    self.cible_rt
                        .CreateSolidColorBrush(&couleur_d2d(couleur), None)
                }
                .map_err(|e| ErreurTexte::Systeme(format!("brosse puce : {}", e.message())))?;
                let inconnu: IUnknown = brosse
                    .cast()
                    .map_err(|e| ErreurTexte::Systeme(format!("IUnknown puce : {}", e.message())))?;
                unsafe { layout.SetDrawingEffect(&inconnu, plage) }.map_err(|e| {
                    ErreurTexte::Systeme(format!("couleur puce : {}", e.message()))
                })?;
            }
        }

        // Pseudo : gras + couleur IRCv3 (ou teinte déterministe).
        let mut brosse = None;
        if pseudo_len > 0 {
            let debut = badges_len + puce_len;
            let plage = DWRITE_TEXT_RANGE {
                startPosition: debut as u32,
                length: pseudo_len as u32,
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
            // `margin-right: 2px` du DOM : espacement **de fin** sur le seul
            // dernier caractère (une avance de fin sur toute la plage
            // espacerait chaque lettre du pseudo).
            let layout1: IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("IDWriteTextLayout1 : {}", e.message())))?;
            unsafe {
                layout1.SetCharacterSpacing(
                    0.0,
                    MARGE_PSEUDO,
                    0.0,
                    DWRITE_TEXT_RANGE {
                        startPosition: (debut + pseudo_len - 1) as u32,
                        length: 1,
                    },
                )
            }
            .map_err(|e| ErreurTexte::Systeme(format!("marge pseudo : {}", e.message())))?;
            brosse = Some(couleur);
        }

        // Séparateur `:` — `opacity: 0.7` (alpha 179).
        let base_texte = badges_len + puce_len + pseudo_len + usize::from(pseudo_len > 0);
        if pseudo_len > 0 {
            let plage = DWRITE_TEXT_RANGE {
                startPosition: (base_texte - 1) as u32,
                length: 1,
            };
            let gris = unsafe {
                self.cible_rt
                    .CreateSolidColorBrush(&couleur_d2d([0xe0, 0xe0, 0xe0, 179]), None)
            }
            .map_err(|e| ErreurTexte::Systeme(format!("brosse séparateur : {}", e.message())))?;
            let inconnu: IUnknown = gris
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("IUnknown séparateur : {}", e.message())))?;
            unsafe { layout.SetDrawingEffect(&inconnu, plage) }.map_err(|e| {
                ErreurTexte::Systeme(format!("couleur séparateur : {}", e.message()))
            })?;
            // `margin-right: 2px` de `.separateur` (une seule lettre).
            let layout1: IDWriteTextLayout1 = layout
                .cast()
                .map_err(|e| ErreurTexte::Systeme(format!("IDWriteTextLayout1 : {}", e.message())))?;
            unsafe { layout1.SetCharacterSpacing(0.0, MARGE_PSEUDO, 0.0, plage) }.map_err(|e| {
                ErreurTexte::Systeme(format!("marge séparateur : {}", e.message()))
            })?;
        }

        // Emotes : image inline si le fichier de cache existe, texte sinon
        // (décision « à la volée » : la 1re occurrence reste en texte).
        for emote in &message.emotes {
            if emote.debut >= emote.fin {
                continue;
            }
            let Some((texture, bitmap, taille)) = self.image_cache(&emote.url) else {
                continue;
            };
            let Some(objet) = objet_bitmap(texture, bitmap, taille, 1.5 * g.police, g.police) else {
                continue;
            };
            let plage = DWRITE_TEXT_RANGE {
                startPosition: (base_texte + emote.debut) as u32,
                length: (emote.fin - emote.debut) as u32,
            };
            if unsafe { layout.SetInlineObject(&objet, plage) }.is_ok() {
                inline.push(objet);
            }
        }

        // --- Avatar (bitmap + cercle de découpe) ---------------------------
        let avatar = message
            .avatar
            .as_deref()
            .and_then(|url| self.image_cache(url));
        let (avatar, avatar_geo) = match avatar {
            Some((texture, bitmap, _)) => {
                let geo = unsafe {
                    self.d2d.CreateEllipseGeometry(&D2D1_ELLIPSE {
                        point: D2D_POINT_2F { x: 0.0, y: 0.0 },
                        radiusX: avatar_largeur / 2.0,
                        radiusY: avatar_largeur / 2.0,
                    })
                }
                .map_err(|e| {
                    ErreurTexte::Systeme(format!("ellipse avatar : {}", e.message()))
                })?;
                (Some((texture, bitmap)), Some(geo))
            }
            None => (None, None),
        };

        let mut metriques = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metriques) }
            .map_err(|e| ErreurTexte::Systeme(format!("métriques chat : {}", e.message())))?;
        // Hauteur de la ligne = max(texte, avatar + margin-top 2 px).
        let hauteur = metriques
            .height
            .ceil()
            .max(if avatar_largeur > 0.0 {
                avatar_largeur + 2.0
            } else {
                0.0
            })
            + g.marge_haut
            + g.marge_bas;
        Ok(MessageRendu {
            layout,
            hauteur,
            pair: false,
            _brosse: brosse,
            _inline: inline,
            avatar,
            avatar_geo,
            avatar_largeur,
        })
    }

    /// Objet inline d'un logo de plateforme (Twitch/Kick) — 12×12 px fixes,
    /// `vertical-align: middle` (`.logo` du DOM).
    fn objet_logo(
        &mut self,
        logo: LogoGoal,
        police: f32,
    ) -> Result<Option<IDWriteInlineObject>, ErreurTexte> {
        // `.logo { width:12px; height:12px }` — taille fixe du DOM, pas en em.
        const COTE: f32 = 12.0;
        let geo = self.logo_goal(logo)?;
        let objet: IDWriteInlineObject = ObjetInline {
            dessin: DessinInline::Logo {
                geo,
                viewbox: viewbox_logo(logo),
                couleur: couleur_logo(logo),
            },
            largeur: COTE,
            hauteur: COTE,
            base: base_inline(COTE, police),
            dessine: Arc::new(AtomicBool::new(false)),
        }
        .into();
        Ok(Some(objet))
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

/// Objet inline d'une emote : bitmap mise à l'échelle sur `1,5em` de haut,
/// largeur au ratio de l'image (`.emote { height:1.5em; width:auto }`).
fn objet_bitmap(
    texture: ID3D11Texture2D,
    bitmap: ID2D1Bitmap1,
    taille: (f32, f32),
    hauteur: f32,
    police: f32,
) -> Option<IDWriteInlineObject> {
    let (lw, lh) = taille;
    if lw <= 0.0 || lh <= 0.0 || hauteur <= 0.0 {
        return None;
    }
    let largeur = hauteur * lw / lh;
    let objet: IDWriteInlineObject = ObjetInline {
        dessin: DessinInline::Bitmap {
            _texture: texture,
            bitmap,
        },
        largeur,
        hauteur,
        base: base_inline(hauteur, police),
        dessine: Arc::new(AtomicBool::new(false)),
    }
    .into();
    Some(objet)
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
    cfg.couleurs.hash(&mut h);
    g.police.to_bits().hash(&mut h);
    g.largeur_texte.to_bits().hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{ContexteGpu, DescTexture, Format};

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
        // Les couleurs du cadre changent la bordure → clé différente.
        let mut couleurs = chat(false);
        couleurs.couleurs = ([255, 0, 0, 255], [0, 0, 255, 255]);
        assert_ne!(
            cle_bloc(&couleurs, &geometrie_chat(&couleurs)),
            cle_bloc(&cfg, &g)
        );
    }

    #[test]
    fn teinte_mediane_et_base_inline_miroir_css() {
        assert_eq!(
            teinte_mediane([0xe0, 0xe0, 0xe0, 255], [0x9a, 0x9a, 0x9a, 255]),
            [0xbd, 0xbd, 0xbd, 255]
        );
        assert_eq!(
            teinte_mediane([0, 0, 0, 0], [255, 255, 255, 255]),
            [127, 127, 127, 127]
        );
        // `vertical-align: middle` : 1,5 em de haut, police 16 px
        // → 12 px sous la ligne de base + 4 px de demi x-height.
        assert_eq!(base_inline(24.0, 16.0), 16.0);
    }

    /// DirectWrite peint-il vraiment nos objets inline ? `DrawTextLayout`
    /// passe la cible Direct2D en `clientDrawingContext` : sans elle, les
    /// emotes et le logo resteraient invisibles (aucun pixel, aucune erreur).
    #[test]
    fn objet_inline_est_dessine_par_draw_text_layout() {
        let Ok(contexte) = ContexteGpu::creer(None) else {
            eprintln!("chat : pas de device D3D11, test ignoré");
            return;
        };
        let canvas = contexte
            .creer_texture(&DescTexture::new(Format::Bgra8, 128, 64).expect("desc"))
            .expect("canvas");
        let mut dessinateur = DessinateurTexte::nouveau(&contexte, &canvas).expect("dessinateur");

        let texture = dessinateur
            .contexte_texture(&DescTexture::new(Format::Bgra8, 8, 8).expect("desc bitmap"))
            .expect("texture");
        let ressource: windows::Win32::Graphics::Direct3D11::ID3D11Resource =
            texture.cast().expect("ressource");
        let ctx = unsafe { dessinateur.device_d3d.GetImmediateContext() }.expect("contexte");
        let pixels = vec![255u8; 8 * 8 * 4];
        unsafe {
            ctx.UpdateSubresource(
                Some(&ressource),
                0,
                None,
                pixels.as_ptr() as *const std::ffi::c_void,
                32,
                0,
            )
        };
        let bitmap = dessinateur.bitmap_media(&texture, false).expect("bitmap");

        let drapeau = Arc::new(AtomicBool::new(false));
        let objet: IDWriteInlineObject = ObjetInline {
            dessin: DessinInline::Bitmap {
                _texture: texture,
                bitmap,
            },
            largeur: 16.0,
            hauteur: 16.0,
            base: base_inline(16.0, 16.0),
            dessine: drapeau.clone(),
        }
        .into();

        let format = dessinateur.format_chat(16.0).expect("format");
        let utf16: Vec<u16> = "\u{FFFC}".encode_utf16().collect();
        let layout = unsafe { dessinateur.dwrite.CreateTextLayout(&utf16, &format, 64.0, 64.0) }
            .expect("layout");
        unsafe {
            layout.SetInlineObject(
                &objet,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: 1,
                },
            )
        }
        .expect("inline object");

        dessinateur.debut();
        let brosse: ID2D1Brush = unsafe {
            dessinateur
                .cible_rt
                .CreateSolidColorBrush(&couleur_d2d([255, 255, 255, 255]), None)
        }
        .expect("brosse")
        .into();
        dessinateur
            .dessiner_layout_chat(&layout, D2D_POINT_2F { x: 8.0, y: 8.0 }, &brosse)
            .expect("draw");
        dessinateur.fin().expect("fin");

        assert!(
            drapeau.load(Ordering::Relaxed),
            "DirectWrite n'a pas peint l'objet inline (clientDrawingContext ≠ cible ?)"
        );
    }
}
