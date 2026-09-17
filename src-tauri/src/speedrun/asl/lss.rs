// =============================================================================
// LSS Parser — Parse les fichiers .lss (LiveSplit Splits XML)
// -----------------------------------------------------------------------------
// Format LSS : XML LiveSplit contenant les segments d'une run avec leurs
// Personal Best (PB) times. On extrait uniquement ce qui est utile au splitter :
//   - Nom de chaque segment
//   - PB RealTime et GameTime (cumulé, depuis le début de la run)
//
// Le parsing est manuel (pas de quick-xml) pour rester léger et ne pas ajouter
// de dépendance. On scanne le XML tag par tag, en gérant l'imbrication
// <Segments> → <Segment> → <Name> + <SplitTime name="Personal Best">.
//
// Structure LSS (extrait) :
//   <Run>
//     <GameName>...</GameName>
//     <CategoryName>...</CategoryName>
//     <Segments>
//       <Segment>
//         <Name>Segment 1</Name>
//         <SplitTimes>
//           <SplitTime name="Personal Best">
//             <RealTime>00:01:23.45</RealTime>
//             <GameTime>00:01:20.00</GameTime>
//           </SplitTime>
//         </SplitTimes>
//       </Segment>
//       ...
//     </Segments>
//   </Run>
// =============================================================================

/// Un segment de run LSS (un "split" dans la terminologie LiveSplit).
#[derive(Debug, Clone, serde::Serialize)]
pub struct LssSegment {
    /// Nom du segment (ex: "World 1", "Boss Fight").
    pub nom: String,
    /// PB RealTime cumulé (secondes), si présent.
    pub pb_real_time: Option<f64>,
    /// PB GameTime cumulé (secondes), si présent.
    pub pb_game_time: Option<f64>,
}

/// Une run LSS parsée.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LssRun {
    /// Nom du jeu (balise <GameName>).
    pub nom_jeu: String,
    /// Nom de la catégorie (balise <CategoryName>).
    pub nom_categorie: String,
    /// Segments dans l'ordre.
    pub segments: Vec<LssSegment>,
}

/// Erreur de parsing LSS.
#[derive(Debug)]
pub struct LssError {
    pub message: String,
}

impl std::fmt::Display for LssError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LSS: {}", self.message)
    }
}

impl std::error::Error for LssError {}

/// Parse un fichier .lss (contenu XML en string) et retourne la run.
pub fn parse(source: &str) -> Result<LssRun, LssError> {
    // Strip BOM UTF-8 (U+FEFF) si présent en tête de fichier.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);

    let parser = XmlScanner::new(source);

    // Nom du jeu
    let nom_jeu = parser
        .lire_texte_balise("GameName")
        .unwrap_or_default()
        .trim()
        .to_string();

    // Nom de la catégorie
    let nom_categorie = parser
        .lire_texte_balise("CategoryName")
        .unwrap_or_default()
        .trim()
        .to_string();

    // Segments — recherche directe des <Segment> dans le source complet.
    // On ne dépend PAS du bloc <Segments> externe (plus robuste face aux
    // variations de format : BOM, attributs, espaces, self-closing, etc.).
    let segments = extraire_segments(source);

    if segments.is_empty() {
        // Debug : compter les occurrences de "<Segment" pour aider l'utilisateur
        let lower = source.to_lowercase();
        let count_segment = lower.matches("<segment").count();
        let count_segments_tag = lower.matches("<segments").count();
        let apercu = source.len().min(200);
        return Err(LssError {
            message: format!(
                "Aucun segment trouvé dans le fichier .lss \
                 (occurrences \"<Segment\": {}, \"<Segments\": {}, \
                 premiers 200 chars: {:?})",
                count_segment, count_segments_tag, &source[..apercu]
            ),
        });
    }

    Ok(LssRun {
        nom_jeu,
        nom_categorie,
        segments,
    })
}

/// Extrait tous les segments depuis le XML en cherchant directement les
/// balises <Segment> (sans dépendre du bloc <Segments> externe).
///
/// Pour distinguer <Segment> de <Segments>, on vérifie que le caractère
/// qui suit "segment" n'est pas 's' (case-insensitive). Les caractères
/// valides après "Segment" sont : '>', '/', espace, tab, newline.
fn extraire_segments(source: &str) -> Vec<LssSegment> {
    let mut segments = Vec::new();
    let lower = source.to_lowercase();

    // Chercher toutes les occurrences de "<segment" et filtrer celles qui
    // sont des <Segment> (pas <Segments>).
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("<segment") {
        let abs_pos = search_from + pos;

        // Vérifier le caractère après "segment" (8 chars = "<segment")
        let after = abs_pos + 8; // position juste après "segment"
        if after >= lower.len() {
            break;
        }
        let next_char = lower.as_bytes()[after];
        // 's' = 0x73 → c'est <Segments>, on skip
        if next_char == b's' {
            search_from = abs_pos + 8;
            continue;
        }

        // C'est un <Segment> — trouver la fin de la balise d'ouverture
        let open_end = match lower[abs_pos..].find('>') {
            Some(p) => abs_pos + p,
            None => break,
        };
        let open_tag = &source[abs_pos..=open_end];

        // Self-closing <Segment/> ? (rare mais possible)
        if open_tag.ends_with("/>") {
            search_from = open_end + 1;
            continue;
        }

        // Trouver </Segment> (la closing tag)
        let close_pattern = "</segment>";
        let close_idx = match lower[open_end + 1..].find(close_pattern) {
            Some(p) => open_end + 1 + p,
            None => break,
        };

        // Extraire le contenu du segment
        let content_start = open_end + 1;
        let content_end = close_idx;
        let seg_block = &source[content_start..content_end];

        let nom = lire_texte_balise_dans(seg_block, "Name")
            .unwrap_or_default()
            .trim()
            .to_string();

        // PB : chercher <SplitTime name="Personal Best">...</SplitTime>
        let (pb_real, pb_game) = extraire_pb_split_time(seg_block);

        segments.push(LssSegment {
            nom,
            pb_real_time: pb_real,
            pb_game_time: pb_game,
        });

        // Continuer après </Segment>
        search_from = close_idx + close_pattern.len();
    }

    segments
}

/// Extrait les temps PB (RealTime + GameTime) depuis le bloc d'un segment.
/// Cherche <SplitTime name="Personal Best"> puis <RealTime> et <GameTime> dedans.
fn extraire_pb_split_time(seg_block: &str) -> (Option<f64>, Option<f64>) {
    // Chercher <SplitTime name="Personal Best"> — l'attribut peut avoir des
    // variations de quoting/espaces. On cherche "Personal Best" dans une
    // balise SplitTime.
    let split_time_block = trouver_split_time_pb(seg_block);

    let pb_real = split_time_block
        .as_ref()
        .and_then(|b| lire_texte_balise_dans(b, "RealTime"))
        .and_then(|s| parser_temps_lss(s.trim()));

    let pb_game = split_time_block
        .as_ref()
        .and_then(|b| lire_texte_balise_dans(b, "GameTime"))
        .and_then(|s| parser_temps_lss(s.trim()));

    (pb_real, pb_game)
}

/// Trouve le bloc <SplitTime name="Personal Best">...</SplitTime> dans un
/// segment. Gère les variations de format (quotes simples/doubles, espaces).
fn trouver_split_time_pb(seg_block: &str) -> Option<String> {
    // On cherche <SplitTime ... name="Personal Best" ...> puis le </SplitTime>
    // correspondant. La balise peut être self-closing (<SplitTime ... />) si
    // pas de PB — dans ce cas on retourne un bloc vide (pas de temps).
    let lower = seg_block.to_lowercase();
    let pattern = "personal best";

    // Trouver toutes les occurrences de <SplitTime
    let mut search_from = 0;
    while let Some(st_idx) = lower[search_from..].find("<splittime") {
        let abs_idx = search_from + st_idx;
        // Trouver la fin de la balise d'ouverture (le '>' après <SplitTime ...)
        let open_end = lower[abs_idx..].find('>')?;
        let open_tag = &seg_block[abs_idx..abs_idx + open_end + 1];

        // Vérifier si c'est "Personal Best"
        if open_tag.to_lowercase().contains(pattern) {
            // Self-closing ? (<SplitTime ... />)
            if open_tag.ends_with("/>") {
                return Some(String::new()); // Bloc vide = pas de temps
            }
            // Sinon, trouver </SplitTime>
            let close_idx = lower[abs_idx + open_end + 1..].find("</splittime>")?;
            let content_start = abs_idx + open_end + 1;
            let content_end = abs_idx + open_end + 1 + close_idx;
            return Some(seg_block[content_start..content_end].to_string());
        }

        search_from = abs_idx + open_end + 1;
    }
    None
}

// =============================================================================
// Scanner XML manuel — utilitaires bas niveau
// =============================================================================

struct XmlScanner<'a> {
    source: &'a str,
}

impl<'a> XmlScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self { source }
    }

    /// Lit le texte contenu dans la première occurrence de <balise>...</balise>.
    fn lire_texte_balise(&self, balise: &str) -> Option<String> {
        lire_texte_balise_dans(self.source, balise)
    }
}

/// Lit le texte de la première occurrence de <balise>...</balise> dans `source`.
fn lire_texte_balise_dans(source: &str, balise: &str) -> Option<String> {
    let (content, _) = trouver_balise(source, balise)?;
    Some(content)
}

/// Trouve la première occurrence de <balise>...</balise> et retourne
/// (contenu interne, position de fin après </balise>).
fn trouver_balise(source: &str, balise: &str) -> Option<(String, usize)> {
    trouver_balise_depuis(source, balise, 0)
}

/// Trouve la première occurrence de <balise>...</balise> à partir de `from`.
/// Retourne (contenu interne, position après </balise>).
/// Gère les balises self-closing <balise/> (retourne contenu vide).
fn trouver_balise_depuis(source: &str, balise: &str, from: usize) -> Option<(String, usize)> {
    let lower = source.to_lowercase();
    let balise_lower = balise.to_lowercase();

    // Chercher <balise ...> ou <balise/>
    let open_pattern = format!("<{}", balise_lower);
    let open_idx = lower[from..].find(&open_pattern)? + from;

    // Trouver la fin de la balise d'ouverture
    let open_end = lower[open_idx..].find('>')? + open_idx;
    let open_tag = &source[open_idx..=open_end];

    // Self-closing ?
    if open_tag.ends_with("/>") {
        return Some((String::new(), open_end + 1));
    }

    // Chercher </balise>
    let close_pattern = format!("</{}>", balise_lower);
    let close_idx = lower[open_end + 1..].find(&close_pattern)? + open_end + 1;
    let content_start = open_end + 1;
    let content_end = close_idx;
    let after_close = close_idx + close_pattern.len();

    Some((source[content_start..content_end].to_string(), after_close))
}

// =============================================================================
// Parsing des temps LSS
// =============================================================================

/// Parse un temps au format LiveSplit ("HH:MM:SS.mmm" ou "MM:SS.mmm" ou "SS.mmm")
/// et retourne le nombre de secondes (f64).
/// Gère aussi les temps négatifs (ex: "-00:01.23" pour un ahead).
fn parser_temps_lss(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }

    let s_trim = s.trim();
    let (neg, reste) = if let Some(r) = s_trim.strip_prefix('-') {
        (true, r)
    } else {
        (false, s_trim)
    };

    // Séparer par ':' — peut avoir 1, 2 ou 3 parties
    let parties: Vec<&str> = reste.split(':').collect();
    let total: f64 = match parties.len() {
        1 => {
            // SS.mmm
            parties[0].parse::<f64>().ok()?
        }
        2 => {
            // MM:SS.mmm
            let mins: f64 = parties[0].parse().ok()?;
            let secs: f64 = parties[1].parse().ok()?;
            mins * 60.0 + secs
        }
        3 => {
            // HH:MM:SS.mmm
            let hours: f64 = parties[0].parse().ok()?;
            let mins: f64 = parties[1].parse().ok()?;
            let secs: f64 = parties[2].parse().ok()?;
            hours * 3600.0 + mins * 60.0 + secs
        }
        _ => return None,
    };

    Some(if neg { -total } else { total })
}

/// Formate un temps en secondes vers "HH:MM:SS.mmm" (pour affichage).
pub fn formater_temps(secondes: f64) -> String {
    let total_ms = (secondes.abs() * 1000.0).round() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let secs = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;

    let prefix = if secondes < 0.0 { "-" } else { "" };
    if hours > 0 {
        format!("{}{:02}:{:02}:{:02}.{:03}", prefix, hours, minutes, secs, ms)
    } else {
        format!("{}{:02}:{:02}.{:03}", prefix, minutes, secs, ms)
    }
}

// =============================================================================
// Tests unitaires
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    const LSS_EXEMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Run version="1.7.0">
  <GameIcon />
  <GameName>Super Mario World</GameName>
  <CategoryName>Any%</CategoryName>
  <Metadata />
  <Time>
    <RealTime>00:10:30.00</RealTime>
    <GameTime>00:10:25.00</GameTime>
  </Time>
  <Segments>
    <Segment>
      <Name>World 1</Name>
      <Icon />
      <SplitTimes>
        <SplitTime name="Personal Best">
          <RealTime>00:01:23.45</RealTime>
          <GameTime>00:01:20.00</GameTime>
        </SplitTime>
        <SplitTime name="Best Segments">
          <RealTime>00:01:20.00</RealTime>
        </SplitTime>
      </SplitTimes>
      <BestSegmentTime>
        <RealTime>00:01:20.00</RealTime>
      </BestSegmentTime>
    </Segment>
    <Segment>
      <Name>World 2</Name>
      <Icon />
      <SplitTimes>
        <SplitTime name="Personal Best">
          <RealTime>00:03:45.00</RealTime>
          <GameTime>00:03:40.00</GameTime>
        </SplitTime>
      </SplitTimes>
    </Segment>
    <Segment>
      <Name>World 3</Name>
      <Icon />
      <SplitTimes>
        <SplitTime name="Personal Best">
          <RealTime>00:10:30.00</RealTime>
          <GameTime>00:10:25.00</GameTime>
        </SplitTime>
      </SplitTimes>
    </Segment>
  </Segments>
  <AutoSplitterSettings />
</Run>"#;

    #[test]
    fn test_parse_lss_nom_jeu_categorie() {
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        assert_eq!(run.nom_jeu, "Super Mario World");
        assert_eq!(run.nom_categorie, "Any%");
    }

    #[test]
    fn test_parse_lss_segments_count() {
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        assert_eq!(run.segments.len(), 3, "Doit avoir 3 segments");
    }

    #[test]
    fn test_parse_lss_segments_noms() {
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        assert_eq!(run.segments[0].nom, "World 1");
        assert_eq!(run.segments[1].nom, "World 2");
        assert_eq!(run.segments[2].nom, "World 3");
    }

    #[test]
    fn test_parse_lss_pb_real_time() {
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        // World 1 PB RealTime = 00:01:23.45 = 83.45s
        assert!((run.segments[0].pb_real_time.unwrap() - 83.45).abs() < 0.01);
        // World 2 PB RealTime = 00:03:45.00 = 225.0s
        assert!((run.segments[1].pb_real_time.unwrap() - 225.0).abs() < 0.01);
        // World 3 PB RealTime = 00:10:30.00 = 630.0s
        assert!((run.segments[2].pb_real_time.unwrap() - 630.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_pb_game_time() {
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        // World 1 PB GameTime = 00:01:20.00 = 80.0s
        assert!((run.segments[0].pb_game_time.unwrap() - 80.0).abs() < 0.01);
        // World 2 PB GameTime = 00:03:40.00 = 220.0s
        assert!((run.segments[1].pb_game_time.unwrap() - 220.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_ignorer_best_segments() {
        // Le parser doit ignorer "Best Segments" et ne garder que "Personal Best"
        let run = parse(LSS_EXEMPLE).expect("Le parse doit réussir");
        // World 1 a Best Segments RealTime = 00:01:20.00 = 80.0s
        // mais PB RealTime = 83.45s → on doit avoir 83.45, pas 80.0
        assert!((run.segments[0].pb_real_time.unwrap() - 83.45).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_self_closing_split_time() {
        // Un segment sans PB (self-closing SplitTime) doit donner None
        let lss = r#"<Run><Segments>
          <Segment>
            <Name>Empty</Name>
            <SplitTimes>
              <SplitTime name="Personal Best" />
            </SplitTimes>
          </Segment>
        </Segments></Run>"#;
        let run = parse(lss).expect("Le parse doit réussir");
        assert_eq!(run.segments[0].nom, "Empty");
        assert!(run.segments[0].pb_real_time.is_none());
        assert!(run.segments[0].pb_game_time.is_none());
    }

    #[test]
    fn test_parse_lss_vide_erreur() {
        let lss = "<Run><Segments></Segments></Run>";
        let result = parse(lss);
        assert!(result.is_err(), "Un LSS sans segments doit retourner une erreur");
    }

    #[test]
    fn test_parser_temps_format_court() {
        assert!((parser_temps_lss("42.5").unwrap() - 42.5).abs() < 0.001);
        assert!((parser_temps_lss("01:23.45").unwrap() - 83.45).abs() < 0.001);
        assert!((parser_temps_lss("00:01:23.45").unwrap() - 83.45).abs() < 0.001);
        assert!((parser_temps_lss("01:00:00.00").unwrap() - 3600.0).abs() < 0.001);
    }

    #[test]
    fn test_parser_temps_negatif() {
        let t = parser_temps_lss("-00:01.50").unwrap();
        assert!((t - (-1.5)).abs() < 0.001);
    }

    #[test]
    fn test_formater_temps() {
        assert_eq!(formater_temps(83.45), "01:23.450");
        assert_eq!(formater_temps(3600.0), "01:00:00.000");
        assert_eq!(formater_temps(-1.5), "-00:01.500");
    }

    // --- Tests avec un vrai .lss LiveSplit (structure réaliste) ---

    /// Extrait d'un vrai fichier .lss LiveSplit (avec métadonnées, attributs,
    /// BOM, self-closing tags, etc.). Structure minimale mais fidèle au format
    /// réel exporté par LiveSplit 1.8+.
    const LSS_REEL: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
<Run version=\"1.8.8\">\r\n\
  <GameIcon />\r\n\
  <GameName>Celeste</GameName>\r\n\
  <CategoryName>Any%</CategoryName>\r\n\
  <Metadata>\r\n\
    <Run>\r\n\
      <SpeedrunComRunID />\r\n\
      <RunTime>00:26:57.00</RunTime>\r\n\
    </Run>\r\n\
    <Platform>PC</Platform>\r\n\
  </Metadata>\r\n\
  <Time>\r\n\
    <RealTime>00:26:57.00</RealTime>\r\n\
    <GameTime>00:26:57.00</GameTime>\r\n\
  </Time>\r\n\
  <Segments>\r\n\
    <Segment>\r\n\
      <Name>Prologue</Name>\r\n\
      <Icon />\r\n\
      <SplitTimes>\r\n\
        <SplitTime name=\"Personal Best\">\r\n\
          <RealTime>00:01:23.45</RealTime>\r\n\
          <GameTime>00:01:23.45</GameTime>\r\n\
        </SplitTime>\r\n\
        <SplitTime name=\"Best Segments\" />\r\n\
        <SplitTime name=\"Comparison Segment\">\r\n\
          <RealTime>00:01:30.00</RealTime>\r\n\
        </SplitTime>\r\n\
      </SplitTimes>\r\n\
      <BestSegmentTime>\r\n\
        <RealTime>00:01:20.00</RealTime>\r\n\
        <GameTime>00:01:20.00</GameTime>\r\n\
      </BestSegmentTime>\r\n\
    </Segment>\r\n\
    <Segment>\r\n\
      <Name>Chapter 1: Forsaken City</Name>\r\n\
      <Icon />\r\n\
      <SplitTimes>\r\n\
        <SplitTime name=\"Personal Best\">\r\n\
          <RealTime>00:04:56.00</RealTime>\r\n\
          <GameTime>00:04:56.00</GameTime>\r\n\
        </SplitTime>\r\n\
      </SplitTimes>\r\n\
    </Segment>\r\n\
    <Segment>\r\n\
      <Name>Chapter 2: Old Site</Name>\r\n\
      <Icon />\r\n\
      <SplitTimes>\r\n\
        <SplitTime name=\"Personal Best\">\r\n\
          <RealTime>00:09:20.00</RealTime>\r\n\
          <GameTime>00:09:20.00</GameTime>\r\n\
        </SplitTime>\r\n\
      </SplitTimes>\r\n\
    </Segment>\r\n\
  </Segments>\r\n\
  <AutoSplitterSettings />\r\n\
</Run>";

    #[test]
    fn test_parse_lss_reel_avec_bom() {
        // Le fichier commence par un BOM UTF-8 — le parser doit le stripper.
        let run = parse(LSS_REEL).expect("Le parse du .lss réel doit réussir");
        assert_eq!(run.nom_jeu, "Celeste");
        assert_eq!(run.nom_categorie, "Any%");
        assert_eq!(run.segments.len(), 3, "Doit avoir 3 segments");
    }

    #[test]
    fn test_parse_lss_reel_segments_noms() {
        let run = parse(LSS_REEL).expect("Le parse doit réussir");
        assert_eq!(run.segments[0].nom, "Prologue");
        assert_eq!(run.segments[1].nom, "Chapter 1: Forsaken City");
        assert_eq!(run.segments[2].nom, "Chapter 2: Old Site");
    }

    #[test]
    fn test_parse_lss_reel_pb_real_time() {
        let run = parse(LSS_REEL).expect("Le parse doit réussir");
        // Prologue PB = 00:01:23.45 = 83.45s
        assert!((run.segments[0].pb_real_time.unwrap() - 83.45).abs() < 0.01);
        // Chapter 1 PB = 00:04:56.00 = 296.0s
        assert!((run.segments[1].pb_real_time.unwrap() - 296.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_reel_pb_game_time() {
        let run = parse(LSS_REEL).expect("Le parse doit réussir");
        assert!((run.segments[0].pb_game_time.unwrap() - 83.45).abs() < 0.01);
        assert!((run.segments[2].pb_game_time.unwrap() - 560.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_reel_ignorer_autres_split_times() {
        // Le segment Prologue a "Best Segments" (self-closing) et
        // "Comparison Segment" — le parser doit ignorer ces deux-là et ne
        // garder que "Personal Best".
        let run = parse(LSS_REEL).expect("Le parse doit réussir");
        // PB RealTime = 83.45, pas 90.00 (Comparison Segment)
        assert!((run.segments[0].pb_real_time.unwrap() - 83.45).abs() < 0.01);
    }

    #[test]
    fn test_parse_lss_reel_crlf() {
        // Le fichier utilise \r\n (Windows) — le parser doit gérer ça.
        let run = parse(LSS_REEL).expect("Le parse doit réussir");
        assert_eq!(run.segments.len(), 3);
        assert_eq!(run.segments[0].nom, "Prologue"); // pas de \r dans le nom
    }

    #[test]
    fn test_parse_lss_erreur_avec_debug() {
        // Un fichier sans segments doit retourner une erreur avec des infos
        // de debug (occurrences, aperçu du contenu).
        let lss = "<?xml version=\"1.0\"?>\n<Run><GameName>Test</GameName></Run>";
        let result = parse(lss);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("occurrences"), "L'erreur doit contenir des infos debug: {}", msg);
        assert!(msg.contains("0"), "L'erreur doit indiquer 0 occurrences: {}", msg);
    }
}
