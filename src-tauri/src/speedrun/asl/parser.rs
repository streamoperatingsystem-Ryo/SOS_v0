// =============================================================================
// ASL Parser — Parse les fichiers .asl (Auto Split Language de LiveSplit)
// -----------------------------------------------------------------------------
// Le format ASL a deux parties :
//
// 1. State definitions : déclarent les processus à surveiller et les adresses
//    mémoire à lire (type + identifiant + module + base + offsets).
//
// 2. Method bodies : du code (C# dans le format original) qui implémente la
//    logique du splitter (startup, init, update, start, split, reset, etc.).
//
// Le parser est manuel (pas de pest) car les blocs de code contiennent des
// accolades imbriquées, des strings avec accolades, des commentaires — ce qui
// rend les grammaires PEG difficiles à utiliser correctement.
// =============================================================================
use std::collections::HashMap;

/// Un fichier ASL parsé : state definitions + method bodies.
#[derive(Debug, Clone)]
pub struct AslScript {
    /// Map : nom du processus → liste de state definitions (une par version).
    pub states: HashMap<String, Vec<AslStateDef>>,
    /// Méthodes extraites (code brut entre accolades).
    pub methods: AslMethods,
}

/// Une state definition (un bloc `state("proc", "version") { ... }`).
#[derive(Debug, Clone)]
pub struct AslStateDef {
    pub process_name: String,
    pub game_version: String,
    pub variables: Vec<AslVarDef>,
}

/// Une variable dans une state definition.
#[derive(Debug, Clone)]
pub struct AslVarDef {
    pub type_name: String,    // "int", "float", "string16", "byte32", etc.
    pub identifier: String,   // nom de la variable (ex: "Progress")
    pub module: String,       // nom du module (ex: "mgsi.exe")
    pub base: i32,            // offset de base
    pub offsets: Vec<i32>,    // chaîne d'offsets
}

/// Les 13 méthodes possibles d'un script ASL.
#[derive(Debug, Clone, Default)]
#[allow(non_snake_case)]
pub struct AslMethods {
    pub startup: Option<String>,
    pub shutdown: Option<String>,
    pub init: Option<String>,
    pub exit: Option<String>,
    pub update: Option<String>,
    pub start: Option<String>,
    pub split: Option<String>,
    pub reset: Option<String>,
    pub isLoading: Option<String>,
    pub gameTime: Option<String>,
    pub onStart: Option<String>,
    pub onSplit: Option<String>,
    pub onReset: Option<String>,
}

impl AslMethods {
    /// Noms des méthodes reconnues (dans l'ordre de priorité du parser).
    const METHOD_NAMES: &'static [&'static str] = &[
        "startup",
        "shutdown",
        "init",
        "exit",
        "update",
        "start",
        "split",
        "reset",
        "isLoading",
        "gameTime",
        "onStart",
        "onSplit",
        "onReset",
    ];

    pub fn set(&mut self, name: &str, code: String) {
        match name {
            "startup" => self.startup = Some(code),
            "shutdown" => self.shutdown = Some(code),
            "init" => self.init = Some(code),
            "exit" => self.exit = Some(code),
            "update" => self.update = Some(code),
            "start" => self.start = Some(code),
            "split" => self.split = Some(code),
            "reset" => self.reset = Some(code),
            "isLoading" => self.isLoading = Some(code),
            "gameTime" => self.gameTime = Some(code),
            "onStart" => self.onStart = Some(code),
            "onSplit" => self.onSplit = Some(code),
            "onReset" => self.onReset = Some(code),
            _ => {}
        }
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        match name {
            "startup" => self.startup.as_ref(),
            "shutdown" => self.shutdown.as_ref(),
            "init" => self.init.as_ref(),
            "exit" => self.exit.as_ref(),
            "update" => self.update.as_ref(),
            "start" => self.start.as_ref(),
            "split" => self.split.as_ref(),
            "reset" => self.reset.as_ref(),
            "isLoading" => self.isLoading.as_ref(),
            "gameTime" => self.gameTime.as_ref(),
            "onStart" => self.onStart.as_ref(),
            "onSplit" => self.onSplit.as_ref(),
            "onReset" => self.onReset.as_ref(),
            _ => None,
        }
    }
}

// =============================================================================
// PARSER MANUEL
// =============================================================================

/// Parse un fichier ASL complet.
pub fn parse(source: &str) -> Result<AslScript, String> {
    let mut parser = Parser::new(source);
    let mut script = AslScript {
        states: HashMap::new(),
        methods: AslMethods::default(),
    };

    loop {
        parser.skip_ws_and_comments();

        if parser.is_eof() {
            break;
        }

        // Essayer de parser un state() ou une méthode
        if parser.peek_keyword("state") {
            let state_def = parser.parse_state_def()?;
            script
                .states
                .entry(state_def.process_name.clone())
                .or_default()
                .push(state_def);
        } else if let Some(method_name) = parser.try_parse_method_name() {
            parser.skip_ws_and_comments();
            let code = parser.parse_braced_block()?;
            script.methods.set(method_name, code);
        } else {
            // Caractère inattendu — skip et continue (tolérant)
            let pos = parser.pos;
            parser.advance(1);
            log::trace!(
                "[ASL Parser] Caractère inattendu à {}: {:?} — skip",
                pos,
                &parser.source[pos..pos + 10.min(parser.source.len() - pos)]
            );
        }
    }

    Ok(script)
}

struct Parser<'a> {
    source: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.source.len());
    }

    /// Vérifie si le texte à la position courante correspond au keyword.
    /// Le keyword doit être suivi d'un non-identifiant (espace, parenthèse, etc.).
    fn peek_keyword(&self, kw: &str) -> bool {
        let kw_bytes = kw.as_bytes();
        if self.pos + kw_bytes.len() > self.source.len() {
            return false;
        }
        if &self.source[self.pos..self.pos + kw_bytes.len()] != kw_bytes {
            return false;
        }
        // Vérifier que le caractère suivant n'est pas un identifiant
        if let Some(next) = self.source.get(self.pos + kw_bytes.len()) {
            if next.is_ascii_alphanumeric() || *next == b'_' {
                return false;
            }
        }
        true
    }

    /// Skip whitespace et commentaires (// et /* */).
    fn skip_ws_and_comments(&mut self) {
        loop {
            // Whitespace
            while let Some(c) = self.peek() {
                if c.is_ascii_whitespace() {
                    self.advance(1);
                } else {
                    break;
                }
            }

            // Commentaires
            if self.peek() == Some(b'/') {
                if self.peek_at(1) == Some(b'/') {
                    // Commentaire ligne : skip jusqu'à \n
                    while let Some(c) = self.peek() {
                        self.advance(1);
                        if c == b'\n' {
                            break;
                        }
                    }
                    continue;
                } else if self.peek_at(1) == Some(b'*') {
                    // Commentaire bloc : skip jusqu'à */
                    self.advance(2);
                    while !self.is_eof() {
                        if self.peek() == Some(b'*') && self.peek_at(1) == Some(b'/') {
                            self.advance(2);
                            break;
                        }
                        self.advance(1);
                    }
                    continue;
                }
            }
            break;
        }
    }

    /// Tente de parser un nom de méthode. Retourne le nom si reconnu.
    fn try_parse_method_name(&mut self) -> Option<&'static str> {
        for &name in AslMethods::METHOD_NAMES {
            if self.peek_keyword(name) {
                self.advance(name.len());
                return Some(name);
            }
        }
        None
    }

    /// Parse un bloc entre accolades en respectant l'imbrication.
    /// Le parser doit être positionné juste avant l'accolade ouvrante.
    /// Retourne le contenu (sans les accolades externes).
    fn parse_braced_block(&mut self) -> Result<String, String> {
        self.skip_ws_and_comments();

        if self.peek() != Some(b'{') {
            return Err(format!(
                "Attendu '{{' à la position {}, trouvé {:?}",
                self.pos,
                self.peek().map(|c| c as char)
            ));
        }

        self.advance(1); // Skip '{'
        let start = self.pos;
        let mut depth: i32 = 1;
        let mut in_string = false;
        let mut in_char = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;
        let mut escape = false;

        while !self.is_eof() && depth > 0 {
            let c = self.peek().unwrap();

            if in_line_comment {
                if c == b'\n' {
                    in_line_comment = false;
                }
                self.advance(1);
                continue;
            }

            if in_block_comment {
                if c == b'*' && self.peek_at(1) == Some(b'/') {
                    in_block_comment = false;
                    self.advance(2);
                } else {
                    self.advance(1);
                }
                continue;
            }

            if in_string {
                if escape {
                    escape = false;
                } else if c == b'\\' {
                    escape = true;
                } else if c == b'"' {
                    in_string = false;
                }
                self.advance(1);
                continue;
            }

            if in_char {
                if escape {
                    escape = false;
                } else if c == b'\\' {
                    escape = true;
                } else if c == b'\'' {
                    in_char = false;
                }
                self.advance(1);
                continue;
            }

            match c {
                b'/' if self.peek_at(1) == Some(b'/') => {
                    in_line_comment = true;
                    self.advance(2);
                }
                b'/' if self.peek_at(1) == Some(b'*') => {
                    in_block_comment = true;
                    self.advance(2);
                }
                b'"' => {
                    in_string = true;
                    self.advance(1);
                }
                b'\'' => {
                    in_char = true;
                    self.advance(1);
                }
                b'{' => {
                    depth += 1;
                    self.advance(1);
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let end = self.pos;
                        let code = String::from_utf8_lossy(&self.source[start..end]).to_string();
                        self.advance(1); // Skip '}'
                        return Ok(code.trim().to_string());
                    }
                    self.advance(1);
                }
                _ => {
                    self.advance(1);
                }
            }
        }

        Err("Bloc non terminé : accolade fermante manquante".to_string())
    }

    /// Parse une state definition : `state("proc", "version") { vars }`
    fn parse_state_def(&mut self) -> Result<AslStateDef, String> {
        // Skip "state"
        self.advance(5);
        self.skip_ws_and_comments();

        // "("
        if self.peek() != Some(b'(') {
            return Err(format!("Attendu '(' après 'state' à {}", self.pos));
        }
        self.advance(1);
        self.skip_ws_and_comments();

        // "processname"
        let process_name = self.parse_string()?;
        self.skip_ws_and_comments();

        // Optionnel : ", version"
        let mut game_version = String::new();
        if self.peek() == Some(b',') {
            self.advance(1);
            self.skip_ws_and_comments();
            game_version = self.parse_string()?;
        }
        self.skip_ws_and_comments();

        // ")"
        if self.peek() != Some(b')') {
            return Err(format!("Attendu ')' à {}", self.pos));
        }
        self.advance(1);
        self.skip_ws_and_comments();

        // "{"
        if self.peek() != Some(b'{') {
            return Err(format!("Attendu '{{' après state() à {}", self.pos));
        }
        self.advance(1);

        // Parser les variables jusqu'à '}'
        let mut variables = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.peek() == Some(b'}') {
                self.advance(1);
                break;
            }
            if self.is_eof() {
                return Err("State definition non terminée".to_string());
            }

            let var = self.parse_var_def()?;
            variables.push(var);

            // Skip le ';' optionnel
            self.skip_ws_and_comments();
            if self.peek() == Some(b';') {
                self.advance(1);
            }
        }

        Ok(AslStateDef {
            process_name,
            game_version,
            variables,
        })
    }

    /// Parse une variable : `type identifier : "module" base, offset1, offset2, ...`
    fn parse_var_def(&mut self) -> Result<AslVarDef, String> {
        // Type
        let type_name = self.parse_identifier()?;
        self.skip_ws_and_comments();

        // Identifier
        let identifier = self.parse_identifier()?;
        self.skip_ws_and_comments();

        // ":"
        if self.peek() != Some(b':') {
            return Err(format!(
                "Attendu ':' après '{}' dans la variable à {}",
                identifier, self.pos
            ));
        }
        self.advance(1);
        self.skip_ws_and_comments();

        // Module (string optionnelle suivie de ',')
        let mut module = String::new();
        if self.peek() == Some(b'"') {
            module = self.parse_string()?;
            self.skip_ws_and_comments();
            if self.peek() == Some(b',') {
                self.advance(1);
                self.skip_ws_and_comments();
            }
        }

        // Offsets : base, offset1, offset2, ...
        let mut offsets = Vec::new();
        loop {
            let num = self.parse_number()?;
            offsets.push(num);
            self.skip_ws_and_comments();
            if self.peek() == Some(b',') {
                self.advance(1);
                self.skip_ws_and_comments();
            } else {
                break;
            }
        }

        // Le premier offset est la base, les suivants sont les offsets
        let base = offsets[0];
        let real_offsets = offsets[1..].to_vec();

        Ok(AslVarDef {
            type_name,
            identifier,
            module,
            base,
            offsets: real_offsets,
        })
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("Attendu '\"' à {}", self.pos));
        }
        self.advance(1);
        let mut result = String::new();
        let mut escape = false;

        while !self.is_eof() {
            let c = self.peek().unwrap();
            if escape {
                match c {
                    b'n' => result.push('\n'),
                    b't' => result.push('\t'),
                    b'r' => result.push('\r'),
                    b'\\' => result.push('\\'),
                    b'"' => result.push('"'),
                    b'0' => result.push('\0'),
                    _ => result.push(c as char),
                }
                escape = false;
                self.advance(1);
            } else if c == b'\\' {
                escape = true;
                self.advance(1);
            } else if c == b'"' {
                self.advance(1);
                return Ok(result);
            } else {
                result.push(c as char);
                self.advance(1);
            }
        }

        Err("String non terminée".to_string())
    }

    fn parse_identifier(&mut self) -> Result<String, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance(1);
            } else {
                break;
            }
        }
        if self.pos == start {
            return Err(format!("Identifiant attendu à {}", self.pos));
        }
        Ok(String::from_utf8_lossy(&self.source[start..self.pos]).to_string())
    }

    fn parse_number(&mut self) -> Result<i32, String> {
        let start = self.pos;

        // Signe optionnel
        if self.peek() == Some(b'-') || self.peek() == Some(b'+') {
            self.advance(1);
        }

        // Hex (0x...) ou décimal
        if self.peek() == Some(b'0')
            && (self.peek_at(1) == Some(b'x') || self.peek_at(1) == Some(b'X'))
        {
            self.advance(2);
            while let Some(c) = self.peek() {
                if c.is_ascii_hexdigit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            let s = &self.source[start..self.pos];
            let hex_str = std::str::from_utf8(s).map_err(|e| e.to_string())?;
            i32::from_str_radix(&hex_str[2..], 16)
                .map_err(|e| format!("Nombre hex invalide '{}': {}", hex_str, e))
        } else {
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.advance(1);
                } else {
                    break;
                }
            }
            let s = std::str::from_utf8(&self.source[start..self.pos]).map_err(|e| e.to_string())?;
            s.parse::<i32>().map_err(|e| format!("Nombre invalide '{}': {}", s, e))
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_state() {
        let source = r#"
            state("testgame") {
                int health : "testgame.exe", 0x400000, 0x10, 0x20;
                float x : "testgame.exe", 0x400000, 0x30;
            }
        "#;
        let script = parse(source).unwrap();
        assert!(script.states.contains_key("testgame"));
        let states = &script.states["testgame"];
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].variables.len(), 2);
        assert_eq!(states[0].variables[0].type_name, "int");
        assert_eq!(states[0].variables[0].identifier, "health");
        assert_eq!(states[0].variables[0].module, "testgame.exe");
        assert_eq!(states[0].variables[0].base, 0x400000);
        assert_eq!(states[0].variables[0].offsets, vec![0x10, 0x20]);
    }

    #[test]
    fn test_parse_method() {
        let source = r#"
            startup {
                vars.test = 42;
                print("hello");
            }
            update {
                if (vars.test > 0) {
                    return true;
                }
                return false;
            }
        "#;
        let script = parse(source).unwrap();
        assert!(script.methods.startup.is_some());
        assert!(script.methods.update.is_some());
        let update = script.methods.update.unwrap();
        assert!(update.contains("return true"));
        assert!(update.contains("return false"));
    }

    #[test]
    fn test_parse_nested_braces() {
        let source = r#"
            update {
                if (x) {
                    while (y) {
                        print("nested {braces}");
                    }
                }
            }
        "#;
        let script = parse(source).unwrap();
        assert!(script.methods.update.is_some());
    }

    #[test]
    fn test_parse_multiple_states() {
        let source = r#"
            state("game1", "1.0") {}
            state("game1", "2.0") {}
            state("game2") {}
        "#;
        let script = parse(source).unwrap();
        assert!(script.states.contains_key("game1"));
        assert!(script.states.contains_key("game2"));
        assert_eq!(script.states["game1"].len(), 2);
        assert_eq!(script.states["game2"].len(), 1);
    }

    #[test]
    fn test_parse_mgs_script() {
        // Test avec un extrait représentatif du script MGS
        let source = r#"
            state("duckstation-qt-x64-ReleaseLTCG") {}
            state("ePSXe") {}
            state("mgsi") {}

            startup {
                vars.D = new ExpandoObject();
                var D = vars.D;
                D.Sets = new ExpandoObject();
                D.Funcs = new ExpandoObject();
                D.Mem = new MemoryWatcherList();
            }

            update {
                var D = vars.D;
                var M = D.Mem;
                M.UpdateAll(game);
            }
        "#;
        let script = parse(source).unwrap();
        assert_eq!(script.states.len(), 3);
        assert!(script.states.contains_key("duckstation-qt-x64-ReleaseLTCG"));
        assert!(script.states.contains_key("ePSXe"));
        assert!(script.states.contains_key("mgsi"));
        assert!(script.methods.startup.is_some());
        assert!(script.methods.update.is_some());
    }

    #[test]
    fn test_parse_comments() {
        let source = r#"
            // Ceci est un commentaire ligne
            /* Ceci est un
               commentaire bloc */
            state("game") {} // fin de ligne
            update {
                // commentaire dans méthode
                return true;
            }
        "#;
        let script = parse(source).unwrap();
        assert!(script.states.contains_key("game"));
        assert!(script.methods.update.is_some());
    }

    #[test]
    fn test_parse_string_with_braces() {
        let source = r#"
            update {
                var s = "text with {braces} inside";
                print("also {braces}");
            }
        "#;
        let script = parse(source).unwrap();
        assert!(script.methods.update.is_some());
    }
}
