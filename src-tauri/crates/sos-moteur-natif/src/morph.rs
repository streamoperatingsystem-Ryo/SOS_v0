//! Warp bulge/pinch — pixel shader D3D11 répliquant `MORPH_FRAG`
//! (diffusion.html ~5400). Le contenu du widget (fit + transform + effets)
//! est d'abord rasterisé dans un offscreen de la taille de la boîte, puis
//! chaque pixel est re-échantillonné selon les points de morph (≤ 20,
//! coords normalisées, `u_aspect` = w/h).
//!
//! `MorphPoint` de la scène → uniform `vec4` : `(x, y, rayon, k)` avec
//! `k = intensite/100` en `retrecir`, `−intensite/200` sinon (bulge).

use windows::core::{Interface, PCSTR, s};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11Resource, ID3D11SamplerState,
    ID3D11ShaderResourceView, ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT,
    D3D11_VIEWPORT,
};
use sos_scene::MorphPoint;

use std::fmt;

/// Erreurs du warp morph. Aucune variante ne doit faire paniquer l'appelant.
#[derive(Debug)]
pub struct ErreurMorph(pub String);

impl fmt::Display for ErreurMorph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

const SHADER: &str = r#"
Texture2D src_tex : register(t0);
SamplerState samp : register(s0);
cbuffer Morphs : register(b0) {
    float4 u_morphs[20];
    int u_count;
    float u_aspect;
    float2 u_pad;
};
struct PSIn { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
PSIn VSMain(uint id : SV_VertexID) {
    PSIn o;
    float2 p = float2((id << 1) & 2, id & 2);
    o.pos = float4(p * float2(2, -2) + float2(-1, 1), 0, 1);
    o.uv = float2(p.x, p.y);
    return o;
}
float4 PSMain(PSIn i) : SV_Target {
    float2 p = i.uv;
    for (int k = 0; k < 20; k++) {
        if (k >= u_count) break;
        float4 m = u_morphs[k];
        float2 d = p - m.xy;
        d.y /= u_aspect;
        float dist = length(d);
        if (dist < m.z && dist > 0.0001) {
            float infl = 1.0 - dist / m.z;
            float smoothInfl = infl * infl * (3.0 - 2.0 * infl);
            float deform = dist * (1.0 + m.w * smoothInfl);
            float2 dir = d / dist;
            p = m.xy + float2(dir.x, dir.y * u_aspect) * deform;
        }
    }
    return src_tex.Sample(samp, p);
}
"#;

/// Pipeline du warp morph : shaders + constant buffer + sampler linéaire.
pub struct WarpMorph {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cbuf: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    contexte: ID3D11DeviceContext,
}

impl WarpMorph {
    /// Compile le shader et prépare le pipeline sur le device du moteur.
    pub fn nouveau(device: &ID3D11Device) -> Result<Self, ErreurMorph> {
        let compiler = |entree: PCSTR, cible: PCSTR| -> Result<Vec<u8>, ErreurMorph> {
            let mut code = None;
            let mut erreurs = None;
            unsafe {
                D3DCompile(
                    SHADER.as_ptr() as *const _,
                    SHADER.len(),
                    None,
                    None,
                    None,
                    entree,
                    cible,
                    0,
                    0,
                    &mut code,
                    Some(&mut erreurs),
                )
            }
            .map_err(|e| {
                let msg = erreurs
                    .as_ref()
                    .map(|b| {
                        String::from_utf8_lossy(unsafe {
                            std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())
                        })
                        .to_string()
                    })
                    .unwrap_or_else(|| e.message());
                ErreurMorph(format!("compile {entree:?} : {msg}"))
            })?;
            let blob = code.ok_or_else(|| ErreurMorph("blob nul".to_string()))?;
            Ok(unsafe {
                std::slice::from_raw_parts(
                    blob.GetBufferPointer() as *const u8,
                    blob.GetBufferSize(),
                )
                .to_vec()
            })
        };
        let bytecode_vs = compiler(s!("VSMain"), s!("vs_4_0"))?;
        let bytecode_ps = compiler(s!("PSMain"), s!("ps_4_0"))?;
        let mut vs = None;
        unsafe { device.CreateVertexShader(&bytecode_vs, None, Some(&mut vs)) }
            .map_err(|e| ErreurMorph(format!("VS : {}", e.message())))?;
        let vs = vs.ok_or_else(|| ErreurMorph("VS nul".to_string()))?;
        let mut ps = None;
        unsafe { device.CreatePixelShader(&bytecode_ps, None, Some(&mut ps)) }
            .map_err(|e| ErreurMorph(format!("PS : {}", e.message())))?;
        let ps = ps.ok_or_else(|| ErreurMorph("PS nul".to_string()))?;
        let mut cbuf = None;
        unsafe {
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: 20 * 16 + 16,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                None,
                Some(&mut cbuf),
            )
        }
        .map_err(|e| ErreurMorph(format!("cbuf : {}", e.message())))?;
        let cbuf = cbuf.ok_or_else(|| ErreurMorph("cbuf nul".to_string()))?;
        let mut sampler = None;
        unsafe {
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                    MinLOD: 0.0,
                    MaxLOD: f32::MAX,
                },
                Some(&mut sampler),
            )
        }
        .map_err(|e| ErreurMorph(format!("sampler : {}", e.message())))?;
        let sampler = sampler.ok_or_else(|| ErreurMorph("sampler nul".to_string()))?;
        let contexte = unsafe { device.GetImmediateContext() }
            .map_err(|e| ErreurMorph(format!("contexte : {}", e.message())))?;
        Ok(Self {
            vs,
            ps,
            cbuf,
            sampler,
            contexte,
        })
    }

    /// Warpe `src` (offscreen `w×h`) dans `dst` (même taille) selon les
    /// points de morph — miroir exact de `MORPH_FRAG`. Les vues sont
    /// créées par l'appelant, une fois par texture.
    pub fn warper(
        &self,
        srv: &ID3D11ShaderResourceView,
        rtv: &ID3D11RenderTargetView,
        morphs: &[MorphPoint],
        largeur: u32,
        hauteur: u32,
    ) -> Result<(), ErreurMorph> {
        let mut data = [0f32; 20 * 4 + 4];
        let n = morphs.len().min(20);
        for (i, m) in morphs.iter().take(n).enumerate() {
            data[i * 4] = m.x as f32;
            data[i * 4 + 1] = m.y as f32;
            data[i * 4 + 2] = m.rayon as f32;
            data[i * 4 + 3] = if m.mode == "retrecir" {
                (m.intensite / 100.0) as f32
            } else {
                -(m.intensite / 200.0) as f32
            };
        }
        // `u_count` est un `int` HLSL : on écrit les **bits** de l'entier.
        data[80] = f32::from_bits(n as u32);
        data[81] = largeur as f32 / (hauteur.max(1)) as f32;
        let ressource: ID3D11Resource = self
            .cbuf
            .cast()
            .map_err(|e| ErreurMorph(format!("cbuf → ressource : {e}")))?;
        unsafe {
            self.contexte.UpdateSubresource(
                &ressource,
                0,
                None,
                data.as_ptr() as *const _,
                0,
                0,
            );
            self.contexte
                .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            self.contexte.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: largeur as f32,
                Height: hauteur as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.contexte.VSSetShader(&self.vs, None);
            self.contexte.PSSetShader(&self.ps, None);
            self.contexte
                .PSSetConstantBuffers(0, Some(&[Some(self.cbuf.clone())]));
            self.contexte
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            self.contexte
                .PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            self.contexte.Draw(3, 0);
            self.contexte.PSSetShaderResources(0, Some(&[None]));
            self.contexte.OMSetRenderTargets(Some(&[None]), None);
        }
        Ok(())
    }
}
