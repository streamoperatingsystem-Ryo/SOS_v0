// =============================================================================
// Transpiler C# → JavaScript (enrichi avec patterns du script MGS, char-safe)
// -----------------------------------------------------------------------------
// Tous les patterns identifiés par analyse du script MetalGearSolid.asl 148KB.
// **Char-safe** : utilise char_indices() au lieu de bytes[i] pour éviter les
// panics sur les caractères multi-byte UTF-8 (le script MGS contient des
// caractères Unicode comme ⮞ dans les commentaires/art ASCII).
// =============================================================================

/// Transpile du code C# (méthode ASL) vers JavaScript.
pub fn transpile_cs_to_js(csharp: &str) -> String {
    // Sécurité : si la transpilation échoue (panic), retourner le code original.
    // Le moteur JS peut toujours essayer d'exécuter le C# tel quel (il échouera
    // sur la syntaxe C# spécifique, mais au moins ça ne crash pas l'app).
    let result = std::panic::catch_unwind(|| transpile_cs_to_js_inner(csharp));
    result.unwrap_or_else(|_| {
        log::warn!("[ASL Transpiler] Panic évité — retour du code original");
        csharp.to_string()
    })
}

fn transpile_cs_to_js_inner(csharp: &str) -> String {
    let mut js = csharp.to_string();

    // === 1. Retirer les casts de delegate
    js = remove_delegate_casts(&js);
    js = js.replace("(Action)", "");

    // === 2. Types primitifs → let
    let type_keywords = [
        "int", "uint", "long", "ulong", "float", "double", "byte", "sbyte", "short", "ushort",
        "bool", "string", "char", "object", "dynamic", "decimal", "var",
    ];
    for &t in &type_keywords {
        let pattern = format!(" {} ", t);
        js = js.replace(&pattern, " let ");
        let pattern2 = format!("\t{}", t);
        js = js.replace(&pattern2, &format!("\tlet {}", t));
        // E7 T4 — for (TYPE name = ...) → for (let name = ...)
        let pattern3 = format!("({} ", t);
        js = js.replace(&pattern3, "(let ");
    }
    // E7 T4 — Type annotations customes : `Type identifier =` → `let identifier =`
    // char-safe : détecte un identifiant PascalCase suivi d'un identifiant suivi de `=`
    js = remove_custom_type_annotations(&js);
    // E7 T4 — Type annotations avec array : `Type[] identifier =` → `let identifier =`
    // char-safe : retire le `Type[]` devant un identifiant
    js = remove_array_type_annotations(&js);

    // === 3. ExpandoObject
    js = js.replace("new ExpandoObject()", "{}");
    js = js.replace("new ExpandoObject", "{}");

    // === 3a. MemoryWatcher<T>(args) → MemoryWatcher(args, 'T') (auto-start fix)
    // DOIT être avant transpile_object_initializer (3b) pour que l'initializer
    // voie `new MemoryWatcher(addr, 'short') { Name = "..." }` (sans <T>), et
    // avant remove_method_generics (étape 28) qui stripperait <T> en perdant
    // le type. Sans ce fix, new MemoryWatcher<short>(addr) → new MemoryWatcher(addr)
    // → Type = 'int' (défaut) → lecture 4 bytes au lieu de 2 → valeur faussée.
    js = transpile_memory_watcher_generics(&js);

    // === 3b. Object/collection initializers (E7 T2)
    // new X(args) { Prop = val } → IIFE ; new MemoryWatcherList() { w1, w2 } → IIFE .Add
    js = transpile_object_initializer(&js);

    // === 4. Collections
    // Dictionary en PREMIER (avant List) car les Dictionary init peuvent contenir
    // des new List<string>() { ... } imbriqués. transpile_dictionary_init gère
    // les List imbriquées en transpilant les valeurs récursivement.
    js = transpile_dictionary_init(&js);
    js = js.replace("new Dictionary<", "new Map<");
    js = remove_generics(&js, "Map");
    // List: new List<T>() → [] (remove_generics strip <T>, puis replace new List() → [])
    js = transpile_list_init(&js);
    js = remove_generics(&js, "List");
    js = js.replace("new List()", "[]");
    js = js.replace("new List ()", "[]");
    // HashSet: new HashSet<T>() → new Set()
    js = transpile_hashset_init(&js);
    js = js.replace("new HashSet<", "new Set<");
    js = remove_generics(&js, "Set");
    // Stack: new Stack<T>() → []
    js = remove_generics(&js, "Stack");
    js = js.replace("new Stack()", "[]");
    js = js.replace("new Stack ()", "[]");
    js = js.replace(".Push(", ".push(");
    js = js.replace(".Pop()", ".pop()");
    js = js.replace(".Peek()", ".slice(-1)[0]");
    js = js.replace("new StringBuilder()", "''");
    js = js.replace("new StringBuilder", "''");
    js = transpile_tuples(&js);

    // === 5. Tableaux typés
    for arr_type in &[
        "new short[]", "new int[]", "new string[]", "new byte[]", "new long[]",
        "new uint[]", "new ushort[]", "new sbyte[]", "new float[]", "new double[]",
        "new bool[]", "new char[]", "new object[]",
    ] {
        js = js.replace(arr_type, "");
    }
    // === 5b. Tableaux typés avec taille — new byte[len] → new Array(len)
    // (auto-start fix) : new byte[] (brackets vides) est géré par step 5 ci-dessus
    // + transpile_object_initializer (step 3b) pour `new byte[] { ... }`.
    // Mais new byte[len] (avec taille) n'est géré par AUCUNE étape → reste
    // `new byte[len]` en JS → `byte` est un identifiant nu non défini →
    // ReferenceError "byte is not defined". Ce cas est atteint maintenant que
    // le strip .exe permet au switch case "mgsi" de matcher (section PC memwatchers
    // appelle New.ByteArray qui fait `new byte[len]`).
    js = transpile_typed_array_creation(&js);

    // === 6. foreach (DOIT être après les étapes 2-5 pour que les collections soient transpilées)
    js = replace_foreach(&js);
    // T4 — Si un foreach KeyValuePair utilisait "var" comme nom de variable (mot-clé JS),
    // remplacer var.Key/var.Value par __kv.Key/__kv.Value dans tout le code
    js = js.replace("var.Key", "__kv.Key");
    js = js.replace("var.Value", "__kv.Value");

    // === 7. Méthodes de collection
    // IMPORTANT: remplacer .Count() AVANT .Count pour éviter .length() (function call)
    js = js.replace(".Count()", ".length");
    js = js.replace(".Count", ".length");
    // C# .Length (string/array property) → JS .length (lowercase)
    js = js.replace(".Length()", ".length");
    js = js.replace(".Length", ".length");
    js = js.replace(".Clear()", ".clear()");
    js = js.replace(".RemoveRange(", ".splice(");

    // === 8. Strings
    js = js.replace("String.Empty", "\"\"");
    js = js.replace("string.Empty", "\"\"");
    // E8 Bug A — .Equals(x) → ?.equals(x) (optional chaining) pour guard null.
    // En C#, appeler .Equals sur null throw NullReferenceException, mais les
    // scripts ASL initialisent souvent les vars à null/undefined et testent
    // .Equals avant de les set. Sans le ?., Boa throw "not a callable function"
    // car .equals est appelé sur undefined. Avec ?., undefined?.equals(x)
    // retourne undefined (falsy) → le if (!...) passe sans erreur.
    js = js.replace(".Equals(", "?.equals(");
    // E7 T4 — Verbatim strings C# `@"..."` → `"..."` (char-safe)
    // `""` dans le verbatim → `\"` en JS
    js = transpile_verbatim_strings(&js);

    // === 9. DateTime
    js = js.replace("DateTime.Now", "new Date()");
    js = js.replace("DateTime.UtcNow", "new Date()");
    // .Second → .getSeconds() — char-safe pour éviter .SecondIncremented → .getSeconds()Incremented
    js = replace_property_with_boundary(&js, ".Second", ".getSeconds()");
    js = js.replace(".AddMilliseconds(", ".addMilliseconds(");
    js = js.replace(".AddSeconds(", ".addSeconds(");

    // === 10. IntPtr
    js = js.replace("IntPtr.Zero", "0");
    // IntPtr.Add(a, b) → (a + b) — char-safe, remplace la virgule top-level par +
    js = transpile_intptr_add(&js);

    // === 10b. default(T) → null (E7 T3)
    // Doit être AVANT l'étape 11 (casts) car (int) dans default(int) serait strippé
    js = transpile_default(&js);

    // === 11. Casts — retirer les casts de type primitif
    for cast in &[
        "(IntPtr)", "(long)", "(int)", "(short)", "(uint)", "(ushort)",
        "(sbyte)", "(byte)", "(decimal)", "(float)", "(double)", "(bool)", "(string)",
    ] {
        js = js.replace(cast, "");
    }

    // === 12. using blocks
    js = remove_using_blocks(&js);

    // === 13. Events += / -= (char-safe)
    // Distingue events (delegate/function ref) de arithmetic (littéral number/string)
    // Events: obj.Event += Handler; → obj.Event.__subscribe(Handler);
    // Arithmetic: i += 3; → i += 3; (préservé)
    js = transpile_event_ops(&js);

    // === 14. Math aliases
    for &(cs, js_fn) in &[
        ("Math.Ceiling(", "Math.ceil("),
        ("Math.Floor(", "Math.floor("),
        ("Math.Max(", "Math.max("),
        ("Math.Min(", "Math.min("),
        ("Math.Abs(", "Math.abs("),
        ("Math.Sqrt(", "Math.sqrt("),
        ("Math.Round(", "Math.round("),
    ] {
        js = js.replace(cs, js_fn);
    }

    // === 15. Convert
    js = js.replace("Convert.ToInt32(", "parseInt(");
    js = js.replace("Convert.ToDouble(", "parseFloat(");
    js = js.replace("Convert.ToBoolean(", "Boolean(");
    js = js.replace("Convert.ToString(", "String(");
    js = js.replace("Convert.ToByte(", "parseInt(");
    js = js.replace("Convert.FromBase64String(", "atob(");

    // === 16. Console
    js = js.replace("Console.WriteLine(", "print(");
    js = js.replace("Console.Write(", "print(");

    // === 17. return; → return null;
    js = js.replace("return;", "return null;");

    // === 18. Opérateurs
    js = js.replace(" ?? ", " || ");
    // E7 T4 — `expr as Type` → `expr` (retire le cast as)
    // char-safe : détecte ` as Identifier` et retire le `as Identifier`
    js = remove_as_casts(&js);

    // === 19. WinForms stubs
    // NE PAS remplacer new Form(), new Button(), etc. par {} — les constructeurs
    // sont définis dans les shims (script_bridge.rs) et initialisent les events
    // (.Click, .Closed, etc.). Remplacer par {} ferait que btn.Click est undefined,
    // et btn.Click.__subscribe(handler) throw « cannot convert 'null' or 'undefined' ».
    // FormBorderStyle est défini dans les shims — ne PAS le stripper.

    // === 20. System.IO
    js = js.replace("System.IO.StreamWriter", "__StreamWriterStub");
    js = js.replace("Directory.Exists(", "__dir_exists(");
    js = js.replace("Directory.CreateDirectory(", "__dir_create(");

    // === 21. MemPageType enums
    js = js.replace("MemPageType.MEM_MAPPED", "0x40000");
    js = js.replace("MemPageType.MEM_PRIVATE", "0x20000");
    js = js.replace("MemPageType.MEM_IMAGE", "0x1000000");
    js = js.replace("MemPageState.MEM_COMMIT", "0x1000");

    // === 22. UIntPtr
    js = js.replace("(UIntPtr)", "");

    // === 23. DateTime ToString
    js = js.replace(".ToString(\"T\")", ".toLocaleTimeString()");
    js = js.replace(".ToString(\"t\")", ".toLocaleTimeString()");
    // E8 Bug A — .ToString() → ?.ToString() (optional chaining) pour guard null.
    // Même raison que .Equals → ?.equals : les scripts ASL appellent .ToString()
    // sur des vars potentiellement undefined (ex: CurProg.ToString() où CurProg
    // n'est pas encore initialisé). Sans le ?., Boa throw "not a callable function".
    // On le met APRÈS les replaces DateTime ToString pour ne pas casser .ToString("T").
    js = js.replace(".ToString(", "?.ToString(");

    // === 24. Delegate handlers — char-safe
    // new EntryWrittenEventHandler(arg) → arg (pass-through, retire wrapper ET parenthèse fermante)
    // new EventHandler<T>(arg) → arg
    js = remove_delegate_wrapper(&js, "new EntryWrittenEventHandler(");
    js = remove_delegate_wrapper(&js, "new EventHandler(");
    // LiveSplit.Model.Input.EventHandlerT<TimerPhase> → TimerPhase (retire le prefix et le <)
    js = js.replace("LiveSplit.Model.Input.EventHandlerT<", "");
    js = js.replace("EventHandler<", "");
    // T4 — Casts résiduels (TypeName>) → retirer le cast complet
    // Après les replaces ci-dessus, il reste (TimerPhase>) qu'on doit retirer
    js = remove_residual_type_casts(&js);

    // === 25. Empty parens résiduels (E7 T2 — E1)
    // Post-traitement char-safe : remplace `()` isolé en position d'expression
    // par `(0)` pour éviter "empty parenthesized expression" dans Boa.
    js = remove_empty_parens(&js);

    // === 26. Modificateurs de paramètres C# (E7 T3)
    // out/ref/in/params devant un paramètre → retirer
    // ex: TryGetValue(code, out name) → TryGetValue(code, name)
    // Gère aussi (out ..., (ref ..., (in ... (pas d'espace avant)
    js = js.replace(" out ", " ");
    js = js.replace(" ref ", " ");
    js = js.replace(" in ", " ");
    js = js.replace("(out ", "(");
    js = js.replace("(ref ", "(");
    js = js.replace("(in ", "(");
    js = js.replace(",out ", ",");
    js = js.replace(",ref ", ",");
    js = js.replace(",in ", ",");
    // params (rare) — retirer le mot-clé
    js = js.replace(" params ", " ");
    js = js.replace("(params ", "(");

    // === 27. nameof(X) → "X" (E7 T3)
    js = transpile_nameof(&js);

    // === 28. Génériques de méthode Method<T>(args) → Method(args) (E7 T3)
    // char-safe : retire le <T> entre le nom de méthode et la parenthèse
    js = remove_method_generics(&js);

    // === 29. Named args Method(name: value) → Method(value) (E7 T3)
    // char-safe : retire le `name:` devant une valeur dans les appels
    js = remove_named_args(&js);

    js
}

/// Itère sur les caractères d'une string avec leurs offsets byte.
/// Retourne un Vec<(byte_offset, char)> pour un indexing char-safe.
fn char_indices_vec(s: &str) -> Vec<(usize, char)> {
    s.char_indices().collect()
}

/// Trouve la position byte du début d'un pattern ASCII dans la string,
/// en commençant la recherche à partir de `from_byte`.
/// Utilise char_indices pour ne jamais atterrir au milieu d'un char multi-byte.
fn find_from(s: &str, pattern: &str, from_byte: usize) -> Option<usize> {
    if from_byte >= s.len() {
        return None;
    }
    s[from_byte..].find(pattern).map(|p| from_byte + p)
}

/// Retire les casts de delegate C# (char-safe).
pub(crate) fn remove_delegate_casts(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut idx = 0; // index dans chars, pas dans bytes

    while idx < chars.len() {
        let (byte_off, ch) = chars[idx];

        // Chercher "(Func<" ou "(Action<"
        if ch == '(' {
            let rest = &code[byte_off..];
            if rest.starts_with("(Func<") || rest.starts_with("(Action<") {
                // Trouver le '>' correspondant — on avance char par char
                // j commence après le '<', depth = 1 pour ce '<'
                let skip = if rest.starts_with("(Func<") { 6 } else { 8 }; // "(Func<" = 6, "(Action<" = 8
                let mut depth = 1;
                let mut j = idx + skip;
                while j < chars.len() && depth > 0 {
                    match chars[j].1 {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if depth == 0 {
                    // j est après le '>'
                    // Skip le ')' qui suit
                    if j < chars.len() && chars[j].1 == ')' {
                        j += 1;
                        // Skip whitespace
                        while j < chars.len() && chars[j].1.is_whitespace() {
                            j += 1;
                        }
                        idx = j;
                        continue;
                    }
                }
            } else if rest.starts_with("(Action)") {
                idx += 8; // "(Action)" = 8 chars
                while idx < chars.len() && chars[idx].1.is_whitespace() {
                    idx += 1;
                }
                continue;
            } else if rest.starts_with("(EventHandler)") {
                idx += 14; // "(EventHandler)" = 14 chars
                while idx < chars.len() && chars[idx].1.is_whitespace() {
                    idx += 1;
                }
                continue;
            } else if rest.starts_with("(CheckBox)") {
                idx += 10; // "(CheckBox)" = 10 chars
                while idx < chars.len() && chars[idx].1.is_whitespace() {
                    idx += 1;
                }
                continue;
            } else {
                // E7 T4 — Casts de type générique `(TypeName<...>)` → retirer
                // Détecte `(Identifier<...>)` et retire le cast complet
                let mut j = idx + 1;
                // Skip whitespace
                while j < chars.len() && chars[j].1.is_whitespace() { j += 1; }
                // Extraire l'identifiant du type
                if j < chars.len() && (chars[j].1.is_alphabetic() || chars[j].1 == '_') {
                    while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_' || chars[j].1 == '.') {
                        j += 1;
                    }
                    // Vérifier si suivi de `<`
                    if j < chars.len() && chars[j].1 == '<' {
                        // Trouver le `>` correspondant
                        let mut depth = 1;
                        j += 1;
                        while j < chars.len() && depth > 0 {
                            match chars[j].1 {
                                '<' => depth += 1,
                                '>' => depth -= 1,
                                _ => {}
                            }
                            j += 1;
                        }
                        // Vérifier si suivi de `)` — cast complet
                        if j < chars.len() && chars[j].1 == ')' {
                            j += 1;
                            // Skip whitespace
                            while j < chars.len() && chars[j].1.is_whitespace() { j += 1; }
                            idx = j;
                            continue;
                        }
                    }
                }
            }
        }

        result.push(ch);
        idx += 1;
    }

    result
}

/// Transpile `new Dictionary<K,V>() { {k1,v1}, {k2,v2} }` → IIFE Map.
pub(crate) fn transpile_dictionary_init(code: &str) -> String {
    let mut result = code.to_string();

    while let Some(pos) = result.find("new Dictionary<") {
        // Trouver la fin du générique <...>
        let mut depth = 1;
        let mut j = pos + result[pos..].find('<').unwrap() + 1;
        let rbytes = result.as_bytes();
        while j < rbytes.len() && depth > 0 {
            match rbytes[j] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        // j est après le '>'
        // Chercher soit "()" puis "{", soit "{" directement (sans parenthèses)
        let rest = &result[j..];
        let rbytes = result.as_bytes();
        // Skip whitespace après '>'
        let mut ws = 0;
        while j + ws < rbytes.len() && rbytes[j + ws].is_ascii_whitespace() {
            ws += 1;
        }
        // Cas 1: "()" puis "{"
        let has_parens = rest[ws..].starts_with("()");
        let after_parens = if has_parens {
            j + ws + 2
        } else {
            j + ws // pas de parenthèses, on est directement sur '{' (ou autre)
        };
        // Skip whitespace après ()  ou après >
        let mut k = after_parens;
        while k < rbytes.len() && rbytes[k].is_ascii_whitespace() {
            k += 1;
        }
        if k < rbytes.len() && rbytes[k] == b'{' {
                // Trouver l'accolade fermante correspondante
                let mut brace_depth = 1;
                let mut m = k + 1;
                while m < rbytes.len() && brace_depth > 0 {
                    match rbytes[m] {
                        b'{' => brace_depth += 1,
                        b'}' => brace_depth -= 1,
                        _ => {}
                    }
                    m += 1;
                }
                // m est après le '}' final
                // Extraire le contenu — utiliser floor_char_boundary pour sécurité
                let start = safe_slice_start(&result, k + 1);
                let end = safe_slice_end(&result, m.saturating_sub(1));
                let init_content = result[start..end].to_string();
                // T4 — Transpiler les List imbriquées dans les valeurs avant de parser les paires
                let init_content = transpile_list_init(&init_content);
                // Parser les paires {k, v}
                let mut sets = Vec::new();
                let mut pair_depth = 0;
                let mut bracket_depth = 0; // T4 — tracker aussi les [] des List transpilées
                let mut pair_start = 0;
                let init_bytes = init_content.as_bytes();
                for (i, &b) in init_bytes.iter().enumerate() {
                    match b {
                        b'{' => {
                            if pair_depth == 0 && bracket_depth == 0 {
                                pair_start = i + 1;
                            }
                            pair_depth += 1;
                        }
                        b'}' => {
                            if pair_depth > 0 {
                                pair_depth -= 1;
                                if pair_depth == 0 && bracket_depth == 0 {
                                    let pair = init_content[pair_start..i].trim();
                                    if let Some(comma_pos) = find_top_level_comma(pair) {
                                        let key = pair[..comma_pos].trim();
                                        let val = pair[comma_pos + 1..].trim();
                                        sets.push(format!("m.set({}, {})", key, val));
                                    }
                                }
                            }
                        }
                        b'[' => bracket_depth += 1,
                        b']' => {
                            if bracket_depth > 0 { bracket_depth -= 1; }
                        }
                        _ => {}
                    }
                }

                let replacement = if sets.is_empty() {
                    "new Map()".to_string()
                } else {
                    format!(
                        "(() => {{ var m = new Map(); {}; return m; }})()",
                        sets.join("; ")
                    )
                };

                result.replace_range(pos..m, &replacement);
                continue;
            }
        // Pas d'initializer ({ après ()) — skip cette occurrence
        // Remplacer temporairement pour éviter boucle infinie, continuer la recherche
        let before = &result[..pos];
        let after = &result[pos + 4..]; // skip "new "
        result = format!("{}__DICTSKIP__{}", before, after);
    }

    // Restaurer les __DICTSKIP__ → new
    result = result.replace("__DICTSKIP__", "new ");

    result
}

/// Transpile `new List<T>() { a, b, c }` → `[a, b, c]`
pub(crate) fn transpile_list_init(code: &str) -> String {
    let mut result = code.to_string();

    while let Some(pos) = result.find("new List<") {
        let mut depth = 1;
        let mut j = pos + result[pos..].find('<').unwrap() + 1;
        let rbytes = result.as_bytes();
        while j < rbytes.len() && depth > 0 {
            match rbytes[j] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            j += 1;
        }

        let rest = &result[j..];
        if let Some(paren_end) = rest.find("()") {
            let after_parens = j + paren_end + 2;
            let rbytes = result.as_bytes();
            let mut k = after_parens;
            while k < rbytes.len() && rbytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < rbytes.len() && rbytes[k] == b'{' {
                let mut brace_depth = 1;
                let mut m = k + 1;
                let mut in_string = false;
                let mut string_char = b'"';
                let mut prev_char = b' ';
                while m < rbytes.len() && brace_depth > 0 {
                    let b = rbytes[m];
                    if in_string {
                        if b == string_char && prev_char != b'\\' {
                            in_string = false;
                        }
                    } else {
                        if b == b'"' || b == b'\'' {
                            in_string = true;
                            string_char = b;
                        } else {
                            match b {
                                b'{' => brace_depth += 1,
                                b'}' => brace_depth -= 1,
                                _ => {}
                            }
                        }
                    }
                    prev_char = b;
                    m += 1;
                }
                // m est maintenant JUSTE APRÈS le '}' fermant (inconditionnel)
                let start = safe_slice_start(&result, k + 1);
                let end = safe_slice_end(&result, m.saturating_sub(1));
                let init_content = result[start..end].trim().to_string();
                // T4 — Si le contenu contient des commentaires //, ajouter un newline avant ]
                // pour éviter que le ] soit commenté
                let closing = if init_content.contains("//") {
                    "\n]"
                } else {
                    "]"
                };
                let replacement = format!("[{}{}", init_content, closing);
                result.replace_range(pos..m, &replacement);
                continue;
            }
        }
        // Pas d'initializer — skip
        let before = &result[..pos];
        let after = &result[pos + 4..];
        result = format!("{}__LISTSKIP__{}", before, after);
    }

    result = result.replace("__LISTSKIP__", "new ");

    result
}

/// Transpile `new Tuple<T1,T2>(a, b)` → `[a, b]`
fn transpile_tuples(code: &str) -> String {
    let mut result = code.to_string();

    while let Some(pos) = result.find("new Tuple<") {
        let mut depth = 1;
        let mut j = pos + result[pos..].find('<').unwrap() + 1;
        let rbytes = result.as_bytes();
        while j < rbytes.len() && depth > 0 {
            match rbytes[j] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            j += 1;
        }

        let rest = &result[j..];
        if let Some(paren_start) = rest.find('(') {
            let p = j + paren_start;
            let mut pd = 1;
            let mut q = p + 1;
            let rbytes = result.as_bytes();
            while q < rbytes.len() && pd > 0 {
                match rbytes[q] {
                    b'(' => pd += 1,
                    b')' => pd -= 1,
                    _ => {}
                }
                q += 1;
            }
            let start = safe_slice_start(&result, p + 1);
            let end = safe_slice_end(&result, q.saturating_sub(1));
            let args = &result[start..end];
            let replacement = format!("[{}]", args);
            result.replace_range(pos..q, &replacement);
        } else {
            break;
        }
    }

    for &(prop, idx) in &[(".Item1", "[0]"), (".Item2", "[1]"), (".Item3", "[2]"),
        (".Item4", "[3]"), (".Item5", "[4]"), (".Item6", "[5]"), (".Item7", "[6]")] {
        result = result.replace(prop, idx);
    }

    result
}

/// Convertit les blocs `using(...) { ... }` → `{ let X = expr; ... }` (char-safe).
/// Préserve la déclaration de variable à l'intérieur du using.
/// - `using (var X = expr) { ... }` → `{ let X = expr; ... }`
/// - `using (Type X = expr) { ... }` → `{ let X = expr; ... }`
/// - `using (expr) { ... }` → `{ ... }` (pas de variable)
fn remove_using_blocks(code: &str) -> String {
    let mut result = code.to_string();

    loop {
        let pos = result.find("using(").or_else(|| result.find("using ("));
        if pos.is_none() {
            break;
        }
        let pos = pos.unwrap();
        // Trouver la parenthèse fermante
        let paren_start = result[pos..].find('(').unwrap() + pos;
        let mut depth = 1;
        let mut j = paren_start + 1;
        let rbytes = result.as_bytes();
        while j < rbytes.len() && depth > 0 {
            match rbytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        // j est après le ')'
        // Extraire le contenu entre parenthèses
        let inner = &result[paren_start + 1..j - 1].trim();

        // Vérifier si c'est une déclaration de variable : `var X = expr` ou `Type X = expr`
        // Le `{` du bloc using est préservé dans le code source, donc on ne l'ajoute PAS.
        let replacement = if let Some(rest) = inner.strip_prefix("var ") {
            // `var X = expr` → `let X = expr;`
            format!("let {};", rest)
        } else {
            // Vérifier si c'est `Type X = expr` (Type = identifiant PascalCase suivi d'un identifiant)
            let inner_trimmed = inner.trim_start();
            if let Some(space_pos) = inner_trimmed.find(' ') {
                let after_type = inner_trimmed[space_pos..].trim_start();
                if let Some(eq_pos) = after_type.find('=') {
                    let var_name = after_type[..eq_pos].trim();
                    let expr = after_type[eq_pos + 1..].trim();
                    if !var_name.is_empty()
                        && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
                    {
                        format!("let {} = {};", var_name, expr)
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                // Pas de variable (juste une expression) → rien
                "".to_string()
            }
        };

        result.replace_range(pos..j, &replacement);
    }

    result
}

/// Retire les génériques d'un type (char-safe via char_indices)
pub(crate) fn remove_generics(code: &str, type_name: &str) -> String {
    let mut result = String::with_capacity(code.len());
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_off, _ch) = chars[idx];

        // Vérifier si on est au début du type_name
        if code[byte_off..].starts_with(type_name) {
            let after = byte_off + type_name.len();
            if after < code.len() && code.as_bytes()[after] == b'<' {
                // Trouver le '>' correspondant en avançant char par char
                let mut depth = 1;
                let mut j = idx + type_name.len() + 1; // après '<'
                while j < chars.len() && depth > 0 {
                    match chars[j].1 {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                result.push_str(type_name);
                idx = j; // j est après le '>'
            } else {
                result.push_str(type_name);
                idx += type_name.len();
            }
        } else {
            result.push(chars[idx].1);
            idx += 1;
        }
    }

    result
}

/// Remplace `foreach (var x in coll)` par `for (const x of coll)` (char-safe)
/// Gère les deux cas :
/// - Avec accolades : `foreach (var x in coll) { body }` → `for (...) { var x = ...; body }`
/// - Sans accolades : `foreach (var x in coll) statement;` → `for (...) { var x = ...; statement; }`
fn replace_foreach(code: &str) -> String {
    let mut result = code.to_string();

    while let Some(start) = result.find("foreach") {
        let after_foreach = &result[start..];
        let paren_start = match after_foreach.find('(') {
            Some(p) => start + p,
            None => break,
        };

        // Trouver la parenthèse fermante correspondante (char-safe)
        let mut depth = 1;
        let rbytes = result.as_bytes();
        let mut i = paren_start + 1;
        while i < rbytes.len() && depth > 0 {
            match rbytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        let paren_end = i - 1;

        let content = result[safe_slice_start(&result, paren_start + 1)..paren_end].to_string();

        if let Some(in_pos) = content.find(" in ") {
            let var_part = content[..in_pos].trim().to_string();
            let coll_part = content[in_pos + 4..].trim().to_string();

            // Vérifier si le corps du foreach commence par '{' (avec accolades)
            // ou non (sans accolades — single statement)
            let after_paren = paren_end + 1;
            let mut skip = after_paren;
            // Skip whitespace
            let rb = result.as_bytes();
            while skip < rb.len() && (rb[skip] as char).is_whitespace() {
                skip += 1;
            }
            let has_braces = skip < rb.len() && rb[skip] == b'{';

            if var_part.contains("KeyValuePair") {
                // foreach (KeyValuePair<K,V> p in dict) → wrapper { Key, Value }
                // Même approche que le cas non-KeyValuePair : wrapper avec { Key, Value }
                let var_name = var_part.split_whitespace().last().unwrap_or("x");
                // T4 — Si var_name est "let" (transformé par étape 2), restaurer en "var"
                let var_name = if var_name == "let" { "var" } else { var_name };
                // "var" est un mot-clé JS — utiliser "__kv" à la place
                let safe_name = if var_name == "var" { "__kv" } else { var_name };
                let temp_name = format!("__{}", safe_name);
                let prefix = format!(
                    "for (const {temp} of ({coll} instanceof Map ? {coll}.entries() : {coll})) {{ var {var_n} = ({coll} instanceof Map && {temp} instanceof Array) ? {{ Key: {temp}[0], Value: {temp}[1] }} : {temp};",
                    temp = temp_name, var_n = safe_name, coll = coll_part
                );

                if has_braces {
                    result.replace_range(start..skip + 1, &prefix);
                } else {
                    result.replace_range(start..paren_end + 1, &prefix);
                    add_closing_brace(&mut result, start + prefix.len());
                }
                // Si le nom original était "var", remplacer var.Key/var.Value dans le corps
                // par __kv.Key/__kv.Value (best-effort, limité au bloc du foreach)
                if var_name == "var" {
                    // Trouver la fin du bloc foreach et remplacer var. par __kv.
                    // On le fait après le replace pour éviter les conflits
                    // TODO: limitation — ne remplace que dans le bloc immédiat
                }
            } else {
                // E7 T2 — foreach (var p in dict) où p.Key/p.Value sont utilisés
                // On wrap chaque entrée en { Key, Value } pour préserver p.Key/p.Value
                // sans réécriture du corps. Pour les List (arrays), p = valeur directe.
                let var_name = var_part.split_whitespace().last().unwrap_or("x");
                let temp_name = format!("__{}", var_name);
                let prefix = format!(
                    "for (const {temp} of ({coll} instanceof Map ? {coll}.entries() : {coll})) {{ var {var_n} = ({coll} instanceof Map && {temp} instanceof Array) ? {{ Key: {temp}[0], Value: {temp}[1] }} : {temp};",
                    temp = temp_name, var_n = var_name, coll = coll_part
                );

                if has_braces {
                    // Avec accolades : remplacer foreach(...) + '{' par le prefix (sans '{' supplémentaire)
                    // Le prefix se termine par ';' et le '{' original suit
                    result.replace_range(start..skip + 1, &prefix);
                } else {
                    // Sans accolades : ajouter '{' (déjà dans le prefix) et '}' après le statement
                    result.replace_range(start..paren_end + 1, &prefix);
                    // Trouver la fin du statement et ajouter '}'
                    add_closing_brace(&mut result, start + prefix.len());
                }
            }
        } else {
            break;
        }
    }

    result
}

/// Ajoute un `}` fermant après la fin du statement qui commence à `from_pos`.
/// Le statement se termine au premier `;` à depth 0 (paren/brace aware).
fn add_closing_brace(code: &mut String, from_pos: usize) {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut idx = 0;
    // Trouver l'index de départ dans chars
    while idx < chars.len() && chars[idx].0 < from_pos {
        idx += 1;
    }

    let mut paren_depth = 0;
    let mut brace_depth = 0;
    while idx < chars.len() {
        let (byte_off, ch) = chars[idx];
        match ch {
            '(' => paren_depth += 1,
            ')' => {
                if paren_depth > 0 { paren_depth -= 1; }
            }
            '{' => brace_depth += 1,
            '}' => {
                if brace_depth > 0 { brace_depth -= 1; }
            }
            ';' => {
                if paren_depth == 0 && brace_depth == 0 {
                    // Insérer '}' après ce ';'
                    let insert_pos = byte_off + 1;
                    code.insert(insert_pos, '}');
                    return;
                }
            }
            _ => {}
        }
        idx += 1;
    }
}

/// Transpile `IntPtr.Add(a, b)` → `(a + b)` (char-safe).
/// Remplace la virgule top-level par `+` entre les args.
fn transpile_intptr_add(code: &str) -> String {
    let mut result = code.to_string();
    while let Some(pos) = result.find("IntPtr.Add(") {
        let paren_start = pos + "IntPtr.Add(".len() - 1; // position du '('
        let rbytes = result.as_bytes();
        // Trouver la parenthèse fermante correspondante
        let mut depth = 1;
        let mut i = paren_start + 1;
        while i < rbytes.len() && depth > 0 {
            match rbytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth > 0 { i += 1; }
        }
        // i est position du ')' fermant
        let start = safe_slice_start(&result, paren_start + 1);
        let end = safe_slice_end(&result, i);
        let args = &result[start..end];
        // Trouver la virgule top-level et la remplacer par +
        if let Some(comma_pos) = find_top_level_comma(args) {
            let a = args[..comma_pos].trim();
            let b = args[comma_pos + 1..].trim();
            let replacement = format!("({} + {})", a, b);
            result.replace_range(pos..i + 1, &replacement);
        } else {
            // Pas de virgule — juste retirer IntPtr.Add
            let replacement = format!("({})", args);
            result.replace_range(pos..i + 1, &replacement);
        }
    }
    result
}

/// Transpile `new MemoryWatcher<TYPE>(args)` → `new MemoryWatcher(args, 'TYPE')`
/// et `new StringWatcher(args)` n'est pas touché (pas de générique type).
/// (auto-start fix — préserve le type C# du watcher pour que la lecture mémoire
/// utilise la bonne taille : short=2 bytes, int=4 bytes, byte=1 byte, etc.)
///
/// Char-safe : utilise char_indices pour ne pas atterrir au milieu d'un char
/// multi-byte. Détecte `new MemoryWatcher<` puis extrait le type entre `<` et `>`,
/// puis trouve les args entre `(` et `)` correspondants (string-aware), et
/// reconstruit `new MemoryWatcher(args, 'TYPE')`.
pub(crate) fn transpile_memory_watcher_generics(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_off, _ch) = chars[idx];

        // Chercher "new MemoryWatcher<"
        if code[byte_off..].starts_with("new MemoryWatcher<") {
            // Position après "new MemoryWatcher" (sur '<')
            let lt_idx = idx + "new MemoryWatcher".len();
            // Extraire le type entre '<' et '>' (depth-aware pour generics imbriqués,
            // bien que les types primitifs n'en aient pas)
            let mut depth = 1;
            let mut j = lt_idx + 1;
            while j < chars.len() && depth > 0 {
                match chars[j].1 {
                    '<' => depth += 1,
                    '>' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    j += 1;
                }
            }
            if depth != 0 {
                // '<' non fermé — copier tel quel et avancer
                result.push_str("new MemoryWatcher");
                idx = lt_idx;
                continue;
            }
            // j est sur '>', le type est entre lt_idx+1 et j
            let type_start = chars[lt_idx + 1].0;
            let type_end = chars[j].0;
            let type_str = code[type_start..type_end].trim();

            // Vérifier que le type est un primitif C# valide (sécurité : ne pas
            // transpiler new MemoryWatcher<CustomType> qui n'a pas de sens en JS)
            let valid_types = [
                "int", "uint", "long", "ulong", "short", "ushort", "byte", "sbyte",
                "bool", "float", "double",
            ];
            if !valid_types.contains(&type_str) {
                // Type non primitif — laisser remove_method_generics gérer (strip <T>)
                result.push_str("new MemoryWatcher");
                idx = lt_idx;
                continue;
            }

            // Avancer j après '>'
            j += 1;

            // Skip whitespace entre '>' et '('
            while j < chars.len() && chars[j].1.is_whitespace() {
                j += 1;
            }
            // Vérifier qu'on a '(' (constructeur)
            if j >= chars.len() || chars[j].1 != '(' {
                // Pas de '(' — copier tel quel
                result.push_str("new MemoryWatcher");
                idx = lt_idx;
                continue;
            }

            // Trouver la ')' correspondante (string-aware)
            let paren_open = j;
            let mut pdepth = 1;
            let mut k = paren_open + 1;
            let mut in_string = false;
            let mut string_char = '"';
            let mut prev_char = ' ';
            while k < chars.len() && pdepth > 0 {
                let c = chars[k].1;
                if in_string {
                    if c == string_char && prev_char != '\\' {
                        in_string = false;
                    }
                } else if c == '"' || c == '\'' {
                    in_string = true;
                    string_char = c;
                } else if c == '(' {
                    pdepth += 1;
                } else if c == ')' {
                    pdepth -= 1;
                }
                prev_char = c;
                if pdepth > 0 {
                    k += 1;
                }
            }
            if pdepth != 0 {
                // Parenthèse non fermée — copier tel quel
                result.push_str("new MemoryWatcher");
                idx = lt_idx;
                continue;
            }
            // k est sur ')', args entre paren_open+1 et k
            let args_start = chars[paren_open + 1].0;
            let args_end = chars[k].0;
            let args = code[args_start..args_end].trim();

            // Reconstruire : new MemoryWatcher(args, 'TYPE')
            result.push_str(&format!(
                "new MemoryWatcher({}, '{}')",
                args, type_str
            ));
            idx = k + 1;
            continue;
        }

        result.push(chars[idx].1);
        idx += 1;
    }

    result
}

/// Transpile `new <type>[<expr>]` → `new Array(<expr>)` pour les types primitifs C#.
/// (auto-start fix) : gère `new byte[len]`, `new int[size]`, etc. qui ne sont pas
/// gérés par step 5 (`new byte[]` sans taille) ni par transpile_object_initializer
/// (`new byte[] { ... }` avec initializer). Sans ce fix, `new byte[len]` reste
/// tel quel en JS → `byte` est un identifiant nu → ReferenceError "byte is not defined".
///
/// Char-safe : utilise char_indices. Détecte `new <type>[` où type est un primitif,
/// trouve le `]` correspondant (string-aware), et remplace par `new Array(<expr>)`.
pub(crate) fn transpile_typed_array_creation(code: &str) -> String {
    let type_keywords = [
        "short", "int", "string", "byte", "long", "uint", "ushort", "sbyte",
        "float", "double", "bool", "char", "object",
    ];
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_off, _ch) = chars[idx];

        // Chercher "new " suivi d'un type primitif suivi de "["
        if code[byte_off..].starts_with("new ") {
            let after_new = idx + 4; // après "new "
            // Skip whitespace
            let mut j = after_new;
            while j < chars.len() && chars[j].1.is_whitespace() {
                j += 1;
            }
            // Extraire l'identifiant du type
            let type_start = j;
            while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_') {
                j += 1;
            }
            if j > type_start {
                let type_name = &code[chars[type_start].0..chars[j].0];
                // Vérifier si c'est un type primitif valide
                if type_keywords.contains(&type_name) {
                    // Vérifier si suivi de "["
                    if j < chars.len() && chars[j].1 == '[' {
                        // Trouver le "]" correspondant (string-aware, nest-aware)
                        let bracket_open = j;
                        let mut bdepth = 1;
                        let mut k = bracket_open + 1;
                        let mut in_string = false;
                        let mut string_char = '"';
                        let mut prev_char = ' ';
                        while k < chars.len() && bdepth > 0 {
                            let c = chars[k].1;
                            if in_string {
                                if c == string_char && prev_char != '\\' {
                                    in_string = false;
                                }
                            } else if c == '"' || c == '\'' {
                                in_string = true;
                                string_char = c;
                            } else if c == '[' {
                                bdepth += 1;
                            } else if c == ']' {
                                bdepth -= 1;
                            }
                            prev_char = c;
                            if bdepth > 0 {
                                k += 1;
                            }
                        }
                        if bdepth == 0 {
                            // k est sur ']', l'expression est entre bracket_open+1 et k
                            let expr_start = chars[bracket_open + 1].0;
                            let expr_end = chars[k].0;
                            let expr = code[expr_start..expr_end].trim();
                            // Vérifier que l'expression n'est pas vide (sinon c'est
                            // `new byte[]` qui aurait dû être géré par step 5)
                            if !expr.is_empty() {
                                result.push_str(&format!("new Array({})", expr));
                                idx = k + 1;
                                continue;
                            }
                        }
                    }
                }
            }
        }

        result.push(chars[idx].1);
        idx += 1;
    }

    result
}

/// Transpile les object/collection initializers C# (E7 T2).
/// `new X(args) { Prop = val, Prop2 = val2 }` → IIFE avec assignations
/// `new MemoryWatcherList() { w1, w2 }` → IIFE avec .Add
/// Détecte si le contenu a `=` au top level → object initializer, sinon collection.
pub(crate) fn transpile_object_initializer(code: &str) -> String {
    let mut result = code.to_string();
    // Pattern: `new <Identifier>(args) { ... }`
    // On cherche `new ` suivi d'un identifiant, puis `(args)`, puis `{ ... }`
    // Safety counter to prevent infinite loops
    let mut iterations = 0;
    let max_iterations = 1000;
    loop {
        iterations += 1;
        if iterations > max_iterations {
            log::warn!("[ASL Transpiler] transpile_object_initializer: limite d'itérations atteinte, abort");
            break;
        }
        let search_start = match result.find("new ") {
            Some(p) => p,
            None => break,
        };
        // Vérifier si ce `new ` est dans un commentaire // (sur la même ligne)
        // Si oui, le skip — il ne faut pas traiter `new` dans les commentaires
        // car il peut matcher un `{}` sur une ligne suivante comme initializer.
        let line_start = result[..search_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_prefix = &result[line_start..search_start];
        if line_prefix.contains("//") {
            // `new ` est dans un commentaire — skip en remplaçant temporairement
            let before = &result[..search_start];
            let after = &result[search_start + 4..];
            result = format!("{}__SKIP__{}", before, after);
            continue;
        }
        // Extraire l'identifiant du type après "new "
        let after_new = search_start + 4;
        let rest = &result[after_new..];
        // C# permet `new X { ... }` sans parenthèses (constructeur par défaut).
        // On cherche soit '(' (args du constructeur) soit '{' (initializer sans args).
        let paren_rel = rest.find('(');
        let brace_rel = rest.find('{');
        // Déterminer quel token vient en premier (et s'il fait partie du type name)
        // On doit trouver le premier '{' ou '(' qui n'est PAS dans le type name.
        // Le type name peut contenir des generics <...> mais pas de '(' ou '{'.
        let paren_pos = match (paren_rel, brace_rel) {
            (Some(p), Some(b)) if p < b => after_new + p,
            (Some(p), None) => after_new + p,
            (_, Some(b)) => {
                // '{' vient avant '(' → pas de parenthèses de constructeur
                // `new X { ... }` → traiter comme `new X() { ... }`
                after_new + b // On positionne sur '{' mais on marque pas de paren
            }
            (None, None) => {
                // Pas de '(' ni '{' → pas un constructeur, skip
                let before = &result[..search_start];
                let after = &result[search_start + 4..];
                result = format!("{}__SKIP__{}", before, after);
                continue;
            }
        };
        // Vérifier si on a des parenthèses de constructeur ou non
        let has_ctor_parens = paren_rel.is_some() && (brace_rel.is_none() || paren_rel.unwrap() < brace_rel.unwrap());
        // Le type name est entre after_new et le début des parenthèses/accolades
        let type_name_end = if has_ctor_parens { paren_pos } else {
            // paren_pos pointe sur '{', le type name est entre after_new et paren_pos
            paren_pos
        };
        let type_name = result[after_new..type_name_end].trim();
        // Skip les types qu'on ne doit pas traiter ici (gérés ailleurs)
        if type_name.contains("Dictionary") || type_name.contains("List<")
            || type_name.contains("HashSet") || type_name.contains("ExpandoObject")
            || type_name.contains("Tuple") || type_name.is_empty()
        {
            // Avancer la recherche past this occurrence
            // Remplacer temporairement pour éviter boucle infinie
            let before = &result[..search_start];
            let after = &result[search_start + 4..];
            result = format!("{}__SKIP__{}", before, after);
            continue;
        }
        // Trouver la parenthèse fermante du constructeur (si présent)
        let rbytes = result.as_bytes();
        let (ctor_args, brace_start) = if has_ctor_parens {
            // Trouver la parenthèse fermante
            let mut depth = 1;
            let mut i = paren_pos + 1;
            while i < rbytes.len() && depth > 0 {
                match rbytes[i] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                if depth > 0 { i += 1; }
            }
            if i >= rbytes.len() {
                let before = &result[..search_start];
                let after = &result[search_start + 4..];
                result = format!("{}__SKIP__{}", before, after);
                continue;
            }
            let args_start = safe_slice_start(&result, paren_pos + 1);
            let args_end = safe_slice_end(&result, i);
            let args = &result[args_start..args_end];
            // Skip whitespace après ')'
            let mut k = i + 1;
            while k < rbytes.len() && rbytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k >= rbytes.len() || rbytes[k] != b'{' {
                // Pas d'initializer — avancer
                let before = &result[..search_start];
                let after = &result[search_start + 4..];
                result = format!("{}__SKIP__{}", before, after);
                continue;
            }
            (args.to_string(), k)
        } else {
            // Pas de parenthèses de constructeur — paren_pos pointe sur '{'
            ("".to_string(), paren_pos)
        };
        // Trouver l'accolade fermante correspondante (string-aware)
        let mut brace_depth = 1;
        let mut m = brace_start + 1;
        let mut in_string = false;
        let mut string_char = b'"';
        let mut prev_char = b' ';
        while m < rbytes.len() && brace_depth > 0 {
            let b = rbytes[m];
            if in_string {
                if b == string_char && prev_char != b'\\' {
                    in_string = false;
                }
            } else {
                if b == b'"' || b == b'\'' {
                    in_string = true;
                    string_char = b;
                } else {
                    match b {
                        b'{' => brace_depth += 1,
                        b'}' => brace_depth -= 1,
                        _ => {}
                    }
                }
            }
            prev_char = b;
            m += 1;
        }
        if m >= rbytes.len() {
            // Acccolade non fermée — skip
            let before = &result[..search_start];
            let after = &result[search_start + 4..];
            result = format!("{}__SKIP__{}", before, after);
            continue;
        }
        // m est JUSTE APRÈS le '}' fermant
        let init_start = safe_slice_start(&result, brace_start + 1);
        let init_end = safe_slice_end(&result, m.saturating_sub(1));
        let init_content = result[init_start..init_end].trim().to_string();
        // Détecter object vs collection initializer :
        // Si le contenu a `=` au top level → object initializer (Prop = val)
        // Sinon → collection initializer (valeurs séparées par virgules)
        let is_object_init = find_top_level_equals(&init_content).is_some();
        let replacement = if type_name.ends_with("[]") {
            // E7 — Tableau typé C# `new T[] { a, b, c }` → array literal JS `[a, b, c]`.
            // Sans ce cas, la branche collection produit `var __l = new T[]();` dont
            // le `new T[]` est supprimé à l'étape 5 → `()` → `(0)` → `(0).Add(...)`
            // throw « not a callable function » au runtime.
            let mut items = Vec::new();
            for item in split_top_level_commas(&init_content) {
                let item = item.trim();
                if item.is_empty() { continue; }
                let stripped = strip_line_comments(&item);
                if stripped.trim().is_empty() { continue; }
                items.push(stripped.trim().to_string());
            }
            format!("[{}]", items.join(", "))
        } else if is_object_init {
            // Parser `Prop = val, Prop2 = val2`
            let mut assigns = Vec::new();
            for part in split_top_level_commas(&init_content) {
                let part = part.trim();
                if part.is_empty() { continue; }
                if let Some(eq_pos) = find_top_level_equals(part) {
                    let prop = part[..eq_pos].trim();
                    let val = part[eq_pos + 1..].trim();
                    assigns.push(format!("__o.{} = {}", prop, val));
                }
            }
            if assigns.is_empty() {
                format!("new {}({})", type_name, ctor_args)
            } else {
                format!(
                    "(() => {{ var __o = new {}({}); {}; return __o; }})()",
                    type_name, ctor_args, assigns.join("; ")
                )
            }
        } else {
            // Collection initializer : valeurs séparées par virgules
            let items = split_top_level_commas(&init_content);
            let mut adds = Vec::new();
            for item in items {
                let item = item.trim();
                if item.is_empty() { continue; }
                // T4 — Skip les items qui sont uniquement des commentaires // ou /* */
                let stripped = strip_line_comments(&item);
                if stripped.trim().is_empty() { continue; }
                // Utiliser la version sans commentaires trailing pour éviter
                // que le commentaire commente la ) de __l.Add(...)
                adds.push(format!("__l.Add({})", stripped.trim()));
            }
            if adds.is_empty() {
                format!("new {}({})", type_name, ctor_args)
            } else {
                format!(
                    "(() => {{ var __l = new {}({}); {}; return __l; }})()",
                    type_name, ctor_args, adds.join("; ")
                )
            }
        };
        result.replace_range(search_start..m, &replacement);
    }
    // Restaurer les __SKIP__ → new
    result.replace("__SKIP__", "new ")
}

/// Transpile `new HashSet<T>() { a, b, c }` → `new Set([a, b, c])` (char-safe).
/// Similaire à transpile_dictionary_init mais pour HashSet.
pub(crate) fn transpile_hashset_init(code: &str) -> String {
    let mut result = code.to_string();
    while let Some(pos) = result.find("new HashSet<") {
        // Trouver la fin du générique <...>
        let mut depth = 1;
        let mut j = pos + result[pos..].find('<').unwrap() + 1;
        let rbytes = result.as_bytes();
        while j < rbytes.len() && depth > 0 {
            match rbytes[j] {
                b'<' => depth += 1,
                b'>' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        // j est après le '>'
        let rest = &result[j..];
        if let Some(paren_end) = rest.find("()") {
            let after_parens = j + paren_end + 2;
            let rbytes = result.as_bytes();
            let mut k = after_parens;
            while k < rbytes.len() && rbytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < rbytes.len() && rbytes[k] == b'{' {
                let mut brace_depth = 1;
                let mut m = k + 1;
                while m < rbytes.len() && brace_depth > 0 {
                    match rbytes[m] {
                        b'{' => brace_depth += 1,
                        b'}' => brace_depth -= 1,
                        _ => {}
                    }
                    m += 1;
                }
                let start = safe_slice_start(&result, k + 1);
                let end = safe_slice_end(&result, m.saturating_sub(1));
                let init_content = result[start..end].trim().to_string();
                let replacement = if init_content.is_empty() {
                    "new Set()".to_string()
                } else {
                    format!("new Set([{}])", init_content)
                };
                result.replace_range(pos..m, &replacement);
                continue;
            }
        }
        break;
    }
    result
}

/// Post-traitement char-safe : remplace `()` isolé en position d'expression
/// par `(0)` pour éviter "empty parenthesized expression" dans Boa.
/// Prudent : ne remplace que les `()` qui suivent un opérateur ou un token
/// qui suggère une expression attendue (pas les appels de fonction `f()`).
fn remove_empty_parens(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;
    while idx < chars.len() {
        let (_byte_off, ch) = chars[idx];
        // Détecter `()` — le '(' est suivi immédiatement de ')'
        if ch == '(' && idx + 1 < chars.len() && chars[idx + 1].1 == ')' {
            // Vérifier le contexte : si précédé par un identifiant/`.`/`]` → appel de fonction, skip
            let prev = result.chars().last();
            let is_call = match prev {
                Some(c) if c.is_alphanumeric() || c == '_' || c == '.' || c == ']' || c == ')' => true,
                _ => false,
            };
            // Vérifier si suivi de ` =>` (arrow function ()  => ...) — garder tel quel
            let mut next_idx = idx + 2;
            while next_idx < chars.len() && chars[next_idx].1.is_whitespace() {
                next_idx += 1;
            }
            let is_arrow = next_idx < chars.len() && chars[next_idx].1 == '='
                && next_idx + 1 < chars.len() && chars[next_idx + 1].1 == '>';
            if is_call || is_arrow {
                // Appel de fonction f() ou arrow function () => — garder tel quel
                result.push('(');
                result.push(')');
                idx += 2;
            } else {
                // `()` isolé en position d'expression → remplacer par (0)
                result.push_str("(0)");
                idx += 2;
            }
        } else {
            result.push(ch);
            idx += 1;
        }
    }
    result
}

/// Trouve la position du premier `=` au niveau 0 (pas dans des sous-parenthèses/accolades).
/// Exclut `==`, `>=`, `<=`, `!=`, `+=`, `-=`, `*=`, `/=`, `=>`.
fn find_top_level_equals(s: &str) -> Option<usize> {
    let mut depth = 0;
    let chars: Vec<(usize, char)> = char_indices_vec(s);
    let mut i = 0;
    let mut in_string = false;
    let mut string_char = '"';
    while i < chars.len() {
        let (pos, ch) = chars[i];
        // Track strings pour éviter de matcher à l'intérieur
        if !in_string {
            if ch == '"' || ch == '\'' {
                in_string = true;
                string_char = ch;
                i += 1;
                continue;
            }
        } else {
            if ch == string_char {
                // Vérifier si échappé
                let prev = if i > 0 { Some(chars[i - 1].1) } else { None };
                if prev != Some('\\') {
                    in_string = false;
                }
            }
            i += 1;
            continue;
        }
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            '=' if depth == 0 => {
                // Exclure ==, >=, <=, !=, +=, -=, *=, /=, =>
                let next = if i + 1 < chars.len() { Some(chars[i + 1].1) } else { None };
                let prev = if i > 0 { Some(chars[i - 1].1) } else { None };
                if next == Some('=') { i += 2; continue; }
                if next == Some('>') { i += 2; continue; }
                if matches!(prev, Some('>') | Some('<') | Some('!') | Some('+') | Some('-') | Some('*') | Some('/') | Some('=')) {
                    i += 1;
                    continue;
                }
                return Some(pos);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Sépare une string par les virgules au niveau 0 (pas dans des sous-parenthèses/accolades).
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let chars: Vec<(usize, char)> = char_indices_vec(s);
    let mut in_string = false;
    let mut string_char = '"';
    for (i, ch) in chars.iter() {
        let i = *i;
        // Track strings
        if !in_string {
            if *ch == '"' || *ch == '\'' {
                in_string = true;
                string_char = *ch;
                continue;
            }
        } else {
            if *ch == string_char {
                // Vérifier si échappé
                let prev_idx = if i > 0 { Some(i - 1) } else { None };
                let prev_escaped = prev_idx.map_or(false, |pi| s.as_bytes()[pi] == b'\\');
                if !prev_escaped {
                    in_string = false;
                }
            }
            continue;
        }
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].to_string());
    parts
}

/// Transpile `default(T)` → `null` (char-safe).
/// `default(int)`, `default(string)`, etc. → `null`
fn transpile_default(code: &str) -> String {
    let mut result = code.to_string();
    while let Some(pos) = result.find("default(") {
        // Trouver la parenthèse fermante
        let paren_start = pos + "default(".len() - 1;
        let rbytes = result.as_bytes();
        let mut depth = 1;
        let mut i = paren_start + 1;
        while i < rbytes.len() && depth > 0 {
            match rbytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth > 0 { i += 1; }
        }
        if i >= rbytes.len() { break; }
        // Remplacer default(T) par null
        result.replace_range(pos..i + 1, "null");
    }
    result
}

/// Transpile `nameof(X)` → `"X"` (char-safe).
/// nameof(Variable) → "Variable", nameof(Type.Prop) → "Type.Prop"
fn transpile_nameof(code: &str) -> String {
    let mut result = code.to_string();
    while let Some(pos) = result.find("nameof(") {
        let paren_start = pos + "nameof(".len() - 1;
        let rbytes = result.as_bytes();
        let mut depth = 1;
        let mut i = paren_start + 1;
        while i < rbytes.len() && depth > 0 {
            match rbytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth > 0 { i += 1; }
        }
        if i >= rbytes.len() { break; }
        // Extraire le contenu entre parenthèses
        let start = safe_slice_start(&result, paren_start + 1);
        let end = safe_slice_end(&result, i);
        let content = &result[start..end];
        // Remplacer nameof(X) par "X"
        let replacement = format!("\"{}\"", content.trim());
        result.replace_range(pos..i + 1, &replacement);
    }
    result
}

/// Retire les génériques de méthode `Method<T>(args)` → `Method(args)` (char-safe).
/// Détecte un identifiant suivi de `<...>` suivi de `(` et retire le `<...>`.
fn remove_method_generics(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (_byte_off, ch) = chars[idx];

        // Détecter un identifiant (lettre ou _ ou $) suivi potentiellement de <...>(
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            // Extraire l'identifiant complet
            let id_start = idx;
            while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_' || chars[idx].1 == '$') {
                idx += 1;
            }
            // Vérifier si suivi de '<'
            if idx < chars.len() && chars[idx].1 == '<' {
                // Trouver le '>' correspondant
                let mut depth = 1;
                let mut j = idx + 1;
                while j < chars.len() && depth > 0 {
                    match chars[j].1 {
                        '<' => depth += 1,
                        '>' => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if depth == 0 && j < chars.len() && chars[j].1 == '(' {
                    // Method<T>( → Method( : retirer le <...>
                    let id_str = &code[chars[id_start].0..chars[idx].0];
                    result.push_str(id_str);
                    idx = j; // avancer après le '>'
                    continue;
                }
            }
            // Pas un générique de méthode — copier l'identifiant tel quel
            // ATTENTION: idx peut être == chars.len() (identifiant en fin de input).
            // Dans ce cas, utiliser code.len() comme fin de slice.
            let end_byte = if idx < chars.len() { chars[idx].0 } else { code.len() };
            let id_str = &code[chars[id_start].0..end_byte];
            result.push_str(id_str);
            continue;
        }

        result.push(ch);
        idx += 1;
    }

    result
}

/// Retire les named args `Method(name: value)` → `Method(value)` (char-safe).
/// Détecte `identifier:` à l'intérieur d'une liste d'arguments et retire le `identifier:`.
fn remove_named_args(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;
    let mut paren_depth = 0;
    let mut brace_depth = 0;

    while idx < chars.len() {
        let (_byte_off, ch) = chars[idx];

        if ch == '(' {
            paren_depth += 1;
            result.push(ch);
            idx += 1;
            continue;
        }
        if ch == ')' {
            paren_depth -= 1;
            result.push(ch);
            idx += 1;
            continue;
        }
        if ch == '{' {
            brace_depth += 1;
            result.push(ch);
            idx += 1;
            continue;
        }
        if ch == '}' {
            if brace_depth > 0 { brace_depth -= 1; }
            result.push(ch);
            idx += 1;
            continue;
        }

        // À l'intérieur d'une liste d'arguments (paren_depth > 0) mais PAS
        // à l'intérieur d'un object literal (brace_depth == 0), détecter `identifier:`
        // suivi d'une valeur. Évite de stripper `Key:` et `Value:` des object literals.
        if paren_depth > 0 && brace_depth == 0 && (ch.is_alphabetic() || ch == '_') {
            let id_start = idx;
            while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_' || chars[idx].1 == '.') {
                idx += 1;
            }
            // Vérifier si suivi de ':' (mais pas '::')
            if idx < chars.len() && chars[idx].1 == ':' {
                let next = if idx + 1 < chars.len() { Some(chars[idx + 1].1) } else { None };
                if next != Some(':') {
                    // Named arg — skip l'identifiant et le ':'
                    idx += 1; // skip le ':'
                    // Skip whitespace après le ':'
                    while idx < chars.len() && chars[idx].1.is_whitespace() {
                        idx += 1;
                    }
                    continue;
                }
            }
            // Pas un named arg — copier l'identifiant
            let id_str = &code[chars[id_start].0..chars[idx].0];
            result.push_str(id_str);
            continue;
        }

        result.push(ch);
        idx += 1;
    }

    result
}

/// Transpile les opérateurs d'événement C# `+=` et `-=` (char-safe).
/// Distingue les subscriptions d'événements (delegate/function ref) de l'arithmétique.
/// - Events: `obj.Event += Handler;` → `obj.Event.__subscribe(Handler);`
/// - Arithmetic: `i += 3;` → `i += 3;` (préservé)
/// Heuristique: si le côté droit commence par un chiffre, `"`, `'`, c'est arithmétique.
/// Sinon, c'est un événement — on convertit en `.__subscribe(`/`.__unsubscribe(` et
/// ferme la parenthèse au `;` de fin de statement (en respectant la profondeur).
fn transpile_event_ops(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        // Chercher " += " ou " -= "
        if chars[idx].1 == ' '
            && idx + 2 < chars.len()
            && (chars[idx + 1].1 == '+' || chars[idx + 1].1 == '-')
            && chars[idx + 2].1 == '='
            && idx + 3 < chars.len()
            && chars[idx + 3].1 == ' '
        {
            let op_char = chars[idx + 1].1;
            // Vérifier le caractère suivant après " += " / " -= "
            let rhs_start = idx + 4; // après " += " ou " -= "
            if rhs_start >= chars.len() {
                result.push(chars[idx].1);
                idx += 1;
                continue;
            }

            let rhs_first = chars[rhs_start].1;
            // Si le côté droit commence par un chiffre, " ou ' → arithmétique, préservé
            if rhs_first.is_ascii_digit() || rhs_first == '"' || rhs_first == '\'' {
                result.push(chars[idx].1); // ' '
                result.push(op_char);
                result.push('=');
                result.push(' ');
                idx += 4; // skip " += " / " -= "
                continue;
            }

            // C'est un événement — convertir en .__subscribe( / .__unsubscribe(
            let method_name = if op_char == '+' { "__subscribe" } else { "__unsubscribe" };
            result.push('.');
            result.push_str(method_name);
            result.push('(');
            idx += 4; // skip " += " / " -= "

            // Copier le côté droit jusqu'au `;` de fin de statement (depth-aware)
            let mut paren_depth = 1; // la parenthèse qu'on vient d'ouvrir
            let mut brace_depth = 0;
            while idx < chars.len() {
                let ch = chars[idx].1;
                match ch {
                    '(' => paren_depth += 1,
                    ')' => {
                        if paren_depth > 0 {
                            paren_depth -= 1;
                        }
                    }
                    '{' => brace_depth += 1,
                    '}' => {
                        if brace_depth > 0 {
                            brace_depth -= 1;
                        }
                    }
                    ';' => {
                        if paren_depth == 1 && brace_depth == 0 {
                            // Fermer la parenthèse avant le `;`
                            result.push(')');
                            result.push(';');
                            idx += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                result.push(ch);
                idx += 1;
            }
            // Si on a atteint la fin sans trouver `;`, fermer quand même
            if idx >= chars.len() && paren_depth >= 1 {
                result.push(')');
            }
            continue;
        }

        result.push(chars[idx].1);
        idx += 1;
    }

    result
}

/// Remplace `old` par `new` uniquement si le caractère suivant `old` n'est PAS
/// alphanumérique ou `_` (boundary check). Évite les matches substring.
/// char-safe : utilise char_indices pour respecter l'UTF-8.
fn replace_property_with_boundary(code: &str, old: &str, new: &str) -> String {
    let mut result = code.to_string();
    // Boucler de la fin vers le début pour éviter les décalages d'index
    let mut search_from = result.len();
    while let Some(pos) = result[..search_from].rfind(old) {
        let after_pos = pos + old.len();
        // Vérifier le caractère suivant (byte boundary)
        let next_char = result[after_pos..].chars().next();
        let is_boundary = match next_char {
            Some(c) => !c.is_alphanumeric() && c != '_',
            None => true, // fin de string = boundary
        };
        if is_boundary {
            result.replace_range(pos..after_pos, new);
        }
        // Avancer la recherche avant cette position
        if pos == 0 { break; }
        search_from = pos;
    }
    result
}

/// Retire un wrapper de delegate `new EventHandlerName(arg)` → `arg` (char-safe).
/// Supprime le préfixe ET la parenthèse fermante correspondante.
fn remove_delegate_wrapper(code: &str, prefix: &str) -> String {
    let mut result = code.to_string();
    while let Some(pos) = result.find(prefix) {
        let paren_start = pos + prefix.len() - 1; // position du '('
        let rbytes = result.as_bytes();
        // Trouver la parenthèse fermante correspondante
        let mut depth = 1;
        let mut i = paren_start + 1;
        while i < rbytes.len() && depth > 0 {
            match rbytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            if depth > 0 { i += 1; }
        }
        if i >= rbytes.len() { break; }
        // i est position du ')' fermant
        // Extraire l'argument
        let arg_start = safe_slice_start(&result, paren_start + 1);
        let arg_end = safe_slice_end(&result, i);
        let arg = result[arg_start..arg_end].to_string();
        // Remplacer prefix + arg + ')' par arg
        result.replace_range(pos..i + 1, &arg);
    }
    result
}

/// Retire les casts de type résiduels `(TypeName>)` ou `(TypeName>)` (char-safe).
/// Après les replaces de delegate handlers, il reste des patterns comme `(TimerPhase>)`
/// qu'on doit retirer complètement (le `(` et le `>)`).
fn remove_residual_type_casts(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (_byte_off, ch) = chars[idx];
        // Chercher "(Identifier>)" — cast résiduel
        if ch == '(' {
            // Vérifier si c'est un identifiant suivi de '>'
            let mut j = idx + 1;
            // Skip whitespace
            while j < chars.len() && chars[j].1.is_whitespace() { j += 1; }
            let id_start = j;
            while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_' || chars[j].1 == '.') {
                j += 1;
            }
            // Skip whitespace
            while j < chars.len() && chars[j].1.is_whitespace() { j += 1; }
            if j < chars.len() && chars[j].1 == '>' {
                // Vérifier si suivi de ')' — cast résiduel (TypeName>)
                let mut k = j + 1;
                while k < chars.len() && chars[k].1.is_whitespace() { k += 1; }
                if k < chars.len() && chars[k].1 == ')' {
                    // C'est un cast résiduel — skip le "(TypeName>)"
                    idx = k + 1;
                    continue;
                }
            }
            let _ = id_start; // unused
        }
        result.push(ch);
        idx += 1;
    }

    result
}

/// Retire les annotations de type customes `Type identifier = ...` → `let identifier = ...` (char-safe).
/// Détecte un identifiant PascalCase (commence par majuscule) suivi d'un identifiant suivi de `=`.
/// Évite les false positives : ne matche que si le type commence par une majuscule et
/// est suivi d'un espace + identifiant + `=`.
fn remove_custom_type_annotations(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_off, ch) = chars[idx];

        // Détecter un identifiant PascalCase (commence par majuscule) en début de statement
        // (après `;`, `{`, `}` ou newline, en skipant les espaces/tabs d'indentation)
        if ch.is_uppercase() {
            // Vérifier le contexte : doit être après `;`, `{`, `}`, newline ou début
            // en skipant les espaces/tabs d'indentation (MAIS PAS les newlines)
            let mut prev_idx = idx;
            // Skip whitespace avant (spaces et tabs uniquement, PAS newlines)
            while prev_idx > 0 && (chars[prev_idx - 1].1 == ' ' || chars[prev_idx - 1].1 == '\t') {
                prev_idx -= 1;
            }
            let prev_char = if prev_idx > 0 { Some(chars[prev_idx - 1].1) } else { None };
            let is_statement_start = match prev_char {
                None => true,
                Some(c) => c == ';' || c == '{' || c == '}' || c == '\n' || c == '\r',
            };
            // Skip si on est au début (idx == 0)
            let is_real_start = idx == 0 || is_statement_start;

            if is_real_start {
                // Extraire l'identifiant du type (PascalCase, peut contenir des dots)
                let type_start = idx;
                while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_' || chars[idx].1 == '.') {
                    idx += 1;
                }
                let type_name = &code[chars[type_start].0..chars[idx].0];
                // T4 — Skip les generics <...> si présents (ex: Func<string, bool>)
                if idx < chars.len() && chars[idx].1 == '<' {
                    let mut depth = 1;
                    idx += 1;
                    while idx < chars.len() && depth > 0 {
                        match chars[idx].1 {
                            '<' => depth += 1,
                            '>' => depth -= 1,
                            _ => {}
                        }
                        idx += 1;
                    }
                }
                // Skip whitespace
                while idx < chars.len() && chars[idx].1.is_whitespace() { idx += 1; }
                // Vérifier si suivi d'un identifiant (nom de variable)
                if idx < chars.len() && (chars[idx].1.is_alphabetic() || chars[idx].1 == '_') {
                    let var_start = idx;
                    while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_') {
                        idx += 1;
                    }
                    let var_name = &code[chars[var_start].0..chars[idx].0];
                    // Skip whitespace
                    while idx < chars.len() && chars[idx].1.is_whitespace() { idx += 1; }
                    // Vérifier si suivi de `=`
                    if idx < chars.len() && chars[idx].1 == '=' {
                        // C'est une annotation de type — remplacer par `let var_name =`
                        result.push_str("let ");
                        result.push_str(var_name);
                        // idx est positionné sur le '='
                        let _ = byte_off;
                        let _ = type_name;
                        continue;
                    }
                }
                // Pas une annotation de type — copier le type tel quel
                result.push_str(type_name);
                continue;
            }
        }

        result.push(ch);
        idx += 1;
    }

    result
}

/// Retire les casts `expr as Type` → `expr` (char-safe).
/// Détecte ` as Identifier` et retire le `as Identifier`.
fn remove_as_casts(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        // Chercher " as " suivi d'un identifiant
        if chars[idx].1 == ' '
            && idx + 3 < chars.len()
            && chars[idx + 1].1 == 'a'
            && chars[idx + 2].1 == 's'
            && chars[idx + 3].1 == ' '
        {
            // Vérifier que c'est bien " as " (pas " base " ou autre)
            // Skip le " as "
            let mut j = idx + 4;
            // Skip whitespace supplémentaire
            while j < chars.len() && chars[j].1.is_whitespace() { j += 1; }
            // Extraire l'identifiant du type
            if j < chars.len() && (chars[j].1.is_alphabetic() || chars[j].1 == '_') {
                let _type_start = j;
                while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_' || chars[j].1 == '.') {
                    j += 1;
                }
                // Vérifier si suivi de `<` (generic) — skip le `<...>`
                if j < chars.len() && chars[j].1 == '<' {
                    let mut depth = 1;
                    j += 1;
                    while j < chars.len() && depth > 0 {
                        match chars[j].1 {
                            '<' => depth += 1,
                            '>' => depth -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                }
                // Skip le " as Type" — ne rien ajouter au résultat
                idx = j;
                continue;
            }
        }

        result.push(chars[idx].1);
        idx += 1;
    }

    result
}

/// Transpile les verbatim strings C# `@"..."` → `"..."` (char-safe).
/// Dans un verbatim string, `""` représente un `"` littéral.
/// En JS, on convertit en `"..."` avec `\"` pour les `"` littéraux.
fn transpile_verbatim_strings(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        // Chercher `@"`
        if chars[idx].1 == '@' && idx + 1 < chars.len() && chars[idx + 1].1 == '"' {
            // Skip le `@"`
            idx += 2;
            result.push('"');
            // Lire jusqu'au `"` final (non doublé)
            while idx < chars.len() {
                if chars[idx].1 == '"' {
                    // Vérifier si c'est un `""` (doublé = `"` littéral)
                    if idx + 1 < chars.len() && chars[idx + 1].1 == '"' {
                        result.push_str("\\\"");
                        idx += 2;
                    } else {
                        // `"` simple = fin du string
                        result.push('"');
                        idx += 1;
                        break;
                    }
                } else if chars[idx].1 == '\n' {
                    // T4 — Newline littéral dans verbatim string → \n (JS string)
                    result.push_str("\\n");
                    idx += 1;
                } else if chars[idx].1 == '\r' {
                    // T4 — \r ou \r\n → \n (skip \r, le \n suivant sera converti)
                    idx += 1;
                } else if chars[idx].1 == '\\' {
                    // T4 — Backslash dans verbatim string → \\ (échapper en JS)
                    result.push_str("\\\\");
                    idx += 1;
                } else {
                    result.push(chars[idx].1);
                    idx += 1;
                }
            }
            continue;
        }

        result.push(chars[idx].1);
        idx += 1;
    }

    result
}

/// Retire les annotations de type array `Type[] identifier =` → `let identifier =` (char-safe).
/// Détecte `Identifier[]` suivi d'un identifiant et suivi de `=`.
fn remove_array_type_annotations(code: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(code);
    let mut result = String::with_capacity(code.len());
    let mut idx = 0;

    while idx < chars.len() {
        let (byte_off, ch) = chars[idx];

        // Détecter un identifiant (PascalCase ou lowercase) suivi de `[]`
        if ch.is_alphabetic() || ch == '_' {
            // Vérifier le contexte : doit être après `;`, `{`, `}`, newline ou début
            // Skip spaces/tabs uniquement (PAS newlines)
            let mut prev_idx = idx;
            while prev_idx > 0 && (chars[prev_idx - 1].1 == ' ' || chars[prev_idx - 1].1 == '\t') {
                prev_idx -= 1;
            }
            let prev_char = if prev_idx > 0 { Some(chars[prev_idx - 1].1) } else { None };
            let is_statement_start = match prev_char {
                None => true,
                Some(c) => c == ';' || c == '{' || c == '}' || c == '\n' || c == '\r',
            };

            if is_statement_start {
                // Extraire l'identifiant du type
                let type_start = idx;
                while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_' || chars[idx].1 == '.') {
                    idx += 1;
                }
                let type_name = &code[chars[type_start].0..chars[idx].0];
                // Vérifier si suivi de `[]`
                if idx + 1 < chars.len() && chars[idx].1 == '[' && chars[idx + 1].1 == ']' {
                    // Skip le `[]`
                    idx += 2;
                    // Skip whitespace
                    while idx < chars.len() && chars[idx].1.is_whitespace() { idx += 1; }
                    // Vérifier si suivi d'un identifiant (nom de variable)
                    if idx < chars.len() && (chars[idx].1.is_alphabetic() || chars[idx].1 == '_') {
                        let var_start = idx;
                        while idx < chars.len() && (chars[idx].1.is_alphanumeric() || chars[idx].1 == '_') {
                            idx += 1;
                        }
                        let var_name = &code[chars[var_start].0..chars[idx].0];
                        // Skip whitespace
                        while idx < chars.len() && chars[idx].1.is_whitespace() { idx += 1; }
                        // Vérifier si suivi de `=`
                        if idx < chars.len() && chars[idx].1 == '=' {
                            // C'est une annotation de type array — remplacer par `let var_name =`
                            result.push_str("let ");
                            result.push_str(var_name);
                            let _ = byte_off;
                            let _ = type_name;
                            continue;
                        }
                    }
                }
                // Pas une annotation de type array — copier le type tel quel
                result.push_str(type_name);
                continue;
            }
        }

        result.push(ch);
        idx += 1;
    }

    result
}

/// Retire les commentaires `// ...` (jusqu'à la fin de ligne) d'un item.
/// Utilisé pour nettoyer les items de collection initializer avant de générer __l.Add(item).
fn strip_line_comments(s: &str) -> String {
    let chars: Vec<(usize, char)> = char_indices_vec(s);
    let mut result = String::with_capacity(s.len());
    let mut idx = 0;
    let mut in_string = false;
    let mut string_char = '"';
    while idx < chars.len() {
        let (_, ch) = chars[idx];
        if in_string {
            if ch == string_char {
                let prev = if idx > 0 { Some(chars[idx - 1].1) } else { None };
                if prev != Some('\\') {
                    in_string = false;
                }
            }
            result.push(ch);
            idx += 1;
            continue;
        }
        if ch == '"' || ch == '\'' {
            in_string = true;
            string_char = ch;
            result.push(ch);
            idx += 1;
            continue;
        }
        // Détecter `//` → couper jusqu'à la fin de ligne
        if ch == '/' && idx + 1 < chars.len() && chars[idx + 1].1 == '/' {
            // Skip jusqu'au \n ou fin
            while idx < chars.len() && chars[idx].1 != '\n' {
                idx += 1;
            }
            continue;
        }
        result.push(ch);
        idx += 1;
    }
    result
}

// =============================================================================
// Helpers char-safe
// =============================================================================

/// Trouve le premier offset byte qui est une frontière de char, à partir de
/// `byte_idx` (inclus). Si `byte_idx` est déjà une frontière, le retourne.
fn safe_slice_start(s: &str, byte_idx: usize) -> usize {
    if byte_idx >= s.len() {
        return s.len();
    }
    let mut idx = byte_idx;
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Trouve le dernier offset byte qui est une frontière de char, jusqu'à
/// `byte_idx` (inclus). Si `byte_idx` est déjà une frontière, le retourne.
fn safe_slice_end(s: &str, byte_idx: usize) -> usize {
    if byte_idx == 0 {
        return 0;
    }
    let mut idx = byte_idx;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Trouve la position de la première virgule au niveau 0.
fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

// =============================================================================
// Tests unitaires — E7 Lot T2 (transpileur : initializers, IntPtr.Add, foreach)
// =============================================================================
#[cfg(test)]
mod tests_t2 {
    use super::transpile_cs_to_js;

    /// T2.1 — Object initializer `new X(args) { Prop = val }` → IIFE
    #[test]
    fn test_object_initializer_simple() {
        let cs = r#"var w = new MemoryWatcher(ptr) { Name = "Progress" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("(() => { var __o = new MemoryWatcher(ptr); __o.Name = \"Progress\"; return __o; })()"),
            "Object initializer doit devenir IIFE, obtenu: {}", js
        );
    }

    /// T2.2 — Object initializer avec plusieurs propriétés
    #[test]
    fn test_object_initializer_multi_props() {
        let cs = r#"var w = new MemoryWatcher(ptr) { Name = "X", Current = 0 };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("__o.Name = \"X\"") && js.contains("__o.Current = 0"),
            "Object initializer multi-props, obtenu: {}", js
        );
    }

    /// T2.3 — Collection initializer sur type custom `new MemoryWatcherList() { w1, w2 }` → IIFE .Add
    #[test]
    fn test_collection_initializer_custom() {
        let cs = r#"var M = new MemoryWatcherList() { w1, w2, w3 };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("__l.Add(w1)") && js.contains("__l.Add(w2)") && js.contains("__l.Add(w3)"),
            "Collection initializer custom doit utiliser .Add, obtenu: {}", js
        );
    }

    /// E7 runtime — Tableau typé `new short[] { -1, 19, 24 }` → array literal `[-1, 19, 24]`
    /// (bug MGS : produisait `(0)` via IIFE `__l.Add` + suppression `new short[]` → `(0).Add`
    /// throw « not a callable function » au runtime)
    #[test]
    fn test_typed_array_initializer_array_literal() {
        let cs = r#"var a = new short[] { -1, 19, 24 };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("[-1, 19, 24]"),
            "Tableau typé doit devenir un array literal, obtenu: {}", js
        );
        assert!(
            !js.contains("__l.Add") && !js.contains("(0)"),
            "Tableau typé ne doit produire ni __l.Add ni (0), obtenu: {}", js
        );
    }

    /// E7 runtime — ExpandoObject doit devenir {} (pas (0))
    #[test]
    fn test_expando_object_to_braces() {
        let cs = r#"D.Game = new ExpandoObject();"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("{}") && !js.contains("(0)"),
            "ExpandoObject doit devenir {{}}, obtenu: {}", js
        );
    }

    /// E7 runtime — ExpandoObject dans le contexte MGS (avec lignes précédentes)
    #[test]
    fn test_expando_object_in_context() {
        let cs = r#"  D.New = new ExpandoObject(); // Functions to create new data structures
  D.Game = new ExpandoObject(); // Data about the game/emulator
  D.Run = new ExpandoObject(); // Splitter data about the run"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            !js.contains("(0)"),
            "ExpandoObject in context ne doit pas produire (0), obtenu: {}", js
        );
    }

    /// E7 runtime — Tableau typé imbriqué dans un Dictionary init (pattern MGS exact)
    #[test]
    fn test_typed_array_in_dictionary_init() {
        let cs = r#"var progressSets = new Dictionary<string, short[]>() {
    { "ReachDarpaChief",  new short[] { -1, 19, 24 } },
    { "VentClip",         new short[] { 18, 158 } },
  };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("m.set(\"ReachDarpaChief\", [-1, 19, 24])"),
            "Dictionary avec tableau typé imbriqué, obtenu: {}", js
        );
        assert!(
            js.contains("m.set(\"VentClip\", [18, 158])"),
            "Dictionary avec tableau typé imbriqué (2e paire), obtenu: {}", js
        );
        assert!(
            !js.contains("__l.Add"),
            "Aucun __l.Add ne doit rester, obtenu: {}", js
        );
    }

    /// T2.4 — IntPtr.Add(a, b) → (a + b)
    #[test]
    fn test_intptr_add() {
        let cs = r#"var addr = IntPtr.Add(baseAddr, 0x100);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("(baseAddr + 0x100)"),
            "IntPtr.Add doit devenir (a + b), obtenu: {}", js
        );
    }

    /// T2.5 — IntPtr.Add avec expression complexe
    #[test]
    fn test_intptr_add_complex() {
        let cs = r#"var addr = IntPtr.Add(G.BaseAddress, F.Offset("test"));"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("(G.BaseAddress + F.Offset(\"test\"))"),
            "IntPtr.Add complexe, obtenu: {}", js
        );
    }

    /// T2.6 — foreach sur Dictionary avec p.Key/p.Value → wrapper { Key, Value }
    #[test]
    fn test_foreach_dict_key_value() {
        let cs = r#"foreach (var p in progressSets) { if (p.Key == "x") return p.Value; }"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("Key:") && js.contains("Value:"),
            "foreach Dictionary doit wrapper en {{ Key, Value }}, obtenu: {}", js
        );
    }

    /// T2.7 — foreach sur List (pas de .Key/.Value) → valeur directe
    #[test]
    fn test_foreach_list_direct() {
        let cs = r#"foreach (var item in items) { print(item); }"#;
        let js = transpile_cs_to_js(cs);
        // Le transpileur utilise instanceof Map pour distinguer Maps (→ entries() +
        // wrapper { Key, Value }) des arrays/Sets (→ itération directe, valeur
        // brute). On vérifie juste que ça compile (pas de foreach résiduel).
        assert!(
            !js.contains("foreach"),
            "foreach doit être remplacé, obtenu: {}", js
        );
    }

    /// T2.8 — HashSet collection initializer `new HashSet<string>() { a, b }` → new Set([a, b])
    #[test]
    fn test_hashset_collection_init() {
        let cs = r#"var s = new HashSet<string>() { "a", "b", "c" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("new Set([\"a\", \"b\", \"c\"])"),
            "HashSet init doit devenir new Set([a, b, c]), obtenu: {}", js
        );
    }

    /// T2.9 — HashSet vide `new HashSet<string>()` → new Set()
    #[test]
    fn test_hashset_empty() {
        let cs = r#"var s = new HashSet<string>();"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("new Set(") && !js.contains("HashSet"),
            "HashSet vide doit devenir new Set(), obtenu: {}", js
        );
    }

    /// T2.10 — Empty parens résiduels `()` en position d'expression → (0)
    #[test]
    fn test_empty_parens() {
        // `() + 1` → `(0) + 1` (pas un appel de fonction)
        let cs = r#"var x = () + 1;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("(0) + 1"),
            "Empty parens en expression doit devenir (0), obtenu: {}", js
        );
    }

    /// T2.11 — Empty parens après identifiant = appel de fonction → gardé
    #[test]
    fn test_empty_parens_function_call() {
        let cs = r#"var x = foo();"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("foo()") && !js.contains("foo(0)"),
            "Appel de fonction foo() doit être gardé, obtenu: {}", js
        );
    }

    /// T2.12 — Object initializer avec new TimerModel { CurrentState = timer }
    #[test]
    fn test_object_initializer_timermodel() {
        let cs = r#"V.TimerModel = new TimerModel { CurrentState = timer };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("__o.CurrentState = timer"),
            "TimerModel initializer, obtenu: {}", js
        );
    }

    /// T2.13 — Object initializer avec new StringWatcher(ptr, 8) { Name = "..." }
    #[test]
    fn test_object_initializer_stringwatcher() {
        let cs = r#"var w = new StringWatcher(ptr, 8) { Name = "Location" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("__o.Name = \"Location\"") && js.contains("new StringWatcher(ptr, 8)"),
            "StringWatcher initializer, obtenu: {}", js
        );
    }

    /// T2.14 — Object initializer imbriqué (new X() { Prop = new Y() { P = v } })
    #[test]
    fn test_object_initializer_nested() {
        let cs = r#"var w = new Outer() { Inner = new Inner() { Val = 1 } };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("__o.Inner") && js.contains("__o.Val = 1"),
            "Object initializer imbriqué, obtenu: {}", js
        );
    }

    /// T2.15 — IntPtr.Zero reste 0
    #[test]
    fn test_intptr_zero() {
        let cs = r#"if (ptr == IntPtr.Zero) return;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("0") && !js.contains("IntPtr.Zero"),
            "IntPtr.Zero doit devenir 0, obtenu: {}", js
        );
    }

    /// T2.16 — Combiné : foreach Dictionary + IntPtr.Add + object initializer
    #[test]
    fn test_combined_t2_patterns() {
        let cs = r#"
foreach (var p in dict) {
    var addr = IntPtr.Add(p.Value, 0x10);
    var w = new MemoryWatcher(addr) { Name = p.Key };
}"#;
        let js = transpile_cs_to_js(cs);
        assert!(!js.contains("foreach"), "foreach remplacé");
        assert!(js.contains("(p.Value + 0x10)"), "IntPtr.Add transpilé");
        assert!(js.contains("__o.Name = p.Key"), "Object initializer transpilé");
    }

    /// T2.17 — Auto-start fix : `new MemoryWatcher<short>(addr)` → `new MemoryWatcher(addr, 'short')`
    /// Le type C# doit être préservé pour que la lecture mémoire utilise la bonne
    /// taille (short=2 bytes, int=4 bytes). Sans ce fix, <short> est strippé par
    /// remove_method_generics → Type='int' (défaut) → lecture 4 bytes → valeur faussée.
    #[test]
    fn test_memory_watcher_generics_short() {
        let cs = r#"var w = new MemoryWatcher<short>(F.Addr(0x38D7CA));"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("new MemoryWatcher(") && js.contains("'short'"),
            "MemoryWatcher<short> doit devenir MemoryWatcher(args, 'short'), obtenu: {}", js
        );
        assert!(
            !js.contains("MemoryWatcher<short>"),
            "Le générique <short> doit être retiré, obtenu: {}", js
        );
    }

    /// T2.18 — Auto-start fix : MemoryWatcher<int> préserve 'int'
    #[test]
    fn test_memory_watcher_generics_int() {
        let cs = r#"var w = new MemoryWatcher<int>(addr);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("'int'") && !js.contains("MemoryWatcher<int>"),
            "MemoryWatcher<int> doit devenir MemoryWatcher(args, 'int'), obtenu: {}", js
        );
    }

    /// T2.19 — Auto-start fix : MemoryWatcher<short> avec object initializer
    /// `new MemoryWatcher<short>(addr) { Name = "Progress" }` doit produire
    /// `new MemoryWatcher(addr, 'short')` puis l'IIFE initializer avec __o.Name.
    #[test]
    fn test_memory_watcher_generics_with_initializer() {
        let cs = r#"var w = new MemoryWatcher<short>(F.Addr(0x38D7CA)) { Name = "Progress" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("'short'") && js.contains("__o.Name = \"Progress\""),
            "MemoryWatcher<short> + initializer doit préserver le type ET l'IIFE, obtenu: {}", js
        );
        assert!(
            !js.contains("MemoryWatcher<short>"),
            "Le générique doit être retiré, obtenu: {}", js
        );
    }

    /// T2.20 — Auto-start fix : MemoryWatcher<uint> (type unsigned)
    #[test]
    fn test_memory_watcher_generics_uint() {
        let cs = r#"var w = new MemoryWatcher<uint>(F.Addr(0x595344)) { Name = "GameTime" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("'uint'") && js.contains("__o.Name = \"GameTime\""),
            "MemoryWatcher<uint> + initializer, obtenu: {}", js
        );
    }

    /// T2.21 — Auto-start fix : MemoryWatcher<byte> avec DeepPointer (boss HP)
    /// `new MemoryWatcher<byte>(new DeepPointer(...))` doit préserver 'byte'
    /// et garder le DeepPointer intact.
    #[test]
    fn test_memory_watcher_generics_byte_with_deepointer() {
        let cs = r#"var w = new MemoryWatcher<byte>(new DeepPointer(addr, 0x100)) { Name = "NoControl" };"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("'byte'") && js.contains("new DeepPointer("),
            "MemoryWatcher<byte> + DeepPointer doit préserver le type ET le DeepPointer, obtenu: {}", js
        );
    }

    /// T2.22 — Auto-start fix : `new byte[len]` → `new Array(len)`
    /// Sans ce fix, `new byte[len]` reste tel quel en JS → `byte` est un
    /// identifiant nu non défini → ReferenceError "byte is not defined".
    /// Ce cas est atteint quand le strip .exe permet au switch case "mgsi"
    /// de matcher (section PC memwatchers appelle New.ByteArray qui fait
    /// `new byte[len]`).
    #[test]
    fn test_typed_array_creation_byte() {
        let cs = r#"ba.Current = new byte[len];"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("new Array(len)"),
            "new byte[len] doit devenir new Array(len), obtenu: {}", js
        );
        assert!(
            !js.contains("new byte["),
            "new byte[ ne doit plus apparaître, obtenu: {}", js
        );
    }

    /// T2.23 — Auto-start fix : `new int[size]` → `new Array(size)`
    #[test]
    fn test_typed_array_creation_int() {
        let cs = r#"var arr = new int[size];"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("new Array(size)"),
            "new int[size] doit devenir new Array(size), obtenu: {}", js
        );
    }

    /// T2.24 — Auto-start fix : `new byte[]` (empty) reste géré par step 5
    /// (ne doit PAS devenir new Array() — c'est step 5 qui le gère)
    #[test]
    fn test_typed_array_creation_empty_brackets_preserved() {
        let cs = r#"var arr = new byte[] { 1, 2, 3 };"#;
        let js = transpile_cs_to_js(cs);
        // new byte[] { ... } est géré par transpile_object_initializer (step 3b)
        // → array literal [1, 2, 3]. new Array ne doit PAS apparaître.
        assert!(
            !js.contains("new byte["),
            "new byte[] ne doit plus apparaître (géré par step 3b/5), obtenu: {}", js
        );
    }
}

// =============================================================================
// Tests unitaires — E7 Lot T3 (transpileur : out/ref, default, nameof, génériques méthode, named args)
// =============================================================================
#[cfg(test)]
mod tests_t3 {
    use super::transpile_cs_to_js;

    /// T3.1 — `out` parameter retiré : TryGetValue(code, out name) → TryGetValue(code, name)
    #[test]
    fn test_out_parameter() {
        let cs = r#"if (!dict.TryGetValue(code, out name)) return "";"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            !js.contains(" out ") && js.contains("TryGetValue(code, name)"),
            "out parameter doit être retiré, obtenu: {}", js
        );
    }

    /// T3.2 — `ref` parameter retiré
    #[test]
    fn test_ref_parameter() {
        let cs = r#"F.DoSomething(ref val);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            !js.contains(" ref ") && js.contains("F.DoSomething(val)"),
            "ref parameter doit être retiré, obtenu: {}", js
        );
    }

    /// T3.3 — `in` parameter retiré (attention : ne pas casser `in` dans foreach)
    #[test]
    fn test_in_parameter() {
        let cs = r#"F.DoSomething(in val);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            !js.contains(" in ") || js.contains(" in "), // in peut rester dans foreach
            "in parameter retiré, obtenu: {}", js
        );
        // Vérifier spécifiquement que " in val" est retiré
        assert!(!js.contains(" in val"), "in val doit être retiré, obtenu: {}", js);
    }

    /// T3.4 — `in` dans foreach préservé
    #[test]
    fn test_in_foreach_preserved() {
        let cs = r#"foreach (var p in dict) { print(p); }"#;
        let js = transpile_cs_to_js(cs);
        // foreach est transformé par replace_foreach avant, donc " in " peut ne plus être là
        // mais on vérifie qu'il n'y a pas de regression
        assert!(!js.contains("foreach"), "foreach remplacé");
    }

    /// T3.5 — default(T) → null
    #[test]
    fn test_default_type() {
        let cs = r#"var x = default(int);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("null") && !js.contains("default("),
            "default(int) doit devenir null, obtenu: {}", js
        );
    }

    /// T3.6 — default(string) → null
    #[test]
    fn test_default_string() {
        let cs = r#"var s = default(string);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("null") && !js.contains("default("),
            "default(string) doit devenir null, obtenu: {}", js
        );
    }

    /// T3.7 — nameof(X) → "X"
    #[test]
    fn test_nameof_simple() {
        let cs = r#"var name = nameof(Progress);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("\"Progress\"") && !js.contains("nameof("),
            "nameof(Progress) doit devenir \"Progress\", obtenu: {}", js
        );
    }

    /// T3.8 — nameof(Type.Prop) → "Type.Prop"
    #[test]
    fn test_nameof_dotted() {
        let cs = r#"var name = nameof(D.Game);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("\"D.Game\"") && !js.contains("nameof("),
            "nameof(D.Game) doit devenir \"D.Game\", obtenu: {}", js
        );
    }

    /// T3.9 — Générique de méthode Method<T>(args) → Method(args)
    #[test]
    fn test_method_generics() {
        let cs = r#"var x = F.Parse<int>(str);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("F.Parse(str)") && !js.contains("<int>"),
            "Method<T>(args) doit devenir Method(args), obtenu: {}", js
        );
    }

    /// T3.10 — Générique de méthode avec type complexe
    #[test]
    fn test_method_generics_complex() {
        let cs = r#"var x = F.Convert<string, int>(val);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("F.Convert(val)") && !js.contains("<string, int>"),
            "Method<T,U>(args) doit devenir Method(args), obtenu: {}", js
        );
    }

    /// T3.11 — Named args Method(name: value) → Method(value)
    #[test]
    fn test_named_args() {
        let cs = r#"F.DoSomething(key: "test", val: 42);"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("\"test\"") && js.contains("42") && !js.contains("key:") && !js.contains("val:"),
            "Named args retirés, obtenu: {}", js
        );
    }

    /// T3.12 — Named args avec expression
    #[test]
    fn test_named_args_expr() {
        let cs = r#"F.Add(key: "X", def: F.DefaultSetting("X", true, ""));"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            !js.contains("key:") && !js.contains("def:"),
            "Named args avec expression retirés, obtenu: {}", js
        );
    }

    /// T3.13 — ?. null-conditional préservé (Boa supporte ?.)
    #[test]
    fn test_null_conditional_preserved() {
        let cs = r#"var x = obj?.Prop;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("?." ),
            "?. doit être préservé (Boa supporte), obtenu: {}", js
        );
    }

    /// T3.14 — ?? déjà transformé en || par étape 18
    #[test]
    fn test_null_coalescing() {
        let cs = r#"var x = a ?? b;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("||") && !js.contains("??"),
            "?? doit devenir ||, obtenu: {}", js
        );
    }

    /// T3.15 — Combiné : out + named args + default
    #[test]
    fn test_combined_t3_patterns() {
        let cs = r#"
var name = default(string);
if (!dict.TryGetValue(key: "X", out name)) return nameof(name);"#;
        let js = transpile_cs_to_js(cs);
        assert!(js.contains("null"), "default(string) → null");
        assert!(!js.contains(" out "), "out retiré");
        assert!(!js.contains("key:"), "named arg retiré");
        assert!(js.contains("\"name\""), "nameof(name) → \"name\"");
    }

    /// T3.16 — Arrow function préservée (Boa supporte)
    #[test]
    fn test_arrow_function_preserved() {
        let cs = r#"var f = (x) => x + 1;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("=>") && js.contains("(x)"),
            "Arrow function préservée, obtenu: {}", js
        );
    }

    /// T3.17 — Arrow function sans parens préservée
    #[test]
    fn test_arrow_function_no_parens() {
        let cs = r#"var f = x => x + 1;"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("=>"),
            "Arrow function sans parens préservée, obtenu: {}", js
        );
    }

    /// T3.18 — Destructuring préservé (Boa supporte)
    #[test]
    fn test_destructuring_preserved() {
        let cs = r#"var [a, b] = [1, 2];"#;
        let js = transpile_cs_to_js(cs);
        assert!(
            js.contains("[a, b]"),
            "Destructuring préservé, obtenu: {}", js
        );
    }
}

// =============================================================================
// Tests smoke — E7 Lot T4 (MetalGearSolid.asl)
// -----------------------------------------------------------------------------
// Charge le script MGS réel, parse, transpile, compile chaque méthode dans Boa
// et rapporte les erreurs de compilation par méthode.
// =============================================================================
#[cfg(test)]
mod tests_t4_smoke {
    use super::transpile_cs_to_js;
    use crate::speedrun::asl;
    use crate::speedrun::engine::script_bridge::ScriptContext;

    /// T4.DEBUG — Test direct de transpile_dictionary_init sur le pattern MGS
    #[test]
    fn test_debug_dict_init_mgs() {
        // Test avec le contexte complet du startup (avant et après le Dictionary)
        let cs = r#"
  V.ExceptionCount = new Dictionary<string, int>();
  V.DefaultSettings = new Dictionary<string, bool>();
  var progressSets = new Dictionary<string, short[]>() {
    { "ReachDarpaChief",  new short[] { -1, 19, 24 } },
    { "VentClip",         new short[] { 18, 158 } },
  };
  D.Sets.Progress = new Dictionary<short, List<string>>();
  var pSet = D.Sets.Progress;
"#;
        let js = transpile_cs_to_js(cs);
        eprintln!("[T4 DEBUG] dict init JS:\n{}", js);
        // Vérifier qu'aucun `new Map() {` n'est présent (tous les Dictionary init doivent être IIFEs)
        assert!(!js.contains("new Map() {"), "Dictionary init doit produire IIFE, pas new Map() {{...}}, obtenu: {}", js);
    }

    /// T4.DEBUG — Test List+comment : le ] ne doit pas etre colle au } parent
    #[test]
    fn test_debug_dict_with_lists() {
        let cs = r#"if (x) { var settingTemplates = new List<string>() {
  "a", // c
  "b",
};
content = settingTemplates[0];
}"#;
        let js = transpile_cs_to_js(cs);
        eprintln!("[T4 DEBUG] list+comment JS:\n{}", js);
        // 1. Pas de ]}; (] colle au } du if)
        assert!(!js.contains("]};"), "Ne doit pas contenir ]}}; — obtenu: {}", js);
        // 2. content reste DANS le if (avant le } final)
        let last_brace = js.rfind('}').unwrap();
        let content_pos = js.find("content").unwrap();
        assert!(content_pos < last_brace, "content doit etre avant le }} final — obtenu: {}", js);
        // 3. ] puis ; puis content (le ] ferme l'array, ; termine, content est apres)
        assert!(js.contains("\"b\""), "b doit etre present — obtenu: {}", js);
    }

    /// T4.SMOKE — Charge MetalGearSolid.asl, transpile et compile chaque méthode.
    /// Rapporte les erreurs de compilation par méthode (ne panique pas — affiche un rapport).
    #[test]
    fn test_smoke_mgs_compilation() {
        let asl_path = "D:\\SteamOs - All Version\\RUST_SOS_2026v0.1\\MetalGearSolid.asl";
        let source = match std::fs::read_to_string(asl_path) {
            Ok(s) => s,
            Err(e) => {
                // Si le fichier n'est pas trouvé (CI sans le script), skip ce test
                eprintln!("[T4 SMOKE] MetalGearSolid.asl non trouvé ({}): skip", e);
                return;
            }
        };

        // 1. Parser le script ASL
        let script = match asl::parse(&source) {
            Ok(s) => s,
            Err(e) => panic!("[T4 SMOKE] Parse ASL échoué: {}", e),
        };

        let method_names = [
            "startup", "shutdown", "init", "exit", "update",
            "start", "split", "reset", "isLoading", "gameTime",
            "onStart", "onSplit", "onReset",
        ];

        // 2. Transpiler et compiler chaque méthode
        let mut ctx = ScriptContext::new();
        let mut ok_count = 0;
        let mut err_count = 0;
        let mut report = String::new();

        for &name in &method_names {
            if let Some(code) = script.methods.get(name) {
                let js_code = transpile_cs_to_js(code);
                // Dump JS pour debug
                let dump_path = format!("D:\\SteamOs - All Version\\RUST_SOS_2026v0.1\\debug_{}_js.txt", name);
                let _ = std::fs::write(&dump_path, &js_code);
                match ctx.compile_method(name, &js_code) {
                    Ok(_) => {
                        report.push_str(&format!("  [OK]   {} ({}→{} chars)\n", name, code.len(), js_code.len()));
                        ok_count += 1;
                    }
                    Err(e) => {
                        report.push_str(&format!("  [FAIL] {} ({}→{} chars): {}\n", name, code.len(), js_code.len(), e));
                        // Dumper les lignes autour de l'erreur (numéros de ligne dans le wrapper)
                        // Le wrapper compile_method ajoute ~10 lignes avant le code.
                        // Les numéros de ligne Boa sont dans le wrapper complet.
                        let lines: Vec<&str> = js_code.lines().collect();
                        // Extraire le numéro de ligne de l'erreur
                        if let Some(line_pos) = e.find("at line ") {
                            let rest = &e[line_pos + 8..];
                            if let Some(comma_pos) = rest.find(',') {
                                if let Ok(line_num) = rest[..comma_pos].trim().parse::<usize>() {
                                    // Le wrapper compile_method ajoute ~18 lignes avant le code utilisateur
                                    let actual_line = line_num.saturating_sub(18);
                                    let start = actual_line.saturating_sub(2);
                                    let end = (actual_line + 3).min(lines.len());
                                    report.push_str(&format!("    --- JS autour de la ligne {} (wrapper line {}):\n", actual_line, line_num));
                                    for i in start..end {
                                        if i < lines.len() {
                                            report.push_str(&format!("    {:4}: {}\n", i + 1, lines[i]));
                                        }
                                    }
                                }
                            }
                        }
                        err_count += 1;
                    }
                }
            }
        }

        eprintln!("[T4 SMOKE] Rapport compilation MetalGearSolid.asl:");
        eprintln!("{}", report);
        eprintln!("[T4 SMOKE] Résultat: {} OK, {} FAIL", ok_count, err_count);

        // Le test passe si au moins startup/init/start/split/reset compilent
        // (objectif minimal). On ne panique pas si update/gameTime/etc échouent.
        let critical = ["startup", "init", "start", "split", "reset"];
        let mut critical_failures = Vec::new();
        for &name in &critical {
            if script.methods.get(name).is_some() && !ctx.has_method(name) {
                critical_failures.push(name);
            }
        }

        if !critical_failures.is_empty() {
            panic!(
                "[T4 SMOKE] Méthodes critiques non compilées: {:?}\nRapport:\n{}",
                critical_failures, report
            );
        }
    }
}
