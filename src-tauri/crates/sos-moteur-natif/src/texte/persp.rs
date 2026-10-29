//! Plans à perspective (`perspective(d) rotateX rotateY`, diffusion.html) :
//! les calques du plan sont rasterisés dans un offscreen taille canvas puis
//! projetés par l'effet `3DPerspectiveTransform` — `DrawBitmap` ne sait pas
//! faire de perspective.
//!
//! Extrait de `texte.rs` (découpage 2026-10-29) : ces fonctions restent
//! des méthodes de `DessinateurTexte` — l'`impl` est scindé, le module
//! enfant voyant les champs privés du parent.

use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D13DPerspectiveTransform, ID2D1Bitmap1, ID2D1Effect,
    D2D1_3DPERSPECTIVETRANSFORM_INTERPOLATION_MODE_LINEAR,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_BORDER_MODE, D2D1_3DPERSPECTIVETRANSFORM_PROP_DEPTH,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_INTERPOLATION_MODE,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_PERSPECTIVE_ORIGIN,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION,
    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION_ORIGIN,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_PROPERTY_TYPE_UNKNOWN,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BORDER_MODE_SOFT, D2D1_COMPOSITE_MODE_SOURCE_OVER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;

use crate::gpu::{ContexteGpu, DescTexture, Format};
use crate::media::PerspCfg;

use super::{couleur_d2d, octets, DessinateurTexte, ErreurTexte};

/// Un plan à perspective (`perspective(d) rotateX rotateY`, diffusion.html) :
/// les calques du plan (voile/média/capture/bordure/cadre d'un widget, ou le
/// média de fond) sont d'abord rasterisés dans `bitmap` — taille canvas,
/// coords canvas comme le DOM — puis l'effet `3DPerspectiveTransform`
/// projette le tout sur le canvas. `DrawBitmap` ne sait pas faire de
/// perspective ; la texture intermédiaire est incontournable.
pub(super) struct PlanPersp {
    cfg: PerspCfg,
    bitmap: ID2D1Bitmap1,
    effet: ID2D1Effect,
    /// Texture porteuse de `bitmap` (la bitmap ne la possède pas).
    _texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
    /// Contenu peint cette frame : un plan vierge n'est pas projeté.
    dirty: bool,
}

impl DessinateurTexte {
    /// Crée l'offscreen + l'effet de projection de chaque plan à
    /// perspective (taille du canvas : les calques y dessinent en coords
    /// canvas, comme le DOM — l'effet projette ensuite le plan entier).
    pub fn configurer_persps(
        &mut self,
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        persps: &[PerspCfg],
    ) -> Result<(), ErreurTexte> {
        self.persps.clear();
        for cfg in persps {
            let desc = DescTexture::new(Format::Bgra8, largeur, hauteur)
                .map_err(|e| ErreurTexte::Gpu(e.to_string()))?;
            let texture = contexte
                .creer_texture_echantillonnable(&desc)
                .map_err(|e| ErreurTexte::Gpu(format!("offscreen persp : {e}")))?;
            let surface: IDXGISurface = texture
                .cast()
                .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface persp : {}", e.message())))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    // Le plan garde son alpha : les zones vides de
                    // l'offscreen ne recouvrent pas le canvas.
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                colorContext: ManuallyDrop::new(None),
            };
            let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
                .map_err(|e| ErreurTexte::Gpu(format!("bitmap persp : {}", e.message())))?;
            let effet = unsafe { self.ctx.CreateEffect(&CLSID_D2D13DPerspectiveTransform) }
                .map_err(|e| ErreurTexte::Gpu(format!("effet persp : {}", e.message())))?;
            unsafe { effet.SetInput(0, &bitmap, true) };
            let (cx, cy) = (
                (cfg.rect.0 + cfg.rect.2 / 2.0) as f32,
                (cfg.rect.1 + cfg.rect.3 / 2.0) as f32,
            );
            // Même sémantique que `perspective(d) rotateX() rotateY()` avec
            // `transform-origin: center` : rotation en degrés, origines au
            // centre de la boîte, profondeur = la constante CSS.
            // `D2D1_PROPERTY_TYPE_UNKNOWN` = « type déjà déclaré par l'effet » :
            // les enums s'enregistrent en `ENUM`, pas `UINT32` (E_INVALIDARG).
            let interpolation = D2D1_3DPERSPECTIVETRANSFORM_INTERPOLATION_MODE_LINEAR.0 as u32;
            let bordure = D2D1_BORDER_MODE_SOFT.0 as u32;
            let profondeur = cfg.profondeur as f32;
            let origine_p = [cx, cy];
            let origine_r = [cx, cy, 0.0];
            let rotation = [cfg.rot_x as f32, cfg.rot_y as f32, 0.0];
            let props: [(u32, &[u8]); 6] = [
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_INTERPOLATION_MODE.0 as u32,
                    octets(&interpolation),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_BORDER_MODE.0 as u32,
                    octets(&bordure),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_DEPTH.0 as u32,
                    octets(&profondeur),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_PERSPECTIVE_ORIGIN.0 as u32,
                    octets(&origine_p),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION_ORIGIN.0 as u32,
                    octets(&origine_r),
                ),
                (
                    D2D1_3DPERSPECTIVETRANSFORM_PROP_ROTATION.0 as u32,
                    octets(&rotation),
                ),
            ];
            for (idx, data) in props {
                unsafe { effet.SetValue(idx, D2D1_PROPERTY_TYPE_UNKNOWN, data) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("propriété persp {idx} : {}", e.message()))
                })?;
            }
            self.persps.push(PlanPersp {
                cfg: cfg.clone(),
                bitmap,
                effet,
                _texture: texture,
                dirty: false,
            });
        }
        Ok(())
    }

    /// Efface chaque offscreen de perspective en transparent — début de
    /// frame, puisque chaque plan est re-rasterisé entièrement.
    pub fn effacer_persps(&mut self) {
        if self.persps.is_empty() {
            return;
        }
        for p in &mut self.persps {
            unsafe { self.ctx.SetTarget(&p.bitmap) };
            unsafe { self.cible_rt.Clear(Some(&couleur_d2d([0, 0, 0, 0]))) };
            p.dirty = false;
        }
        unsafe { self.ctx.SetTarget(&self.cible_bitmap) };
    }

    /// Plan à perspective couvrant (`rect`, `z`) — clé de regroupement des
    /// calques d'un widget (tous partagent son rect et son z, voir
    /// `persps_depuis_scene`). `None` = dessiner sur le canvas.
    pub fn groupe_de(&self, rect: (f64, f64, f64, f64), z: i32) -> Option<usize> {
        self.persps
            .iter()
            .position(|p| p.cfg.z == z && p.cfg.rect == rect)
    }

    /// Redirige la cible de dessin : offscreen du plan, ou canvas.
    ///
    /// `SetTarget` en cours de passe est légal sur `ID2D1DeviceContext` ;
    /// les clips/transforms des `dessiner_*` sont équilibrés avant retour,
    /// le basculement entre deux calques est donc sûr.
    pub fn cible_pour(&mut self, groupe: Option<usize>) {
        match groupe {
            Some(g) => {
                let p = &mut self.persps[g];
                p.dirty = true;
                unsafe { self.ctx.SetTarget(&p.bitmap) };
            }
            None => unsafe { self.ctx.SetTarget(&self.cible_bitmap) },
        }
    }

    /// Projette sur le canvas les plans dépassés (`cfg.z < z`) qui ont du
    /// contenu — le plan transformé se pose à son niveau de z, comme le
    /// `.widget` DOM dans son stacking context.
    pub fn flusher_persps(&mut self, z: i32) -> Result<(), ErreurTexte> {
        // `persps` est trié par z à la configuration — l'ordre de balayage
        // est déjà l'ordre de projection.
        for g in 0..self.persps.len() {
            if self.persps[g].dirty && self.persps[g].cfg.z < z {
                self.peindre_persp(g)?;
            }
        }
        Ok(())
    }

    /// Projette tous les plans restants — fin du plan de composition.
    pub fn flusher_persps_fin(&mut self) -> Result<(), ErreurTexte> {
        self.flusher_persps(i32::MAX)
    }

    /// Projette un plan sur le canvas : sortie de l'effet
    /// `3DPerspectiveTransform`, puis `scale(bgZ)` éventuel appliqué en
    /// espace écran autour du centre (`scale` en premier sur `#bg`).
    fn peindre_persp(&mut self, g: usize) -> Result<(), ErreurTexte> {
        unsafe { self.ctx.SetTarget(&self.cible_bitmap) };
        let p = &self.persps[g];
        let ze = p.cfg.zoom_ecran as f32;
        let (cx, cy) = (
            (p.cfg.rect.0 + p.cfg.rect.2 / 2.0) as f32,
            (p.cfg.rect.1 + p.cfg.rect.3 / 2.0) as f32,
        );
        let image = unsafe { p.effet.GetOutput() }
            .map_err(|e| ErreurTexte::Gpu(format!("sortie effet persp : {}", e.message())))?;
        unsafe {
            let mut avant = Matrix3x2::identity();
            if ze != 1.0 {
                self.cible_rt.GetTransform(&mut avant);
                let t_mc = Matrix3x2::translation(-cx, -cy);
                let t_c = Matrix3x2::translation(cx, cy);
                let z = Matrix3x2 {
                    M11: ze,
                    M12: 0.0,
                    M21: 0.0,
                    M22: ze,
                    M31: 0.0,
                    M32: 0.0,
                };
                self.cible_rt.SetTransform(&(t_mc * z * t_c * avant));
            }
            self.ctx.DrawImage(
                &image,
                None,
                None,
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            if ze != 1.0 {
                self.cible_rt.SetTransform(&avant);
            }
        }
        self.persps[g].dirty = false;
        Ok(())
    }
}
