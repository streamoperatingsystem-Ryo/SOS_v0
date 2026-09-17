// =============================================================================
// Memory — Scan mémoire processus jeu (port de ComponentUtil C#)
// -----------------------------------------------------------------------------
// Toutes les opérations de lecture mémoire utilisent l'API Windows via
// le crate `windows` (ReadProcessMemory, EnumProcessModulesEx, etc.).
//
// Modules :
//   process.rs          — Wrapper processus (OpenProcess, modules, Is64Bit)
//   deep_pointer.rs     — Chaîne de pointeurs (module + base + offsets)
//   memory_watcher.rs   — Watchers typés (MemoryWatcher<T>, StringWatcher)
//   signature_scanner.rs— Scan de signatures binaires (pattern matching)
// =============================================================================
pub mod deep_pointer;
pub mod memory_watcher;
pub mod process;
pub mod signature_scanner;

use serde::{Deserialize, Serialize};

// =============================================================================
// LecteurMemoire — Abstraction de lecture mémoire processus (pour mock tests)
// -----------------------------------------------------------------------------
// Permet de câbler la lecture mémoire Win32 (Process) ET un mock en tests
// unitaires, sans dupliquer la logique de DeepPointer / MemoryWatcher.
// Règle §2 : on étend le moteur existant, on ne crée pas un 2e système.
// =============================================================================
/// Abstraction d'un lecteur mémoire processus (handle jeu + modules + lectures typées).
/// Implémenté par `Process` (Win32, prod) et par les mocks de tests.
pub trait LecteurMemoire {
    /// Nom du processus (insensible à la casse).
    fn nom(&self) -> &str;
    /// PID du processus.
    fn pid(&self) -> u32;
    /// Retourne true si le processus est 64-bit.
    fn est_64_bit(&self) -> bool;
    /// Retourne true si le processus est encore en vie.
    fn est_vivant(&self) -> bool;
    /// Liste des modules chargés.
    fn modules(&self) -> &[process::ProcessModule];
    /// Recherche un module par nom (insensible à la casse).
    fn trouver_module(&self, name: &str) -> Option<&process::ProcessModule> {
        self.modules()
            .iter()
            .find(|m| m.module_name.to_lowercase() == name.to_lowercase())
    }
    /// Module principal (généralement l'exécutable).
    fn module_principal(&self) -> Option<&process::ProcessModule> {
        self.modules().first()
    }
    /// Lit `count` bytes à l'adresse `addr`. Retourne None si échec.
    fn lire_bytes(&self, addr: usize, count: usize) -> Option<Vec<u8>>;
    /// Lit un pointeur (4 ou 8 bytes selon l'architecture).
    fn lire_pointeur(&self, addr: usize) -> Option<usize>;
    // --- Lectures typées (chaque impl fournit sa propre lecture little-endian) ---
    fn lire_i32(&self, addr: usize) -> Option<i32>;
    fn lire_u32(&self, addr: usize) -> Option<u32>;
    fn lire_i64(&self, addr: usize) -> Option<i64>;
    fn lire_u64(&self, addr: usize) -> Option<u64>;
    fn lire_f32(&self, addr: usize) -> Option<f32>;
    fn lire_f64(&self, addr: usize) -> Option<f64>;
    fn lire_u8(&self, addr: usize) -> Option<u8>;
    fn lire_i8(&self, addr: usize) -> Option<i8>;
    fn lire_i16(&self, addr: usize) -> Option<i16>;
    fn lire_u16(&self, addr: usize) -> Option<u16>;
    fn lire_bool(&self, addr: usize) -> Option<bool> { self.lire_u8(addr).map(|b| b != 0) }
    /// Lit une string (détection auto ASCII/UTF16) jusqu'à `max_bytes` ou null.
    fn lire_string(&self, addr: usize, max_bytes: usize) -> Option<String> {
        let bytes = self.lire_bytes(addr, max_bytes)?;
        if bytes.len() >= 2 && bytes[1] == 0 {
            let utf16: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&c| c != 0)
                .collect();
            Some(String::from_utf16_lossy(&utf16))
        } else {
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from_utf8(bytes[..end].to_vec()).ok()
        }
    }
}

/// Type de valeur mémoire lu via DeepPointer (équivalent des types ASL).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MemValue {
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

impl MemValue {
    /// Convertit un nom de type ASL (ex: "int", "float", "string16") en MemValue
    /// en lisant la valeur à l'adresse pointée par `ptr` dans le processus.
    pub fn from_type_name(type_name: &str, ptr: &deep_pointer::DeepPointer, proc: &dyn LecteurMemoire) -> Option<Self> {
        match type_name {
            "int" => ptr.deref_i32(proc).map(Self::Int),
            "uint" => ptr.deref_u32(proc).map(Self::UInt),
            "long" => ptr.deref_i64(proc).map(Self::Long),
            "ulong" => ptr.deref_u64(proc).map(Self::ULong),
            "float" => ptr.deref_f32(proc).map(Self::Float),
            "double" => ptr.deref_f64(proc).map(Self::Double),
            "byte" => ptr.deref_u8(proc).map(Self::Byte),
            "sbyte" => ptr.deref_i8(proc).map(Self::SByte),
            "short" => ptr.deref_i16(proc).map(Self::Short),
            "ushort" => ptr.deref_u16(proc).map(Self::UShort),
            "bool" => ptr.deref_bool(proc).map(Self::Bool),
            s if s.starts_with("string") => {
                let len: usize = s[6..].parse().ok()?;
                ptr.deref_string(proc, len).map(Self::String)
            }
            b if b.starts_with("byte") && !b.starts_with("sbyte") => {
                let len: usize = b[4..].parse().ok()?;
                ptr.deref_bytes(proc, len).map(Self::Bytes)
            }
            _ => None,
        }
    }
}
