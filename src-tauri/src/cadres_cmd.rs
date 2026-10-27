/// Commande chat « !cadres <couleur> » : un viewer change la couleur vive
/// (couleurFin — extrémité du dégradé) du cadre unifié de la scène courante.
///
/// Différence avec commandes.rs : pas d'overlay — la commande change le
/// rendu EN DIRECT via un override ÉPHÉMÈRE sur ScenesState.cadres_chat,
/// injecté dans les snapshots WS (scenes::push_snapshot / snapshot_courant).
/// JAMAIS persisté : ni state.scene ni scenes/<id>.json ne sont touchés →
/// la couleur du streamer prime au redémarrage. L'override est purgé au
/// changement de scène et dès que le streamer édite son cadre
/// (update_scene). Zéro appel OBS : la forme (clip-path, masque caméra)
/// ne dépend pas de la couleur.
///
/// Cooldown GLOBAL obligatoire : 60 s (constante — volontairement non
/// configurable : les cadres sont l'identité visuelle du stream).
///
/// Accusé : retourne Option<String> posté en PRIVMSG par twitch_chat.rs ;
/// les autres plateformes n'ont pas de canal d'envoi (retour ignoré).
///
/// Persistance : `data/cadres_chat.json`.
use crate::config::data_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

/// Cooldown global OBLIGATOIRE entre deux changements de couleur (secondes).
const COOLDOWN_GLOBAL_S: u64 = 60;

/// Une couleur nommée de la palette fermée (nom anglais → hex).
#[derive(Debug, Clone, Serialize)]
pub struct PaletteCouleur {
    pub nom: &'static str,
    pub hex: &'static str,
}

/// Palette fermée de couleurs « de base » (~50 noms anglais, valeurs CSS).
/// Liste blanche stricte : aucun hex libre accepté en chat.
const PALETTE: &[PaletteCouleur] = &[
    PaletteCouleur { nom: "red", hex: "#ff0000" },
    PaletteCouleur { nom: "crimson", hex: "#dc143c" },
    PaletteCouleur { nom: "scarlet", hex: "#ff2400" },
    PaletteCouleur { nom: "coral", hex: "#ff7f50" },
    PaletteCouleur { nom: "salmon", hex: "#fa8072" },
    PaletteCouleur { nom: "orange", hex: "#ffa500" },
    PaletteCouleur { nom: "gold", hex: "#ffd700" },
    PaletteCouleur { nom: "yellow", hex: "#ffff00" },
    PaletteCouleur { nom: "khaki", hex: "#f0e68c" },
    PaletteCouleur { nom: "beige", hex: "#f5f5dc" },
    PaletteCouleur { nom: "ivory", hex: "#fffff0" },
    PaletteCouleur { nom: "peach", hex: "#ffe5b4" },
    PaletteCouleur { nom: "sand", hex: "#c2b280" },
    PaletteCouleur { nom: "tan", hex: "#d2b48c" },
    PaletteCouleur { nom: "white", hex: "#ffffff" },
    PaletteCouleur { nom: "silver", hex: "#c0c0c0" },
    PaletteCouleur { nom: "gray", hex: "#808080" },
    PaletteCouleur { nom: "grey", hex: "#808080" },
    PaletteCouleur { nom: "black", hex: "#000000" },
    PaletteCouleur { nom: "maroon", hex: "#800000" },
    PaletteCouleur { nom: "brown", hex: "#8b4513" },
    PaletteCouleur { nom: "chocolate", hex: "#d2691e" },
    PaletteCouleur { nom: "olive", hex: "#808000" },
    PaletteCouleur { nom: "lime", hex: "#00ff00" },
    PaletteCouleur { nom: "green", hex: "#008000" },
    PaletteCouleur { nom: "emerald", hex: "#50c878" },
    PaletteCouleur { nom: "mint", hex: "#3eb489" },
    PaletteCouleur { nom: "jade", hex: "#00a86b" },
    PaletteCouleur { nom: "forestgreen", hex: "#228b22" },
    PaletteCouleur { nom: "seagreen", hex: "#2e8b57" },
    PaletteCouleur { nom: "teal", hex: "#008080" },
    PaletteCouleur { nom: "turquoise", hex: "#40e0d0" },
    PaletteCouleur { nom: "cyan", hex: "#00ffff" },
    PaletteCouleur { nom: "aqua", hex: "#00ffff" },
    PaletteCouleur { nom: "sky", hex: "#87ceeb" },
    PaletteCouleur { nom: "steelblue", hex: "#4682b4" },
    PaletteCouleur { nom: "royalblue", hex: "#4169e1" },
    PaletteCouleur { nom: "blue", hex: "#0000ff" },
    PaletteCouleur { nom: "slateblue", hex: "#6a5acd" },
    PaletteCouleur { nom: "navy", hex: "#000080" },
    PaletteCouleur { nom: "indigo", hex: "#4b0082" },
    PaletteCouleur { nom: "purple", hex: "#800080" },
    PaletteCouleur { nom: "violet", hex: "#ee82ee" },
    PaletteCouleur { nom: "plum", hex: "#dda0dd" },
    PaletteCouleur { nom: "orchid", hex: "#da70d6" },
    PaletteCouleur { nom: "lavender", hex: "#e6e6fa" },
    PaletteCouleur { nom: "lilac", hex: "#c8a2c8" },
    PaletteCouleur { nom: "magenta", hex: "#ff00ff" },
    PaletteCouleur { nom: "fuchsia", hex: "#ff00ff" },
    PaletteCouleur { nom: "pink", hex: "#ffc0cb" },
    PaletteCouleur { nom: "hotpink", hex: "#ff69b4" },
    PaletteCouleur { nom: "deeppink", hex: "#ff1493" },
    PaletteCouleur { nom: "rose", hex: "#ff007f" },
];

// ===== Signature couleur dérivée (miroir TS : signatureCadresChat) =====
//
// Une SEULE couleur viewer → duo NOIR + couleur du cadre :
// - base : NOIR (#000000) — le « trait principal atténué » du rendu par
//   défaut (CadreConfig::defaut_scene : #000000 + #8b5cf6). C'est cette base
//   noire qui préserve les nuances d'épaisseur : le halo (strokeWidth × 2,
//   opacité 0.18) et la bande principale disparaissent dans le fond, seuls
//   les liserés de stopsLiseres restent visibles. Une base TEINTÉE (ancienne
//   couleur × 0.65) rend toute la largeur du trait visible → le cadre se
//   réduit à un bloc de couleur unie et les encoches des biseaux se
//   remplissent (triangles de coin du motif stripe-mg).
// - vive : la couleur choisie, brute — liserés + TOUS les ornements du
//   cadre (équerres, arcs, graduations, circuits).
// Miroir IDENTIQUE côté TS (cadresChat.ts) — les deux implémentations
// produisent les mêmes hex.

/// Signature d'UNE couleur viewer : (base = noir, vive = couleur).
/// Miroir EXACT de signatureCadresChat (cadresChat.ts).
fn signature_couleurs(hex: &str) -> (String, String) {
    ("#000000".to_string(), hex.to_lowercase())
}

// ===== Config =====

/// Configuration de la commande « cadres » (persistée).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CadresChatConfig {
    #[serde(default = "default_true")]
    pub actif: bool,
    /// Mot de la commande SANS le "!" (ex: "cadres"). Normalisé lowercase.
    #[serde(default = "default_commande")]
    pub commande: String,
    /// false = aucun accusé chat (même sur Twitch qui peut répondre).
    #[serde(default = "default_true")]
    pub reponses_chat: bool,
}

fn default_true() -> bool {
    true
}
fn default_commande() -> String {
    "cadres".to_string()
}

impl Default for CadresChatConfig {
    fn default() -> Self {
        Self {
            actif: true,
            commande: default_commande(),
            reponses_chat: true,
        }
    }
}

// ===== État partagé =====

/// État partagé de la commande cadres. Clonable (Arc internes).
#[derive(Clone)]
pub struct CadresChatState {
    pub app: AppHandle,
    config: Arc<Mutex<CadresChatConfig>>,
    /// Dernier changement accepté (cooldown global obligatoire).
    dernier: Arc<Mutex<Option<Instant>>>,
}

impl CadresChatState {
    /// Crée l'état + charge la config depuis le disque.
    pub fn new(app: AppHandle) -> Self {
        let cfg = load_config(&app).unwrap_or_default();
        Self {
            app,
            config: Arc::new(Mutex::new(cfg)),
            dernier: Arc::new(Mutex::new(None)),
        }
    }

    // ===== Déclenchement (hook chat) =====

    /// Appelé par les modules chat à chaque message (à côté des hooks
    /// commandes/musique/fight). Match « !<commande> <couleur> » + actif +
    /// cooldown global 60 s → override éphémère + push_snapshot.
    /// Retourne un accusé à poster en chat (Twitch) ou None.
    pub fn on_message(&self, pseudo: &str, texte: &str) -> Option<String> {
        let rest = texte.trim().strip_prefix('!')?;
        let premier = rest.split_whitespace().next()?;
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.actif || premier.to_lowercase() != cfg.commande {
            return None;
        }
        let arg = rest[premier.len()..].trim().to_lowercase();
        if arg.is_empty() {
            return self.reponse(&cfg, format!("Usage : !{} <couleur>", cfg.commande));
        }
        let Some(couleur) = PALETTE.iter().find(|c| c.nom == arg) else {
            return self.reponse(&cfg, format!("✗ Couleur inconnue « {} »", arg));
        };

        // Garde : la commande ne fait rien sans cadre actif sur la scène
        // (jamais de création/réactivation — réglages utilisateur préservés).
        // AVANT le cooldown : un appel refusé ne doit pas griller les 60 s.
        let Some(scenes) = self.app.try_state::<crate::scenes::ScenesState>() else {
            return self.reponse(&cfg, "✗ Aucun cadre actif sur cette scène".to_string());
        };
        let (cadre_actif, scene_id) = {
            let s = scenes.scene.lock().unwrap();
            let cid = scenes.current_id.lock().unwrap();
            (
                s.cadreWidget.as_ref().is_some_and(|c| c.actif),
                cid.clone(),
            )
        };
        if !cadre_actif {
            return self.reponse(&cfg, "✗ Aucun cadre actif sur cette scène".to_string());
        }

        // Cooldown global obligatoire (60 s, non configurable).
        let now = Instant::now();
        {
            let mut d = self.dernier.lock().unwrap();
            if let Some(last) = *d {
                let restant = COOLDOWN_GLOBAL_S.saturating_sub(now.duration_since(last).as_secs());
                if restant > 0 {
                    return self.reponse(
                        &cfg,
                        format!("⏳ Cadres : prochain changement dans {} s", restant),
                    );
                }
            }
            *d = Some(now);
        }

        // Override ÉPHÉMÈRE (RAM, jamais persisté) : signature bi-tonale
        // dérivée de la couleur viewer (sombre teinté → vive duotone) —
        // miroir de signatureCadresChat (cadresChat.ts). Injectée dans les
        // snapshots WS : la diffusion montre la couleur viewer, le fichier
        // scène garde celles du streamer (priment au redémarrage).
        let (c_base, c_vive) = signature_couleurs(couleur.hex);
        *scenes.cadres_chat.lock().unwrap() = Some(crate::scenes::CadresChatOverride {
            scene_id,
            couleur: c_base,
            couleur_fin: c_vive,
        });
        crate::scenes::push_snapshot(scenes.inner());
        self.reponse(&cfg, format!("🎨 Cadres → {} (par {})", couleur.nom, pseudo))
    }

    /// Accusé chat (None si reponses_chat désactivé — même règle que musique).
    fn reponse(&self, cfg: &CadresChatConfig, texte: String) -> Option<String> {
        cfg.reponses_chat.then_some(texte)
    }

    // ===== Config =====

    /// Sauvegarde la config (nom normalisé : trim, sans "!", lowercase, non
    /// vide) + refus de collision avec les commandes chat personnalisées,
    /// la musique ET l'arène d'emojis (même règle que fight.rs).
    pub fn set_config(&self, mut cfg: CadresChatConfig) -> Result<CadresChatConfig, String> {
        cfg.commande = cfg.commande.trim().trim_start_matches('!').to_lowercase();
        if cfg.commande.is_empty() {
            return Err("Nom de commande vide".into());
        }
        if cfg.commande == crate::commandes::CMD_LISTE {
            return Err("« !cmd » est réservé à la liste des commandes".into());
        }
        if let Some(commandes) = self.app.try_state::<crate::commandes::CommandesState>() {
            for exist in commandes.etat() {
                if exist.commande == cfg.commande {
                    return Err(format!(
                        "« !{} » est déjà une commande chat — choisissez un autre nom",
                        exist.commande
                    ));
                }
            }
        }
        if let Some(m) = self.app.try_state::<crate::musique::MusiqueState>() {
            for r in m.noms_reserves() {
                if r == cfg.commande {
                    return Err(format!(
                        "« !{} » est réservé à la musique — choisissez un autre nom",
                        r
                    ));
                }
            }
        }
        if let Some(f) = self.app.try_state::<crate::fight::FightState>() {
            for r in f.noms_reserves() {
                if r == cfg.commande {
                    return Err(format!(
                        "« !{} » est réservé à l'arène d'emojis — choisissez un autre nom",
                        r
                    ));
                }
            }
        }
        *self.config.lock().unwrap() = cfg.clone();
        self.save_config()?;
        self.emit_update();
        // Désactivation de la commande : purge de l'override couleur viewer
        // en RAM + repousse du snapshot — le cadre reprend immédiatement
        // les couleurs du streamer (sinon l'override survivait jusqu'au
        // prochain changement de scène ou à la prochaine édition du cadre).
        if !cfg.actif {
            if let Some(scenes) = self.app.try_state::<crate::scenes::ScenesState>() {
                *scenes.cadres_chat.lock().unwrap() = None;
                crate::scenes::push_snapshot(scenes.inner());
            }
        }
        Ok(cfg)
    }

    /// Noms de commandes du module réservés (sans "!") — utilisés par
    /// commandes.rs pour refuser les collisions avec les commandes chat.
    pub fn noms_reserves(&self) -> Vec<String> {
        vec![self.config.lock().unwrap().commande.clone()]
    }

    /// Retourne la config courante (pour l'UI dashboard).
    pub fn etat(&self) -> CadresChatConfig {
        self.config.lock().unwrap().clone()
    }

    /// Émet `cadres:update` vers le dashboard (config complète).
    pub fn emit_update(&self) {
        let _ = self.app.emit("cadres:update", self.etat());
    }

    // ===== Persistance =====

    fn save_config(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        let dir = data_dir(&self.app)?;
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Serialize cadres_chat.json: {}", e))?;
        fs::write(dir.join("cadres_chat.json"), json)
            .map_err(|e| format!("Écrire cadres_chat.json: {}", e))?;
        Ok(())
    }
}

/// Palette complète exposée à l'UI (grille de couleurs de l'onglet Cadres).
pub fn palette() -> Vec<PaletteCouleur> {
    PALETTE.to_vec()
}

/// Charge cadres_chat.json (absent → défaut).
fn load_config(app: &AppHandle) -> Result<CadresChatConfig, String> {
    let dir = data_dir(app)?;
    let path = dir.join("cadres_chat.json");
    if !path.exists() {
        return Ok(CadresChatConfig::default());
    }
    let content =
        fs::read_to_string(&path).map_err(|e| format!("Lire cadres_chat.json: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Parse cadres_chat.json: {}", e))
}
