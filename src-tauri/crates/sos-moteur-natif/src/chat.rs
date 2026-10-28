//! Chat natif — buffer borné, filtres et couleur de pseudo.
//!
//! Le moteur ne connaît **aucune plateforme** : il reçoit des messages déjà
//! normalisés (`MessageChat`) et ne fait que les rendre. La mise en page
//! (wrap, lignes, troncature) appartient à DirectWrite, dans `texte.rs` :
//! ce module porte les **données** et les règles vérifiables sans GPU.
//!
//! Contrats (miroir de `diffusion.html`) :
//! - buffer borné à [`MAX_CHAT`] = 200, **drop-oldest** (`chatMessages.splice`);
//! - filtre `unifie` = toutes plateformes, sinon égalité stricte ;
//! - couleur du pseudo = tag IRCv3 s'il existe, sinon teinte déterministe
//!   dérivée du pseudo (`couleurDepuisNom`, `hsl(h, 70%, 65%)`).
//!
//! Aucune écriture disque, aucun accès réseau : les messages arrivent de
//! l'app (abonnement `chat_tx`), jamais d'ici.

use std::collections::VecDeque;

use crate::cadres::couleur_hex;

/// Taille du buffer de chat (`MAX_CHAT` de `diffusion.html`).
pub const MAX_CHAT: usize = 200;

/// Filtre « toutes plateformes » (valeur par défaut de `chatFiltre`).
pub const FILTRE_UNIFIE: &str = "unifie";

/// Un message prêt à rendre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageChat {
    /// `twitch` | `youtube` | `kick` | `tiktok` (chaîne libre).
    pub plateforme: String,
    pub pseudo: String,
    pub texte: String,
    /// Couleur IRCv3 du pseudo (`#rrggbb`), si la plateforme en fournit une.
    pub couleur: Option<[u8; 4]>,
}

impl MessageChat {
    /// Construit un message ; `couleur` = chaîne hexadécimale éventuelle.
    ///
    /// Une couleur invalide est **ignorée** (repli sur la teinte du pseudo),
    /// jamais remplacée par du blanc : c'est ce que fait `couleurPseudo`.
    pub fn nouveau(
        plateforme: impl Into<String>,
        pseudo: impl Into<String>,
        texte: impl Into<String>,
        couleur: Option<&str>,
    ) -> Self {
        Self {
            plateforme: plateforme.into(),
            pseudo: pseudo.into(),
            texte: texte.into(),
            couleur: couleur.and_then(couleur_hex),
        }
    }

    /// Un message vide (pseudo **et** texte blancs) n'a rien à afficher.
    pub fn est_vide(&self) -> bool {
        self.pseudo.trim().is_empty() && self.texte.trim().is_empty()
    }

    /// Couleur du pseudo : IRCv3 si présente, sinon teinte du pseudo.
    pub fn couleur_pseudo(&self) -> [u8; 4] {
        self.couleur
            .unwrap_or_else(|| couleur_depuis_nom(&self.pseudo))
    }
}

/// Buffer borné des messages, dans l'ordre chronologique (ancien → récent).
#[derive(Debug, Default)]
pub struct FileChat {
    messages: VecDeque<MessageChat>,
    version: u64,
}

impl FileChat {
    pub fn nouveau() -> Self {
        Self::default()
    }

    /// Ajoute un message ; le plus ancien est retiré au-delà de [`MAX_CHAT`].
    ///
    /// Un message vide est ignoré (aucune incrémentation de version : le
    /// rendu n'a pas à se reconstruire pour rien).
    pub fn pousser(&mut self, message: MessageChat) {
        if message.est_vide() {
            return;
        }
        self.messages.push_back(message);
        while self.messages.len() > MAX_CHAT {
            self.messages.pop_front();
        }
        self.version = self.version.wrapping_add(1);
    }

    /// Compteur monotone : change dès que le contenu du buffer change.
    ///
    /// Le rendu s'en sert comme clé de cache (aucun re-rendu sans message).
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Nom conventionnel (`len_without_is_empty`) : le buffer est-il vide ?
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Messages visibles pour un filtre de widget (`unifie` ou plateforme).
    pub fn pour_filtre<'a>(&'a self, filtre: &'a str) -> impl Iterator<Item = &'a MessageChat> {
        self.messages.iter().filter(move |m| {
            filtre == FILTRE_UNIFIE || m.plateforme.eq_ignore_ascii_case(filtre)
        })
    }

    /// Vide le buffer (changement de scène, arrêt de sortie).
    pub fn vider(&mut self) {
        if !self.messages.is_empty() {
            self.messages.clear();
            self.version = self.version.wrapping_add(1);
        }
    }
}

/// Configuration d'un widget chat, extraite de la scène.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatCfg {
    pub rect: (f64, f64, f64, f64),
    /// `unifie` ou nom de plateforme.
    pub filtre: String,
    /// Taille de police en px canvas (`taillePolice`, défaut 16).
    pub taille_police: f32,
    /// `chatStyle: "bulles"` — messages détachés, zébrés.
    pub bulles: bool,
    pub z: i32,
}

impl ChatCfg {
    /// Valeurs par défaut de `diffusion.html` : filtre `unifie`, 16 px,
    /// style `compact` (rendu en bulles par cette tranche, cf. §36).
    pub fn depuis_widget(rect: (f64, f64, f64, f64), z: i32) -> Self {
        Self {
            rect,
            filtre: FILTRE_UNIFIE.to_string(),
            taille_police: 16.0,
            bulles: false,
            z,
        }
    }
}

/// Teinte déterministe d'un pseudo — miroir de `couleurDepuisNom`.
///
/// Hachage 32 bits signé sur les **unités UTF-16** (`charCodeAt`), puis
/// `|hash| % 360`. `unsigned_abs` évite le piège de `i32::MIN` (JS calcule
/// `Math.abs` en flottant, donc 2147483648 reste positif).
pub fn teinte_pseudo(pseudo: &str) -> u32 {
    if pseudo.is_empty() {
        return 0;
    }
    let mut hash: i32 = 0;
    for unite in pseudo.encode_utf16() {
        hash = hash
            .wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(unite as i32);
    }
    hash.unsigned_abs() % 360
}

/// Couleur de repli d'un pseudo : `hsl(teinte, 70%, 65%)` → RVB.
///
/// Conversion HSL → RVB standard (la même que le moteur CSS), arrondie au
/// plus proche. Un pseudo vide donne le gris de `couleurDepuisNom`
/// (`hsl(0, 0%, 70%)`).
pub fn couleur_depuis_nom(pseudo: &str) -> [u8; 4] {
    if pseudo.is_empty() {
        return [179, 179, 179, 255];
    }
    let (s, l): (f64, f64) = (0.70, 0.65);
    let teinte = teinte_pseudo(pseudo) as f64;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = teinte / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = l - c / 2.0;
    let octet = |v: f64| (v + m).mul_add(255.0, 0.5).floor().clamp(0.0, 255.0) as u8;
    [octet(r), octet(g), octet(b), 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(pseudo: &str, texte: &str, plateforme: &str) -> MessageChat {
        MessageChat::nouveau(plateforme, pseudo, texte, None)
    }

    #[test]
    fn buffer_borne_a_200_et_drop_oldest() {
        let mut file = FileChat::nouveau();
        for i in 0..MAX_CHAT + 5 {
            file.pousser(msg("viewer", &format!("m{i}"), "twitch"));
        }
        assert_eq!(file.len(), MAX_CHAT);
        let premiers: Vec<&str> = file.pour_filtre(FILTRE_UNIFIE).map(|m| m.texte.as_str()).collect();
        assert_eq!(premiers[0], "m5", "les 5 plus anciens sont partis");
        assert_eq!(premiers[MAX_CHAT - 1], "m204");
    }

    #[test]
    fn message_vide_ignore_et_version_stable() {
        let mut file = FileChat::nouveau();
        let v = file.version();
        file.pousser(msg("   ", "  ", "twitch"));
        assert!(file.is_empty());
        assert_eq!(file.version(), v);
        file.pousser(msg("Ryo", "salut", "twitch"));
        assert_eq!(file.version(), v.wrapping_add(1));
        file.vider();
        assert_eq!(file.version(), v.wrapping_add(2));
        file.vider();
        assert_eq!(file.version(), v.wrapping_add(2), "vider un buffer vide ne change rien");
    }

    #[test]
    fn filtre_unifie_ou_plateforme() {
        let mut file = FileChat::nouveau();
        file.pousser(msg("a", "un", "twitch"));
        file.pousser(msg("b", "deux", "kick"));
        file.pousser(msg("c", "trois", "Twitch"));
        assert_eq!(file.pour_filtre(FILTRE_UNIFIE).count(), 3);
        assert_eq!(file.pour_filtre("twitch").count(), 2, "insensible à la casse");
        assert_eq!(file.pour_filtre("youtube").count(), 0);
    }

    #[test]
    fn teinte_identique_au_ts() {
        // Valeurs mesurées de `couleurDepuisNom` (diffusion.html).
        assert_eq!(teinte_pseudo("Ryo"), 224);
        assert_eq!(teinte_pseudo("StreamOS"), 316);
        assert_eq!(teinte_pseudo("a"), 97);
        assert_eq!(teinte_pseudo("Zoe"), 32);
        assert_eq!(teinte_pseudo("viewer_42"), 277);
        assert_eq!(teinte_pseudo("é"), 233);
        assert_eq!(teinte_pseudo(""), 0);
    }

    #[test]
    fn couleur_de_repli_du_pseudo() {
        assert_eq!(couleur_depuis_nom("Ryo"), [103, 137, 228, 255]);
        assert_eq!(couleur_depuis_nom(""), [179, 179, 179, 255]);
    }

    #[test]
    fn couleur_ircv3_prime_sur_le_repli() {
        let m = MessageChat::nouveau("twitch", "Ryo", "hi", Some("#ff0000"));
        assert_eq!(m.couleur_pseudo(), [255, 0, 0, 255]);
        // Couleur invalide : ignorée, repli sur la teinte.
        let m = MessageChat::nouveau("twitch", "Ryo", "hi", Some("rouge"));
        assert_eq!(m.couleur, None);
        assert_eq!(m.couleur_pseudo(), couleur_depuis_nom("Ryo"));
    }

    #[test]
    fn chat_cfg_par_defaut() {
        let c = ChatCfg::depuis_widget((0.0, 0.0, 400.0, 200.0), 3);
        assert_eq!(c.filtre, "unifie");
        assert_eq!(c.taille_police, 16.0);
        assert!(!c.bulles);
        assert_eq!(c.z, 3);
    }
}
