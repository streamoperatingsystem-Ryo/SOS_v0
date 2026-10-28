//! Compositeur GPU — N calques → canvas NV12, en un `VideoProcessorBlt`.
//!
//! Composition et conversion colorimétrique (BT.709, 4:2:0) sont fusionnées :
//! l'encodeur consomme directement la texture de sortie (zéro-copie).
//! Z-order = ordre des calques (le premier est en dessous).
//!
//! Le processeur vidéo D3D11 accepte plusieurs flux par appel ; chaque flux
//! porte sa fenêtre source (crop), sa destination (position + échelle) et un
//! alpha constant. Les zones non couvertes prennent le fond (noir).
//!
//! **Au-delà de `MaxInputStreams`** (plafond matériel : 5 sur GTX 1070 Ti),
//! la composition passe en **multi-passes** : premier lot plein canvas, puis
//! un appel par calque restant, la fenêtre de sortie étant restreinte à sa
//! destination (`VideoProcessorSetOutputTargetRect`). Le fond noir n'est
//! alors peint que dans cette fenêtre, entièrement recouverte par le calque :
//! les passes précédentes ne sont jamais effacées.

use std::fmt;
use std::mem::ManuallyDrop;
use std::ptr::null_mut;

use windows::Win32::Foundation::{BOOL, RECT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
    ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorInputView,
    ID3D11VideoProcessorOutputView, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV, D3D11_VIDEO_COLOR,
    D3D11_VIDEO_COLOR_0, D3D11_VIDEO_COLOR_RGBA, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
    D3D11_VIDEO_PROCESSOR_CAPS, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
    D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
    D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
    D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_RATIONAL;
use windows::core::Interface;

use crate::gpu::{ContexteGpu, DescTexture, Format};

/// Un calque : texture source, fenêtre de crop, destination (échelle comprise).
pub struct Calque<'a> {
    pub texture: &'a ID3D11Texture2D,
    pub source: RECT,
    pub destination: RECT,
}

/// Erreurs du compositeur. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErreurComposition {
    /// Création ou appel D3D11 Video.
    Gpu(String),
    /// Aucun calque à composer.
    AucunCalque,
}

impl fmt::Display for ErreurComposition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErreurComposition::Gpu(m) => write!(f, "GPU : {m}"),
            ErreurComposition::AucunCalque => write!(f, "aucun calque"),
        }
    }
}

impl std::error::Error for ErreurComposition {}

/// Compositeur : canvas NV12 + processeur vidéo.
pub struct Compositeur {
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerateur: ID3D11VideoProcessorEnumerator,
    processeur: ID3D11VideoProcessor,
    sortie: ID3D11Texture2D,
    vue_sortie: ID3D11VideoProcessorOutputView,
    largeur: u32,
    hauteur: u32,
    max_flux: u32,
}

impl Compositeur {
    /// Prépare le processeur vidéo pour un canvas NV12 de `largeur × hauteur`.
    pub fn nouveau(
        contexte: &ContexteGpu,
        largeur: u32,
        hauteur: u32,
        fps: u32,
    ) -> Result<Self, ErreurComposition> {
        let desc = DescTexture::new(Format::Nv12, largeur, hauteur)
            .map_err(|e| ErreurComposition::Gpu(e.to_string()))?;
        let sortie = contexte
            .creer_texture(&desc)
            .map_err(|e| ErreurComposition::Gpu(e.to_string()))?;
        Self::nouveau_sur(contexte, &sortie, fps)
    }

    /// Compose vers une texture **existante** — voie A du texte (2026-10-28).
    ///
    /// Le canvas de composition est alors en BGRA : Direct2D dessine les
    /// textes dessus (alpha réel), puis `color_gpu::ConvertisseurNv12`
    /// convertit vers la texture NV12 enregistrée auprès de l'encodeur.
    /// La texture NV12 du chemin direct reste produite par [`Compositeur::nouveau`].
    pub fn nouveau_sur(
        contexte: &ContexteGpu,
        sortie: &ID3D11Texture2D,
        fps: u32,
    ) -> Result<Self, ErreurComposition> {
        if fps == 0 {
            return Err(ErreurComposition::Gpu("fps nul".into()));
        }
        let mut desc = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC::default();
        unsafe { sortie.GetDesc(&mut desc) };
        let (largeur, hauteur) = (desc.Width, desc.Height);
        if largeur == 0 || hauteur == 0 {
            return Err(ErreurComposition::Gpu("texture de sortie nulle".into()));
        }

        let video_device: ID3D11VideoDevice = contexte.device().cast().map_err(|e| {
            ErreurComposition::Gpu(format!("ID3D11VideoDevice : {}", e.message()))
        })?;
        let video_context: ID3D11VideoContext = contexte.context().cast().map_err(|e| {
            ErreurComposition::Gpu(format!("ID3D11VideoContext : {}", e.message()))
        })?;

        let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: fps,
                Denominator: 1,
            },
            InputWidth: largeur,
            InputHeight: hauteur,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: fps,
                Denominator: 1,
            },
            OutputWidth: largeur,
            OutputHeight: hauteur,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };

        let enumerateur = unsafe { video_device.CreateVideoProcessorEnumerator(&content) }
            .map_err(|e| {
                ErreurComposition::Gpu(format!(
                    "CreateVideoProcessorEnumerator : {}",
                    e.message()
                ))
            })?;
        let processeur = unsafe { video_device.CreateVideoProcessor(&enumerateur, 0) }
            .map_err(|e| ErreurComposition::Gpu(format!("CreateVideoProcessor : {}", e.message())))?;

        let mut caps: D3D11_VIDEO_PROCESSOR_CAPS = unsafe { std::mem::zeroed() };
        unsafe { enumerateur.GetVideoProcessorCaps(&mut caps) }
            .map_err(|e| ErreurComposition::Gpu(format!("GetVideoProcessorCaps : {}", e.message())))?;

        let desc_sortie = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut vue_sortie: Option<ID3D11VideoProcessorOutputView> = None;
        unsafe {
            video_device.CreateVideoProcessorOutputView(
                sortie,
                &enumerateur,
                &desc_sortie,
                Some(&mut vue_sortie),
            )
        }
        .map_err(|e| {
            ErreurComposition::Gpu(format!(
                "CreateVideoProcessorOutputView : {}",
                e.message()
            ))
        })?;
        let vue_sortie =
            vue_sortie.ok_or_else(|| ErreurComposition::Gpu("vue de sortie nulle".into()))?;

        // Fond noir pour les zones non couvertes par un calque.
        let noir = D3D11_VIDEO_COLOR {
            Anonymous: D3D11_VIDEO_COLOR_0 {
                RGBA: D3D11_VIDEO_COLOR_RGBA {
                    R: 0.0,
                    G: 0.0,
                    B: 0.0,
                    A: 1.0,
                },
            },
        };
        unsafe { video_context.VideoProcessorSetOutputBackgroundColor(&processeur, false, &noir) };

        Ok(Self {
            video_device,
            video_context,
            enumerateur,
            processeur,
            sortie: sortie.clone(),
            vue_sortie,
            largeur,
            hauteur,
            max_flux: caps.MaxInputStreams,
        })
    }

    /// Compose les calques (z = ordre du tableau) dans la texture de sortie.
    ///
    /// Chemin rapide — un seul `VideoProcessorBlt` — tant que le nombre de
    /// calques tient dans `MaxInputStreams`. Au-delà, multi-passes : premier
    /// lot en un appel (il peint tout le canvas, fond compris), puis un appel
    /// par calque restant borné à sa destination. Aucune limite de scène.
    pub fn composer(&mut self, calques: &[Calque<'_>]) -> Result<(), ErreurComposition> {
        if calques.is_empty() {
            return Err(ErreurComposition::AucunCalque);
        }
        let lot = (self.max_flux as usize).max(1);
        if calques.len() <= lot {
            return self.composer_en_un_appel(calques);
        }
        self.composer_en_un_appel(&calques[..lot])?;
        for calque in &calques[lot..] {
            self.composer_un_calque(calque)?;
        }
        // La fenêtre de sortie est un état du processeur : la relâcher, sinon
        // la frame suivante serait rognée.
        unsafe {
            self.video_context
                .VideoProcessorSetOutputTargetRect(&self.processeur, false, None);
        }
        Ok(())
    }

    /// Un appel, N flux : chemin historique (toute la surface est écrite).
    fn composer_en_un_appel(&mut self, calques: &[Calque<'_>]) -> Result<(), ErreurComposition> {
        let mut vues: Vec<ID3D11VideoProcessorInputView> = Vec::with_capacity(calques.len());
        for (i, calque) in calques.iter().enumerate() {
            let vue = self.creer_vue(calque.texture)?;
            unsafe {
                self.video_context.VideoProcessorSetStreamSourceRect(
                    &self.processeur,
                    i as u32,
                    true,
                    Some(&calque.source as *const RECT),
                );
                self.video_context.VideoProcessorSetStreamDestRect(
                    &self.processeur,
                    i as u32,
                    true,
                    Some(&calque.destination as *const RECT),
                );
                self.video_context.VideoProcessorSetStreamAlpha(&self.processeur, i as u32, true, 1.0);
            }
            vues.push(vue);
        }

        let mut flux: Vec<D3D11_VIDEO_PROCESSOR_STREAM> = vues
            .iter()
            .map(|vue| D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: BOOL(1),
                OutputIndex: 0,
                InputFrameOrField: 0,
                PastFrames: 0,
                FutureFrames: 0,
                ppPastSurfaces: null_mut(),
                pInputSurface: ManuallyDrop::new(Some(vue.clone())),
                ppFutureSurfaces: null_mut(),
                ppPastSurfacesRight: null_mut(),
                pInputSurfaceRight: ManuallyDrop::new(None),
                ppFutureSurfacesRight: null_mut(),
            })
            .collect();

        let resultat = unsafe {
            self.video_context
                .VideoProcessorBlt(&self.processeur, &self.vue_sortie, 0, &flux)
        };

        // Les flux ne sont pas propriétaires : libération explicite des vues.
        for f in flux.iter_mut() {
            unsafe { ManuallyDrop::drop(&mut f.pInputSurface) };
        }

        resultat.map_err(|e| ErreurComposition::Gpu(format!("VideoProcessorBlt : {}", e.message())))?;
        Ok(())
    }

    /// Une passe, un calque, bornée à sa destination : le fond noir n'est
    /// peint que dans cette fenêtre, que le calque recouvre entièrement.
    fn composer_un_calque(&mut self, calque: &Calque<'_>) -> Result<(), ErreurComposition> {
        let Some(fenetre) = fenetre_destination(calque.destination, self.largeur, self.hauteur)
        else {
            // Entièrement hors canvas : rien à écrire.
            return Ok(());
        };
        let vue = self.creer_vue(calque.texture)?;
        unsafe {
            self.video_context.VideoProcessorSetStreamSourceRect(
                &self.processeur,
                0,
                true,
                Some(&calque.source as *const RECT),
            );
            self.video_context.VideoProcessorSetStreamDestRect(
                &self.processeur,
                0,
                true,
                Some(&calque.destination as *const RECT),
            );
            self.video_context.VideoProcessorSetStreamAlpha(&self.processeur, 0, true, 1.0);
            self.video_context.VideoProcessorSetOutputTargetRect(
                &self.processeur,
                true,
                Some(&fenetre as *const RECT),
            );
        }

        let mut flux = [D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: BOOL(1),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: null_mut(),
            pInputSurface: ManuallyDrop::new(Some(vue)),
            ppFutureSurfaces: null_mut(),
            ppPastSurfacesRight: null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: null_mut(),
        }];

        let resultat = unsafe {
            self.video_context
                .VideoProcessorBlt(&self.processeur, &self.vue_sortie, 0, &flux)
        };
        for f in flux.iter_mut() {
            unsafe { ManuallyDrop::drop(&mut f.pInputSurface) };
        }
        resultat.map_err(|e| ErreurComposition::Gpu(format!("VideoProcessorBlt : {}", e.message())))
    }

    /// Texture de sortie (NV12, encodable telle quelle).
    pub fn sortie(&self) -> &ID3D11Texture2D {
        &self.sortie
    }

    pub fn taille(&self) -> (u32, u32) {
        (self.largeur, self.hauteur)
    }

    /// Nombre de flux simultanés supportés par le processeur.
    pub fn max_flux(&self) -> u32 {
        self.max_flux
    }

    fn creer_vue(
        &self,
        texture: &ID3D11Texture2D,
    ) -> Result<ID3D11VideoProcessorInputView, ErreurComposition> {
        let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut vue: Option<ID3D11VideoProcessorInputView> = None;
        unsafe {
            self.video_device
                .CreateVideoProcessorInputView(texture, &self.enumerateur, &desc, Some(&mut vue))
        }
        .map_err(|e| {
            ErreurComposition::Gpu(format!(
                "CreateVideoProcessorInputView : {}",
                e.message()
            ))
        })?;
        vue.ok_or_else(|| ErreurComposition::Gpu("vue d'entrée nulle".into()))
    }
}

/// Fenêtre de sortie d'une passe multi-calques : la destination du calque,
/// bornée au canvas. `None` si elle est vide (calque hors canvas).
fn fenetre_destination(destination: RECT, largeur: u32, hauteur: u32) -> Option<RECT> {
    let fenetre = RECT {
        left: destination.left.max(0),
        top: destination.top.max(0),
        right: destination.right.min(largeur as i32),
        bottom: destination.bottom.min(hauteur as i32),
    };
    (fenetre.right > fenetre.left && fenetre.bottom > fenetre.top).then_some(fenetre)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn fenetre_bornee_au_canvas() {
        let f = fenetre_destination(rect(-10, -10, 2000, 2000), 1920, 1080).expect("fenêtre");
        assert_eq!((f.left, f.top, f.right, f.bottom), (0, 0, 1920, 1080));
    }

    #[test]
    fn fenetre_interieure_intacte() {
        let f = fenetre_destination(rect(100, 50, 300, 150), 1920, 1080).expect("fenêtre");
        assert_eq!((f.left, f.top, f.right, f.bottom), (100, 50, 300, 150));
    }

    #[test]
    fn fenetre_hors_canvas_ou_vide_est_ignoree() {
        assert!(fenetre_destination(rect(2000, 0, 2200, 100), 1920, 1080).is_none());
        assert!(fenetre_destination(rect(-50, 0, 0, 100), 1920, 1080).is_none());
        assert!(fenetre_destination(rect(10, 10, 10, 50), 1920, 1080).is_none());
    }
}
