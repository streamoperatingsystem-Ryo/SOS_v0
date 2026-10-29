//! Médias dans le canvas BGRA : `DrawBitmap` sous fit/transform/clip,
//! chaîne d'effets `chaineFilter` (`ColorMatrix` → `GaussianBlur`),
//! pixelisation (`mediaPixel`/`bgPixel`, upscale sans lissage) et morphs
//! bulge/pinch (offscreen → `WarpMorph` D3D11 → boîte).
//!
//! Extrait de `texte.rs` (découpage 2026-10-29) : ces fonctions restent
//! des méthodes de `DessinateurTexte` — l'`impl` est scindé, le module
//! enfant voyant les champs privés du parent.

use std::hash::{Hash, Hasher};
use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_POINT_2F,
    D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1Effect, ID2D1Geometry, ID2D1Image, ID2D1Layer, ID2D1PathGeometry,
    CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, D2D1_COLORMATRIX_PROP_ALPHA_MODE,
    D2D1_COLORMATRIX_PROP_COLOR_MATRIX, D2D1_GAUSSIANBLUR_PROP_BORDER_MODE,
    D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
    D2D1_BITMAP_OPTIONS, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_PROPERTY_TYPE_UNKNOWN,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BORDER_MODE_SOFT, D2D1_COLORMATRIX_ALPHA_MODE_PREMULTIPLIED,
    D2D1_COLORMATRIX_ALPHA_MODE_STRAIGHT, D2D1_COMPOSITE_MODE_SOURCE_OVER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGISurface;

use crate::media::{EffetsCfg, Fit, Transform2D};

use super::{couleur_d2d, octets, rect_visible, CleBoite, DessinateurTexte, ErreurTexte};

/// Paramètres de tracé d'un média dans sa boîte — regroupement clippy
/// (`too_many_arguments`, convention §37.4.5).
pub struct TraceMedia<'a> {
    /// Destination du fit en coords canvas (`geometrie` ou `fit_rects`).
    pub destination: RECT,
    /// Portion source du fit.
    pub source: RECT,
    /// Boîte du widget (ou le canvas pour le fond) — clip `overflow`.
    pub boite: RECT,
    /// `Transform2D::matrice` autour du centre de `boite` — `None` = neutre.
    pub transforme: Option<Matrix3x2>,
    /// Polygone du cadre — couche masquée ; `None` = clip axe-aligné.
    pub clip: Option<&'a ID2D1PathGeometry>,
    /// Chaîne `chaineFilter` — `EffetsCfg::NEUTRE` = `DrawBitmap` historique.
    pub effets: &'a EffetsCfg,
    /// Opacité globale du média — fondu du carrousel (`opacity` des
    /// `.slide` / `globalAlpha` de `dessinerContenuFond`). 1,0 hors fondu.
    pub opacite: f32,
}

/// Paramètres d'un média morphé (bulge/pinch, lot T4) — regroupement
/// clippy (`too_many_arguments`, convention §37.4.5).
pub struct TraceMorphe<'a> {
    /// Taille source du média.
    pub taille: (u32, u32),
    /// Boîte en coords canvas — l'offscreen vaut `w×h` de la boîte.
    pub boite: (f64, f64, f64, f64),
    pub fit: Fit,
    /// Transform 2D complet (`zoom_ecran` est retiré à l'appel — il vit
    /// hors de l'offscreen : `scale(bgZ)` est appliqué au plan entier).
    pub transform: &'a Transform2D,
    /// Polygone du cadre — couche masquée ; `None` = clip axe-aligné.
    pub clip: Option<&'a ID2D1PathGeometry>,
    /// Chaîne `chaineFilter` dessinée **dans** l'offscreen (`ctx.filter`).
    pub effets: &'a EffetsCfg,
    /// Points de morph (`w.morphs` / `scene.bgMorphs`).
    pub morphs: &'a [sos_scene::MorphPoint],
    /// Pixelisation en chemin morph (`mediaPixel` / `bgPixel`).
    pub pixel_morph: f64,
}

/// Paramètres d'un média pixelisé (`bgPixel`, chemin canvas du fond) —
/// regroupement clippy (`too_many_arguments`, convention §37.4.5).
pub struct TracePixel<'a> {
    /// Taille source du média (fit recalculé dans le petit espace).
    pub taille: (u32, u32),
    /// Boîte en coords canvas — le petit buffer vaut `boite / pf`.
    pub boite: (f64, f64, f64, f64),
    pub fit: Fit,
    /// Transform 2D complet (`zoom_ecran` appliqué à l'upscale).
    pub transform: &'a Transform2D,
    /// Polygone du cadre — couche masquée ; `None` = clip axe-aligné.
    pub clip: Option<&'a ID2D1PathGeometry>,
    /// Chaîne `chaineFilter` + facteur `pixel`.
    pub effets: &'a EffetsCfg,
    /// Opacité globale — fondu du carrousel de fond (`globalAlpha = kFade`).
    pub opacite: f32,
}

impl DessinateurTexte {
    /// Vue Direct2D d'une texture média (créée une fois, réutilisée).
    ///
    /// Clé = (pointeur de la texture, alpha) : les médias vivent toute la
    /// session. La texture doit porter `D3D11_BIND_SHADER_RESOURCE` (voir
    /// `ContexteGpu::creer_texture_echantillonnable`).
    ///
    /// `alpha` = le média porte de l'alpha par pixel (`MediaChargee::alpha`) :
    /// les pixels sont alors **prémultipliés** et Direct2D doit le savoir,
    /// sinon les zones transparentes ressortent sur un rectangle noir
    /// (défaut D1 du 2026-10-28). Une vidéo ou une image opaque reste en
    /// `IGNORE` : l'octet alpha d'un RGB32 n'est pas garanti.
    pub fn bitmap_media(
        &mut self,
        texture: &windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        alpha: bool,
    ) -> Result<ID2D1Bitmap1, ErreurTexte> {
        let cle = (texture.as_raw() as usize, alpha);
        if let Some((_, bitmap)) = self.bitmaps.get(&cle) {
            return Ok(bitmap.clone());
        }
        let surface: IDXGISurface = texture
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface média : {}", e.message())))?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: if alpha {
                    D2D1_ALPHA_MODE_PREMULTIPLIED
                } else {
                    D2D1_ALPHA_MODE_IGNORE
                },
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| ErreurTexte::Gpu(format!("bitmap média : {}", e.message())))?;
        self.bitmaps.insert(cle, (texture.clone(), bitmap.clone()));
        Ok(bitmap)
    }

    /// Dessine un média dans sa boîte — dans une passe ouverte.
    ///
    /// `source`/`destination` viennent de `media::geometrie` (fit + rognage
    /// canvas, chemin neutre) ou de `media::fit_rects` (fit seul, chemin
    /// transformé). `boite` = rect du widget (ou le canvas pour le fond) :
    /// c'est le clip `overflow: hidden` quand il n'y a pas de cadre, posé
    /// **avant** le transform pour rester dans l'espace de la boîte comme le
    /// clip-path CSS. `transforme` = matrice `Transform2D::matrice` (zoom,
    /// rotation, offset, miroir, zoom écran) autour du centre de `boite`.
    /// `clip` = polygone du cadre : couche masquée, même rôle que le clip
    /// axe-aligné — le média transformé est rogné, jamais le masque.
    /// `effets` = la chaîne `chaineFilter` (`ColorMatrix` [→ `GaussianBlur`])
    /// — `EffetsCfg::NEUTRE` = le `DrawBitmap` historique.
    pub fn dessiner_media(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &TraceMedia,
    ) -> Result<(), ErreurTexte> {
        let (transforme, clip, effets) = (t.transforme, t.clip, t.effets);
        let Some(dst) = rect_visible(t.destination) else {
            // Destination vide : rien à peindre.
            return Ok(());
        };
        let src = D2D_RECT_F {
            left: t.source.left as f32,
            top: t.source.top as f32,
            right: t.source.right as f32,
            bottom: t.source.bottom as f32,
        };
        let boite_f = D2D_RECT_F {
            left: t.boite.left as f32,
            top: t.boite.top as f32,
            right: t.boite.right as f32,
            bottom: t.boite.bottom as f32,
        };
        let trace = |s: &mut Self| -> Result<(), ErreurTexte> {
            if effets.couleur_active() || effets.flou > 0.0 {
                let image = s.image_effets(bitmap, src, dst, effets)?;
                let image = if t.opacite < 1.0 {
                    s.image_opacite(&image, t.opacite)?
                } else {
                    image
                };
                s.tracer_image(&image, src, dst, transforme, marge_flou(effets, src, dst));
            } else {
                s.tracer_bitmap(bitmap, dst, src, transforme, t.opacite);
            }
            Ok(())
        };
        let Some(clip) = clip else {
            // Sans cadre : `overflow: hidden` du widget = clip axe-aligné
            // sur la boîte (même mode que les clips chat/input du fichier).
            // Posé avant le transform : la boîte rogne le média transformé.
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&boite_f, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            trace(self)?;
            unsafe { self.cible_rt.PopAxisAlignedClip() };
            return Ok(());
        };
        let masque: ID2D1Geometry = clip
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            // Sous transform le contenu déborde `dst` : borner à la boîte.
            contentBounds: if transforme.is_some() { boite_f } else { dst },
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer média : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        trace(self)?;
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
    }

    /// `DrawBitmap` sous transform éventuel — dans une passe et un clip
    /// déjà posés. Le transform existant est restauré ensuite (la cible
    /// est à l'identité en pratique, mais le motif `avant` du fichier est
    /// suivi).
    fn tracer_bitmap(
        &self,
        bitmap: &ID2D1Bitmap1,
        dst: D2D_RECT_F,
        src: D2D_RECT_F,
        transforme: Option<Matrix3x2>,
        opacite: f32,
    ) {
        unsafe {
            let mut avant = Matrix3x2::identity();
            if let Some(m) = transforme {
                self.cible_rt.GetTransform(&mut avant);
                self.cible_rt.SetTransform(&(m * avant));
            }
            self.cible_rt.DrawBitmap(
                bitmap,
                Some(&dst),
                opacite,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&src),
            );
            if transforme.is_some() {
                self.cible_rt.SetTransform(&avant);
            }
        }
    }

    /// `image × opacite` — `ColorMatrix` diagonale en alpha **prémultiplié**
    /// (entrées/sorties déjà PBGRA : k uniforme sur r,g,b,a = l'`opacity`/
    /// `globalAlpha` du DOM). Construite à chaque appel : `k` varie à chaque
    /// frame de fondu — le cache `cle_effets` n'y a pas de sens.
    fn image_opacite(
        &mut self,
        image: &ID2D1Image,
        opacite: f32,
    ) -> Result<ID2D1Image, ErreurTexte> {
        let e = unsafe { self.ctx.CreateEffect(&CLSID_D2D1ColorMatrix) }
            .map_err(|e| ErreurTexte::Gpu(format!("effet opacité : {}", e.message())))?;
        unsafe { e.SetInput(0, image, true) };
        let mut matrice = [[0.0f32; 4]; 5];
        for (i, ligne) in matrice.iter_mut().take(4).enumerate() {
            ligne[i] = opacite;
        }
        for (idx, data) in [
            (
                D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32,
                octets(&matrice) as &[u8],
            ),
            (
                D2D1_COLORMATRIX_PROP_ALPHA_MODE.0 as u32,
                octets(&(D2D1_COLORMATRIX_ALPHA_MODE_PREMULTIPLIED.0 as u32)),
            ),
        ] {
            unsafe { e.SetValue(idx, D2D1_PROPERTY_TYPE_UNKNOWN, data) }.map_err(|e| {
                ErreurTexte::Gpu(format!("propriété opacité {idx} : {}", e.message()))
            })?;
        }
        unsafe { e.GetOutput() }
            .map_err(|e| ErreurTexte::Gpu(format!("sortie opacité : {}", e.message())))
    }

    /// Sortie de la chaîne d'effets d'un média, construite une fois par
    /// (bitmap, matrice, σ) : `bitmap → [ColorMatrix] → [GaussianBlur]`
    /// — même ordre que `chaineFilter` (le `blur` est dernier).
    ///
    /// `src`/`dst` servent à convertir `flou` (px de l'espace destination —
    /// `blur(px)` sur l'élément DOM) en écart-type de l'espace source :
    /// `σ_src = flou / s_fit`, `s_fit` = moyenne géométrique des échelles
    /// (approximation isotrope sous étirement non uniforme).
    fn image_effets(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        src: D2D_RECT_F,
        dst: D2D_RECT_F,
        effets: &EffetsCfg,
    ) -> Result<ID2D1Image, ErreurTexte> {
        let s = echelle_fit(src, dst);
        let sigma = (effets.flou / s) as f32;
        let cle = cle_effets(bitmap, effets, sigma);
        if let Some(chaine) = self.effets.get(&cle) {
            if let Some(sortie) = chaine.last() {
                return unsafe { sortie.GetOutput() }.map_err(|e| {
                    ErreurTexte::Gpu(format!("sortie effet média : {}", e.message()))
                });
            }
        }
        let mut chaine: Vec<ID2D1Effect> = Vec::new();
        if effets.couleur_active() {
            let matrice = effets.matrice_couleur();
            let e = unsafe { self.ctx.CreateEffect(&CLSID_D2D1ColorMatrix) }
                .map_err(|e| ErreurTexte::Gpu(format!("effet couleur : {}", e.message())))?;
            unsafe { e.SetInput(0, bitmap, true) };
            for (idx, data) in [
                (
                    D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32,
                    octets(&matrice) as &[u8],
                ),
                (
                    D2D1_COLORMATRIX_PROP_ALPHA_MODE.0 as u32,
                    octets(&(D2D1_COLORMATRIX_ALPHA_MODE_STRAIGHT.0 as u32)),
                ),
            ] {
                unsafe { e.SetValue(idx, D2D1_PROPERTY_TYPE_UNKNOWN, data) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("propriété couleur {idx} : {}", e.message()))
                })?;
            }
            chaine.push(e);
        }
        if sigma > 0.0 {
            let e = unsafe { self.ctx.CreateEffect(&CLSID_D2D1GaussianBlur) }
                .map_err(|e| ErreurTexte::Gpu(format!("effet flou : {}", e.message())))?;
            match chaine.last() {
                Some(couleur) => {
                    let sortie = unsafe { couleur.GetOutput() }.map_err(|e| {
                        ErreurTexte::Gpu(format!("sortie couleur : {}", e.message()))
                    })?;
                    unsafe { e.SetInput(0, &sortie, true) };
                }
                None => unsafe { e.SetInput(0, bitmap, true) },
            }
            for (idx, data) in [
                (
                    D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                    octets(&sigma) as &[u8],
                ),
                (
                    D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
                    octets(&(D2D1_BORDER_MODE_SOFT.0 as u32)),
                ),
            ] {
                unsafe { e.SetValue(idx, D2D1_PROPERTY_TYPE_UNKNOWN, data) }.map_err(|e| {
                    ErreurTexte::Gpu(format!("propriété flou {idx} : {}", e.message()))
                })?;
            }
            chaine.push(e);
        }
        let Some(sortie) = chaine.last() else {
            return Err(ErreurTexte::Gpu("chaîne d'effets vide".to_string()));
        };
        let image = unsafe { sortie.GetOutput() }
            .map_err(|e| ErreurTexte::Gpu(format!("sortie effet média : {}", e.message())))?;
        self.effets.insert(cle, chaine);
        Ok(image)
    }

    /// `DrawImage` d'une sortie d'effet — l'image vit en **espace source**
    /// (mêmes pixels que la bitmap) et `DrawImage` ne sait pas scaler : la
    /// paire `src`→`dst` du fit devient le transform
    /// `T(−src)·S(dst/src)·T(dst)` enfilé dans le transform éventuel.
    /// `marge` = débord du flou (3σ) dans l'image : l'`imageRectangle` est
    /// étendu d'autant, sinon le halo serait rogné à la source.
    fn tracer_image(
        &self,
        image: &ID2D1Image,
        src: D2D_RECT_F,
        dst: D2D_RECT_F,
        transforme: Option<Matrix3x2>,
        marge: f32,
    ) {
        let sw = (src.right - src.left).max(f32::EPSILON);
        let sh = (src.bottom - src.top).max(f32::EPSILON);
        let m_fit = Matrix3x2::translation(-src.left, -src.top)
            * Matrix3x2 {
                M11: (dst.right - dst.left) / sw,
                M12: 0.0,
                M21: 0.0,
                M22: (dst.bottom - dst.top) / sh,
                M31: 0.0,
                M32: 0.0,
            }
            * Matrix3x2::translation(dst.left, dst.top);
        let m = transforme.map(|t| m_fit * t).unwrap_or(m_fit);
        let echantillon = D2D_RECT_F {
            left: src.left - marge,
            top: src.top - marge,
            right: src.right + marge,
            bottom: src.bottom + marge,
        };
        let offset = D2D_POINT_2F {
            x: echantillon.left,
            y: echantillon.top,
        };
        unsafe {
            let mut avant = Matrix3x2::identity();
            self.cible_rt.GetTransform(&mut avant);
            self.cible_rt.SetTransform(&(m * avant));
            self.ctx.DrawImage(
                image,
                Some(&offset),
                Some(&echantillon),
                D2D1_INTERPOLATION_MODE_LINEAR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            self.cible_rt.SetTransform(&avant);
        }
    }

    /// Bitmap réduit `largeur×hauteur` du chemin de pixelisation —
    /// renderable + échantillonnable, PBGRA (mêmes propriétés que les
    /// offscreens de perspective).
    fn bitmap_petit(
        &mut self,
        largeur: u32,
        hauteur: u32,
    ) -> Result<ID2D1Bitmap1, ErreurTexte> {
        if let Some((_, bitmap)) = self.petits.get(&(largeur, hauteur)) {
            return Ok(bitmap.clone());
        }
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: largeur,
            Height: hauteur,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> = None;
        unsafe {
            self.device_d3d
                .CreateTexture2D(&desc, None, Some(&mut texture))
        }
        .map_err(|e| ErreurTexte::Gpu(format!("texture petit : {}", e.message())))?;
        let texture = texture
            .ok_or_else(|| ErreurTexte::Gpu("texture petit nulle".to_string()))?;
        let surface: IDXGISurface = texture
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface petit : {}", e.message())))?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: ManuallyDrop::new(None),
        };
        let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
            .map_err(|e| ErreurTexte::Gpu(format!("bitmap petit : {}", e.message())))?;
        self.petits
            .insert((largeur, hauteur), (texture, bitmap.clone()));
        Ok(bitmap)
    }

    /// Média morphé (bulge/pinch, lot T4) : le contenu (fit + transform +
    /// effets + pixelisation) est d'abord rasterisé dans un offscreen
    /// **taille boîte** — miroir de `entry.morphOffscreen` — puis le pixel
    /// shader `WarpMorph` (réplique de `MORPH_FRAG`) le warpe dans une
    /// sortie dessinée sur la boîte sous le clip.
    pub fn dessiner_media_morphe(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &TraceMorphe,
    ) -> Result<(), ErreurTexte> {
        let (bw, bh) = (
            (t.boite.2.round() as u32).max(1),
            (t.boite.3.round() as u32).max(1),
        );
        let cle = (
            t.boite.0.to_bits(),
            t.boite.1.to_bits(),
            t.boite.2.to_bits(),
            t.boite.3.to_bits(),
        );
        let (off_bmp, srv, sortie_bmp, rtv) = self.buffers_morphe(cle, bw, bh)?;
        // 1. Contenu dans l'offscreen, en espace boîte local (0,0 → bw,bh)
        //    — même dessin que le canvas DOM : fit + transform + effets.
        let cible_avant = unsafe { self.ctx.GetTarget() }.ok();
        unsafe { self.ctx.SetTarget(&off_bmp) };
        unsafe { self.cible_rt.Clear(Some(&couleur_d2d([0, 0, 0, 0]))) };
        self.peindre_media_local(bitmap, t, (bw, bh))?;
        if let Some(cible) = &cible_avant {
            unsafe { self.ctx.SetTarget(cible) };
        }
        // 2. Warp D3D11 (mêmes coords normalisées que le WebGL).
        if self.warp.is_none() {
            self.warp = Some(
                crate::morph::WarpMorph::nouveau(&self.device_d3d)
                    .map_err(|e| ErreurTexte::Gpu(e.to_string()))?,
            );
        }
        unsafe { self.ctx.Flush(None, None) }
            .map_err(|e| ErreurTexte::Gpu(format!("Flush morph : {}", e.message())))?;
        self.warp
            .as_ref()
            .ok_or_else(|| ErreurTexte::Gpu("warp absent".to_string()))?
            .warper(&srv, &rtv, t.morphs, bw, bh)
            .map_err(|e| ErreurTexte::Gpu(e.to_string()))?;
        // 3. Sortie 1:1 sur la boîte, sous le clip (cadre ou overflow).
        let boite_f = D2D_RECT_F {
            left: t.boite.0 as f32,
            top: t.boite.1 as f32,
            right: (t.boite.0 + t.boite.2) as f32,
            bottom: (t.boite.1 + t.boite.3) as f32,
        };
        let src_tout = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: bw as f32,
            bottom: bh as f32,
        };
        let trace = |s: &Self| unsafe {
            s.cible_rt.DrawBitmap(
                &sortie_bmp,
                Some(&boite_f),
                1.0,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&src_tout),
            );
        };
        let Some(masque_geo) = t.clip else {
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&boite_f, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            trace(self);
            unsafe { self.cible_rt.PopAxisAlignedClip() };
            return Ok(());
        };
        let masque: ID2D1Geometry = masque_geo
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: boite_f,
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer morph : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        trace(self);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
    }

    /// Dessine le contenu d'un média dans la cible courante, en espace
    /// **boîte locale** `(0,0) → (bw,bh)` — l'intérieur du `morphOffscreen`
    /// DOM : fit + transform + effets (+ pixelisation si `pixel_morph`).
    fn peindre_media_local(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &TraceMorphe,
        dims: (u32, u32),
    ) -> Result<(), ErreurTexte> {
        let boite_l = (0.0, 0.0, dims.0 as f64, dims.1 as f64);
        if t.pixel_morph > 1.0 {
            // Petit buffer `boite/pf` + upscale sans lissage dans
            // l'offscreen — miroir du `mPixel` du chemin morph DOM.
            let pf = t.pixel_morph.max(1.0);
            let (pw, ph) = (
                ((dims.0 as f64 / pf).round() as u32).max(1),
                ((dims.1 as f64 / pf).round() as u32).max(1),
            );
            let petit = self.bitmap_petit(pw, ph)?;
            let cible_avant = unsafe { self.ctx.GetTarget() }.ok();
            unsafe { self.ctx.SetTarget(&petit) };
            unsafe { self.cible_rt.Clear(Some(&couleur_d2d([0, 0, 0, 0]))) };
            let (src_i, dst_i) =
                crate::media::fit_rects(t.taille, (0.0, 0.0, pw as f64, ph as f64), t.fit);
            let t_p = Transform2D {
                offset_x: t.transform.offset_x / pf,
                offset_y: t.transform.offset_y / pf,
                zoom_ecran: 1.0,
                ..*t.transform
            };
            let transforme_p = (!t_p.est_neutre())
                .then(|| t_p.matrice((pw as f64 / 2.0, ph as f64 / 2.0)));
            let (src, dst) = (rect_d2d(&src_i), rect_d2d(&dst_i));
            if t.effets.couleur_active() || t.effets.flou > 0.0 {
                let image = self.image_effets(bitmap, src, dst, t.effets)?;
                self.tracer_image(&image, src, dst, transforme_p, marge_flou(t.effets, src, dst));
            } else {
                self.tracer_bitmap(bitmap, dst, src, transforme_p, 1.0);
            }
            if let Some(cible) = &cible_avant {
                unsafe { self.ctx.SetTarget(cible) };
            }
            unsafe {
                self.cible_rt.DrawBitmap(
                    &petit,
                    Some(&D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: dims.0 as f32,
                        bottom: dims.1 as f32,
                    }),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                    Some(&D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: pw as f32,
                        bottom: ph as f32,
                    }),
                )
            };
            return Ok(());
        }
        let (src_i, dst_i) = crate::media::fit_rects(t.taille, boite_l, t.fit);
        let t_l = Transform2D {
            zoom_ecran: 1.0,
            ..*t.transform
        };
        let transforme = (!t_l.est_neutre())
            .then(|| t_l.matrice((dims.0 as f64 / 2.0, dims.1 as f64 / 2.0)));
        let (src, dst) = (rect_d2d(&src_i), rect_d2d(&dst_i));
        if t.effets.couleur_active() || t.effets.flou > 0.0 {
            let image = self.image_effets(bitmap, src, dst, t.effets)?;
            self.tracer_image(&image, src, dst, transforme, marge_flou(t.effets, src, dst));
        } else {
            self.tracer_bitmap(bitmap, dst, src, transforme, 1.0);
        }
        Ok(())
    }

    /// Offscreen pré-warp + sortie d'une boîte morphée (créés une fois).
    /// Les deux textures portent `RENDER_TARGET | SHADER_RESOURCE` : D2D
    /// peint l'entrée, D3D la lit puis écrit la sortie, D2D la lit.
    fn buffers_morphe(
        &mut self,
        cle: CleBoite,
        bw: u32,
        bh: u32,
    ) -> Result<
        (
            ID2D1Bitmap1,
            windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
            ID2D1Bitmap1,
            windows::Win32::Graphics::Direct3D11::ID3D11RenderTargetView,
        ),
        ErreurTexte,
    > {
        if self.morphes.contains_key(&cle) && self.sorties_morph.contains_key(&cle) {
            let (_, bmp_in, srv) = &self.morphes[&cle];
            let (_, bmp_out, rtv) = &self.sorties_morph[&cle];
            return Ok((bmp_in.clone(), srv.clone(), bmp_out.clone(), rtv.clone()));
        }
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT,
        };
        use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: bw,
            Height: bh,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let creer = |opts: D2D1_BITMAP_OPTIONS| -> Result<_, ErreurTexte> {
            let mut texture = None;
            unsafe { self.device_d3d.CreateTexture2D(&desc, None, Some(&mut texture)) }
                .map_err(|e| ErreurTexte::Gpu(format!("texture morph : {}", e.message())))?;
            let texture = texture
                .ok_or_else(|| ErreurTexte::Gpu("texture morph nulle".to_string()))?;
            let surface: IDXGISurface = texture
                .cast()
                .map_err(|e| ErreurTexte::Gpu(format!("IDXGISurface morph : {}", e.message())))?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: opts,
                colorContext: ManuallyDrop::new(None),
            };
            let bitmap = unsafe { self.ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props)) }
                .map_err(|e| ErreurTexte::Gpu(format!("bitmap morph : {}", e.message())))?;
            Ok((texture, bitmap))
        };
        let (tex_in, bmp_in) = creer(D2D1_BITMAP_OPTIONS_TARGET)?;
        let (tex_out, bmp_out) = creer(D2D1_BITMAP_OPTIONS_NONE)?;
        let mut srv = None;
        unsafe { self.device_d3d.CreateShaderResourceView(&tex_in, None, Some(&mut srv)) }
            .map_err(|e| ErreurTexte::Gpu(format!("SRV morph : {}", e.message())))?;
        let srv = srv.ok_or_else(|| ErreurTexte::Gpu("SRV morph nulle".to_string()))?;
        let mut rtv = None;
        unsafe { self.device_d3d.CreateRenderTargetView(&tex_out, None, Some(&mut rtv)) }
            .map_err(|e| ErreurTexte::Gpu(format!("RTV morph : {}", e.message())))?;
        let rtv = rtv.ok_or_else(|| ErreurTexte::Gpu("RTV morph nulle".to_string()))?;
        self.morphes.insert(cle, (tex_in, bmp_in.clone(), srv.clone()));
        self.sorties_morph
            .insert(cle, (tex_out, bmp_out.clone(), rtv.clone()));
        Ok((bmp_in, srv, bmp_out, rtv))
    }

    /// Média pixelisé (`bgPixel`, chemin canvas du fond — parité
    /// `dessinerContenuFond`) : effets + transform sont dessinés dans un
    /// buffer réduit `boite / pf`, puis l'upscale vers la boîte se fait
    /// **sans lissage** (`imageSmoothingEnabled = false` → NEAREST).
    /// `zoom_ecran` (`scale` de `#bg`) s'applique à l'upscale, en espace
    /// écran — pas dans le petit buffer. `clip` = polygone du cadre.
    ///
    /// DOM : le `ctx.filter` est posé **dans** le petit canvas → `blur(f)`
    /// vaut f px du petit espace (le halo grossit ×pf à l'upscale — la
    /// formule `flou / s_fit` le reproduit, `s_fit` étant l'échelle vers le
    /// petit buffer).
    pub fn dessiner_media_pixel(
        &mut self,
        bitmap: &ID2D1Bitmap1,
        t: &TracePixel,
    ) -> Result<(), ErreurTexte> {
        let (taille, boite, fit, transform, clip, effets) =
            (t.taille, t.boite, t.fit, t.transform, t.clip, t.effets);
        let pf = effets.pixel.max(1.0);
        let (pw, ph) = (
            ((boite.2 / pf).round() as u32).max(1),
            ((boite.3 / pf).round() as u32).max(1),
        );
        let petit = self.bitmap_petit(pw, ph)?;
        // Le contenu se dessine dans le petit buffer, en coords `boite / pf`
        // (le canvas réduit du DOM). Pas de clip : les bornes du buffer
        // rognent, comme le petit canvas.
        let cible_avant = unsafe { self.ctx.GetTarget() }.ok();
        unsafe { self.ctx.SetTarget(&petit) };
        unsafe { self.cible_rt.Clear(Some(&couleur_d2d([0, 0, 0, 0]))) };
        let boite_p = (boite.0 / pf, boite.1 / pf, boite.2 / pf, boite.3 / pf);
        let (src_i, dst_i) = crate::media::fit_rects(taille, boite_p, fit);
        let t_p = Transform2D {
            offset_x: transform.offset_x / pf,
            offset_y: transform.offset_y / pf,
            zoom_ecran: 1.0,
            ..*transform
        };
        let transforme_p = (!t_p.est_neutre()).then(|| {
            t_p.matrice((
                boite_p.0 + boite_p.2 / 2.0,
                boite_p.1 + boite_p.3 / 2.0,
            ))
        });
        let src = D2D_RECT_F {
            left: src_i.left as f32,
            top: src_i.top as f32,
            right: src_i.right as f32,
            bottom: src_i.bottom as f32,
        };
        let dst = D2D_RECT_F {
            left: dst_i.left as f32,
            top: dst_i.top as f32,
            right: dst_i.right as f32,
            bottom: dst_i.bottom as f32,
        };
        if effets.couleur_active() || effets.flou > 0.0 {
            let image = self.image_effets(bitmap, src, dst, effets)?;
            self.tracer_image(&image, src, dst, transforme_p, marge_flou(effets, src, dst));
        } else {
            self.tracer_bitmap(bitmap, dst, src, transforme_p, 1.0);
        }
        if let Some(cible) = &cible_avant {
            unsafe { self.ctx.SetTarget(cible) };
        }
        // Upscale ×pf sans lissage sur la boîte entière, sous le clip et le
        // `zoom_ecran` (transform externe, espace écran autour du centre).
        let boite_f = D2D_RECT_F {
            left: boite.0 as f32,
            top: boite.1 as f32,
            right: (boite.0 + boite.2) as f32,
            bottom: (boite.1 + boite.3) as f32,
        };
        let ze = (transform.zoom_ecran != 1.0).then(|| {
            let (cx, cy) = ((boite.0 + boite.2 / 2.0) as f32, (boite.1 + boite.3 / 2.0) as f32);
            let z = transform.zoom_ecran as f32;
            Matrix3x2::translation(-cx, -cy)
                * Matrix3x2 {
                    M11: z,
                    M12: 0.0,
                    M21: 0.0,
                    M22: z,
                    M31: 0.0,
                    M32: 0.0,
                }
                * Matrix3x2::translation(cx, cy)
        });
        let src_petit = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: pw as f32,
            bottom: ph as f32,
        };
        let trace = |s: &Self| unsafe {
            let mut avant = Matrix3x2::identity();
            if let Some(m) = ze {
                s.cible_rt.GetTransform(&mut avant);
                s.cible_rt.SetTransform(&(m * avant));
            }
            s.cible_rt.DrawBitmap(
                &petit,
                Some(&boite_f),
                t.opacite,
                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                Some(&src_petit),
            );
            if ze.is_some() {
                s.cible_rt.SetTransform(&avant);
            }
        };
        let Some(masque_geo) = clip else {
            unsafe {
                self.cible_rt
                    .PushAxisAlignedClip(&boite_f, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            trace(self);
            unsafe { self.cible_rt.PopAxisAlignedClip() };
            return Ok(());
        };
        let masque: ID2D1Geometry = masque_geo
            .cast()
            .map_err(|e| ErreurTexte::Gpu(format!("ID2D1Geometry : {}", e.message())))?;
        let mut params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: boite_f,
            geometricMask: ManuallyDrop::new(Some(masque)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        let layer: ID2D1Layer = unsafe { self.cible_rt.CreateLayer(None) }
            .map_err(|e| ErreurTexte::Gpu(format!("CreateLayer pixel : {}", e.message())))?;
        unsafe { self.ctx.PushLayer(&params, &layer) };
        trace(self);
        unsafe { self.ctx.PopLayer() };
        unsafe { ManuallyDrop::drop(&mut params.geometricMask) };
        Ok(())
    }
}

/// Clé du cache `effets` : bitmap + matrice couleur + σ (la pixelisation
/// ne fait pas partie de la chaîne — chemins distincts).
fn cle_effets(bitmap: &ID2D1Bitmap1, effets: &EffetsCfg, sigma: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (bitmap.as_raw() as usize).hash(&mut h);
    effets.lum.to_bits().hash(&mut h);
    effets.contraste.to_bits().hash(&mut h);
    effets.teinte.to_bits().hash(&mut h);
    sigma.to_bits().hash(&mut h);
    h.finish()
}

/// Échelle géométrique du fit `src`→`dst` (moyenne des deux axes —
/// isotrope sous `Etirer` non uniforme, approximation documentée).
fn echelle_fit(src: D2D_RECT_F, dst: D2D_RECT_F) -> f64 {
    let sw = (src.right - src.left).abs().max(f32::EPSILON) as f64;
    let sh = (src.bottom - src.top).abs().max(f32::EPSILON) as f64;
    let dw = (dst.right - dst.left).abs() as f64;
    let dh = (dst.bottom - dst.top).abs() as f64;
    ((dw * dh) / (sw * sh)).sqrt().max(f64::MIN_POSITIVE)
}

/// Marge d'échantillonnage du flou dans l'image source : `blur` déborde
/// ~3σ des bornes — ramené à l'espace source comme l'écart-type.
fn marge_flou(effets: &EffetsCfg, src: D2D_RECT_F, dst: D2D_RECT_F) -> f32 {
    if effets.flou <= 0.0 {
        return 0.0;
    }
    (3.0 * effets.flou / echelle_fit(src, dst)).ceil() as f32
}

/// `RECT` → `D2D_RECT_F` (simple cast, jamais vide-filtré).
fn rect_d2d(r: &RECT) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.left as f32,
        top: r.top as f32,
        right: r.right as f32,
        bottom: r.bottom as f32,
    }
}
