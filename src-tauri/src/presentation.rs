/// Présentation streamer — portage de la feature « streamers favoris » de
/// l'ancienne app (stream-operating-system-react).
///
/// Présente un streamer sur la diffusion (overlay autonome, pas un widget de
/// scène) : rideaux théâtraux ou carte, avatar, bio, badges, statut live,
/// jeu courant, jeux récents, clips jouables, 2 pistes audio (voix/musique)
/// par favori. Déclenchement 100 % manuel depuis l'onglet « Présentation »
/// de la modale Interactions chat — PAS de détection automatique de message.
///
/// Flux :
///   1. Dashboard : recherche de chaîne (search_channels) ou favori persisté.
///   2. `lancer(user_id)` → fetch Helix (broadcaster + channel + stream +
///      clips + game_info) → émet `{"type":"presentation-show",...}` sur
///      chat_tx (forwardé par server.rs vers diffusion.html :4321) → mémorise
///      `courant` (les données servent de source aux contrôles live).
///   3. Contrôles live : clip_jouer (résout le MP4 signé au clic — les URL
///      Twitch expirent), audio voix/musique, bascule de mode, fermer.
///
/// Persistance : `data/presentation_favoris.json` (favoris + sons + mode).
/// `courant` est en RAM uniquement (pas persisté — une présentation n'est
/// pas restaurée au reboot, comme le welcome clip).
///
/// Sons : fichiers importés via la commande `import_son` existante
/// (medias/<uuid>.<ext>, servi par :4321 sur /medias/). La référence est
/// stockée dans le favori ; le dashboard passe `src` explicitement à
/// `audio_jouer` (état Rust minimal — le favori est la source de vérité).
use crate::config::data_dir;
use crate::twitch_clips;
use crate::twitch_helix;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;

/// Modes d'affichage de la présentation côté diffusion.
const MODE_THEATRICAL: &str = "theatrical";
const MODE_CARD: &str = "card";

/// Pistes audio configurables par favori.
const PISTES: &[&str] = &["voix", "musique"];

/// Nombre max de clips récents chargés à la présentation (l'overlay en
/// affiche 4, le dashboard la liste complète).
const NB_CLIPS: u32 = 12;

/// Nombre max de jeux récents affichés (déduits des game_id des clips).
const NB_JEUX: usize = 4;

/// Favori persisté : un streamer présentable + ses réglages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriConfig {
    pub user_id: String,
    /// Clé de la map : login lowercase (résolu à la création).
    pub login: String,
    pub display_name: String,
    /// Avatar Twitch (rafraîchi à chaque lancer — cache).
    #[serde(default)]
    pub avatar: String,
    /// Piste « voix » : medias/<uuid>.<ext> ou None.
    #[serde(default)]
    pub son_voix: Option<String>,
    /// Piste « musique » : medias/<uuid>.<ext> ou None.
    #[serde(default)]
    pub son_musique: Option<String>,
    /// Mode d'affichage : "theatrical" (défaut) | "card".
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    MODE_THEATRICAL.to_string()
}

/// Données complètes d'une présentation : payload de la frame
/// `presentation-show` ET retour de `presentation_lancer` vers le dashboard
/// (contrôles live : grille de clips, boutons audio). Non persisté.
#[derive(Debug, Clone, Serialize)]
pub struct PresentationDonnees {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    pub avatar: String,
    /// Bio Twitch (description du profil). Peut être vide.
    pub bio: String,
    /// Bannière/entête de chaîne (offline_image_url Helix). Vide si le
    /// streamer n'en a pas défini — l'overlay la masque dans ce cas.
    pub banniere: String,
    /// "partner" | "affiliate" | "" — affiché en badge côté diffusion.
    pub broadcaster_type: String,
    pub en_live: bool,
    /// Nombre de viewers si en live (None hors-ligne).
    pub viewers_live: Option<u32>,
    /// Titre du live en cours (channel_info). Vide si hors-ligne/erreur.
    pub titre_live: String,
    /// Jeu courant (channel_info). Vide si hors-ligne/erreur.
    pub jeu_courant: String,
    pub tags: Vec<String>,
    /// Jeux récents déduits des game_id des clips (max NB_JEUX).
    pub jeux_recents: Vec<twitch_helix::CategoryEntry>,
    /// Clips récents (max NB_CLIPS) — affichés en grille, résolus MP4 au clic.
    pub clips: Vec<twitch_clips::ClipInfo>,
    /// Mode d'affichage effectif (favori > paramètre > défaut theatrical).
    pub mode: String,
}

/// État exposé au dashboard (commande `presentation_etat` + event
/// `presentation:etat` émis après chaque mutation).
#[derive(Debug, Clone, Serialize)]
pub struct PresentationEtat {
    pub favoris: Vec<FavoriConfig>,
    /// true = une présentation est visible côté diffusion.
    pub visible: bool,
    /// Données de la présentation en cours (contrôles live dashboard).
    pub courant: Option<PresentationDonnees>,
}

/// Format du fichier presentation_favoris.json sur disque.
/// Wrapper objet (pas la map nue) pour pouvoir ajouter des champs globaux
/// plus tard sans migration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PresentationFile {
    /// login (lowercase) → favori.
    #[serde(default)]
    favoris: HashMap<String, FavoriConfig>,
}

/// État partagé présentation. Clonable (Arc internes) — passé aux commandes
/// Tauri via tauri::State.
#[derive(Clone)]
pub struct PresentationState {
    pub app: AppHandle,
    pub chat_tx: broadcast::Sender<String>,
    /// Favoris : login (lowercase) → FavoriConfig. Persistés.
    favoris: Arc<Mutex<HashMap<String, FavoriConfig>>>,
    /// Présentation en cours (None = rien d'affiché). RAM uniquement.
    courant: Arc<Mutex<Option<PresentationDonnees>>>,
}

impl PresentationState {
    /// Crée l'état + charge les favoris depuis le disque.
    pub fn new(app: AppHandle, chat_tx: broadcast::Sender<String>) -> Self {
        let file = load_file(&app).unwrap_or_default();
        Self {
            app,
            chat_tx,
            favoris: Arc::new(Mutex::new(file.favoris)),
            courant: Arc::new(Mutex::new(None)),
        }
    }

    // ===== État / favoris =====

    /// État courant pour le dashboard (favoris triés par nom d'affichage).
    pub fn etat(&self) -> PresentationEtat {
        let mut favoris: Vec<FavoriConfig> = {
            let map = self.favoris.lock().unwrap();
            map.values().cloned().collect()
        };
        favoris.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
        });
        let courant = self.courant.lock().unwrap().clone();
        let visible = courant.is_some();
        PresentationEtat {
            favoris,
            visible,
            courant,
        }
    }

    /// Ajoute/remplace un favori (upsert par login) + persiste + émet état.
    pub fn sauver_favori(&self, mut favori: FavoriConfig) -> Result<(), String> {
        favori.login = favori.login.to_lowercase();
        if favori.login.is_empty() || favori.user_id.is_empty() {
            return Err("Favori incomplet (login ou user_id vide)".into());
        }
        if favori.mode != MODE_THEATRICAL && favori.mode != MODE_CARD {
            favori.mode = default_mode();
        }
        {
            let mut map = self.favoris.lock().unwrap();
            map.insert(favori.login.clone(), favori);
        }
        self.sauver_fichier()?;
        self.emit_etat();
        Ok(())
    }

    /// Retire un favori + persiste + émet état. Idempotent.
    pub fn supprimer_favori(&self, login: &str) -> Result<(), String> {
        {
            let mut map = self.favoris.lock().unwrap();
            map.remove(&login.to_lowercase());
        }
        self.sauver_fichier()?;
        self.emit_etat();
        Ok(())
    }

    /// Affecte une piste audio à un favori (src = medias/<uuid>.<ext>,
    /// None = retirer). Persiste + émet état.
    pub fn favori_set_son(
        &self,
        login: &str,
        piste: &str,
        src: Option<String>,
    ) -> Result<(), String> {
        if !PISTES.contains(&piste) {
            return Err(format!("Piste inconnue : {}", piste));
        }
        if let Some(s) = &src {
            if !s.starts_with("medias/") {
                return Err("Source audio invalide (attendu medias/…)".into());
            }
        }
        {
            let mut map = self.favoris.lock().unwrap();
            let fav = map
                .get_mut(&login.to_lowercase())
                .ok_or_else(|| "Favori introuvable".to_string())?;
            if piste == "voix" {
                fav.son_voix = src;
            } else {
                fav.son_musique = src;
            }
        }
        self.sauver_fichier()?;
        self.emit_etat();
        Ok(())
    }

    /// Change le mode d'affichage d'un favori + de la présentation en cours
    /// si c'est ce streamer qui est affiché (émet `presentation-mode` pour
    /// une bascule à chaud côté diffusion).
    pub fn favori_set_mode(&self, login: &str, mode: &str) -> Result<(), String> {
        if mode != MODE_THEATRICAL && mode != MODE_CARD {
            return Err(format!("Mode inconnu : {}", mode));
        }
        let login_lc = login.to_lowercase();
        {
            let mut map = self.favoris.lock().unwrap();
            if let Some(fav) = map.get_mut(&login_lc) {
                fav.mode = mode.to_string();
            }
        }
        self.sauver_fichier()?;

        // Si la présentation courante concerne ce login → bascule à chaud.
        let est_courant = {
            let courant = self.courant.lock().unwrap();
            courant
                .as_ref()
                .map(|d| d.login == login_lc)
                .unwrap_or(false)
        };
        if est_courant {
            {
                let mut courant = self.courant.lock().unwrap();
                if let Some(d) = courant.as_mut() {
                    d.mode = mode.to_string();
                }
            }
            let msg = serde_json::json!({
                "type": "presentation-mode",
                "mode": mode,
            });
            let _ = self.chat_tx.send(msg.to_string());
        }
        self.emit_etat();
        Ok(())
    }

    // ===== Cycle de vie de la présentation =====

    /// Lance une présentation : fetch Helix (broadcaster + channel + stream +
    /// clips + jeux) → émet `presentation-show` → mémorise `courant` →
    /// retourne les données au dashboard.
    ///
    /// `mode` = mode demandé par le dashboard, ignoré si le streamer est un
    /// favori (le mode persisté du favori gagne). Erreurs non-broadcaster
    /// (channel, stream, clips, jeux) → valeurs par défaut, jamais fatales.
    pub async fn lancer(
        &self,
        user_id: &str,
        mode: Option<String>,
        access: &str,
    ) -> Result<PresentationDonnees, String> {
        // Les 4 appels sont indépendants (tous par user_id + access).
        let (broadcaster, channel, live, clips) = tokio::join!(
            twitch_helix::broadcaster(user_id, access),
            twitch_helix::channel_info(user_id, access),
            twitch_helix::stream_viewers(user_id, access),
            twitch_clips::liste_clips(user_id, access, NB_CLIPS),
        );

        // Seul le profil est fatal : sans lui il n'y a rien à présenter.
        let b = broadcaster.map_err(crate::helix_err_to_string)?;
        let ch = channel.ok();
        let viewers = live.ok().flatten();
        let clips_list = clips.unwrap_or_default();

        // Jeux récents : game_ids uniques des clips, dans l'ordre, bornés.
        let mut game_ids: Vec<String> = Vec::new();
        for c in &clips_list {
            if !c.game_id.is_empty() && !game_ids.contains(&c.game_id) {
                game_ids.push(c.game_id.clone());
            }
        }
        game_ids.truncate(NB_JEUX);
        let mut jeux = Vec::with_capacity(game_ids.len());
        for id in &game_ids {
            if let Ok(Some(g)) = twitch_helix::game_info(id, access).await {
                jeux.push(g);
            }
        }

        // Mode : favori persisté > paramètre > défaut theatrical.
        let login_lc = b.login.to_lowercase();
        let fav = {
            let map = self.favoris.lock().unwrap();
            map.get(&login_lc).cloned()
        };
        let mode_eff = fav
            .as_ref()
            .map(|f| f.mode.clone())
            .or(mode)
            .unwrap_or_else(default_mode);

        let donnees = PresentationDonnees {
            user_id: b.user_id.clone(),
            login: login_lc.clone(),
            display_name: b.display_name.clone(),
            avatar: b.profile_image_url.clone(),
            bio: b.description.clone(),
            banniere: b.offline_image_url.clone(),
            broadcaster_type: b.broadcaster_type.clone(),
            en_live: viewers.is_some(),
            viewers_live: viewers,
            titre_live: ch.as_ref().map(|c| c.title.clone()).unwrap_or_default(),
            jeu_courant: ch
                .as_ref()
                .map(|c| c.game_name.clone())
                .unwrap_or_default(),
            tags: ch.map(|c| c.tags).unwrap_or_default(),
            jeux_recents: jeux,
            clips: clips_list,
            mode: mode_eff.clone(),
        };

        // Rafraîchir le cache favori (avatar + display_name) — comme welcome.
        if fav.is_some() {
            let mut map = self.favoris.lock().unwrap();
            if let Some(f) = map.get_mut(&login_lc) {
                f.avatar = donnees.avatar.clone();
                f.display_name = donnees.display_name.clone();
            }
        }
        if fav.is_some() {
            let _ = self.sauver_fichier();
        }

        // Émettre la frame WS puis mémoriser + notifier le dashboard.
        let msg = serde_json::json!({
            "type": "presentation-show",
            "mode": mode_eff,
            "donnees": &donnees,
        });
        let _ = self.chat_tx.send(msg.to_string());
        {
            let mut courant = self.courant.lock().unwrap();
            *courant = Some(donnees.clone());
        }
        self.emit_etat();
        Ok(donnees)
    }

    /// Ferme la présentation : émet `presentation-hide` + efface `courant`.
    /// Idempotent (rien affiché → no-op, frame quand même émise : la diffusion
    /// est idempotente sur hide).
    pub fn fermer(&self) {
        let msg = serde_json::json!({ "type": "presentation-hide" });
        let _ = self.chat_tx.send(msg.to_string());
        {
            let mut courant = self.courant.lock().unwrap();
            *courant = None;
        }
        self.emit_etat();
    }

    /// Joue un clip sur la diffusion : résout l'URL MP4 signée (cache 10 min)
    /// puis émet `presentation-clip-play`. Le titre/durée sont repris du clip
    /// connu dans `courant` (meilleure info que rien).
    pub async fn clip_jouer(&self, slug: &str) -> Result<(), String> {
        let mp4_url = twitch_clips::resoudre_mp4(slug).await?;
        let (titre, duree_ms) = {
            let courant = self.courant.lock().unwrap();
            courant
                .as_ref()
                .and_then(|d| d.clips.iter().find(|c| c.id == slug))
                .map(|c| (c.title.clone(), (c.duration * 1000.0).round() as u64))
                .unwrap_or_default()
        };
        let msg = serde_json::json!({
            "type": "presentation-clip-play",
            "mp4_url": mp4_url,
            "titre": titre,
            "duree_ms": duree_ms,
        });
        let _ = self.chat_tx.send(msg.to_string());
        Ok(())
    }

    /// Arrête le clip en cours côté diffusion (retour à la carte).
    pub fn clip_stopper(&self) {
        let msg = serde_json::json!({ "type": "presentation-clip-stop" });
        let _ = self.chat_tx.send(msg.to_string());
    }

    /// Joue une piste audio sur la diffusion. `src` = medias/<uuid>.<ext>
    /// (servi par :4321 — la diffusion fait new Audio("/" + src)).
    pub fn audio_jouer(&self, piste: &str, src: &str) -> Result<(), String> {
        if !PISTES.contains(&piste) {
            return Err(format!("Piste inconnue : {}", piste));
        }
        if src.is_empty() || !src.starts_with("medias/") {
            return Err("Source audio invalide (attendu medias/…)".into());
        }
        let msg = serde_json::json!({
            "type": "presentation-audio-play",
            "piste": piste,
            "src": src,
        });
        let _ = self.chat_tx.send(msg.to_string());
        Ok(())
    }

    /// Stoppe une piste audio (ou "toutes").
    pub fn audio_stopper(&self, piste: &str) -> Result<(), String> {
        if piste != "toutes" && !PISTES.contains(&piste) {
            return Err(format!("Piste inconnue : {}", piste));
        }
        let msg = serde_json::json!({
            "type": "presentation-audio-stop",
            "piste": piste,
        });
        let _ = self.chat_tx.send(msg.to_string());
        Ok(())
    }

    // ===== Interne =====

    /// Émet `presentation:etat` vers le dashboard (après chaque mutation).
    fn emit_etat(&self) {
        let etat = self.etat();
        let _ = self.app.emit("presentation:etat", &etat);
    }

    /// Sauve les favoris sur disque (presentation_favoris.json).
    fn sauver_fichier(&self) -> Result<(), String> {
        let dir = data_dir(&self.app)?;
        let path = dir.join("presentation_favoris.json");
        let file = {
            let map = self.favoris.lock().unwrap();
            PresentationFile {
                favoris: map.clone(),
            }
        };
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| format!("Serialize presentation_favoris.json: {}", e))?;
        fs::write(&path, json)
            .map_err(|e| format!("Écrire presentation_favoris.json: {}", e))
    }
}

/// Charge le fichier presentation_favoris.json depuis le disque.
/// Default (vide) si absent ou illisible — jamais fatal au boot.
fn load_file(app: &AppHandle) -> Result<PresentationFile, String> {
    let dir = data_dir(app)?;
    let path = dir.join("presentation_favoris.json");
    if !path.exists() {
        return Ok(PresentationFile::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Lire presentation_favoris.json: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Parse presentation_favoris.json: {}", e))
}
