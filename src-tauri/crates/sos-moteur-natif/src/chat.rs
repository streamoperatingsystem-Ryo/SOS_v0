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
//! l'app (abonnement `chat_tx`), jamais d'ici. Les **images** (avatars,
//! emotes) sont des URLs distantes : le rendu ne lit qu'un fichier local du
//! cache disque de l'app (`%APPDATA%\com.streamos.v0\medias\cache\`),
//! dont le nom est dérivé de l'URL par [`empreinte_url`] — c'est l'app qui
//! télécharge (`cache_images.rs`, lot G), le moteur ne connaît pas le réseau.

use std::collections::VecDeque;

use crate::cadres::couleur_hex;

/// Taille du buffer de chat (`MAX_CHAT` de `diffusion.html`).
pub const MAX_CHAT: usize = 200;

/// Filtre « toutes plateformes » (valeur par défaut de `chatFiltre`).
pub const FILTRE_UNIFIE: &str = "unifie";

/// Segment d'emote d'un message — offsets **UTF-16** (`debut` inclusif,
/// `fin` EXCLUSIVE), exactement ceux que le DOM découpe (`String.slice`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emote {
    pub debut: usize,
    pub fin: usize,
    /// URL de l'image CDN — sert de clé du cache disque ([`empreinte_url`]).
    pub url: String,
    /// Texte remplacé (nom de l'emote — rendu si l'image manque).
    pub nom: String,
}

/// Un message prêt à rendre.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageChat {
    /// `twitch` | `youtube` | `kick` | `tiktok` (chaîne libre).
    pub plateforme: String,
    pub pseudo: String,
    pub texte: String,
    /// Couleur IRCv3 du pseudo (`#rrggbb`), si la plateforme en fournit une.
    pub couleur: Option<[u8; 4]>,
    /// Tag IRCv3 `badges` brut (`"moderator/1,subscriber/12"`) — rendu en
    /// emoji, aucune image (miroir `parseBadges` de diffusion.html).
    pub badges: Option<String>,
    /// URL de l'avatar du viewer (vide = pas d'avatar, comme le DOM).
    pub avatar: Option<String>,
    /// Emotes du message, triées par position (le domaine les trie déjà).
    pub emotes: Vec<Emote>,
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
            badges: None,
            avatar: None,
            emotes: Vec::new(),
        }
    }

    /// Avatars / badges / emotes — chaînables après [`Self::nouveau`].
    pub fn avec_medias(
        mut self,
        badges: Option<String>,
        avatar: Option<String>,
        emotes: Vec<Emote>,
    ) -> Self {
        self.badges = badges.filter(|b| !b.trim().is_empty());
        self.avatar = avatar.filter(|a| !a.trim().is_empty());
        self.emotes = emotes;
        self
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
    /// Couleurs du dégradé de bordure/anneau (`gradientCadre` du DOM :
    /// `cadreWidget` actif sinon `#e0e0e0 → #9a9a9a`).
    pub couleurs: ([u8; 4], [u8; 4]),
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
            couleurs: ([0xe0, 0xe0, 0xe0, 255], [0x9a, 0x9a, 0x9a, 255]),
            z,
        }
    }
}

/// Badge de chat rendu en emoji (`.badge` du DOM) — aucun fichier image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Badge {
    Mod,
    Vip,
    Sub,
    Broadcaster,
}

impl Badge {
    /// Emoji du badge — constantes de `BADGES_EMOJI` (diffusion.html).
    pub fn emoji(self) -> &'static str {
        match self {
            Badge::Mod => "\u{1f5e1}\u{fe0f}",
            Badge::Vip => "\u{1f48e}",
            Badge::Sub => "\u{2b50}",
            Badge::Broadcaster => "\u{1f451}",
        }
    }
}

/// Badges d'un tag IRCv3, dans l'ordre du DOM — miroir de `parseBadges`.
///
/// Les badges inconnus sont ignorés ; l'ordre du tag est conservé
/// (`moderator` avant `vip` avant `subscriber` s'il est écrit ainsi).
pub fn badges_emoji(brut: Option<&str>) -> Vec<Badge> {
    let Some(brut) = brut else {
        return Vec::new();
    };
    brut.split(',')
        .filter_map(|partie| {
            let nom = partie.split('/').next().unwrap_or("");
            match nom {
                "moderator" => Some(Badge::Mod),
                "vip" => Some(Badge::Vip),
                "subscriber" => Some(Badge::Sub),
                "broadcaster" => Some(Badge::Broadcaster),
                _ => None,
            }
        })
        .collect()
}

/// FNV-1a 64 bits → 16 hex — nom du fichier de cache d'une image.
///
/// ⚠️ **Miroir exact** de `cache_images::empreinte` (app) : les deux
/// implémentations doivent produire la même chaîne pour la même URL, sinon
/// le moteur ne retrouve jamais le fichier téléchargé. Verrouillé par le
/// même vecteur de test des deux côtés.
pub fn empreinte_url(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for octet in url.as_bytes() {
        hash ^= *octet as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Chemin du fichier de cache d'une image (`base` = dossier de données).
///
/// Sans extension : WIC détecte le format au contenu (vérifié PNG / GIF /
/// WebP) — l'app n'a donc pas à deviner l'extension du CDN.
pub fn chemin_cache(base: &std::path::Path, url: &str) -> std::path::PathBuf {
    base.join("medias").join("cache").join(empreinte_url(url))
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
    fn badges_emoji_miroir_de_parse_badges() {
        use Badge::*;
        assert_eq!(
            badges_emoji(Some("moderator/1,subscriber/12,vip/1")),
            vec![Mod, Sub, Vip],
            "ordre du tag conservé"
        );
        assert_eq!(badges_emoji(Some("broadcaster/1")), vec![Broadcaster]);
        assert!(badges_emoji(Some("partner/1,premium/1")).is_empty());
        assert!(badges_emoji(None).is_empty());
        assert!(badges_emoji(Some("")).is_empty());
        // Emojis exacts de `BADGES_EMOJI` (variation selector compris).
        assert_eq!(Mod.emoji(), "\u{1f5e1}\u{fe0f}");
        assert_eq!(Vip.emoji(), "\u{1f48e}");
        assert_eq!(Sub.emoji(), "\u{2b50}");
        assert_eq!(Broadcaster.emoji(), "\u{1f451}");
    }

    #[test]
    fn empreinte_url_miroir_de_l_app_et_chemin_de_cache() {
        // Même vecteur que `cache_images::tests::empreinte_miroir_du_crate_moteur`.
        assert_eq!(
            empreinte_url("https://cdn.7tv.app/emote/01FCY771D800007PQ2DF3GDTN6/2x.webp"),
            "3d695665d84ffc9e"
        );
        assert_eq!(
            empreinte_url("https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0"),
            "0d8894bf3abf0e01"
        );
        assert_eq!(empreinte_url(""), "cbf29ce484222325");
        let base = std::path::Path::new("C:/data");
        assert_eq!(
            chemin_cache(base, "https://cdn.7tv.app/emote/x/2x.webp"),
            base.join("medias")
                .join("cache")
                .join(empreinte_url("https://cdn.7tv.app/emote/x/2x.webp"))
        );
    }

    #[test]
    fn medias_chainables_et_vides_ignores() {
        let m = MessageChat::nouveau("twitch", "Ryo", "hi", None)
            .avec_medias(
                Some("  ".to_string()),
                Some("https://static-cdn.jtvnw.net/a.png".to_string()),
                vec![Emote {
                    debut: 0,
                    fin: 2,
                    url: "https://cdn.7tv.app/e/2x.webp".to_string(),
                    nom: "hi".to_string(),
                }],
            );
        assert!(m.badges.is_none(), "badges blancs ignorés");
        assert!(m.avatar.is_some());
        assert_eq!(m.emotes.len(), 1);
        assert_eq!(m.emotes[0].fin, 2);
    }

    #[test]
    fn chat_cfg_par_defaut() {
        let c = ChatCfg::depuis_widget((0.0, 0.0, 400.0, 200.0), 3);
        assert_eq!(c.filtre, "unifie");
        assert_eq!(c.taille_police, 16.0);
        assert!(!c.bulles);
        assert_eq!(c.couleurs, ([0xe0, 0xe0, 0xe0, 255], [0x9a, 0x9a, 0x9a, 255]));
        assert_eq!(c.z, 3);
    }
}
