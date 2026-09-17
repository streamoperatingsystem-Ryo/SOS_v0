// =============================================================================
// Script Bridge — Moteur JavaScript Boa + API de compatibilité ASL
// -----------------------------------------------------------------------------
// Remplace la compilation C# à la volée (Roslyn) par un moteur JS pur Rust.
//
// IMPORTANT : Boa's Context n'est PAS Send (utilise Rc/NonNull en interne).
// Le ScriptContext doit donc vivre dans un seul thread (le thread de polling
// du moteur ASL). La communication avec les commandes Tauri se fait par
// channels (mpsc).
//
// API JS exposée aux scripts ASL (imite l'API C# de LiveSplit) :
//   - MemoryWatcherList : collection de watchers (UpdateAll, add, get by name)
//   - MemoryWatcher<T>  : surveillance d'une adresse mémoire typée
//   - StringWatcher     : surveillance d'une string mémoire
//   - DeepPointer       : chaîne de pointeurs (module + base + offsets)
//   - SignatureScanner  : scan de signatures binaires
//   - SigScanTarget     : cible de scan (pattern + wildcards)
//   - Vector3f          : vecteur 3D (X, Y, Z)
//   - string            : { IsNullOrEmpty, Format, Join }
//   - Convert           : { ToInt32, ToDouble, ToBoolean }
//   - Console           : { WriteLine, Write }
//   - Math              : (déjà en JS, aliases C# ajoutés)
//
// Les lectures mémoire réelles sont faites côté Rust : les objets JS
// (DeepPointer, MemoryWatcher) stockent les paramètres, et quand le script
// appelle .Update() ou .Deref(), un callback Rust fait la vraie lecture
// via ReadProcessMemory.
// =============================================================================
use boa_engine::{Context, JsValue, NativeFunction, Source};

/// Paramètres ASL (basic settings : start, split, reset activés/désactivés).
#[derive(Clone, Debug, Default)]
pub struct AslSettings {
    pub start: bool,
    pub split: bool,
    pub reset: bool,
}

/// Un setting ASL individuel (ex: "OL-s00a" → "Dock", coché/décoché).
/// Retourné par lire_settings_asl() pour la modale de configuration.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SettingDetail {
    /// Code-signature du split (ex: "OL-s00a", "CP-18", "OL-s01a.CL-s02a.CP-18").
    pub id: String,
    /// Description lisible (ex: "Dock", "Heliport", "Tank Hangar").
    pub label: String,
    /// Valeur courante (coché = true).
    pub value: bool,
    /// Parent (catégorie/groupement) si défini par CurrentDefaultParent.
    pub parent: Option<String>,
}

/// Contexte d'exécution du script ASL. Vit dans un seul thread.
pub struct ScriptContext {
    ctx: Context,
    /// Vars JSON (sérialisé pour passage vers JS)
    pub vars_json: String,
    /// Les méthodes ASL compilées en fonctions JS
    methods_compiled: std::collections::HashSet<String>,
    /// PID du processus jeu courant (pour les lectures mémoire depuis JS)
    pub current_pid: Option<u32>,
}

impl ScriptContext {
    pub fn new() -> Self {
        let mut ctx = Context::default();

        // --- Enregistrer print() comme fonction native globale ---
        // Boa n'a pas de print() intégré — on doit l'enregistrer AVANT le code
        // de l'API JS qui l'utilise (Console, Debug, etc.).
        let print_fn = NativeFunction::from_fn_ptr(|_this, args, ctx| {
            if let Some(msg) = args.first() {
                let s = msg.to_string(ctx)?;
                log::debug!("[ASL Script] {}", s.to_std_string_escaped());
            }
            Ok(JsValue::undefined())
        });
        let _ = ctx.register_global_callable("print".into(), 0, print_fn);

        // Définir l'API de compatibilité ASL complète en JavaScript
        let api = r#"
        // =============================================================================
        // API de compatibilité ASL (Auto Split Language) — équivalent JS du runtime C#
        // =============================================================================
        
        // --- Helpers globaux ---
        function __str_or_null(s) { return s === null || s === undefined || s.length === 0; }
        function __format(fmt) {
            var args = Array.prototype.slice.call(arguments, 1);
            return fmt.replace(/\{(\d+)\}/g, function(m, i) { return args[i] !== undefined ? String(args[i]) : ''; });
        }

        // --- Polyfill String.prototype.substr (Boa ne le supporte pas nativement) ---
        // Utilisé par Path.GetDirectoryName, string.Substring, SigScanTarget, et le
        // code transpilé. Sans ce polyfill, "abc".substr(0,2) throw « not a callable function ».
        if (!String.prototype.substr) {
            String.prototype.substr = function(start, length) {
                if (start < 0) start = this.length + start;
                if (start < 0) start = 0;
                if (length === undefined) return this.substring(start);
                return this.substring(start, start + length);
            };
        }

        // --- C# String instance methods (polyfills sur String.prototype) ---
        // Les scripts ASL utilisent les méthodes d'instance C# comme s.StartsWith("x"),
        // s.Substring(0, 3), s.Split('.'), etc. JS ne supporte pas toutes ces méthodes
        // ou a des signatures différentes (ex: Substring(start, length) vs substring(start, end)).
        if (!String.prototype.StartsWith) {
            String.prototype.StartsWith = function(prefix) { return this.indexOf(prefix) === 0; };
        }
        if (!String.prototype.EndsWith) {
            String.prototype.EndsWith = function(suffix) { return this.indexOf(suffix, this.length - suffix.length) !== -1; };
        }
        if (!String.prototype.Contains) {
            String.prototype.Contains = function(sub) { return this.indexOf(sub) !== -1; };
        }
        // C# Substring(start) ou Substring(start, length) — différent de JS substring(start, end)
        if (!String.prototype.Substring) {
            String.prototype.Substring = function(start, length) {
                if (start < 0) start = 0;
                if (length === undefined) return this.substring(start);
                return this.substring(start, start + length);
            };
        }
        // C# Split — supporte char et string separator
        if (!String.prototype.Split) {
            String.prototype.Split = function(sep) {
                if (sep === null || sep === undefined) return [this.toString()];
                if (typeof sep === 'string') return this.split(sep);
                // Si sep est un array de chars
                if (Array.isArray(sep)) {
                    var pattern = sep.map(function(c) { return c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }).join('|');
                    return this.split(new RegExp(pattern));
                }
                return this.split(sep);
            };
        }
        if (!String.prototype.Trim) {
            String.prototype.Trim = function() { return this.trim(); };
        }
        if (!String.prototype.TrimStart) {
            String.prototype.TrimStart = function() { return this.replace(/^\s+/, ''); };
        }
        if (!String.prototype.TrimEnd) {
            String.prototype.TrimEnd = function() { return this.replace(/\s+$/, ''); };
        }
        if (!String.prototype.ToUpper) {
            String.prototype.ToUpper = function() { return this.toUpperCase(); };
        }
        if (!String.prototype.ToLower) {
            String.prototype.ToLower = function() { return this.toLowerCase(); };
        }
        // E8 Bug A — ToLowerInvariant/ToUpperInvariant (C# String methods).
        // Les scripts ASL utilisent .ToLowerInvariant() au lieu de .ToLower().
        // Sans ce shim, Boa throw "not a callable function" car la méthode
        // n'existe pas sur String.prototype.
        if (!String.prototype.ToLowerInvariant) {
            String.prototype.ToLowerInvariant = function() { return this.toLowerCase(); };
        }
        if (!String.prototype.ToUpperInvariant) {
            String.prototype.ToUpperInvariant = function() { return this.toUpperCase(); };
        }
        if (!String.prototype.PadLeft) {
            String.prototype.PadLeft = function(totalWidth, padChar) {
                padChar = padChar || ' ';
                var s = this.toString();
                while (s.length < totalWidth) s = padChar + s;
                return s;
            };
        }
        if (!String.prototype.PadRight) {
            String.prototype.PadRight = function(totalWidth, padChar) {
                padChar = padChar || ' ';
                var s = this.toString();
                while (s.length < totalWidth) s = s + padChar;
                return s;
            };
        }
        if (!String.prototype.IsNullOrEmpty) {
            String.prototype.IsNullOrEmpty = function() { return this === null || this === undefined || this.length === 0; };
        }
        
        // --- string (équivalent C# System.String) ---
        var string = {
            IsNullOrEmpty: __str_or_null,
            IsNullOrWhiteSpace: function(s) { return s === null || s === undefined || s.trim().length === 0; },
            Format: __format,
            Join: function(sep, parts) {
                if (Array.isArray(parts)) return parts.join(sep);
                // Si on reçoit des args individuels (string.Join(sep, a, b, c))
                var rest = Array.prototype.slice.call(arguments, 2);
                return rest.join(sep);
            },
            Concat: function() { return Array.prototype.slice.call(arguments).join(''); },
            Compare: function(a, b) { return a < b ? -1 : (a > b ? 1 : 0); },
            Contains: function(s, sub) { return s !== null && s.indexOf(sub) >= 0; },
            StartsWith: function(s, prefix) { return s !== null && s.indexOf(prefix) === 0; },
            EndsWith: function(s, suffix) { return s !== null && s.lastIndexOf(suffix) === s.length - suffix.length; },
            Split: function(s, sep) { return s !== null ? s.split(sep) : []; },
            Trim: function(s) { return s !== null ? s.trim() : s; },
            Substring: function(s, start, len) {
                if (s === null) return s;
                if (len === undefined) return s.substring(start);
                return s.substr(start, len);
            },
            ToLower: function(s) { return s !== null ? s.toLowerCase() : s; },
            ToUpper: function(s) { return s !== null ? s.toUpperCase() : s; },
            ToInt32: function(s) { return parseInt(s, 10) || 0; },
        };
        
        // --- Console (équivalent System.Console) ---
        var Console = { WriteLine: print, Write: print };
        
        // --- Convert (équivalent System.Convert) ---
        var Convert = {
            ToInt32: function(x) { return parseInt(x, 10) || 0; },
            ToDouble: function(x) { return parseFloat(x) || 0.0; },
            ToBoolean: function(x) {
                if (typeof x === 'string') return x.toLowerCase() === 'true';
                return Boolean(x);
            },
            ToString: function(x) { return String(x); },
            ToByte: function(x) { return (parseInt(x, 10) || 0) & 0xFF; },
        };
        
        // --- Math aliases C# ---
        if (typeof Math !== 'undefined') {
            Math.Ceiling = Math.ceil;
            Math.Floor = Math.floor;
            Math.Max = Math.max;
            Math.Min = Math.min;
            Math.Abs = Math.abs;
            Math.Sqrt = Math.sqrt;
            Math.Round = Math.round;
            Math.Pow = Math.pow;
            Math.Sin = Math.sin;
            Math.Cos = Math.cos;
            Math.Tan = Math.tan;
            Math.PI = Math.PI;
            Math.E = Math.E;
        }
        
        // --- Vector3f (équivalent C# struct Vector3f) ---
        function Vector3f(x, y, z) {
            this.X = x || 0.0;
            this.Y = y || 0.0;
            this.Z = z || 0.0;
        }
        Vector3f.prototype.Distance = function(other) {
            var dx = this.X - other.X, dy = this.Y - other.Y, dz = this.Z - other.Z;
            return Math.sqrt(dx*dx + dy*dy + dz*dz);
        };
        Vector3f.prototype.DistanceXY = function(other) {
            var dx = this.X - other.X, dy = this.Y - other.Y;
            return Math.sqrt(dx*dx + dy*dy);
        };
        Vector3f.prototype.toString = function() { return this.X + " " + this.Y + " " + this.Z; };
        
        // --- ExpandoObject (équivalent System.Dynamic.ExpandoObject) ---
        // En JS, un objet simple {} suffit — ExpandoObject est juste un objet dynamique.
        function ExpandoObject() { return {}; }
        
        // --- DeepPointer (équivalent C# LiveSplit.ComponentUtil.DeepPointer) ---
        // Stocke les paramètres du pointeur. La lecture réelle est faite par le runtime Rust
        // quand on appelle .Deref() ou .Deref<T>().
        function DeepPointer(module, base, offsets) {
            this.module = module || null;
            this.base = base || 0;
            this.offsets = offsets ? Array.prototype.slice.call(offsets) : [];
            // Si on reçoit (base, offsets...) sans module
            if (typeof module === 'number') {
                this.base = module;
                this.offsets = Array.prototype.slice.call(arguments, 1);
                this.module = null;
            }
        }
        // Les méthodes Deref sont des stubs — le runtime Rust les intercepte
        // via __deref_callback quand disponible, sinon retourne null.
        DeepPointer.prototype.Deref = function(game, defaultVal) {
            if (typeof __deref_callback === 'function') {
                return __deref_callback(this.module, this.base, this.offsets, 'int', game);
            }
            return defaultVal !== undefined ? defaultVal : 0;
        };
        DeepPointer.prototype.DerefInt = function(game) {
            if (typeof __deref_callback === 'function') return __deref_callback(this.module, this.base, this.offsets, 'int', game);
            return 0;
        };
        DeepPointer.prototype.DerefFloat = function(game) {
            if (typeof __deref_callback === 'function') return __deref_callback(this.module, this.base, this.offsets, 'float', game);
            return 0.0;
        };
        DeepPointer.prototype.DerefString = function(game, maxBytes) {
            if (typeof __deref_callback === 'function') return __deref_callback(this.module, this.base, this.offsets, 'string', game, maxBytes || 256);
            return "";
        };
        DeepPointer.prototype.DerefBytes = function(game, count) {
            if (typeof __deref_callback === 'function') return __deref_callback(this.module, this.base, this.offsets, 'bytes', game, count);
            return [];
        };
        
        // --- MemoryWatcher<T> (équivalent C# MemoryWatcher<T>) ---
        // Stocke un DeepPointer et garde trace de Current, Old, Changed.
        function MemoryWatcher(pointer, type) {
            this.Pointer = pointer;
            this.Type = type || 'int';
            this.Name = '';
            this.Enabled = true;
            this.Current = null;
            this.Old = null;
            this.Changed = false;
            this._initialUpdate = false;
        }
        MemoryWatcher.prototype.Update = function(game) {
            this.Changed = false;
            if (!this.Enabled) return false;
            var newVal = null;
            if (this.Pointer instanceof DeepPointer && typeof __deref_callback === 'function') {
                newVal = __deref_callback(this.Pointer.module, this.Pointer.base, this.Pointer.offsets, this.Type, game);
            }
            // Auto-start fix — adresse absolue (number Pointer).
            // Les scripts ASL PC font `new MemoryWatcher<short>(F.Addr(0x38D7CA))`
            // où F.Addr(offset) = IntPtr.Add(G.BaseAddress, offset) = (0x400000 + offset),
            // soit un NOMBRE (pas un DeepPointer). Sans ce branch, newVal reste null
            // → Current jamais peuplé → M["Progress"].Current = null → start()=false.
            // On appelle __deref_callback avec module=null, base=number, offsets=[].
            else if (typeof this.Pointer === 'number' && typeof __deref_callback === 'function') {
                newVal = __deref_callback(null, this.Pointer, [], this.Type, game);
            }
            if (newVal !== null) {
                this.Old = this.Current;
                this.Current = newVal;
            }
            if (!this._initialUpdate) {
                this._initialUpdate = true;
                return false;
            }
            if (this.Current !== this.Old) {
                this.Changed = true;
                return true;
            }
            return false;
        };
        MemoryWatcher.prototype.Reset = function() {
            this.Current = null;
            this.Old = null;
            this.Changed = false;
            this._initialUpdate = false;
        };
        
        // --- StringWatcher (équivalent C# StringWatcher) ---
        function StringWatcher(pointer, maxBytes) {
            this.Pointer = pointer;
            this.MaxBytes = maxBytes || 256;
            this.Name = '';
            this.Enabled = true;
            this.Current = null;
            this.Old = null;
            this.Changed = false;
            this._initialUpdate = false;
        }
        StringWatcher.prototype.Update = function(game) {
            this.Changed = false;
            if (!this.Enabled) return false;
            var newVal = null;
            if (this.Pointer instanceof DeepPointer && typeof __deref_callback === 'function') {
                newVal = __deref_callback(this.Pointer.module, this.Pointer.base, this.Pointer.offsets, 'string', game, this.MaxBytes);
            }
            // Auto-start fix — adresse absolue (number Pointer), voir MemoryWatcher.Update.
            // Les scripts ASL PC font `new StringWatcher(F.Addr(0x2504CE), 8)` pour
            // lire la location (8 chars). Sans ce branch, Location.Current = null.
            else if (typeof this.Pointer === 'number' && typeof __deref_callback === 'function') {
                newVal = __deref_callback(null, this.Pointer, [], 'string', game, this.MaxBytes);
            }
            if (newVal !== null) {
                this.Old = this.Current;
                this.Current = newVal;
            }
            if (!this._initialUpdate) {
                this._initialUpdate = true;
                return false;
            }
            if (this.Current !== this.Old) {
                this.Changed = true;
                return true;
            }
            return false;
        };
        StringWatcher.prototype.Reset = function() {
            this.Current = null;
            this.Old = null;
            this.Changed = false;
            this._initialUpdate = false;
        };
        
        // --- MemoryWatcherList (équivalent C# MemoryWatcherList) ---
        // Indexeur par string : list["name"] → watcher.
        // En C#, MemoryWatcherList expose un indexeur this[string].
        // En JS, on pose directement this[name] = watcher dans Add() pour
        // que list["X"] retourne le watcher (le watcher expose .Current/.Old/.Changed).
        function MemoryWatcherList() {
            this._watchers = [];
            this._byName = {};
        }
        MemoryWatcherList.prototype.Add = function(watcher) {
            this._watchers.push(watcher);
            if (watcher && watcher.Name) {
                this._byName[watcher.Name] = this._watchers.length - 1;
                // Indexeur : this["name"] → watcher (compat C# this[string])
                this[watcher.Name] = watcher;
            }
        };
        // Indexeur par string : list["name"] → watcher (alias explicite)
        MemoryWatcherList.prototype.getByName = function(name) {
            var idx = this._byName[name];
            return idx !== undefined ? this._watchers[idx] : undefined;
        };
        MemoryWatcherList.prototype.AddRange = function(other) {
            if (!other) return;
            var arr = (other instanceof MemoryWatcherList) ? other._watchers : other;
            for (var i = 0; i < arr.length; i++) this.Add(arr[i]);
        };
        MemoryWatcherList.prototype.Clear = function() {
            // Nettoyer aussi les clés d'indexeur posées sur this
            for (var name in this._byName) {
                if (Object.prototype.hasOwnProperty.call(this, name)) {
                    delete this[name];
                }
            }
            this._watchers = [];
            this._byName = {};
        };
        // Alias lowercase .clear() — le transpileur convertit .Clear() → .clear()
        MemoryWatcherList.prototype.clear = MemoryWatcherList.prototype.Clear;
        MemoryWatcherList.prototype.UpdateAll = function(game) {
            for (var i = 0; i < this._watchers.length; i++) {
                this._watchers[i].Update(game);
            }
        };
        MemoryWatcherList.prototype.ResetAll = function() {
            for (var i = 0; i < this._watchers.length; i++) {
                this._watchers[i].Reset();
            }
        };
        // Count (équivalent .Count en C#)
        Object.defineProperty(MemoryWatcherList.prototype, 'Count', {
            get: function() { return this._watchers.length; }
        });
        // E8 Bug A — length (alias de Count). Les scripts ASL font
        // `M.length != 0` pour guard l'accès aux watchers avant qu'ils soient
        // peuplés. Sans .length, M.length retourne undefined → undefined != 0
        // → true → le guard ne fonctionne pas → M["GameTime"].Current throw
        // "cannot convert null to object" car M["GameTime"] est undefined.
        Object.defineProperty(MemoryWatcherList.prototype, 'length', {
            get: function() { return this._watchers.length; }
        });
        
        // --- SigScanTarget (équivalent C# SigScanTarget) ---
        function SigScanTarget() {
            this.Signatures = [];
            this.OnFound = null;
        }
        SigScanTarget.prototype.AddSignature = function() {
            var args = Array.prototype.slice.call(arguments);
            var offset = 0;
            var sigStr = '';
            if (typeof args[0] === 'number') {
                offset = args[0];
                sigStr = args.slice(1).join('');
            } else {
                sigStr = args.join('');
            }
            sigStr = sigStr.replace(/\s/g, '');
            var pattern = [];
            var mask = [];
            for (var i = 0; i + 1 < sigStr.length; i += 2) {
                var pair = sigStr.substr(i, 2);
                if (pair[0] === '?' || pair[1] === '?') {
                    pattern.push(0);
                    mask.push(true);
                } else {
                    pattern.push(parseInt(pair, 16));
                    mask.push(false);
                }
            }
            this.Signatures.push({ Pattern: pattern, Mask: mask, Offset: offset });
        };
        
        // --- SignatureScanner (équivalent C# SignatureScanner) ---
        function SignatureScanner(game, address, size) {
            this.Game = game;
            this.Address = address;
            this.Size = size;
        }
        SignatureScanner.prototype.Scan = function(target) {
            if (typeof __sigscan_callback === 'function') {
                return __sigscan_callback(this.Game, this.Address, this.Size, target.Signatures);
            }
            return 0;
        };
        
        // --- Helpers LINQ (extension des arrays JS) ---
        // .FirstOrDefault() → déjà géré par .find(() => true) || null
        // Mais on ajoute des méthodes directes pour compatibilité C#
        if (!Array.prototype.FirstOrDefault) {
            Array.prototype.FirstOrDefault = function(pred) {
                if (!pred) return this.length > 0 ? this[0] : null;
                for (var i = 0; i < this.length; i++) {
                    if (pred(this[i])) return this[i];
                }
                return null;
            };
        }
        if (!Array.prototype.First) {
            Array.prototype.First = function(pred) {
                if (!pred) { if (this.length > 0) return this[0]; throw new Error("Sequence contains no elements"); }
                for (var i = 0; i < this.length; i++) {
                    if (pred(this[i])) return this[i];
                }
                throw new Error("Sequence contains no matching element");
            };
        }
        if (!Array.prototype.Where) {
            Array.prototype.Where = function(pred) { return this.filter(pred); };
        }
        if (!Array.prototype.Select) {
            Array.prototype.Select = function(proj) { return this.map(proj); };
        }
        if (!Array.prototype.Any) {
            Array.prototype.Any = function(pred) {
                if (!pred) return this.length > 0;
                return this.some(pred);
            };
        }
        if (!Array.prototype.All) {
            Array.prototype.All = function(pred) { return this.every(pred); };
        }
        if (!Array.prototype.Count) {
            Array.prototype.Count = function(pred) {
                if (!pred) return this.length;
                return this.filter(pred).length;
            };
        }
        if (!Array.prototype.Contains) {
            Array.prototype.Contains = function(item) { return this.indexOf(item) >= 0; };
        }
        if (!Array.prototype.OrderBy) {
            Array.prototype.OrderBy = function(keySel) {
                return this.slice().sort(function(a, b) {
                    var ka = keySel(a), kb = keySel(b);
                    return ka < kb ? -1 : (ka > kb ? 1 : 0);
                });
            };
        }
        if (!Array.prototype.OrderByDescending) {
            Array.prototype.OrderByDescending = function(keySel) {
                return this.slice().sort(function(a, b) {
                    var ka = keySel(a), kb = keySel(b);
                    return ka > kb ? -1 : (ka < kb ? 1 : 0);
                });
            };
        }
        if (!Array.prototype.Distinct) {
            Array.prototype.Distinct = function() {
                var seen = {};
                var result = [];
                for (var i = 0; i < this.length; i++) {
                    var key = JSON.stringify(this[i]);
                    if (!seen[key]) { seen[key] = true; result.push(this[i]); }
                }
                return result;
            };
        }
        if (!Array.prototype.ElementAtOrDefault) {
            Array.prototype.ElementAtOrDefault = function(i) {
                return i >= 0 && i < this.length ? this[i] : null;
            };
        }
        if (!Array.prototype.Skip) {
            Array.prototype.Skip = function(n) { return this.slice(n); };
        }
        if (!Array.prototype.Take) {
            Array.prototype.Take = function(n) { return this.slice(0, n); };
        }
        if (!Array.prototype.ToArray) {
            Array.prototype.ToArray = function() { return this.slice(); };
        }
        if (!Array.prototype.ToList) {
            Array.prototype.ToList = function() { return this.slice(); };
        }
        if (!Array.prototype.Skip) {
            Array.prototype.Skip = function(n) { return this.slice(n); };
        }
        if (!Array.prototype.ElementAt) {
            Array.prototype.ElementAt = function(i) { return this[i]; };
        }
        if (!Array.prototype.ElementAtOrDefault) {
            Array.prototype.ElementAtOrDefault = function(i, def) { return i >= 0 && i < this.length ? this[i] : def; };
        }
        if (!Array.prototype.LastOrDefault) {
            Array.prototype.LastOrDefault = function(pred) {
                if (!pred) return this.length > 0 ? this[this.length - 1] : null;
                for (var i = this.length - 1; i >= 0; i--) if (pred(this[i])) return this[i];
                return null;
            };
        }
        if (!Array.prototype.Last) {
            Array.prototype.Last = function(pred) {
                if (!pred) { if (this.length === 0) throw new Error("Sequence contains no elements"); return this[this.length - 1]; }
                for (var i = this.length - 1; i >= 0; i--) if (pred(this[i])) return this[i];
                throw new Error("Sequence contains no matching element");
            };
        }
        if (!Array.prototype.Where) {
            Array.prototype.Where = function(pred) { return this.filter(pred); };
        }
        if (!Array.prototype.Select) {
            Array.prototype.Select = function(sel) { return this.map(sel); };
        }
        if (!Array.prototype.Single) {
            Array.prototype.Single = function(pred) {
                var f = pred ? this.filter(pred) : this;
                if (f.length !== 1) throw new Error("Sequence contains 0 or more than 1 element");
                return f[0];
            };
        }
        if (!Array.prototype.SingleOrDefault) {
            Array.prototype.SingleOrDefault = function(pred) {
                var f = pred ? this.filter(pred) : this;
                return f.length === 1 ? f[0] : null;
            };
        }
        if (!Array.prototype.Distinct) {
            Array.prototype.Distinct = function() {
                var seen = []; for (var i = 0; i < this.length; i++) if (seen.indexOf(this[i]) < 0) seen.push(this[i]);
                return seen;
            };
        }
        if (!Array.prototype.Concat) {
            Array.prototype.Concat = function(other) { return this.concat(other); };
        }
        if (!Array.prototype.Contains) {
            Array.prototype.Contains = function(item) { return this.indexOf(item) >= 0; };
        }
        if (!Array.prototype.Reverse) {
            Array.prototype.Reverse = function() { return this.slice().reverse(); };
        }
        if (!Array.prototype.Last) {
            Array.prototype.Last = function(pred) {
                if (!pred) return this.length > 0 ? this[this.length - 1] : null;
                for (var i = this.length - 1; i >= 0; i--) {
                    if (pred(this[i])) return this[i];
                }
                return null;
            };
        }
        if (!Array.prototype.AddRange) {
            Array.prototype.AddRange = function(other) {
                if (other) for (var i = 0; i < other.length; i++) this.push(other[i]);
                return this;
            };
        }
        if (!Array.prototype.Add) {
            Array.prototype.Add = function(item) { this.push(item); };
        }
        // C# List<T>.Clear() → transpilé en .clear() mais Array n'a pas .clear()
        if (!Array.prototype.clear) {
            Array.prototype.clear = function() { this.length = 0; };
        }
        // C# .Clear() non transpilé (ex: if conditions) → same
        if (!Array.prototype.Clear) {
            Array.prototype.Clear = function() { this.length = 0; };
        }

        // --- Object.prototype.__subscribe / __unsubscribe (no-op générique) ---
        // Les scripts ASL font `btn.Click += handler` → transpilé en `btn.Click.__subscribe(handler)`.
        // Les objets WinForms stub (`var __o = {}`) n'ont pas de propriété `.Click` → undefined → throw.
        // Ce polyfill no-op sur Object.prototype permet à n'importe quel objet/plain object
        // de recevoir des subscriptions d'événements sans throw. Les objets qui définissent
        // leur propre __subscribe (ex: timer.OnReset) utilisent leur version.
        if (!Object.prototype.__subscribe) {
            Object.prototype.__subscribe = function(_handler) { return this; };
        }
        if (!Object.prototype.__unsubscribe) {
            Object.prototype.__unsubscribe = function(_handler) { return this; };
        }
        
        // --- Dictionary<K,V> helper (compatible C# API) ---
        // En JS on utilise Map, mais les scripts ASL utilisent .Add(k,v), .ContainsKey(k), etc.
        // On crée un wrapper qui étend Map avec les méthodes C#.
        function Dictionary() {
            var m = new Map();
            m.ContainsKey = function(k) { return m.has(k); };
            m.Add = function(k, v) { m.set(k, v); m[k] = v; };
            m.Remove = m.delete; // Map.delete retourne bool
            // [] accessor : on ajoute get/set par clé string
            return m;
        }

        // --- Map.prototype C# methods (pour new Map() direct dans le code transpilé) ---
        // Les scripts ASL font `new Dictionary<K,V>()` → transpilé en `new Map()`.
        // Map n'a pas .ContainsKey, .Add, ni [] indexer. On ajoute ces méthodes.
        if (!Map.prototype.ContainsKey) {
            Map.prototype.ContainsKey = function(k) { return this.has(k); };
        }
        if (!Map.prototype.Add) {
            Map.prototype.Add = function(k, v) { this.set(k, v); this[k] = v; };
        }
        if (!Map.prototype.Contains) {
            Map.prototype.Contains = Map.prototype.ContainsKey;
        }
        if (!Map.prototype.Remove) {
            Map.prototype.Remove = Map.prototype.delete;
        }
        // [] indexer pour Map : on surcharge set pour aussi stocker comme propriété.
        // C# Dictionary<K,V> utilise [] indexer, transpilé en Map[key].
        // Sans ça, Map[key] retourne undefined → Map[key].Add(...) throw.
        var __orig_map_set = Map.prototype.set;
        Map.prototype.set = function(k, v) { __orig_map_set.call(this, k, v); this[k] = v; return this; };
        
        // --- HashSet<T> helper ---
        function HashSet() {
            var s = new Set();
            s.Add = function(item) { var had = s.has(item); s.add(item); return !had; };
            s.Contains = s.has;
            s.Remove = s.delete;
            return s;
        }

        // --- Set.prototype C# methods (pour new Set() direct dans le code transpilé) ---
        // C# HashSet<T>.Add/Contains/Remove → JS Set n'a que add/has/delete (lowercase)
        if (!Set.prototype.Add) {
            Set.prototype.Add = function(item) { this.add(item); };
        }
        if (!Set.prototype.Contains) {
            Set.prototype.Contains = Set.prototype.has;
        }
        if (!Set.prototype.Remove) {
            Set.prototype.Remove = Set.prototype.delete;
        }
        if (!Set.prototype.Clear) {
            Set.prototype.Clear = function() { this.clear(); };
        }
        
        // --- List<T> helper ---
        function List() {
            var arr = [];
            // Accepter un argument optionnel (IEnumerable<T>) comme C# new List<T>(collection)
            if (arguments.length > 0 && arguments[0]) {
                var src = arguments[0];
                if (Array.isArray(src)) { for (var i = 0; i < src.length; i++) arr.push(src[i]); }
                else if (src && typeof src[Symbol.iterator] === 'function') { for (var item of src) arr.push(item); }
                else if (src && src.length !== undefined) { for (var i = 0; i < src.length; i++) arr.push(src[i]); }
            }
            arr.Add = function(item) { arr.push(item); };
            arr.AddRange = function(other) { if (other) for (var i = 0; i < other.length; i++) arr.push(other[i]); };
            arr.Remove = function(item) { var i = arr.indexOf(item); if (i >= 0) { arr.splice(i, 1); return true; } return false; };
            arr.RemoveAt = function(i) { arr.splice(i, 1); };
            arr.Clear = function() { arr.length = 0; };
            arr.Contains = function(item) { return arr.indexOf(item) >= 0; };
            arr.IndexOf = function(item) { return arr.indexOf(item); };
            arr.Insert = function(i, item) { arr.splice(i, 0, item); };
            arr.ToArray = function() { return arr.slice(); };
            Object.defineProperty(arr, 'Count', { get: function() { return arr.length; } });
            return arr;
        }
        
        // --- Int32, UInt32, etc. parse helpers (équivalent Int32.Parse) ---
        var Int32 = { Parse: function(s) { return parseInt(s, 10); }, TryParse: function(s, r) { var v = parseInt(s, 10); if (isNaN(v)) return false; r.val = v; return true; } };
        var UInt32 = { Parse: function(s) { return parseInt(s, 10) >>> 0; } };
        var Double = { Parse: function(s) { return parseFloat(s); } };
        var Single = { Parse: function(s) { return parseFloat(s); } };
        var Boolean = { Parse: function(s) { return s === 'true' || s === 'True'; } };
        
        // --- TimeSpan (équivalent System.TimeSpan — basique) ---
        function TimeSpan(seconds) { this.TotalSeconds = seconds || 0; }
        TimeSpan.FromSeconds = function(s) { return new TimeSpan(s); };
        TimeSpan.FromMilliseconds = function(ms) { return new TimeSpan(ms / 1000); };
        TimeSpan.prototype.TotalMilliseconds = function() { return this.TotalSeconds * 1000; };
        
        // --- Stopwatch (équivalent System.Diagnostics.Stopwatch) ---
        function Stopwatch() { this._start = 0; this._running = false; this._elapsed = 0; }
        Stopwatch.StartNew = function() { var s = new Stopwatch(); s.Start(); return s; };
        Stopwatch.prototype.Start = function() { this._start = Date.now(); this._running = true; };
        Stopwatch.prototype.Stop = function() { if (this._running) { this._elapsed += Date.now() - this._start; this._running = false; } };
        Stopwatch.prototype.Reset = function() { this._elapsed = 0; this._running = false; };
        Stopwatch.prototype.ElapsedMilliseconds = function() { return this._elapsed + (this._running ? Date.now() - this._start : 0); };
        
        // --- Environment (équivalent System.Environment) ---
        var Environment = {
            ProcessorCount: 1,
            NewLine: '\n',
            TickCount: Date.now(),
        };
        
        // --- Debug (équivalent System.Diagnostics.Debug) ---
        var Debug = { WriteLine: print, Assert: function(cond, msg) { if (!cond) print("Assert failed: " + (msg || '')); } };
        
        // --- File (équivalent System.IO.File — stubs, lecture via runtime Rust) ---
        var File = {
            Exists: function(path) { if (typeof __file_exists === 'function') return __file_exists(path); return false; },
            ReadAllText: function(path) { if (typeof __file_read === 'function') return __file_read(path); return ""; },
            ReadAllLines: function(path) { if (typeof __file_read === 'function') return __file_read(path).split('\n'); return []; },
        };
        
        // --- Path (équivalent System.IO.Path) ---
        var Path = {
            Combine: function() { return Array.prototype.slice.call(arguments).join('/').replace(/\/+/g, '/'); },
            GetFileName: function(p) { var i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\')); return i >= 0 ? p.substr(i + 1) : p; },
            GetDirectoryName: function(p) { var i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\')); return i >= 0 ? p.substr(0, i) : ''; },
            GetExtension: function(p) { var i = p.lastIndexOf('.'); return i >= 0 ? p.substr(i) : ''; },
            GetFileNameWithoutExtension: function(p) { var f = Path.GetFileName(p); var i = f.lastIndexOf('.'); return i >= 0 ? f.substr(0, i) : f; },
        };

        // --- Directory (équivalent System.IO.Directory) ---
        var Directory = {
            Exists: function(path) { if (typeof __dir_exists === 'function') return __dir_exists(path); return false; },
            CreateDirectory: function(path) { if (typeof __dir_create === 'function') return __dir_create(path); },
        };

        // --- Date extensions (pour compatibilité C# DateTime) ---
        Date.prototype.equals = function(other) {
            if (other instanceof Date) return this.getTime() === other.getTime();
            return this.getTime() === other;
        };
        Date.prototype.addMilliseconds = function(ms) { return new Date(this.getTime() + ms); };
        Date.prototype.addSeconds = function(s) { return new Date(this.getTime() + s * 1000); };
        Date.prototype.addMinutes = function(m) { return new Date(this.getTime() + m * 60000); };
        Date.prototype.addHours = function(h) { return new Date(this.getTime() + h * 3600000); };
        // E8 Bug A — toLocaleTimeString/toLocaleDateString : Boa définit ces
        // méthodes mais throw "Function Unimplemented" à l'appel. On override
        // avec un format simple (HH:MM:SS) pour que F.Debug() ne throw pas.
        Date.prototype.toLocaleTimeString = function() {
            var h = this.getHours(), m = this.getMinutes(), s = this.getSeconds();
            return (h < 10 ? "0" : "") + h + ":" + (m < 10 ? "0" : "") + m + ":" + (s < 10 ? "0" : "") + s;
        };
        Date.prototype.toLocaleDateString = function() {
            return this.getFullYear() + "-" + (this.getMonth()+1) + "-" + this.getDate();
        };
        // .Seconds (propriété, pas méthode) → on simule avec un getter
        Object.defineProperty(Date.prototype, 'Seconds', {
            get: function() { return this.getSeconds(); }
        });
        Date.prototype.ToString = function(fmt) {
            if (fmt === 'T' || fmt === 't') return this.toLocaleTimeString();
            return this.toString();
        };

        // --- String extensions (pour compatibilité C# string.Equals) ---
        String.prototype.equals = function(other) { return this.toString() === String(other); };
        String.prototype.Equals = String.prototype.equals;

        // --- Set extensions (pour compatibilité C# HashSet.UnionWith) ---
        Set.prototype.UnionWith = function(other) {
            if (other) { var it = other.values ? other.values() : other; for (var v of it) this.add(v); }
        };
        Set.prototype.ExceptWith = function(other) {
            if (other) { var it = other.values ? other.values() : other; for (var v of it) this.delete(v); }
        };
        Set.prototype.IntersectWith = function(other) {
            if (other) { var toRemove = []; var it = this.values(); for (var v of it) { if (!other.has(v)) toRemove.push(v); } for (var v of toRemove) this.delete(v); }
        };

        // --- Map extensions (pour compatibilité C# Dictionary) ---
        Map.prototype.Clear = Map.prototype.clear;
        Map.prototype.ContainsKey = Map.prototype.has;
        Map.prototype.Add = function(k, v) { this.set(k, v); };
        Map.prototype.TryGetValue = function(k, outHolder) {
            if (this.has(k)) { if (outHolder) outHolder.val = this.get(k); return true; }
            return false;
        };

        // --- Object.__subscribe / __unsubscribe (pour events C# += / -=) ---
        Object.prototype.__subscribe = function(handler) {
            var key = '__event_handlers';
            if (!this[key]) this[key] = [];
            this[key].push(handler);
        };
        Object.prototype.__unsubscribe = function(handler) {
            var key = '__event_handlers';
            if (!this[key]) return;
            var idx = this[key].indexOf(handler);
            if (idx >= 0) this[key].splice(idx, 1);
        };

        // --- __StreamWriterStub (pour using(new StreamWriter(...))) ---
        function __StreamWriterStub(file, append) {
            this._file = file;
            this._append = append;
            this._content = '';
        }
        __StreamWriterStub.prototype.WriteLine = function(line) { this._content += line + '\n'; };
        __StreamWriterStub.prototype.Write = function(text) { this._content += text; };
        __StreamWriterStub.prototype.Close = function() { if (typeof __file_write === 'function') __file_write(this._file, this._content, this._append); };
        __StreamWriterStub.prototype.Dispose = __StreamWriterStub.prototype.Close;

        // --- Number extensions (pour compatibilité C# casts + Equals) ---
        Number.prototype.equals = function(other) { return this.valueOf() === Number(other); };
        // .Equals (PascalCase C#) — ex: Prog.Current.Equals(1) où Current est un number
        try { Number.prototype.Equals = Number.prototype.equals; } catch(e) {}
        // .ToString (PascalCase C#) — ex: CurProg.ToString() dans F.SetStateCodes (MGS).
        // Le transpileur convertit .ToString( → ?.ToString(, mais JS n'a que .toString()
        // (lowercase). Sans ce shim, CurProg?.ToString() throw "not a function" →
        // F.SetStateCodes échoue → split() ne génère jamais de validCodes → aucun split.
        try { Number.prototype.ToString = Number.prototype.toString; } catch(e) {}

        // --- String extensions (pour compatibilité C# string.Equals + ToString) ---
        // .ToString (PascalCase C#) — ex: entry.EntryType.ToString().Equals("Error")
        try { String.prototype.ToString = String.prototype.toString; } catch(e) {}

        // --- Boolean extensions ---
        // NB: Boa peut rejeter l'assignation directe Boolean.prototype.X = ...
        // (TypeError: cannot convert 'null' or 'undefined' to object). On wrap
        // dans un try/catch pour ne pas aborter le reste de l'API.
        try {
            Boolean.prototype.equals = function(other) { return this.valueOf() === Boolean(other); };
            Boolean.prototype.Equals = Boolean.prototype.equals;
            Boolean.prototype.ToString = Boolean.prototype.toString;
        } catch(e) {}

        // --- atob (pour Convert.FromBase64String) ---
        if (typeof atob === 'undefined') {
            globalThis.atob = function(b64) {
                // Stub simple — Boa peut ne pas avoir atob natif
                // Retourne une string décodée approximative
                return b64;
            };
        }

        // === E7 T1 shims (moteur générique §2) ===

        // --- Settings LiveSplit (objet permissif générique) ---
        // E7 T1 — moteur générique (§2) : pas de table par jeu.
        // settings["key"] retourne la valeur stockée (bool), ou false si absente.
        // settings.Add(key, val, desc) stocke la clé. SetToolTip/CurrentDefaultParent = stubs.
        // settings.Reader conserve la compat avec le wrapper existant (start/split/reset).
        function __creerSettings(reader) {
            var _vals = {};
            var _descs = {};   // descriptions (3e arg de Add)
            var _parents = {};  // parent (CurrentDefaultParent au moment du Add)
            var _order = [];    // ordre d'insertion (pour affichage stable)
            var s = {
                Reader: reader || { start: false, split: false, reset: false },
                CurrentDefaultParent: null,
                Add: function(key, val, desc) {
                    _vals[key] = val; s[key] = val;
                    if (desc) _descs[key] = desc;
                    if (s.CurrentDefaultParent) _parents[key] = s.CurrentDefaultParent;
                    if (_order.indexOf(key) === -1) _order.push(key);
                },
                SetToolTip: function(key, tip) {},
                ContainsKey: function(key) { return Object.prototype.hasOwnProperty.call(_vals, key); },
                _vals: _vals,
                _descs: _descs,
                _parents: _parents,
                _order: _order
            };
            // Indexeur permissif via Proxy si disponible, sinon propriétés directes
            // (Add pose s[key] = val, donc settings["key"] marche pour les clés déclarées).
            // Pour les clés non déclarées, Proxy retourne false ; sans Proxy, undefined.
            try {
                return new Proxy(s, {
                    get: function(target, prop) {
                        if (prop in target) return target[prop];
                        if (typeof prop === 'string') return _vals[prop] !== undefined ? _vals[prop] : false;
                        return undefined;
                    },
                    set: function(target, prop, value) {
                        if (prop in target) { target[prop] = value; return true; }
                        _vals[prop] = value; s[prop] = value;
                        return true;
                    },
                    has: function(target, prop) { return (prop in target) || (_vals[prop] !== undefined); }
                });
            } catch(e) {
                return s;
            }
        }

        // --- TimerModel (équivalent LiveSplit.Model.TimerModel) ---
        // Shim léger : les actions split/undo/skip réelles sont gérées côté Rust
        // via le retour des méthodes ASL (start/split/reset). Le shim expose
        // CurrentState (= timer) et des méthodes no-op pour que le script ne crash pas.
        function TimerModel() {
            this.CurrentState = null;
        }
        TimerModel.prototype.Split = function() {};
        TimerModel.prototype.UndoSplit = function() {};
        TimerModel.prototype.SkipSplit = function() {};
        TimerModel.prototype.Reset = function() {};
        TimerModel.prototype.Pause = function() {};

        // --- Application (équivalent System.Windows.Forms.Application) ---
        // Stub : ExecutablePath retourne un chemin neutre.
        var Application = {
            ExecutablePath: './StreamOS.exe',
        };

        // --- Environment.SpecialFolder (équivalent System.Environment.SpecialFolder) ---
        var __localAppData = './appdata';
        var Environment = {
            ProcessorCount: 1,
            NewLine: '\n',
            TickCount: Date.now(),
            GetFolderPath: function(special) {
                if (typeof __local_appdata === 'function') return __local_appdata();
                return __localAppData;
            },
            SpecialFolder: {
                LocalApplicationData: 'LocalApplicationData',
                ApplicationData: 'ApplicationData',
                CommonApplicationData: 'CommonApplicationData',
            },
        };

        // --- System.Text.Encoding (équivalent System.Text.Encoding) ---
        // GetString(bytes) décode un array d'octets en string UTF-8.
        var Encoding = {
            UTF8: {
                GetString: function(bytes) {
                    if (!bytes) return '';
                    var s = '';
                    for (var i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i] & 0xFF);
                    try { return decodeURIComponent(escape(s)); } catch(e) { return s; }
                },
                GetBytes: function(str) {
                    var arr = [];
                    for (var i = 0; i < str.length; i++) {
                        var c = str.charCodeAt(i);
                        if (c < 0x80) arr.push(c);
                        else if (c < 0x800) { arr.push(0xC0 | (c >> 6)); arr.push(0x80 | (c & 0x3F)); }
                        else { arr.push(0xE0 | (c >> 12)); arr.push(0x80 | ((c >> 6) & 0x3F)); arr.push(0x80 | (c & 0x3F)); }
                    }
                    return arr;
                },
            },
            ASCII: {
                GetString: function(bytes) {
                    if (!bytes) return '';
                    var s = '';
                    for (var i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i] & 0x7F);
                    return s;
                },
            },
        };

        // --- MemoryStream (équivalent System.IO.MemoryStream) ---
        // Stub léger : stocke un buffer, expose Read/Write/ToArray/Close.
        function MemoryStream(buffer) {
            this._buf = buffer ? Array.prototype.slice.call(buffer) : [];
            this._pos = 0;
        }
        MemoryStream.prototype.Write = function(data, offset, count) {
            if (!data) return;
            var arr = (data instanceof Array) ? data : Array.prototype.slice.call(data);
            for (var i = 0; i < (count || arr.length); i++) {
                this._buf[this._pos + i] = arr[offset + i];
            }
            this._pos += (count || arr.length);
        };
        MemoryStream.prototype.Read = function(buffer, offset, count) {
            var n = Math.min(count, this._buf.length - this._pos);
            for (var i = 0; i < n; i++) buffer[offset + i] = this._buf[this._pos + i];
            this._pos += n;
            return n;
        };
        MemoryStream.prototype.ToArray = function() { return this._buf.slice(); };
        MemoryStream.prototype.Close = function() {};
        MemoryStream.prototype.Dispose = MemoryStream.prototype.Close;

        // --- WinForms étendus (stubs permissifs pour éviter crash init) ---
        // Le chemin critique auto-split ne touche pas WinForms, mais init les
        // référence (DebugForm, Controls, AppendText...). Stubs légers.
        // Events WinForms communs — initialisés comme objets vides avec __subscribe
        // no-op (hérité de Object.prototype.__subscribe).
        var __WINFORM_EVENTS = ['Click', 'Closed', 'Closing', 'Load', 'Shown',
            'FormClosing', 'FormClosed', 'SelectedIndexChanged', 'TextChanged',
            'CheckedChanged', 'KeyPress', 'KeyDown', 'KeyUp', 'MouseClick',
            'MouseDown', 'MouseUp', 'MouseMove', 'Resize', 'Activated',
            'Deactivate', 'Enter', 'Leave', 'Validated', 'Validating'];
        function __winform_stub() {
            this.Controls = [];
            this.Controls.Add = function(c) { this.push(c); };
            this.Text = '';
            this.Size = [0, 0];
            // Initialiser tous les events communs comme objets vides
            for (var i = 0; i < __WINFORM_EVENTS.length; i++) {
                this[__WINFORM_EVENTS[i]] = {};
            }
        }
        __winform_stub.prototype.AppendText = function(t) {};
        __winform_stub.prototype.Dispose = function() {};
        __winform_stub.prototype.Close = function() {};
        __winform_stub.prototype.Show = function() {};
        __winform_stub.prototype.Hide = function() {};
        __winform_stub.prototype.ShowDialog = function() { return 0; };
        // PictureBox hérite du stub + ajoute Image
        function PictureBox() { __winform_stub.call(this); this.Image = null; }
        PictureBox.prototype = Object.create(__winform_stub.prototype);
        PictureBox.prototype.constructor = PictureBox;
        // Button hérite + Click event (déjà initialisé par __winform_stub)
        function Button() { __winform_stub.call(this); }
        Button.prototype = Object.create(__winform_stub.prototype);
        Button.prototype.constructor = Button;
        // Label, CheckBox, ListBox, ComboBox, TextBox, FolderBrowserDialog
        function Label() { __winform_stub.call(this); }
        Label.prototype = Object.create(__winform_stub.prototype);
        Label.prototype.constructor = Label;
        function CheckBox() { __winform_stub.call(this); this.Checked = false; }
        CheckBox.prototype = Object.create(__winform_stub.prototype);
        CheckBox.prototype.constructor = CheckBox;
        function ListBox() { __winform_stub.call(this); this.Items = []; this.Items.Add = function(i) { this.push(i); }; this.SelectedIndex = -1; }
        ListBox.prototype = Object.create(__winform_stub.prototype);
        ListBox.prototype.constructor = ListBox;
        function ComboBox() { __winform_stub.call(this); this.Items = []; this.Items.Add = function(i) { this.push(i); }; this.SelectedIndex = -1; }
        ComboBox.prototype = Object.create(__winform_stub.prototype);
        ComboBox.prototype.constructor = ComboBox;
        function TextBox() { __winform_stub.call(this); this.Multiline = false; }
        TextBox.prototype = Object.create(__winform_stub.prototype);
        TextBox.prototype.constructor = TextBox;
        function FolderBrowserDialog() { __winform_stub.call(this); this.SelectedPath = ''; }
        FolderBrowserDialog.prototype = Object.create(__winform_stub.prototype);
        FolderBrowserDialog.prototype.constructor = FolderBrowserDialog;
        // Form (si non déjà défini plus haut)
        if (typeof Form === 'undefined') {
            var Form = function() { __winform_stub.call(this); };
            Form.prototype = Object.create(__winform_stub.prototype);
            Form.prototype.constructor = Form;
        }
        // DockStyle enum stub
        var DockStyle = { Fill: 'Fill', Top: 'Top', Bottom: 'Bottom', Left: 'Left', Right: 'Right' };

        // --- Stubs WinForms supplémentaires (constructeurs manquants) ---
        // Ces constructeurs sont référencés par les scripts ASL dans startup/init
        // pour construire des UI de debug/toolbox. Côté StreamOS, aucun UI n'est
        // affiché : stubs no-op qui héritent de __winform_stub.
        function TableLayoutPanel() { __winform_stub.call(this); }
        TableLayoutPanel.prototype = Object.create(__winform_stub.prototype);
        TableLayoutPanel.prototype.constructor = TableLayoutPanel;

        function Padding(p) { this.All = p || 0; this.Left = p || 0; this.Top = p || 0; this.Right = p || 0; this.Bottom = p || 0; }

        var PictureBoxSizeMode = { Normal: 0, StretchImage: 1, AutoSize: 2, CenterImage: 3, Zoom: 4 };
        var FormBorderStyle = { None: 0, FixedSingle: 1, Fixed3D: 2, FixedDialog: 3, Sizable: 4, FixedToolWindow: 5, SizableToolWindow: 6 };

        // MessageBox, MessageBoxButtons, MessageBoxIcon (stubs no-op)
        var MessageBox = { Show: function(msg, title, buttons, icon) {} };
        var MessageBoxButtons = { OK: 0, OKCancel: 1, YesNo: 4, YesNoCancel: 3 };
        var MessageBoxIcon = { None: 0, Information: 1, Warning: 2, Error: 3, Question: 4 };

        // ProcessStartInfo + Process (stubs pour F.OpenExplorer)
        function ProcessStartInfo() { this.FileName = ''; this.Arguments = ''; }
        var Process = { Start: function(info) {} };

        // --- EventLog (équivalent System.Diagnostics.EventLog — stub générique) ---
        // Les scripts ASL font `new EventLog("Application")` puis s'abonnent à
        // EntryWritten via __subscribe (transpilé de `+=`). Aucun event réel n'est
        // levé côté StreamOS : stub no-op.
        function EventLog(source) {
            this.Source = source || '';
            this.EnableRaisingEvents = false;
            this.EntryWritten = {};
        }
        EventLog.prototype.Close = function() {};
        EventLog.prototype.Dispose = function() {};

        // --- Process.Memory (mem) — accès lecture bytes depuis JS ---
        // mem.ReadBytes(addr, count) → array d'octets via __rust_deref (type 'bytes').
        // Exposé via game.ReadBytes (le wrapper méthode pose var memory = game).
        function __mem_read_bytes(addr, count) {
            if (typeof __rust_deref === 'function') {
                var base = (typeof addr === 'number') ? addr : (addr && addr.base ? addr.base : 0);
                var offsets = (addr && addr.offsets) ? addr.offsets : [];
                var module = (addr && addr.module) ? addr.module : null;
                var raw = __rust_deref(module, base, offsets, 'bytes', count);
                return raw || new Array(count).fill(0);
            }
            return new Array(count).fill(0);
        }

        // --- System namespace (équivalent System.* — agrège les stubs existants) ---
        // MGS et autres scripts ASL référencent System.IO.Path, System.Text.Encoding,
        // System.Drawing.Point, etc. On expose un objet System qui pointe vers les
        // variables déjà définies (File, Path, Directory, Encoding, Debug, etc.).
        var System = {
            IO: { File: File, Path: Path, Directory: Directory },
            Text: { Encoding: Encoding },
            Diagnostics: { Debug: Debug, Stopwatch: typeof Stopwatch !== 'undefined' ? Stopwatch : {} },
            Drawing: {
                Point: function(x, y) { this.X = x; this.Y = y; },
                Size: function(w, h) { this.Width = w; this.Height = h; },
                Color: { FromArgb: function(a, r, g, b) {
                    // FromArgb(int) ou FromArgb(a, r, g, b) ou FromArgb(r, g, b)
                    if (arguments.length === 1) { r = (a >> 16) & 0xFF; g = (a >> 8) & 0xFF; b = a & 0xFF; a = 0xFF; }
                    else if (arguments.length === 3) { b = g; g = r; r = a; a = 0xFF; }
                    return { A: a, R: r, G: g, B: b, ToString: function() { return '#' + [r,g,b].map(function(v){return ('0'+v.toString(16)).slice(-2);}).join(''); } };
                }},
                Font: function(family, size) { this.FontFamily = family; this.Size = size; this.Name = family; },
                Bitmap: function(stream) { this._stream = stream; },
                ContentAlignment: { BottomCenter: 0x200, BottomLeft: 0x100, BottomRight: 0x400, MiddleCenter: 0x20, MiddleLeft: 0x10, MiddleRight: 0x40, TopCenter: 0x2, TopLeft: 0x1, TopRight: 0x4 },
                FontStyle: { Regular: 0x0, Bold: 0x1, Italic: 0x2, Underline: 0x4, Strikeout: 0x8 },
            },
            Windows: { Forms: {
                ScrollBars: { Vertical: 0x2, Horizontal: 0x1, Both: 0x3, None: 0x0 },
            }},
        };

        print("[ASL Runtime] API de compatibilité chargée");
        globalThis.__asl_api_loaded = true;
        "#;

        if let Err(e) = ctx.eval(Source::from_bytes(api)) {
            log::warn!("[ASL] Erreur initialisation API JS: {}", e);
        }

        // --- Enregistrer les callbacks natifs Rust (pont mémoire JS ↔ Rust) ---
        // Ces fonctions sont appelées par le JS de l'API ASL (DeepPointer.Deref,
        // MemoryWatcher.Update, SignatureScanner.Scan, File.Exists, etc.).
        // L'état du processus jeu est dans un thread_local (pont_memoire.rs).
        enregistrer_callbacks_natifs(&mut ctx);

        // --- vars vivant en mémoire Boa (E8 Bug A) ---
        // globalThis.__asl_vars est l'objet `vars` partagé entre tous les appels
        // de méthodes ASL (startup/init/update/start/split/...). Avant ce fix,
        // vars était sérialisé en JSON (self.vars_json) puis re-JSON.parse à
        // chaque appel → les fonctions et instances (vars.D.Funcs, vars.D.Mem
        // MemoryWatcherList) étaient détruites après startup. L'objet JS vit
        // maintenant dans le Context Boa et est muté en place par le script.
        let _ = ctx.eval(Source::from_bytes("globalThis.__asl_vars = {};"));

        // --- settings vivant en mémoire Boa (LOT 1 — splits bloqués) ---
        // globalThis.__asl_settings est l'objet `settings` partagé entre tous
        // les appels de méthodes ASL. Avant ce fix, settings était recréé à
        // chaque appel depuis {start,split,reset} → les codes split enregistrés
        // pendant init via settings.Add("OL-s00a", true, ...) étaient perdus →
        // F.SettingEnabled(code) = settings.ContainsKey(code) && settings[code]
        // retournait false → F.Split(code) retournait false → aucun split.
        // Même anti-pattern que le fix vars E8 Bug A. L'objet est créé paresse
        // au premier appel (call_method) via __creerSettings, puis muté en place
        // par le script (settings.Add pendant init). Le Reader {start,split,
        // reset} est rafraîchi à chaque appel (l'utilisateur peut toggler en
        // cours de run via majSettings).
        let _ = ctx.eval(Source::from_bytes("globalThis.__asl_settings = null;"));

        Self {
            ctx,
            vars_json: "{}".to_string(),
            methods_compiled: std::collections::HashSet::new(),
            current_pid: None,
        }
    }

    /// Compile une méthode ASL (code JS transpilé) en fonction JS globale.
    pub fn compile_method(&mut self, name: &str, js_code: &str) -> Result<(), String> {
        let wrapped = format!(
            r#"
            globalThis.__asl_{name} = function(timer, old, current, vars, game, settings) {{
                var refreshRate = 1000/15;
                var version = "";
                var memory = game;
                var modules = game ? game.modules : null;
                // E7 T1 — accesseurs PascalCase sur game (compat C# LiveSplit)
                // game.Modules / game.Process / game.Id / game.ProcessName / game.Memory
                if (game) {{
                    if (game.modules && !game.Modules) game.Modules = game.modules;
                    if (!game.Process) game.Process = game;
                    if (game.id && !game.Id) game.Id = game.id;
                    // Auto-start fix — strip .exe de ProcessName côté JS.
                    // Rust (Process::find_by_name) matche "mgsi" contre "mgsi.exe"
                    // en strippant .exe, mais garde proc.name = "mgsi.exe" pour le
                    // log. serialize_process envoie donc processName = "mgsi.exe".
                    // Les scripts ASL font switch (processName) case "mgsi" ...
                    // (sans .exe) → sans ce strip, le case ne matche pas →
                    // G.BaseAddress reste IntPtr.Zero → update() early-return →
                    // M.UpdateAll jamais appelé → start() retourne false à vie.
                    if (game.processName && !game.ProcessName) game.ProcessName = game.processName.replace(/\.exe$/i, '');
                    if (!game.Memory) game.Memory = (typeof MemoryWatcherList === 'function') ? new MemoryWatcherList() : {{}};
                    // mem = game (le script utilise mem.ReadBytes via __mem_read_bytes)
                    if (!game.ReadBytes) game.ReadBytes = __mem_read_bytes;
                }}
                // E7 — events LiveSplit standard sur timer (API LiveSplit générique).
                // Les scripts font timer.OnReset.__subscribe(handler) (transpilé de `+=`) :
                // on pose des objets vides pour que l'abonnement soit un no-op propre.
                if (timer) {{
                    var __timer_evts = ['OnStart', 'OnSplit', 'OnReset', 'OnPause', 'OnResume', 'OnUndoSplit', 'OnSkipSplit'];
                    for (var __ti = 0; __ti < __timer_evts.length; __ti++) {{
                        if (!timer[__timer_evts[__ti]]) timer[__timer_evts[__ti]] = {{}};
                    }}
                }}
                globalThis.__asl_last_error = null;
                try {{
                    {code}
                }} catch(e) {{
                    globalThis.__asl_last_error = "{name}: " + e.message;
                    // E8 Bug A — ne plus print ici (spam 15Hz). Le log throttle
                    // est géré côté Rust (asl_engine.rs : "Init échoué (retry)"
                    // tous les ~15 ticks via globalThis.__asl_last_error).
                    return null;
                }}
                return null;
            }};
            "#,
            name = name,
            code = js_code
        );

        match self.ctx.eval(Source::from_bytes(&wrapped)) {
            Ok(_) => {
                self.methods_compiled.insert(name.to_string());
                log::debug!("[ASL] Méthode '{}' compilée", name);
                Ok(())
            }
            Err(e) => {
                log::warn!("[ASL] Méthode '{}' non compilée: {}", name, e);
                Err(format!("Compilation {}: {}", name, e))
            }
        }
    }

    /// Appelle une méthode ASL. Retourne le résultat sous forme de string JSON.
    pub fn call_method(
        &mut self,
        name: &str,
        timer_json: &str,
        old_json: &str,
        current_json: &str,
        process_json: Option<&str>,
        settings: &AslSettings,
    ) -> Result<String, String> {
        if !self.methods_compiled.contains(name) {
            return Ok("null".to_string());
        }

        let proc_arg = process_json.unwrap_or("null");
        let settings_json = format!(
            r#"{{"start":{},"split":{},"reset":{}}}"#,
            settings.start, settings.split, settings.reset
        );

        // Échapper les single quotes dans les JSON (les strings peuvent
        // contenir des ' — on les remplace par \x27 pour rester valide en JS).
        let esc_timer = echapper_single_quote(timer_json);
        let esc_old = echapper_single_quote(old_json);
        let esc_current = echapper_single_quote(current_json);
        let esc_settings = echapper_single_quote(&settings_json);

        // Le callback natif __rust_deref est enregistré une fois pour toutes
        // dans ScriptContext::new(). Le wrapper JS __deref_callback délègue
        // directement (pas de JSON.parse — __rust_deref retourne une valeur JS).
        // __file_exists / __file_read sont aussi natifs (enregistrés à l'init).
        let deref_setup = if self.current_pid.is_some() {
            r#"
            globalThis.__deref_callback = function(module, base, offsets, type, game, extra) {
                return __rust_deref(module, base, offsets, type, extra || 0);
            };
            "#.to_string()
        } else {
            "globalThis.__deref_callback = null;".to_string()
        };

        // E8 Bug A — vars vivant en mémoire Boa (globalThis.__asl_vars).
        // Avant ce fix, vars était sérialisé en JSON (self.vars_json) puis
        // re-JSON.parse à chaque appel → les fonctions et instances stockées
        // dans vars (ex: vars.D.Funcs, vars.D.Mem MemoryWatcherList du script
        // MGS) étaient détruites après startup. On passe maintenant la référence
        // directe à globalThis.__asl_vars ; le script la mute en place.
        //
        // LOT 1 — splits bloqués : même fix pour settings (globalThis.__asl_settings).
        // Avant ce fix, settings était recréé à chaque appel depuis {start,split,
        // reset} → les codes split enregistrés pendant init via settings.Add(...)
        // étaient perdus → F.SettingEnabled(code) retournait false → aucun split.
        // L'objet settings est créé paresse au premier appel (via __creerSettings),
        // puis muté en place par le script (settings.Add pendant init). Le Reader
        // {start,split,reset} est rafraîchi à chaque appel (l'utilisateur peut
        // toggler en cours de run via majSettings).
        let call = format!(
            r#"
            (function() {{
                {deref_setup}
                var __timer = JSON.parse('{esc_timer}');
                var __old = JSON.parse('{esc_old}');
                var __current = JSON.parse('{esc_current}');
                var __vars = globalThis.__asl_vars;
                var __game = {proc_arg};
                var __reader = JSON.parse('{esc_settings}');
                // LOT 1 — settings persistant (même pattern que __asl_vars).
                // Création paresse au premier appel ; réutilisation ensuite.
                // Le Reader est rafraîchi à chaque appel (start/split/reset
                // booléens peuvent changer en cours de run via majSettings).
                if (!globalThis.__asl_settings) {{
                    globalThis.__asl_settings = (typeof __creerSettings === 'function')
                        ? __creerSettings(__reader)
                        : {{ Reader: __reader }};
                }} else {{
                    globalThis.__asl_settings.Reader = __reader;
                }}
                var __settings = globalThis.__asl_settings;
                var __result = globalThis.__asl_{name}(__timer, __old, __current, __vars, __game, __settings);
                // vars et settings sont mutés en place dans globalThis — pas de
                // re-sérialisation (les fonctions/instances ne survivent pas au
                // round-trip JSON). On ne renvoie que le résultat.
                return JSON.stringify({{ result: __result }});
            }})()
            "#,
            deref_setup = deref_setup,
            esc_timer = esc_timer,
            esc_old = esc_old,
            esc_current = esc_current,
            proc_arg = proc_arg,
            esc_settings = esc_settings,
            name = name,
        );

        match self.ctx.eval(Source::from_bytes(&call)) {
            Ok(val) => {
                let result_str = val
                    .as_string()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|| "null".to_string());

                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&result_str) {
                    if let Some(vars) = parsed.get("vars").and_then(|v| v.as_str()) {
                        self.vars_json = vars.to_string();
                    }
                    if let Some(result) = parsed.get("result") {
                        return Ok(result.to_string());
                    }
                }
                Ok("null".to_string())
            }
            Err(e) => {
                log::warn!("[ASL] Erreur exécution '{}': {}", name, e);
                Err(format!("Exécution {}: {}", name, e))
            }
        }
    }

    /// Exécute la méthode startup (sans processus).
    pub fn run_startup(&mut self, settings: &AslSettings) -> Result<(), String> {
        if !self.methods_compiled.contains("startup") {
            return Ok(());
        }
        let result = self.call_method("startup", "{}", "{}", "{}", None, settings)?;
        log::debug!("[ASL] Startup result: {}", result);
        Ok(())
    }

    /// Retourne true si une méthode est compilée.
    pub fn has_method(&self, name: &str) -> bool {
        self.methods_compiled.contains(name)
    }

    /// Définit le PID courant (pour les lectures mémoire depuis JS).
    pub fn set_pid(&mut self, pid: Option<u32>) {
        self.current_pid = pid;
    }

    /// Réinitialise l'état script persistant dans le Context Boa
    /// (globalThis.__asl_vars + globalThis.__asl_settings). À appeler au
    /// chargement d'un nouveau script ASL (handle_load_asl) pour éviter
    /// qu'un script précédent ne laisse des vars/settings stale.
    /// LOT 1 — splits bloqués : reset settings en plus de vars.
    pub fn reset_etat_script(&mut self) {
        let _ = self.ctx.eval(Source::from_bytes(
            "globalThis.__asl_vars = {}; globalThis.__asl_settings = null;",
        ));
    }

    /// Sérialise globalThis.__asl_vars en JSON (lecture-only). Utilisé par les
    /// tests pour vérifier l'état des vars après un appel de méthode. En prod,
    /// les vars vivent dans le Context Boa et ne sont jamais sérialisés (E8
    /// Bug A — les fonctions/instances ne survivent pas au round-trip JSON).
    pub fn lire_vars_json(&mut self) -> String {
        match self.ctx.eval(Source::from_bytes(
            "JSON.stringify(globalThis.__asl_vars || {})",
        )) {
            Ok(val) => val
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_else(|| "{}".to_string()),
            Err(_) => "{}".to_string(),
        }
    }

    /// Lit les settings ASL individuels (ceux déclarés via settings.Add
    /// pendant startup/init) depuis globalThis.__asl_settings. Retourne un
    /// Vec<SettingDetail> avec id, label (description), value, parent.
    ///
    /// Ces settings sont les 120+ cases à cocher individuelles de l'ASL MGS
    /// (ex: "OL-s00a" → "Dock", "OL-s01a.CL-s02a.CP-18" → "Heliport"). Sans
    /// cette fonction, l'utilisateur ne peut ni voir ni configurer quels splits
    /// sont activés → le timer démarre mais aucun split ne se déclenche.
    ///
    /// Doit être appelé APRÈS run_startup() (qui exécute settings.Add).
    pub fn lire_settings_asl(&mut self) -> Vec<SettingDetail> {
        let js = r#"
            (function() {
                var s = globalThis.__asl_settings;
                if (!s || !s._vals) return JSON.stringify([]);
                var order = s._order || [];
                var vals = s._vals;
                var descs = s._descs || {};
                var parents = s._parents || {};
                var out = [];
                for (var i = 0; i < order.length; i++) {
                    var k = order[i];
                    out.push({
                        id: k,
                        label: descs[k] || k,
                        value: !!vals[k],
                        parent: parents[k] || null
                    });
                }
                return JSON.stringify(out);
            })()
        "#;
        match self.ctx.eval(Source::from_bytes(js)) {
            Ok(val) => {
                let json = val
                    .as_string()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|| "[]".to_string());
                serde_json::from_str::<Vec<SettingDetail>>(&json).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Lit le dictionnaire D.Names.Split depuis globalThis.__asl_vars.
    /// Ce dictionnaire mappe les codes-signature (ex: "OL-s00a") vers les
    /// noms lisibles (ex: "Dock"). Utilisé par la modale de config pour
    /// auto-mapper les noms de segments LSS vers les codes settings.
    ///
    /// Format retourné : Vec<(code, nom)> (ex: [("OL-s00a", "Dock"), ...]).
    /// Retourne Vec vide si D.Names.Split n'existe pas (ASL simple type SOR).
    pub fn lire_noms_splits(&mut self) -> Vec<(String, String)> {
        let js = r#"
            (function() {
                var v = globalThis.__asl_vars;
                if (!v || !v.D || !v.D.Names || !v.D.Names.Split) return JSON.stringify([]);
                var out = [];
                for (var k in v.D.Names.Split) {
                    if (Object.prototype.hasOwnProperty.call(v.D.Names.Split, k)) {
                        out.push([k, v.D.Names.Split[k]]);
                    }
                }
                return JSON.stringify(out);
            })()
        "#;
        match self.ctx.eval(Source::from_bytes(js)) {
            Ok(val) => {
                let json = val
                    .as_string()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_else(|| "[]".to_string());
                serde_json::from_str::<Vec<(String, String)>>(&json).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    /// Met à jour les valeurs des settings ASL individuels dans
    /// globalThis.__asl_settings._vals. Appelé quand l'utilisateur coche/
    /// décoche des splits dans la modale de config. Les valeurs sont appliquées
    /// en place dans le Context Boa → F.SettingEnabled(code) les verra au
    /// prochain appel de split().
    pub fn maj_settings_asl(&mut self, valeurs: &std::collections::HashMap<String, bool>) {
        if valeurs.is_empty() {
            return;
        }
        // Sérialiser la map en JSON pour injection dans JS.
        let json = match serde_json::to_string(valeurs) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ASL] maj_settings_asl: serialize map: {}", e);
                return;
            }
        };
        let js = format!(
            r#"
            (function() {{
                var s = globalThis.__asl_settings;
                if (!s || !s._vals) return;
                var updates = JSON.parse('{json}');
                for (var k in updates) {{
                    if (Object.prototype.hasOwnProperty.call(updates, k)) {{
                        s._vals[k] = updates[k];
                        s[k] = updates[k];
                    }}
                }}
            }})()
            "#,
            json = json.replace('\\', "\\\\").replace('\'', "\\x27"),
        );
        if let Err(e) = self.ctx.eval(Source::from_bytes(&js)) {
            log::warn!("[ASL] maj_settings_asl: eval: {}", e);
        }
    }

    /// Restaure des settings ASL sauvegardés (depuis speedrun.json) dans le
    /// Context Boa. Appelé au chargement d'un ASL si l'utilisateur a déjà
    /// configuré les splits une fois. Évite de réouvrir la modale à chaque
    /// boot. Les valeurs absentes de la map sont ignorées (l'ASL peut avoir
    /// changé de version et avoir de nouveaux settings).
    pub fn restaurer_settings_asl(&mut self, valeurs: &std::collections::HashMap<String, bool>) {
        self.maj_settings_asl(valeurs);
    }

    /// Retourne la dernière erreur JS catchée par le wrapper d'une méthode
    /// (globalThis.__asl_last_error). Retourne None si la dernière méthode
    /// s'est exécutée sans erreur. E8 Bug A : utilisé pour détecter si init
    /// a échoué silencieusement (le try/catch du wrapper attrape l'erreur,
    /// imprime un log, et retourne null — call_method retourne Ok("null")
    /// de son côté, impossible à distinguer d'un succès sans ce flag).
    pub fn derniere_erreur(&mut self) -> Option<String> {
        match self.ctx.eval(Source::from_bytes("globalThis.__asl_last_error")) {
            Ok(val) => {
                let s = val
                    .as_string()
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_default();
                if s.is_empty() || s == "null" || s == "undefined" {
                    None
                } else {
                    Some(s)
                }
            }
            Err(_) => None,
        }
    }

    /// Accès test-only au Context Boa (pour les tests unitaires T1).
    #[cfg(test)]
    pub(crate) fn ctx_ref_mut(&mut self) -> &mut Context {
        &mut self.ctx
    }
}

impl Default for ScriptContext {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Callbacks natifs Boa — pont mémoire JS ↔ Rust
// =============================================================================
// Enregistrés comme NativeFunction (from_fn_ptr) dans ScriptContext::new().
// L'état du processus jeu est dans le thread_local de pont_memoire.rs.
//
// __rust_deref(module, base, offsets, type, extra) → valeur JS (number/string/bool/null)
//   Effectue une lecture mémoire typée via DeepPointer.
// __rust_file_exists(path) → bool
// __rust_file_read(path) → string
// __sigscan_callback(game, address, size, signatures) → number (adresse ou 0)
// =============================================================================

/// Enregistre les callbacks natifs Rust dans le contexte Boa.
fn enregistrer_callbacks_natifs(ctx: &mut Context) {
    // __rust_deref(module, base, offsets, type, extra)
    let _ = ctx.register_global_callable(
        "__rust_deref".into(),
        5,
        NativeFunction::from_fn_ptr(rust_deref_fn),
    );
    // __rust_file_exists(path)
    let _ = ctx.register_global_callable(
        "__rust_file_exists".into(),
        1,
        NativeFunction::from_fn_ptr(rust_file_exists_fn),
    );
    // __rust_file_read(path)
    let _ = ctx.register_global_callable(
        "__rust_file_read".into(),
        1,
        NativeFunction::from_fn_ptr(rust_file_read_fn),
    );
    // __sigscan_callback(game, address, size, signatures)
    let _ = ctx.register_global_callable(
        "__rust_sigscan".into(),
        4,
        NativeFunction::from_fn_ptr(rust_sigscan_fn),
    );
    // __app_dir() → string (répertoire exécutable, stub neutre)
    let _ = ctx.register_global_callable(
        "__app_dir".into(),
        0,
        NativeFunction::from_fn_ptr(rust_app_dir_fn),
    );
    // __local_appdata() → string (%LOCALAPPDATA% ou équivalent)
    let _ = ctx.register_global_callable(
        "__local_appdata".into(),
        0,
        NativeFunction::from_fn_ptr(rust_local_appdata_fn),
    );
    log::debug!("[ASL] Callbacks natifs pont mémoire enregistrés");
}

/// __rust_deref(module, base, offsets, type, extra) → valeur JS
fn rust_deref_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    use super::pont_memoire::deref_memoire;

    // module : string ou null
    let module = args.first()
        .and_then(|v| if v.is_null() || v.is_undefined() { None } else { v.as_string() })
        .map(|s| s.to_std_string_escaped());

    // base : number → i32
    let base = args.get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as i32)
        .unwrap_or(0);

    // offsets : array → Vec<i32>
    let offsets: Vec<i32> = args.get(2)
        .and_then(|v| v.as_object())
        .and_then(|obj| {
            if obj.is_array() {
                let len = obj.get(boa_engine::string::JsString::from("length"), ctx).ok()?
                    .as_number().unwrap_or(0.0) as usize;
                let mut out = Vec::with_capacity(len);
                for i in 0..len {
                    let val = obj.get(i, ctx).ok()?;
                    out.push(val.as_number().unwrap_or(0.0) as i32);
                }
                Some(out)
            } else {
                None
            }
        })
        .unwrap_or_default();

    // type : string
    let type_str = args.get(3)
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|| "int".to_string());

    // extra : number → usize
    let extra = args.get(4)
        .and_then(|v| v.as_number())
        .map(|n| n as usize)
        .unwrap_or(0);

    let result = deref_memoire(module.as_deref(), base, &offsets, &type_str, extra);
    Ok(lecture_vers_js(result, ctx))
}

/// __rust_file_exists(path) → bool
fn rust_file_exists_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    let path = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped());
    let exists = path
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false);
    Ok(JsValue::new(exists))
}

/// __rust_file_read(path) → string
fn rust_file_read_fn(_this: &JsValue, args: &[JsValue], _ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    let path = args.first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped());
    match path {
        Some(p) => match std::fs::read_to_string(&p) {
            Ok(content) => Ok(JsValue::String(boa_engine::string::JsString::from(content))),
            Err(e) => {
                log::warn!("[ASL] __rust_file_read '{}' : {}", p, e);
                Ok(JsValue::undefined())
            }
        },
        None => Ok(JsValue::undefined()),
    }
}

/// __sigscan_callback(game, address, size, signatures) → number (adresse ou 0)
fn rust_sigscan_fn(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    use super::pont_memoire::{sigscan_memoire, SignatureJson};

    // game : ignoré (le processus est dans le thread_local)
    // address : number → usize
    let address = args.get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as usize)
        .unwrap_or(0);

    // size : number → usize
    let size = args.get(2)
        .and_then(|v| v.as_number())
        .map(|n| n as usize)
        .unwrap_or(0);

    // signatures : array d'objets { Pattern, Mask, Offset }
    let signatures: Vec<SignatureJson> = {
        let mut out = Vec::new();
        if let Some(obj) = args.get(3).and_then(|v| v.as_object()) {
            if obj.is_array() {
                let len = obj.get(boa_engine::string::JsString::from("length"), ctx)
                    .ok().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                for i in 0..len {
                    let sig_val = match obj.get(i, ctx) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let sig_obj = match sig_val.as_object() {
                        Some(o) => o,
                        None => continue,
                    };
                    // Pattern : array de numbers
                    let pattern = lire_array_u8(sig_obj, "Pattern", ctx);
                    // Mask : array de bools
                    let mask = lire_array_bool(sig_obj, "Mask", ctx);
                    // Offset : number → i32
                    let offset = sig_obj.get(boa_engine::string::JsString::from("Offset"), ctx)
                        .ok().and_then(|v| v.as_number()).map(|n| n as i32).unwrap_or(0);
                    out.push(SignatureJson { pattern, mask, offset });
                }
            }
        }
        out
    };

    let result = sigscan_memoire(address, size, &signatures);
    Ok(JsValue::new(result.unwrap_or(0) as f64))
}

/// __app_dir() → string (répertoire exécutable courant, stub neutre)
fn rust_app_dir_fn(_this: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    let dir = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    Ok(JsValue::String(boa_engine::string::JsString::from(dir)))
}

/// __local_appdata() → string (%LOCALAPPDATA% ou équivalent cross-platform)
fn rust_local_appdata_fn(_this: &JsValue, _args: &[JsValue], _ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    let dir = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    Ok(JsValue::String(boa_engine::string::JsString::from(dir)))
}

/// Convertit une LectureMemoire en JsValue.
fn lecture_vers_js(lecture: Option<super::pont_memoire::LectureMemoire>, _ctx: &mut Context) -> JsValue {
    use super::pont_memoire::LectureMemoire;
    match lecture {
        Some(LectureMemoire::Int(v)) => JsValue::new(v as f64),
        Some(LectureMemoire::Long(v)) => JsValue::new(v as f64),
        Some(LectureMemoire::Float(v)) => JsValue::new(v),
        Some(LectureMemoire::Bool(v)) => JsValue::new(v),
        Some(LectureMemoire::String(s)) => JsValue::String(boa_engine::string::JsString::from(s)),
        Some(LectureMemoire::Bytes(_)) => JsValue::null(), // bytes non supportés en JS direct
        None => JsValue::null(),
    }
}

/// Lit une propriété array d'u8 depuis un JsObject (helper pour sigscan).
fn lire_array_u8(obj: &boa_engine::object::JsObject, prop: &str, ctx: &mut Context) -> Vec<u8> {
    let arr_val = match obj.get(boa_engine::string::JsString::from(prop), ctx) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match arr_val.as_object() {
        Some(o) if o.is_array() => o,
        _ => return Vec::new(),
    };
    let len = arr.get(boa_engine::string::JsString::from("length"), ctx)
        .ok().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
    let mut out = Vec::with_capacity(len);
    for j in 0..len {
        let val = arr.get(j, ctx).ok();
        out.push(val.and_then(|v| v.as_number()).unwrap_or(0.0) as u8);
    }
    out
}

/// Lit une propriété array de bools depuis un JsObject (helper pour sigscan).
fn lire_array_bool(obj: &boa_engine::object::JsObject, prop: &str, ctx: &mut Context) -> Vec<bool> {
    let arr_val = match obj.get(boa_engine::string::JsString::from(prop), ctx) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = match arr_val.as_object() {
        Some(o) if o.is_array() => o,
        _ => return Vec::new(),
    };
    let len = arr.get(boa_engine::string::JsString::from("length"), ctx)
        .ok().and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
    let mut out = Vec::with_capacity(len);
    for j in 0..len {
        let val = arr.get(j, ctx).ok();
        out.push(val.and_then(|v| v.as_boolean()).unwrap_or(false));
    }
    out
}

/// Échappe les single quotes et backslashes dans une string JSON pour
/// l'insérer entre single quotes en JS (sécurité : les strings JSON peuvent
/// contenir des ' dans les valeurs, ex: noms de splits).
fn echapper_single_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

// =============================================================================
// Tests unitaires — E7 Lot T1 (shims JS LiveSplit)
// -----------------------------------------------------------------------------
// Vérifie que les shims ajoutés à l'API ASL (indexeur MemoryWatcherList,
// AddRange/Clear, .Equals sur Number, settings permissif, game.Memory/PascalCase)
// fonctionnent correctement dans le moteur Boa.
// =============================================================================
#[cfg(test)]
mod tests_t1 {
    use super::ScriptContext;
    use boa_engine::{Context, Source};

    /// Évalue un snippet JS dans un Context Boa frais (sans ScriptContext complet)
    /// et retourne le résultat sous forme de string.
    fn eval_js(snippet: &str) -> String {
        // On utilise ScriptContext comme wrapper d'évaluation : son ctx contient
        // déjà toute l'API ASL (MemoryWatcherList, settings, etc.).
        let mut sc = ScriptContext::new();
        let wrapped = format!(
            r#"
            globalThis.__test_t1 = function() {{
                var __r = (function() {{ {} }})();
                return String(__r);
            }};
            "#,
            snippet
        );
        match sc.ctx_ref_mut().eval(Source::from_bytes(&wrapped)) {
            Ok(_) => {}
            Err(e) => return format!("COMPILE_ERROR: {}", e),
        }
        match sc.ctx_ref_mut().eval(Source::from_bytes("globalThis.__test_t1()")) {
            Ok(v) => v
                .as_string()
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_else(|| format!("{:?}", v)),
            Err(e) => format!("RUNTIME_ERROR: {}", e),
        }
    }

    /// T1.DEBUG — Vérifie que l'API ASL charge complètement (pas d'abort mid-eval).
    #[test]
    fn test_debug_shims_availability() {
        let mut sc = ScriptContext::new();
        let c = sc.ctx_ref_mut();
        let check = |ctx: &mut Context, code: &str| -> String {
            match ctx.eval(boa_engine::Source::from_bytes(code)) {
                Ok(v) => v.as_string().map(|s| s.to_std_string_escaped()).unwrap_or_else(|| format!("{:?}", v)),
                Err(e) => format!("ERR:{}", e),
            }
        };
        let r1 = check(c, "(function(){ try { return typeof __creerSettings; } catch(e){ return 'ERR:'+e.message; } })()");
        let r2 = check(c, "(function(){ try { return typeof TimerModel; } catch(e){ return 'ERR:'+e.message; } })()");
        let r3 = check(c, "(function(){ try { return typeof MemoryStream; } catch(e){ return 'ERR:'+e.message; } })()");
        let r4 = check(c, "(function(){ try { return typeof Application; } catch(e){ return 'ERR:'+e.message; } })()");
        let r5 = check(c, "(function(){ try { return typeof Environment; } catch(e){ return 'ERR:'+e.message; } })()");
        let r6 = check(c, "(function(){ try { return typeof Encoding; } catch(e){ return 'ERR:'+e.message; } })()");
        let r7 = check(c, "(function(){ try { return typeof atob; } catch(e){ return 'ERR:'+e.message; } })()");
        let r8 = check(c, "(function(){ try { return typeof __mem_read_bytes; } catch(e){ return 'ERR:'+e.message; } })()");
        assert_eq!(r1, "function", "__creerSettings doit être défini");
        assert_eq!(r2, "function", "TimerModel doit être défini");
        assert_eq!(r3, "function", "MemoryStream doit être défini");
        assert_eq!(r4, "object", "Application doit être défini");
        assert_eq!(r5, "object", "Environment doit être défini");
        assert_eq!(r6, "object", "Encoding doit être défini");
        assert_eq!(r7, "function", "atob doit être défini");
        assert_eq!(r8, "function", "__mem_read_bytes doit être défini");
    }

    /// T1.1 — Indexeur MemoryWatcherList["name"] retourne le watcher ajouté.
    #[test]
    fn test_indexeur_memory_watcher_list() {
        let r = eval_js(
            r#"
            var M = new MemoryWatcherList();
            var w = new MemoryWatcher(null, 'int');
            w.Name = 'Location';
            w.Current = 'heliport';
            M.Add(w);
            return M['Location'].Current;
            "#,
        );
        assert_eq!(r, "heliport", "M['Location'].Current doit retourner 'heliport'");
    }

    /// T1.2 — MemoryWatcherList.AddRange + Clear.
    #[test]
    fn test_memory_watcher_list_addrange_clear() {
        let r = eval_js(
            r#"
            var M1 = new MemoryWatcherList();
            var w1 = new MemoryWatcher(null, 'int'); w1.Name = 'A';
            var w2 = new MemoryWatcher(null, 'int'); w2.Name = 'B';
            M1.Add(w1); M1.Add(w2);
            var M2 = new MemoryWatcherList();
            M2.AddRange(M1);
            var countAfter = M2.Count;
            M2.Clear();
            var countAfterClear = M2.Count;
            return countAfter + ',' + countAfterClear;
            "#,
        );
        assert_eq!(r, "2,0", "AddRange doit ajouter 2 watchers, Clear doit vider");
    }

    /// T1.3 — Number.Equals (PascalCase C#).
    #[test]
    fn test_number_equals_pascalcase() {
        let r = eval_js(
            r#"
            var n = 42;
            return n.Equals(42) + ',' + n.Equals(43);
            "#,
        );
        assert_eq!(r, "true,false", "42.Equals(42)=true, 42.Equals(43)=false");
    }

    /// T1.3b — Number.ToString (PascalCase C#).
    /// MGS F.SetStateCodes fait CurProg.ToString() où CurProg est un number.
    /// Le transpileur convertit .ToString( → ?.ToString(, mais JS n'a que .toString()
    /// (lowercase). Sans ce shim, CurProg?.ToString() throw "not a function" →
    /// F.SetStateCodes échoue → split() ne génère jamais de validCodes → aucun split.
    #[test]
    fn test_number_tostring_pascalcase() {
        let r = eval_js(
            r#"
            var n = 42;
            return n.ToString();
            "#,
        );
        assert_eq!(r, "42", "42.ToString() doit retourner '42'");
    }

    /// T1.3c — String.ToString (PascalCase C#).
    #[test]
    fn test_string_tostring_pascalcase() {
        let r = eval_js(
            r#"
            var s = 'hello';
            return s.ToString();
            "#,
        );
        assert_eq!(r, "hello", "'hello'.ToString() doit retourner 'hello'");
    }

    /// T1.4 — String.Equals (PascalCase, déjà présent via alias).
    #[test]
    fn test_string_equals_pascalcase() {
        let r = eval_js(
            r#"
            var s = 'vrtitle';
            return s.Equals('vrtitle') + ',' + s.Equals('selectvr');
            "#,
        );
        assert_eq!(r, "true,false", "'vrtitle'.Equals('vrtitle')=true");
    }

    /// T1.5 — Settings permissif : settings["key"] retourne false si absent,
    /// puis la valeur après Add.
    #[test]
    fn test_settings_permissif() {
        let r = eval_js(
            r#"
            var s = __creerSettings({ start: true, split: false, reset: false });
            var before = s['Opt.ASL.FPS']; // false (non déclaré)
            s.Add('Opt.ASL.FPS', true, '');
            var after = s['Opt.ASL.FPS']; // true (déclaré)
            var contains = s.ContainsKey('Opt.ASL.FPS');
            var readerStart = s.Reader.start;
            return before + ',' + after + ',' + contains + ',' + readerStart;
            "#,
        );
        assert_eq!(
            r, "false,true,true,true",
            "settings permissif: false avant Add, true après, ContainsKey=true, Reader.start=true"
        );
    }

    /// T1.6 — Settings permissif : SetToolTip + CurrentDefaultParent stubs.
    #[test]
    fn test_settings_stubs() {
        let r = eval_js(
            r#"
            var s = __creerSettings({ start: false, split: false, reset: false });
            s.SetToolTip('key', 'tip'); // no-op, ne doit pas crasher
            s.CurrentDefaultParent = 'Splits';
            s.Add('child', true, '');
            return s.CurrentDefaultParent + ',' + s['child'];
            "#,
        );
        assert_eq!(r, "Splits,true", "SetToolTip no-op, CurrentDefaultParent get/set");
    }

    /// T1.7 — TimerModel stub : CurrentState + méthodes no-op.
    #[test]
    fn test_timer_model_stub() {
        let r = eval_js(
            r#"
            var tm = new TimerModel();
            tm.CurrentState = { phase: 'NotRunning' };
            tm.Split(); tm.UndoSplit(); tm.SkipSplit(); tm.Reset(); tm.Pause();
            return tm.CurrentState.phase;
            "#,
        );
        assert_eq!(r, "NotRunning", "TimerModel stub méthodes no-op, CurrentState préservé");
    }

    /// T1.8 — Application.ExecutablePath + Environment.GetFolderPath stubs.
    #[test]
    fn test_application_environment_stubs() {
        let r = eval_js(
            r#"
            var exe = Application.ExecutablePath;
            var folder = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            return (exe.length > 0) + ',' + (folder.length > 0);
            "#,
        );
        assert_eq!(r, "true,true", "Application.ExecutablePath et Environment.GetFolderPath retournent un chemin non vide");
    }

    /// T1.9 — Encoding.UTF8.GetString décode un array d'octets ASCII.
    #[test]
    fn test_encoding_utf8_getstring() {
        let r = eval_js(
            r#"
            var bytes = [72, 101, 108, 108, 111]; // "Hello"
            return Encoding.UTF8.GetString(bytes);
            "#,
        );
        assert_eq!(r, "Hello", "Encoding.UTF8.GetString([72,101,108,108,111]) = 'Hello'");
    }

    /// T1.10 — MemoryStream stub : Write + ToArray.
    #[test]
    fn test_memory_stream_stub() {
        let r = eval_js(
            r#"
            var ms = new MemoryStream();
            ms.Write([1, 2, 3], 0, 3);
            var arr = ms.ToArray();
            return arr.join(',');
            "#,
        );
        assert_eq!(r, "1,2,3", "MemoryStream Write + ToArray");
    }

    /// T1.11 — game.Memory + accesseurs PascalCase via compile_method.
    /// On compile une méthode "test" qui utilise game.Memory et game.Modules.
    #[test]
    fn test_game_memory_pascalcase_via_compile() {
        let mut sc = ScriptContext::new();
        let code = r#"
            var M = game.Memory;
            var mods = game.Modules;
            return (M instanceof MemoryWatcherList) + ',' + (mods !== null);
        "#;
        sc.compile_method("test", code)
            .expect("compile_method('test') doit réussir");

        let proc_json = r#"{"id":1234,"processName":"mgsi","is64Bit":true,"modules":[{"moduleName":"mgsi.exe","baseAddress":4194304,"moduleMemorySize":1048576}]}"#;
        let result = sc
            .call_method("test", "{}", "{}", "{}", Some(proc_json), &super::AslSettings::default())
            .expect("call_method('test') doit réussir");
        // Résultat attendu : "true,true" (M est MemoryWatcherList, mods non null)
        assert!(
            result.contains("true"),
            "game.Memory doit être un MemoryWatcherList et game.Modules non null, obtenu: {}",
            result
        );
    }

    /// T1.12 — Script ASL minimal avec shims T1 : startup + init + start simples.
    /// Vérifie que les shims permettent à un script type de compiler et tourner.
    #[test]
    fn test_script_asl_minimal_avec_shims() {
        let mut sc = ScriptContext::new();

        // startup : crée vars.D.Mem comme MemoryWatcherList
        sc.compile_method(
            "startup",
            r#"
            vars.D = new ExpandoObject();
            var D = vars.D;
            D.Mem = new MemoryWatcherList();
            D.Game = new ExpandoObject();
            D.Game.VRMissions = false;
            return null;
            "#,
        )
        .expect("compile startup");

        // init : ajoute un watcher à D.Mem
        sc.compile_method(
            "init",
            r#"
            var D = vars.D;
            var M = D.Mem;
            var w = new MemoryWatcher(null, 'int');
            w.Name = 'Progress';
            w.Current = 0;
            w.Old = 0;
            M.Add(w);
            return null;
            "#,
        )
        .expect("compile init");

        // start : utilise M["Progress"].Current.Equals(1) et M["Progress"].Changed
        sc.compile_method(
            "start",
            r#"
            var D = vars.D;
            var M = D.Mem;
            var Prog = M["Progress"];
            if (Prog.Current.Equals(1) && Prog.Changed) return true;
            return false;
            "#,
        )
        .expect("compile start");

        // Exécuter startup
        sc.run_startup(&super::AslSettings::default())
            .expect("run_startup doit réussir");

        // Exécuter init
        let init_result = sc
            .call_method("init", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('init')");
        assert_eq!(init_result, "null", "init doit retourner null");

        // Exécuter start avec Progress=1 et Changed=true
        // On ne peut pas facilement simuler Changed=true via call_method (vars persiste).
        // On vérifie juste que start compile et s'exécute sans crash.
        let start_result = sc
            .call_method("start", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('start')");
        // start retourne false (Progress=0, pas 1)
        assert!(
            start_result == "false" || start_result == "null",
            "start doit retourner false ou null (Progress=0), obtenu: {}",
            start_result
        );
    }

    /// E7 runtime — Array.prototype.Add (idiome C# .Add sur listes transpilées en arrays).
    #[test]
    fn test_array_add_shim() {
        let r = eval_js(
            r#"
            var a = [];
            a.Add(1);
            a.Add(2);
            return a.length + ',' + a[0] + ',' + a[1];
            "#,
        );
        assert_eq!(r, "2,1,2", "Array.Add doit pousser les items");
    }

    /// E7 runtime — EventLog(args) + EntryWritten.__subscribe ne throw pas.
    #[test]
    fn test_eventlog_stub() {
        let r = eval_js(
            r#"
            var log = new EventLog("Application");
            log.EnableRaisingEvents = true;
            log.EntryWritten.__subscribe(function(sender, e) {});
            return log.Source;
            "#,
        );
        assert_eq!(r, "Application", "EventLog stub doit exposer Source");
    }

    /// E7 runtime — timer.OnReset.__subscribe no-op via compile_method/call_method.
    #[test]
    fn test_timer_events_subscribe() {
        let mut sc = ScriptContext::new();
        sc.compile_method(
            "startup",
            r#"
            timer.OnReset.__subscribe(function(sender, e) {});
            timer.OnStart.__subscribe(function(sender, e) {});
            vars.ok = true;
            return null;
            "#,
        )
        .expect("compile startup");
        sc.run_startup(&super::AslSettings::default())
            .expect("run_startup doit réussir");
        // E8 Bug A — vars vit dans globalThis.__asl_vars (plus de round-trip
        // JSON). On relit via lire_vars_json() pour vérifier vars.ok=true.
        let vars_json = sc.lire_vars_json();
        assert!(
            vars_json.contains("\"ok\":true"),
            "startup doit aller au bout (vars.ok=true), vars: {}",
            vars_json
        );
    }

    /// E7 runtime — Reproduction du throw « not a callable function » avec le vrai
    /// MetalGearSolid.asl. Charge l'ASL, transpile startup, compile, exécute, et
    /// logge le JS transpilé + l'erreur pour diagnostic.
    #[test]
    fn test_mgs_startup_runtime_dump() {
        let asl_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("MetalGearSolid.asl");
        if !asl_path.exists() {
            eprintln!("[test_mgs_startup_runtime_dump] MetalGearSolid.asl non trouvé, skip");
            return;
        }
        let source = std::fs::read_to_string(&asl_path)
            .expect("lecture MetalGearSolid.asl");
        let script = crate::speedrun::asl::parse(&source).expect("parse ASL");
        let startup_code = script.methods.startup.as_ref().expect("startup présent");
        let js_code = crate::speedrun::engine::transpiler::transpile_cs_to_js(startup_code);

        // Dump du JS transpilé pour diagnostic
        let dump_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("debug_startup_js_fresh.txt");
        let _ = std::fs::write(&dump_path, &js_code);
        eprintln!("[test_mgs_startup_runtime_dump] JS transpilé écrit dans {:?}", dump_path);

        // Vérifier qu'il n'y a plus de (0).Add ou new short[]
        assert!(
            !js_code.contains("(0).Add"),
            "Le JS transpilé ne doit plus contenir (0).Add"
        );
        assert!(
            !js_code.contains("new short[]") && !js_code.contains("new byte[]") && !js_code.contains("new int[]"),
            "Le JS transpilé ne doit plus contenir de tableaux typés non convertis"
        );

        // Compiler et exécuter startup
        let mut sc = ScriptContext::new();
        sc.compile_method("startup", &js_code)
            .expect("compile startup");

        // Probe FULL : exécuter le code entier
        let probe_full = "(function() {\n\
            var vars = {};\n\
            var timer = {};\n\
            var old = {};\n\
            var current = {};\n\
            var game = null;\n\
            var refreshRate = 1000/15;\n\
            var version = \"\";\n\
            var memory = game;\n\
            var modules = game ? game.modules : null;\n\
            if (timer) {\n\
                var __evts = ['OnStart','OnSplit','OnReset','OnPause','OnResume','OnUndoSplit','OnSkipSplit'];\n\
                for (var __i = 0; __i < __evts.length; __i++) {\n\
                    if (!timer[__evts[__i]]) timer[__evts[__i]] = {};\n\
                }\n\
            }\n\
            var settings = (typeof __creerSettings === 'function')\n\
                ? __creerSettings({\"start\":false,\"split\":false,\"reset\":false})\n\
                : { Reader: {\"start\":false,\"split\":false,\"reset\":false} };\n\
            try {\n\
        ".to_string();
        let probe_full = probe_full + &js_code + "\n\
                return \"OK\";\n\
            } catch(e) {\n\
                return \"THROW: \" + e.message;\n\
            }\n\
        })()";
        let r_full = sc.ctx_ref_mut().eval(boa_engine::Source::from_bytes(&probe_full));
        let result_str = match &r_full {
            Ok(v) => {
                let s = v.as_string().map(|s| s.to_std_string_escaped()).unwrap_or_else(|| format!("{:?}", v));
                eprintln!("[test_mgs_startup_runtime_dump] PROBE FULL: {}", s);
                s
            }
            Err(e) => {
                eprintln!("[test_mgs_startup_runtime_dump] PROBE FULL EVAL ERROR: {}", e);
                format!("EVAL_ERROR: {}", e)
            }
        };

        // Le startup doit s'exécuter sans throw
        assert!(
            result_str.starts_with("OK"),
            "startup doit s'exécuter sans erreur, obtenu: {}", result_str
        );
    }

    /// E8 Bug A — Reproduction du throw « not a callable function » dans init
    /// avec le vrai MetalGearSolid.asl. Charge l'ASL, transpile init, compile,
    /// exécute (après startup pour peupler vars.D), et logge le JS transpilé +
    /// l'erreur pour diagnostic.
    #[test]
    fn test_mgs_init_runtime_dump() {
        let asl_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("MetalGearSolid.asl");
        if !asl_path.exists() {
            eprintln!("[test_mgs_init_runtime_dump] MetalGearSolid.asl non trouvé, skip");
            return;
        }
        let source = std::fs::read_to_string(&asl_path)
            .expect("lecture MetalGearSolid.asl");
        let script = crate::speedrun::asl::parse(&source).expect("parse ASL");

        // 1. Transpiler + exécuter startup pour peupler vars.D
        let startup_code = script.methods.startup.as_ref().expect("startup présent");
        let startup_js = crate::speedrun::engine::transpiler::transpile_cs_to_js(startup_code);
        let mut sc = ScriptContext::new();
        sc.compile_method("startup", &startup_js)
            .expect("compile startup");
        sc.run_startup(&super::AslSettings::default())
            .expect("run_startup");

        // 2. Transpiler init
        let init_code = script.methods.init.as_ref().expect("init présent");
        let init_js = crate::speedrun::engine::transpiler::transpile_cs_to_js(init_code);

        // Dump du JS transpilé pour diagnostic
        let dump_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("debug_init_js_fresh.txt");
        let _ = std::fs::write(&dump_path, &init_js);
        eprintln!("[test_mgs_init_runtime_dump] JS transpilé écrit dans {:?}", dump_path);

        // 3. Compiler + exécuter init avec vars.D peuplé par startup
        sc.compile_method("init", &init_js)
            .expect("compile init");
        // Simuler le contexte prod : game avec modules + current/old non vides
        // (les watchers JS sont vides car pas de process réel, mais vars.D.Mem
        // existe grâce à startup qui a fait D.Mem = new MemoryWatcherList()).
        let result = sc.call_method(
            "init",
            r#"{"currentPhase":"NotRunning","currentSplitIndex":-1,"totalSplits":71,"currentTime":"00:00.00","gameTime":null,"realTime":0.0}"#,
            "{}",
            "{}",
            Some(r#"{"id":1234,"processName":"mgsi.exe","is64Bit":false,"modules":[]}"#),
            &super::AslSettings::default(),
        );
        let err = sc.derniere_erreur();
        eprintln!("[test_mgs_init_runtime_dump] call_method result: {:?}", result);
        eprintln!("[test_mgs_init_runtime_dump] derniere_erreur: {:?}", err);

        // 4. Vérifier que init s'exécute sans erreur (E8 Bug A).
        // Avant les fixes, init throw "not a callable function" (ToLowerInvariant
        // manquant) puis "cannot convert null to object" (M.length guard) puis
        // "Function Unimplemented" (toLocaleTimeString non implémenté dans Boa).
        // Fixes génériques appliqués :
        //   - String.prototype.ToLowerInvariant/ToUpperInvariant (shim)
        //   - MemoryWatcherList.prototype.length (alias de Count pour guard M.length != 0)
        //   - Date.prototype.toLocaleTimeString/toLocaleDateString (override Boa Unimplemented)
        //   - .Equals → ?.equals, .ToString → ?.ToString (optional chaining transpileur)
        assert!(
            err.is_none(),
            "init doit s'exécuter sans erreur après les fixes E8, obtenu: {:?}",
            err
        );

        // Nettoyer le dump
        let _ = std::fs::remove_file(&dump_path);
    }

    /// Auto-start fix — Vérifie que le wrapper compile_method strippe .exe
    /// de game.ProcessName. Le script ASL fait `switch (processName) { case "mgsi": }`
    /// (sans .exe), mais serialize_process envoie processName = "mgsi.exe".
    /// Sans le strip, le case ne matche pas → G.BaseAddress reste 0 → update()
    /// early-return → M.UpdateAll jamais appelé → start()=false à vie.
    #[test]
    fn test_wrapper_strip_exe_processname() {
        let mut sc = ScriptContext::new();
        // Méthode simple qui retourne game.ProcessName (pour vérifier le strip)
        let js_code = r#"return game.ProcessName;"#;
        sc.compile_method("test_strip_exe", js_code)
            .expect("compile test_strip_exe");

        // game avec processName = "mgsi.exe" (comme serialize_process en prod)
        let result = sc.call_method(
            "test_strip_exe",
            "{}",
            "{}",
            "{}",
            Some(r#"{"processName":"mgsi.exe","id":1234,"modules":[]}"#),
            &super::AslSettings::default(),
        );
        let result_str = result.unwrap_or_else(|e| format!("ERR:{}", e));
        eprintln!("[test_wrapper_strip_exe_processname] result: {}", result_str);
        // Le résultat doit être "mgsi" (sans .exe), pas "mgsi.exe"
        assert!(
            result_str.contains("mgsi") && !result_str.contains("mgsi.exe"),
            "game.ProcessName doit être 'mgsi' (sans .exe), obtenu: {}", result_str
        );
    }

    /// Auto-start fix — Vérifie que MemoryWatcher.Update lit un number Pointer
    /// (adresse absolue PC). Les watchers PC sont créés avec
    /// `new MemoryWatcher<short>(F.Addr(0x38D7CA))` où F.Addr retourne un nombre.
    /// Sans le branch number Pointer, Current reste null → start()=false.
    #[test]
    fn test_memory_watcher_number_pointer() {
        let mut sc = ScriptContext::new();
        // Simuler __deref_callback pour qu'il retourne une valeur fixe (42)
        sc.ctx_ref_mut().eval(boa_engine::Source::from_bytes(
            r#"
            globalThis.__deref_callback = function(module, base, offsets, type, game, extra) {
                return 42;
            };
            globalThis.__test_watcher_result = (function() {
                var w = new MemoryWatcher(0x400000, 'short');
                w.Update(null);
                return w.Current;
            })();
            "#,
        )).expect("eval test watcher");
        let val = sc.ctx_ref_mut().eval(boa_engine::Source::from_bytes(
            "globalThis.__test_watcher_result",
        )).expect("read result");
        let n = val.as_number().unwrap_or(-999.0);
        eprintln!("[test_memory_watcher_number_pointer] Current: {}", n);
        assert_eq!(
            n, 42.0,
            "MemoryWatcher avec number Pointer doit lire la mémoire via __deref_callback"
        );
    }

    /// Auto-start fix — Vérifie que StringWatcher.Update lit un number Pointer
    /// (adresse absolue PC). Les watchers PC de location sont créés avec
    /// `new StringWatcher(F.Addr(0x2504CE), 8)` (number Pointer).
    #[test]
    fn test_string_watcher_number_pointer() {
        let mut sc = ScriptContext::new();
        sc.ctx_ref_mut().eval(boa_engine::Source::from_bytes(
            r#"
            globalThis.__deref_callback = function(module, base, offsets, type, game, extra) {
                return "s01a";
            };
            globalThis.__test_str_watcher_result = (function() {
                var w = new StringWatcher(0x400000, 8);
                w.Update(null);
                return w.Current;
            })();
            "#,
        )).expect("eval test str watcher");
        let val = sc.ctx_ref_mut().eval(boa_engine::Source::from_bytes(
            "globalThis.__test_str_watcher_result",
        )).expect("read str result");
        let s = val.as_string().map(|s| s.to_std_string_escaped()).unwrap_or_default();
        eprintln!("[test_string_watcher_number_pointer] Current: {}", s);
        assert_eq!(
            s, "s01a",
            "StringWatcher avec number Pointer doit lire la string via __deref_callback"
        );
    }

    /// LOT 1 — splits bloqués : vérifie que les settings enregistrés pendant
    /// `init` via settings.Add(...) survivent jusqu'à l'appel de `split`.
    /// Avant le fix, settings était recréé à chaque call_method depuis
    /// {start,split,reset} → settings.ContainsKey("CP-18") retournait false
    /// → F.SettingEnabled(code) = false → F.Split(code) = false → aucun split.
    /// Après le fix, settings vit dans globalThis.__asl_settings (persistant,
    /// même pattern que __asl_vars).
    #[test]
    fn test_settings_persistence_entre_init_et_split() {
        let mut sc = ScriptContext::new();

        // init : enregistre un code split comme setting (pattern MGS réel)
        sc.compile_method(
            "init",
            r#"
            settings.Add("CP-18", true, "Heliport -> Tank Hangar");
            return null;
            "#,
        )
        .expect("compile init");

        // split : vérifie que le setting enregistré dans init est toujours là
        // (pattern MGS : F.SettingEnabled(code) = settings.ContainsKey(code) && settings[code])
        sc.compile_method(
            "split",
            r#"
            if (settings.ContainsKey("CP-18") && settings["CP-18"]) return true;
            return false;
            "#,
        )
        .expect("compile split");

        // Exécuter init (peuple globalThis.__asl_settings avec CP-18=true)
        let init_result = sc
            .call_method("init", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('init')");
        assert_eq!(init_result, "null", "init doit retourner null");

        // Exécuter split — doit retourner true (le setting a survécu)
        let split_result = sc
            .call_method("split", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('split')");
        assert_eq!(
            split_result, "true",
            "split doit retourner true : le setting CP-18 enregistré dans init doit survivre \
             jusqu'à split (persistance globalThis.__asl_settings). Avant le fix, retournait false."
        );
    }

    /// LOT 1 — reset_etat_script : vérifie que le reset nettoie les settings
    /// persistants (et vars). Sans ça, un second script ASL hérite des settings
    /// du premier → faux positifs de split.
    #[test]
    fn test_reset_etat_script_nettoie_settings() {
        let mut sc = ScriptContext::new();

        // init : enregistre un setting
        sc.compile_method(
            "init",
            r#"
            settings.Add("CP-99", true, "test");
            vars.toto = 42;
            return null;
            "#,
        )
        .expect("compile init");
        sc.call_method("init", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('init')");

        // Vérifier que le setting est bien présent avant reset
        let vars_avant = sc.lire_vars_json();
        assert!(vars_avant.contains("\"toto\":42"), "vars.toto doit être présent avant reset");

        // Reset
        sc.reset_etat_script();

        // Après reset, un nouveau init ne doit pas voir l'ancien setting
        sc.compile_method(
            "split",
            r#"
            return settings.ContainsKey("CP-99");
            "#,
        )
        .expect("compile split");
        let split_result = sc
            .call_method("split", "{}", "{}", "{}", None, &super::AslSettings::default())
            .expect("call_method('split')");
        assert_eq!(
            split_result, "false",
            "Après reset_etat_script, l'ancien setting CP-99 ne doit plus exister"
        );

        // vars aussi doit être nettoyé
        let vars_apres = sc.lire_vars_json();
        assert!(!vars_apres.contains("toto"), "vars.toto doit être nettoyé après reset");
    }
}
