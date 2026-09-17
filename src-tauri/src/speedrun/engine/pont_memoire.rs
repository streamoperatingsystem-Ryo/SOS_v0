// =============================================================================
// Pont mémoire JS ↔ Rust — callbacks natifs Boa pour lecture mémoire processus
// -----------------------------------------------------------------------------
// Les scripts ASL (transpilés en JS) accèdent à la mémoire du jeu via des
// objets JS (DeepPointer, MemoryWatcher, SignatureScanner) dont les méthodes
// .Deref()/.Update()/.Scan() délèguent à des callbacks globaux :
//
//   __rust_deref(module, base, offsets, type, extra)  → valeur JS (number/string/bool/null)
//   __rust_file_exists(path)  → bool
//   __rust_file_read(path)    → string
//   __sigscan_callback(game, address, size, signatures) → number (adresse)
//
// Ces callbacks sont enregistrés comme NativeFunction Boa (from_fn_ptr —
// pointeurs de fonction simples). L'état du lecteur mémoire (processus jeu
// courant) est stocké dans un thread_local : tout vit dans le thread de
// polling du moteur ASL, donc pas de contrainte Send (le handle Win32 n'est
// pas Send). Le moteur (asl_engine.rs) appelle connecter_lecteur /
// deconnecter_lecteur depuis le même thread.
//
// Règle §2 : on étend le moteur existant (DeepPointer/MemoryWatcher Rust),
// on ne crée pas un 2e système de lecture mémoire.
// =============================================================================
use std::cell::RefCell;
use std::rc::Rc;

use crate::speedrun::memory::deep_pointer::DeepPointer;
use crate::speedrun::memory::signature_scanner::{SigScanTarget, SignatureScanner};
use crate::speedrun::memory::LecteurMemoire;

// =============================================================================
// État partagé (thread_local — thread de polling du moteur ASL)
// =============================================================================
thread_local! {
    /// Le processus jeu courant (None tant qu'aucun jeu n'est connecté).
    /// Rc<dyn LecteurMemoire> : partagé entre le pont mémoire (lectures JS via
    /// __rust_deref) et l'EngineInner (is_alive, serialize_process, watchers).
    /// Tout vit dans le thread de polling du moteur ASL → pas de contrainte
    /// Send (le handle Win32 n'est pas Send). Le handle est fermé une seule
    /// fois quand le dernier Rc est libéré.
    static LECTEUR: RefCell<Option<Rc<dyn LecteurMemoire>>> = const { RefCell::new(None) };
}

/// Connecte un processus jeu au pont mémoire (appelé depuis le thread engine).
/// Le même Rc est conservé par l'EngineInner (self.process) pour is_alive /
/// serialize_process / watchers.update_all — d'où le partage via Rc.
pub fn connecter_lecteur(process: Rc<dyn LecteurMemoire>) {
    LECTEUR.with(|l| {
        *l.borrow_mut() = Some(process);
    });
}

/// Déconnecte le processus jeu (appelé depuis le thread engine).
pub fn deconnecter_lecteur() {
    LECTEUR.with(|l| {
        *l.borrow_mut() = None;
    });
}

/// Retourne true si un processus est connecté au pont mémoire.
pub fn lecteur_est_connecte() -> bool {
    LECTEUR.with(|l| l.borrow().is_some())
}

// =============================================================================
// Logique de lecture (appelée par les NativeFunction Boa)
// =============================================================================

/// Effectue une lecture mémoire typée via DeepPointer.
/// Retourne une valeur sérialisable en JSON pour Boa.
pub fn deref_memoire(
    module: Option<&str>,
    base: i32,
    offsets: &[i32],
    type_str: &str,
    extra: usize,
) -> Option<LectureMemoire> {
    LECTEUR.with(|l| {
        let guard = l.borrow();
        let proc = guard.as_ref()?;
        let module_str = module.unwrap_or("");
        let ptr = if module_str.is_empty() {
            DeepPointer::from_absolute(base, offsets)
        } else {
            DeepPointer::new(module_str, base, offsets)
        };
        match type_str {
            "int" => ptr.deref_i32(proc.as_ref()).map(LectureMemoire::Int),
            "uint" => ptr.deref_u32(proc.as_ref()).map(|v| LectureMemoire::Int(v as i32)),
            "long" => ptr.deref_i64(proc.as_ref()).map(LectureMemoire::Long),
            "ulong" => ptr.deref_u64(proc.as_ref()).map(|v| LectureMemoire::Long(v as i64)),
            "float" => ptr.deref_f32(proc.as_ref()).map(|v| LectureMemoire::Float(v as f64)),
            "double" => ptr.deref_f64(proc.as_ref()).map(LectureMemoire::Float),
            "bool" => ptr.deref_bool(proc.as_ref()).map(LectureMemoire::Bool),
            "byte" => ptr.deref_u8(proc.as_ref()).map(|v| LectureMemoire::Int(v as i32)),
            "sbyte" => ptr.deref_i8(proc.as_ref()).map(|v| LectureMemoire::Int(v as i32)),
            "short" => ptr.deref_i16(proc.as_ref()).map(|v| LectureMemoire::Int(v as i32)),
            "ushort" => ptr.deref_u16(proc.as_ref()).map(|v| LectureMemoire::Int(v as i32)),
            "string" => ptr.deref_string(proc.as_ref(), extra.max(1)).map(LectureMemoire::String),
            "bytes" => ptr.deref_bytes(proc.as_ref(), extra.max(1)).map(LectureMemoire::Bytes),
            _ => None,
        }
    })
}

/// Scan de signature dans une région mémoire.
pub fn sigscan_memoire(address: usize, size: usize, signatures: &[SignatureJson]) -> Option<usize> {
    LECTEUR.with(|l| {
        let guard = l.borrow();
        let proc = guard.as_ref()?;
        let mut target = SigScanTarget::new();
        for sig in signatures {
            // Reconstruire la string hex depuis pattern + mask
            let mut sig_str = String::new();
            for (i, &byte) in sig.pattern.iter().enumerate() {
                if sig.mask.get(i).copied().unwrap_or(false) {
                    sig_str.push_str("??");
                } else {
                    sig_str.push_str(&format!("{:02X}", byte));
                }
            }
            target.add_signature_string(sig.offset, &sig_str);
        }
        let scanner = SignatureScanner::new(proc.as_ref(), address, size);
        scanner.scan(&target)
    })
}

// =============================================================================
// Types de passage (sérialisables pour Boa)
// =============================================================================

/// Valeur lue en mémoire (sérialisable en JSON pour Boa).
#[derive(Clone, Debug, serde::Serialize)]
#[serde(untagged)]
pub enum LectureMemoire {
    Int(i32),
    Long(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
}

/// Signature reçue depuis le JS (pattern + mask + offset).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct SignatureJson {
    pub pattern: Vec<u8>,
    pub mask: Vec<bool>,
    pub offset: i32,
}
