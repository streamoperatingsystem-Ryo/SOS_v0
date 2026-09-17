// =============================================================================
// MemoryWatcher — Surveillance de valeurs mémoire avec détection de changement
// (port de MemoryWatcher.cs)
// -----------------------------------------------------------------------------
// Un MemoryWatcher lit une valeur à une adresse (via DeepPointer) à intervalle
// régulier et garde trace de la valeur précédente (Old) et de la valeur courante
// (Current). La propriété Changed indique si la valeur a changé depuis le
// dernier Update().
//
// Types supportés :
//   - MemoryWatcher (typé : i32, u32, f32, f64, bool, etc.)
//   - StringWatcher (string de longueur fixe)
//
// MemoryWatcherList : collection de watchers avec UpdateAll() pour rafraîchir
// tous les watchers en une fois.
// =============================================================================
use super::deep_pointer::DeepPointer;
use super::LecteurMemoire;
use std::collections::HashMap;

/// Valeur surveillée par un MemoryWatcher (peut être typée ou string).
#[derive(Clone, Debug)]
pub enum WatchedValue {
    Int(i32),
    UInt(u32),
    Long(i64),
    ULong(u64),
    Float(f32),
    Double(f64),
    Byte(u8),
    SByte(i8),
    Short(i16),
    UShort(u16),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
}

impl WatchedValue {
    pub fn as_f64(&self) -> f64 {
        match self {
            Self::Int(v) => *v as f64,
            Self::UInt(v) => *v as f64,
            Self::Long(v) => *v as f64,
            Self::ULong(v) => *v as f64,
            Self::Float(v) => *v as f64,
            Self::Double(v) => *v,
            Self::Byte(v) => *v as f64,
            Self::SByte(v) => *v as f64,
            Self::Short(v) => *v as f64,
            Self::UShort(v) => *v as f64,
            Self::Bool(v) => {
                if *v {
                    1.0
                } else {
                    0.0
                }
            }
            Self::String(_) | Self::Bytes(_) => 0.0,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            Self::Int(v) => *v != 0,
            Self::UInt(v) => *v != 0,
            Self::Byte(v) => *v != 0,
            _ => self.as_f64() != 0.0,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Int(v) => v.to_string(),
            Self::UInt(v) => v.to_string(),
            Self::Float(v) => v.to_string(),
            Self::Double(v) => v.to_string(),
            Self::Bool(v) => v.to_string(),
            _ => format!("{:?}", self),
        }
    }
}

/// Type de valeur lu par un watcher (détermine comment lire la mémoire).
#[derive(Clone, Debug)]
pub enum WatcherType {
    Int,
    UInt,
    Long,
    ULong,
    Float,
    Double,
    Byte,
    SByte,
    Short,
    UShort,
    Bool,
    String(usize),  // longueur max en bytes
    Bytes(usize),   // nombre de bytes
}

/// Surveille une valeur mémoire via un DeepPointer.
/// Garde trace de Current (valeur actuelle) et Old (valeur précédente).
pub struct MemoryWatcher {
    pub name: String,
    pub enabled: bool,
    pub current: Option<WatchedValue>,
    pub old: Option<WatchedValue>,
    pub changed: bool,
    pointer: DeepPointer,
    wtype: WatcherType,
    initial_update: bool,
}

impl MemoryWatcher {
    pub fn new(name: &str, pointer: DeepPointer, wtype: WatcherType) -> Self {
        Self {
            name: name.to_string(),
            enabled: true,
            current: None,
            old: None,
            changed: false,
            pointer,
            wtype,
            initial_update: false,
        }
    }

    /// Lit la valeur mémoire et met à jour Current/Old/Changed.
    /// Retourne true si la valeur a changé.
    pub fn update(&mut self, proc: &dyn LecteurMemoire) -> bool {
        self.changed = false;

        if !self.enabled {
            return false;
        }

        let new_val = match &self.wtype {
            WatcherType::Int => self.pointer.deref_i32(proc).map(WatchedValue::Int),
            WatcherType::UInt => self.pointer.deref_u32(proc).map(WatchedValue::UInt),
            WatcherType::Long => self.pointer.deref_i64(proc).map(WatchedValue::Long),
            WatcherType::ULong => self.pointer.deref_u64(proc).map(WatchedValue::ULong),
            WatcherType::Float => self.pointer.deref_f32(proc).map(WatchedValue::Float),
            WatcherType::Double => self.pointer.deref_f64(proc).map(WatchedValue::Double),
            WatcherType::Byte => self.pointer.deref_u8(proc).map(WatchedValue::Byte),
            WatcherType::SByte => self.pointer.deref_i8(proc).map(WatchedValue::SByte),
            WatcherType::Short => self.pointer.deref_i16(proc).map(WatchedValue::Short),
            WatcherType::UShort => self.pointer.deref_u16(proc).map(WatchedValue::UShort),
            WatcherType::Bool => self.pointer.deref_bool(proc).map(WatchedValue::Bool),
            WatcherType::String(len) => {
                self.pointer.deref_string(proc, *len).map(WatchedValue::String)
            }
            WatcherType::Bytes(len) => {
                self.pointer.deref_bytes(proc, *len).map(WatchedValue::Bytes)
            }
        };

        if let Some(val) = new_val {
            self.old = self.current.clone();
            self.current = Some(val);
        }

        // La première update ne compte pas comme un changement
        if !self.initial_update {
            self.initial_update = true;
            return false;
        }

        // Comparer current et old
        self.changed = match (&self.old, &self.current) {
            (Some(o), Some(c)) => !values_equal(o, c),
            (None, Some(_)) => true,
            _ => false,
        };

        self.changed
    }

    pub fn reset(&mut self) {
        self.current = None;
        self.old = None;
        self.initial_update = false;
        self.changed = false;
    }
}

fn values_equal(a: &WatchedValue, b: &WatchedValue) -> bool {
    match (a, b) {
        (WatchedValue::Int(x), WatchedValue::Int(y)) => x == y,
        (WatchedValue::UInt(x), WatchedValue::UInt(y)) => x == y,
        (WatchedValue::Long(x), WatchedValue::Long(y)) => x == y,
        (WatchedValue::ULong(x), WatchedValue::ULong(y)) => x == y,
        (WatchedValue::Float(x), WatchedValue::Float(y)) => x == y,
        (WatchedValue::Double(x), WatchedValue::Double(y)) => x == y,
        (WatchedValue::Byte(x), WatchedValue::Byte(y)) => x == y,
        (WatchedValue::SByte(x), WatchedValue::SByte(y)) => x == y,
        (WatchedValue::Short(x), WatchedValue::Short(y)) => x == y,
        (WatchedValue::UShort(x), WatchedValue::UShort(y)) => x == y,
        (WatchedValue::Bool(x), WatchedValue::Bool(y)) => x == y,
        (WatchedValue::String(x), WatchedValue::String(y)) => x == y,
        (WatchedValue::Bytes(x), WatchedValue::Bytes(y)) => x == y,
        _ => false,
    }
}

/// Collection de MemoryWatchers avec accès par nom.
pub struct MemoryWatcherList {
    watchers: Vec<MemoryWatcher>,
    by_name: HashMap<String, usize>,
}

impl MemoryWatcherList {
    pub fn new() -> Self {
        Self {
            watchers: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    pub fn add(&mut self, watcher: MemoryWatcher) {
        let idx = self.watchers.len();
        self.by_name.insert(watcher.name.clone(), idx);
        self.watchers.push(watcher);
    }

    pub fn get(&self, name: &str) -> Option<&MemoryWatcher> {
        self.by_name.get(name).and_then(|&i| self.watchers.get(i))
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut MemoryWatcher> {
        self.by_name.get(name).and_then(|&i| self.watchers.get_mut(i))
    }

    /// Met à jour tous les watchers. Retourne la liste des noms de watchers
    /// dont la valeur a changé.
    pub fn update_all(&mut self, proc: &dyn LecteurMemoire) -> Vec<String> {
        let mut changed = Vec::new();
        for w in &mut self.watchers {
            if w.update(proc) {
                changed.push(w.name.clone());
            }
        }
        changed
    }

    pub fn reset_all(&mut self) {
        for w in &mut self.watchers {
            w.reset();
        }
    }

    pub fn len(&self) -> usize {
        self.watchers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.watchers.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &MemoryWatcher> {
        self.watchers.iter()
    }
}

impl Default for MemoryWatcherList {
    fn default() -> Self {
        Self::new()
    }
}
