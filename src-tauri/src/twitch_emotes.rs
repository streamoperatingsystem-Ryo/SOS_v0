/// Emotes de chat Twitch — officielles (tag IRC `emotes`) + tierces
/// (BTTV / 7TV / FFZ, globales + de chaine).
///
/// Architecture :
///   connexion IRC → 1 chargement des listes tierces (APIs publiques sans
///   auth, non-fatal : chaine absente des services = globales seules).
///   PRIVMSG → `construire_segments` : les positions du tag IRC sont en
///   CODEPOINTS Unicode (confirmé forums officiels Twitch + dank-twitch-irc :
///   `👩❤️💋👩 Kappa` → `25:6-10`, fin INCLUSIVE), converties en unités UTF-16
///   pour que le frontend découpe avec `String.slice()` directement.
///   Matching tierce : mot entier par token (couvre les codes type `:tf:`)
///   puis runs de caractères de mot (couvre `(KEKW)`, `KEKW!`), case-sensitive.
///   Priorité : tag IRC (Twitch a déjà validé le match) > BTTV > 7TV > FFZ.
///
/// Contrat frontend (`ChatMessage.emotes`) : `[{ debut, fin, url, nom }]` —
/// `debut` inclusif, `fin` EXCLUSIVE, unités UTF-16.
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Segment d'emote rendu par le frontend à la place du texte.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EmoteSeg {
    /// Début du segment (unités UTF-16, inclusif).
    pub debut: usize,
    /// Fin du segment (unités UTF-16, EXCLUSIVE).
    pub fin: usize,
    /// URL de l'image CDN (≈56-64px, émojis animés inclus).
    pub url: String,
    /// Texte remplacé (alt / title de l'image).
    pub nom: String,
}

/// Parseur de réponse JSON → paires (nom emote, URL image).
type ParseurEmotes = fn(&Value) -> Vec<(String, String)>;

/// Cache des emotes tierces : nom exact → URL image.
#[derive(Debug, Default)]
pub struct CacheEmotes {
    tierces: HashMap<String, String>,
}

impl CacheEmotes {
    /// Charge les listes BTTV/7TV/FFZ (global + chaine). Non-fatal : une
    /// source absente (404 = streamer non inscrit) ou en erreur réseau est
    /// ignorée — les emotes officielles (tag IRC) fonctionnent toujours.
    pub async fn charger(client: &reqwest::Client, user_id: &str, login: &str) -> Self {
        // Ordre = priorité (entry().or_insert : le 1er inscrit gagne).
        let sources: [(&str, String, ParseurEmotes); 6] = [
            (
                "BTTV global",
                "https://api.betterttv.net/3/cached/emotes/global".to_string(),
                parse_bttv,
            ),
            (
                "BTTV chaine",
                format!("https://api.betterttv.net/3/cached/users/twitch/{}", user_id),
                parse_bttv_chaine,
            ),
            (
                "7TV global",
                "https://7tv.io/v3/emote-sets/global".to_string(),
                parse_7tv,
            ),
            (
                "7TV chaine",
                format!("https://7tv.io/v3/users/twitch/{}", user_id),
                parse_7tv_chaine,
            ),
            (
                "FFZ global",
                "https://api.frankerfacez.com/v1/set/global".to_string(),
                parse_ffz,
            ),
            (
                "FFZ chaine",
                format!(
                    "https://api.frankerfacez.com/v1/room/{}",
                    login.to_lowercase()
                ),
                parse_ffz,
            ),
        ];
        let mut tierces: HashMap<String, String> = HashMap::new();
        for (nom_src, url, parseur) in sources {
            let Some(v) = fetch_source(client, &url).await else {
                continue;
            };
            let liste = parseur(&v);
            eprintln!("[Twitch-emotes] {} : {} emotes", nom_src, liste.len());
            for (nom, url_img) in liste {
                tierces.entry(nom).or_insert(url_img);
            }
        }
        Self { tierces }
    }

    fn est_vide(&self) -> bool {
        self.tierces.is_empty()
    }

    fn url_pour(&self, nom: &str) -> Option<&str> {
        self.tierces.get(nom).map(|s| s.as_str())
    }
}

/// GET JSON avec timeout, non-fatal. 404 = chaine absente du service (cas
/// normal, silencieux) ; autre erreur HTTP/réseau = log + None.
async fn fetch_source(client: &reqwest::Client, url: &str) -> Option<Value> {
    match client.get(url).timeout(Duration::from_secs(10)).send().await {
        Ok(resp) if resp.status().as_u16() == 404 => None,
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        Ok(resp) => {
            eprintln!("[Twitch-emotes] HTTP {} ← {}", resp.status(), url);
            None
        }
        Err(e) => {
            eprintln!("[Twitch-emotes] réseau {} ← {}", e, url);
            None
        }
    }
}

// ===== Parsers des réponses API (formes vérifiées en live le 2026-09-29) =====

/// BTTV : `[{id, code}]` (endpoint global).
fn parse_bttv(v: &Value) -> Vec<(String, String)> {
    bttv_liste(Some(v))
}

/// BTTV chaine : `{channelEmotes: [...], sharedEmotes: [...]}`.
fn parse_bttv_chaine(v: &Value) -> Vec<(String, String)> {
    let mut out = bttv_liste(v.get("channelEmotes"));
    out.extend(bttv_liste(v.get("sharedEmotes")));
    out
}

/// BTTV : liste `[{id, code}]` → (code, url CDN `2x`).
fn bttv_liste(arr: Option<&Value>) -> Vec<(String, String)> {
    arr.and_then(|a| a.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|e| {
                    let id = e["id"].as_str()?;
                    let code = e["code"].as_str()?;
                    Some((
                        code.to_string(),
                        format!("https://cdn.betterttv.net/emote/{}/2x", id),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 7TV set global : `{emotes: [{id, name}]}`.
fn parse_7tv(v: &Value) -> Vec<(String, String)> {
    stv_liste(v.get("emotes"))
}

/// 7TV chaine : `{emote_set: {emotes: [{id, name}]}}`.
fn parse_7tv_chaine(v: &Value) -> Vec<(String, String)> {
    stv_liste(v["emote_set"].get("emotes"))
}

/// 7TV : liste `[{id, name}]` → (name, url CDN `2x.webp`).
fn stv_liste(arr: Option<&Value>) -> Vec<(String, String)> {
    arr.and_then(|a| a.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|e| {
                    let id = e["id"].as_str()?;
                    let nom = e["name"].as_str()?;
                    Some((
                        nom.to_string(),
                        format!("https://cdn.7tv.app/emote/{}/2x.webp", id),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// FFZ (global + chaine) : `{sets: {<id>: {emoticons: [{id, name, urls}]}}}`
/// — TOUS les sets sont fusionnés (le global en a 3).
fn parse_ffz(v: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(sets) = v["sets"].as_object() else {
        return out;
    };
    for set in sets.values() {
        let Some(ems) = set["emoticons"].as_array() else {
            continue;
        };
        for e in ems {
            let Some(id) = e["id"].as_u64() else {
                continue;
            };
            let Some(nom) = e["name"].as_str() else {
                continue;
            };
            // `urls["2"]` si présent (protocole-relatif `//` → https),
            // sinon schéma CDN stable depuis l'id.
            let url = e["urls"]["2"]
                .as_str()
                .map(|u| {
                    if u.starts_with("//") {
                        format!("https:{}", u)
                    } else {
                        u.to_string()
                    }
                })
                .unwrap_or_else(|| {
                    format!("https://cdn.frankerfacez.com/emote/{}/2", id)
                });
            out.push((nom.to_string(), url));
        }
    }
    out
}

// ===== Construction des segments d'un message =====

/// Construit les segments d'emotes d'un message (officielles + tierces).
/// `tag_emotes` : valeur brute du tag IRC (`25:0-4,6-10/1902:12-16`),
/// None/vide si le message n'en a pas.
pub fn construire_segments(
    texte: &str,
    tag_emotes: Option<&str>,
    cache: &CacheEmotes,
) -> Vec<EmoteSeg> {
    // Tables codepoint → (offset byte, offset UTF-16) en une passe.
    let mut bytes: Vec<usize> = Vec::new();
    let mut u16s: Vec<usize> = vec![0];
    for (b, c) in texte.char_indices() {
        bytes.push(b);
        u16s.push(u16s.last().unwrap() + c.len_utf16());
    }
    let len_cp = bytes.len();
    let byte_de = |cp: usize| if cp >= len_cp { texte.len() } else { bytes[cp] };
    let u16_de = |cp: usize| u16s[cp.min(len_cp)];

    // 1. Officielles : positions du tag en codepoints, fin INCLUSIVE.
    let mut segs: Vec<EmoteSeg> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new(); // cp debut, fin exclusive
    if let Some(tag) = tag_emotes.filter(|t| !t.is_empty()) {
        for part in tag.split('/') {
            let Some((id, plages)) = part.split_once(':') else {
                continue;
            };
            for p in plages.split(',') {
                let Some((d, f)) = p.split_once('-') else {
                    continue;
                };
                let (Ok(d), Ok(f)) = (d.parse::<usize>(), f.parse::<usize>()) else {
                    continue;
                };
                // Clamp au texte (workaround bornes hors texte, cf. dank-twitch-irc).
                let (d, f) = (d.min(len_cp), (f + 1).min(len_cp));
                if f <= d {
                    continue;
                }
                ranges.push((d, f));
                segs.push(EmoteSeg {
                    debut: u16_de(d),
                    fin: u16_de(f),
                    url: format!(
                        "https://static-cdn.jtvnw.net/emoticons/v2/{}/default/dark/2.0",
                        id
                    ),
                    nom: texte[byte_de(d)..byte_de(f)].to_string(),
                });
            }
        }
    }
    ranges.sort();

    // 2. Tierces : matching par mot dans les zones non couvertes par le tag.
    if !cache.est_vide() {
        let mut cur = 0usize;
        for &(d, f) in &ranges {
            if d > cur {
                segments_tierces_morceau(
                    &texte[byte_de(cur)..byte_de(d)],
                    u16_de(cur),
                    cache,
                    &mut segs,
                );
            }
            cur = cur.max(f);
        }
        if cur < len_cp {
            segments_tierces_morceau(&texte[byte_de(cur)..], u16_de(cur), cache, &mut segs);
        }
    }

    // 3. Tri + neutralisation des chevauchements (défensif, tag corrompu).
    segs.sort_by_key(|s| s.debut);
    let mut propres: Vec<EmoteSeg> = Vec::new();
    for s in segs {
        if propres.last().map(|d| s.debut >= d.fin).unwrap_or(true) {
            propres.push(s);
        }
    }
    propres
}

/// Matching tierce d'un fragment non couvert par le tag IRC. Tokens séparés
/// par espaces : le token ENTIER d'abord (codes type `:tf:`), sinon les runs
/// de caractères de mot `[a-zA-Z0-9_]` à l'intérieur (couvre `(KEKW)`, `!x`).
fn segments_tierces_morceau(
    fragment: &str,
    u16_debut: usize,
    cache: &CacheEmotes,
    out: &mut Vec<EmoteSeg>,
) {
    // (u16 debut, u16 fin exclusive, texte) — tokens et runs de mots.
    let mut tokens: Vec<(usize, usize, String)> = Vec::new();
    let mut runs: Vec<(usize, usize, String)> = Vec::new();
    let mut tok_d: Option<(usize, String)> = None;
    let mut run_d: Option<(usize, String)> = None;
    let mut u16 = u16_debut;
    for c in fragment.chars() {
        let lu16 = c.len_utf16();
        if c.is_whitespace() {
            if let Some((d, t)) = tok_d.take() {
                tokens.push((d, u16, t));
            }
            run_d = None;
        } else {
            if tok_d.is_none() {
                tok_d = Some((u16, String::new()));
            }
            if c.is_ascii_alphanumeric() || c == '_' {
                if run_d.is_none() {
                    run_d = Some((u16, String::new()));
                }
            } else if let Some((d, r)) = run_d.take() {
                runs.push((d, u16, r));
            }
            if let Some((_, t)) = tok_d.as_mut() {
                t.push(c);
            }
            if let Some((_, r)) = run_d.as_mut() {
                r.push(c);
            }
        }
        u16 += lu16;
    }
    if let Some((d, t)) = tok_d {
        tokens.push((d, u16, t));
    }
    if let Some((d, r)) = run_d {
        runs.push((d, u16, r));
    }

    fn pousser(cache: &CacheEmotes, out: &mut Vec<EmoteSeg>, d: usize, f: usize, nom: &str) {
        if let Some(url) = cache.url_pour(nom) {
            out.push(EmoteSeg {
                debut: d,
                fin: f,
                url: url.to_string(),
                nom: nom.to_string(),
            });
        }
    }
    for (d, f, t) in &tokens {
        // Token entier : si match, ses runs sont couverts — on passe au suivant.
        if cache.url_pour(t).is_some() {
            pousser(cache, out, *d, *f, t);
            continue;
        }
        for (rd, rf, r) in runs.iter().filter(|(rd, _, _)| *rd >= *d && *rd < *f) {
            pousser(cache, out, *rd, *rf, r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_de(paires: &[(&str, &str)]) -> CacheEmotes {
        let mut tierces = HashMap::new();
        for (n, u) in paires {
            tierces.insert(n.to_string(), u.to_string());
        }
        CacheEmotes { tierces }
    }

    #[test]
    fn tag_officiel_simple() {
        let segs = construire_segments(
            "Kappa PogChamp",
            Some("25:0-4/305954335:6-13"),
            &CacheEmotes::default(),
        );
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].debut, segs[0].fin), (0, 5));
        assert_eq!(segs[0].nom, "Kappa");
        assert_eq!(
            segs[0].url,
            "https://static-cdn.jtvnw.net/emoticons/v2/25/default/dark/2.0"
        );
        assert_eq!((segs[1].debut, segs[1].fin), (6, 14));
        assert_eq!(segs[1].nom, "PogChamp");
    }

    #[test]
    fn tag_plusieurs_plages_meme_emote() {
        let segs = construire_segments("Kappa ok Kappa", Some("25:0-4,8-12"), &CacheEmotes::default());
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].debut, segs[0].fin), (0, 5));
        assert_eq!((segs[1].debut, segs[1].fin), (8, 13));
    }

    /// Sémantique confirmée : les positions du tag sont en CODEPOINTS
    /// (👩 = 1 codepoint = 2 unités UTF-16) → conversion obligatoire.
    #[test]
    fn tag_emoji_unicode_avant() {
        let segs = construire_segments("👩 Kappa", Some("25:2-6"), &CacheEmotes::default());
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (3, 8));
        assert_eq!(segs[0].nom, "Kappa");
    }

    /// Exemple des forums officiels : `👩❤️💋👩 Kappa` → `25:6-10`.
    #[test]
    fn tag_famille_emoji_zwj() {
        let texte = "👩‍👩‍👧 Kappa";
        // 5 codepoints (3 personnes + 2 ZWJ) = 8 unités UTF-16, puis espace.
        let segs = construire_segments(texte, Some("25:6-10"), &CacheEmotes::default());
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (9, 14));
        assert_eq!(segs[0].nom, "Kappa");
    }

    #[test]
    fn tag_bornes_hors_texte_clampees() {
        let segs = construire_segments("ok", Some("25:0-50"), &CacheEmotes::default());
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (0, 2));
        assert_eq!(segs[0].nom, "ok");
    }

    #[test]
    fn sans_tag_vide() {
        assert!(construire_segments("coucou", None, &CacheEmotes::default()).is_empty());
        assert!(construire_segments("coucou", Some(""), &CacheEmotes::default()).is_empty());
    }

    #[test]
    fn tierce_exacte() {
        let cache = cache_de(&[("KEKW", "https://x/kekew")]);
        let segs = construire_segments("coucou KEKW lol", None, &cache);
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (7, 11));
        assert_eq!(segs[0].nom, "KEKW");
    }

    /// `(KEKW)!!` : le token entier ne matche pas, le run de mot oui.
    #[test]
    fn tierce_ponctuation_collee() {
        let cache = cache_de(&[("KEKW", "https://x/kekew")]);
        let segs = construire_segments("(KEKW)!!", None, &cache);
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (1, 5));
        assert_eq!(segs[0].nom, "KEKW");
    }

    /// Codes avec ponctuation (BTTV legacy `:tf:`) : match du token entier.
    #[test]
    fn tierce_code_ponctue() {
        let cache = cache_de(&[(":tf:", "https://x/tf")]);
        let segs = construire_segments("ha :tf: x", None, &cache);
        assert_eq!(segs.len(), 1);
        assert_eq!((segs[0].debut, segs[0].fin), (3, 7));
        assert_eq!(segs[0].nom, ":tf:");
    }

    #[test]
    fn tierce_case_sensitive() {
        let cache = cache_de(&[("KEKW", "https://x/kekew")]);
        assert!(construire_segments("kekw", None, &cache).is_empty());
    }

    /// Le tag IRC prime : pas de double segment pour un mot à la fois
    /// officiel (couvert par le tag) et présent dans le cache tierce.
    #[test]
    fn officielle_prime_sur_tierce() {
        let cache = cache_de(&[("Kappa", "https://x/kappa")]);
        let segs = construire_segments("Kappa", Some("25:0-4"), &cache);
        assert_eq!(segs.len(), 1);
        assert!(segs[0].url.contains("jtvnw.net"));
    }

    #[test]
    fn tierces_multiples_et_tri() {
        let cache = cache_de(&[("KEKW", "https://x/1"), ("LUL", "https://x/2")]);
        let segs = construire_segments("LUL aa KEKW", None, &cache);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].nom, "LUL");
        assert_eq!(segs[1].nom, "KEKW");
    }

    #[test]
    fn parse_bttv_reponse() {
        let v: Value = serde_json::from_str(
            r#"[{"id":"54fa8fce01e468494b85b53c","code":"CiGrip","imageType":"png"}]"#,
        )
        .unwrap();
        let l = parse_bttv(&v);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].0, "CiGrip");
        assert_eq!(
            l[0].1,
            "https://cdn.betterttv.net/emote/54fa8fce01e468494b85b53c/2x"
        );
    }

    #[test]
    fn parse_bttv_chaine_reponse() {
        let v: Value = serde_json::from_str(
            r#"{"channelEmotes":[{"id":"a","code":"Ch"}],
               "sharedEmotes":[{"id":"b","code":"Sh"}]}"#,
        )
        .unwrap();
        let l = parse_bttv_chaine(&v);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].0, "Ch");
        assert_eq!(l[1].0, "Sh");
    }

    #[test]
    fn parse_7tv_reponses() {
        let global: Value = serde_json::from_str(
            r#"{"emotes":[{"id":"01FCY771D800007PQ2DF3GDTN6","name":"RainTime"}]}"#,
        )
        .unwrap();
        let l = parse_7tv(&global);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].0, "RainTime");
        assert_eq!(
            l[0].1,
            "https://cdn.7tv.app/emote/01FCY771D800007PQ2DF3GDTN6/2x.webp"
        );

        let chaine: Value = serde_json::from_str(
            r#"{"emote_set":{"emotes":[{"id":"01G3WEGZN0000ET2J0MQP5YJ0G","name":"GAMBA"}]}}"#,
        )
        .unwrap();
        assert_eq!(parse_7tv_chaine(&chaine).len(), 1);
    }

    #[test]
    fn parse_ffz_reponse() {
        let v: Value = serde_json::from_str(
            r#"{"sets":{"3":{"emoticons":[{"id":9,"name":"ZrehplaR",
               "urls":{"1":"//cdn.frankerfacez.com/emote/9/1","2":"//cdn.frankerfacez.com/emote/9/2"}}]}}}"#,
        )
        .unwrap();
        let l = parse_ffz(&v);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].0, "ZrehplaR");
        assert_eq!(l[0].1, "https://cdn.frankerfacez.com/emote/9/2");
    }
}
