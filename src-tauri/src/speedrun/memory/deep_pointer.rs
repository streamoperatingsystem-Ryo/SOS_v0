// =============================================================================
// DeepPointer — Chaîne de pointeurs pour lire une valeur mémoire indirecte
// (port de DeepPointer.cs)
// -----------------------------------------------------------------------------
// Un DeepPointer représente un chemin d'accès à une valeur en mémoire :
//   module + base + offset1 → pointer → + offset2 → pointer → ... → + lastOffset
//
// Exemple ASL : `int Progress : "mgsi.exe", 0x00400000, 0x10, 0x20`
//   → module = "mgsi.exe", base = 0x00400000, offsets = [0x10, 0x20]
//   → addr = module.base + 0x00400000
//   → addr = read_pointer(addr + 0x10)
//   → valeur = read(addr + 0x20)
// =============================================================================
use super::LecteurMemoire;

/// Chaîne de pointeurs pour accéder à une valeur mémoire dans un processus.
#[derive(Clone, Debug)]
pub struct DeepPointer {
    /// Nom du module (ex: "mgsi.exe"). Si None, utilise le module principal.
    module: Option<String>,
    /// Offset de base par rapport au début du module.
    base: i32,
    /// Chaîne d'offsets pour suivre les pointeurs.
    /// Le dernier offset est ajouté au pointeur final (pas déréférencé).
    offsets: Vec<i32>,
    /// Si true, `base` est une adresse absolue (ex: F.Addr(0x38D7CA) = 0x400000 + offset).
    /// Si false, `base` est relative au module (ou au module principal si module=None).
    is_absolute: bool,
}

impl DeepPointer {
    /// Crée un DeepPointer avec module, base et offsets.
    /// `module` = nom du module (ex: "mgsi.exe")
    /// `base` = offset par rapport au début du module
    /// `offsets` = chaîne d'offsets (le dernier est l'offset final de la valeur)
    pub fn new(module: &str, base: i32, offsets: &[i32]) -> Self {
        Self {
            module: Some(module.to_lowercase()),
            base,
            offsets: offsets.to_vec(),
            is_absolute: false,
        }
    }

    /// Crée un DeepPointer avec base absolue (sans module).
    /// `base` est l'adresse absolue en mémoire (ex: 0x78D7CA pour F.Addr(0x38D7CA)).
    /// Les watchers JS créés dans init() via `new MemoryWatcher<T>(F.Addr(offset))`
    /// passent une adresse absolue — ne PAS ajouter module.base_address.
    pub fn from_absolute(base: i32, offsets: &[i32]) -> Self {
        Self {
            module: None,
            base,
            offsets: offsets.to_vec(),
            is_absolute: true,
        }
    }

    /// Crée un DeepPointer depuis une variable de `state()` ASL, avec la
    /// sémantique exacte de LiveSplit `ComponentUtil.DeepPointer`.
    ///
    /// Deux corrections vs `DeepPointer::new` (RCA-1 + RCA-2 SoR/Fusion) :
    ///
    /// 1. **module=None → module principal** (pas le nom du process). LiveSplit
    ///    utilise `process.MainModuleWow64Safe()` quand state() n'a pas de
    ///    module explicite. Avant ce fix, `construire_watchers` passait
    ///    `process_name` comme module → `"Fusion"` ne matchait jamais
    ///    `"Fusion.exe"` (comparaison exacte dans `trouver_module`) → tous les
    ///    watchers restaient `null` → `current.gameState = null` →
    ///    `start()` = false à vie.
    ///
    /// 2. **Prépend `0` aux offsets** (LiveSplit `InitializeOffsets` : deref la
    ///    base en premier). Avant ce fix, la base était traitée comme l'adresse
    ///    de la valeur au lieu d'un pointeur à déréférencer → lectures à
    ///    mauvaise adresse même après fix du module.
    ///
    /// **Scope** : réservé au chemin state() (`construire_watchers`). NE PAS
    /// utiliser pour les watchers JS (`pont_memoire`) ni `from_absolute` (MGS) —
    /// la sémantique prepend-0 y est déjà correcte (offsets vides → `[0]` →
    /// `n=1` → loop 0 → `ptr=base` → `ptr+=0=base`, inchangé) mais on isole le
    /// fix pour garantir zéro régression MGS.
    pub fn new_state_var(module: Option<&str>, base: i32, offsets: &[i32]) -> Self {
        let mut full_offsets = Vec::with_capacity(offsets.len() + 1);
        full_offsets.push(0); // LiveSplit InitializeOffsets : deref base first
        full_offsets.extend_from_slice(offsets);
        Self {
            module: module.map(|m| m.to_lowercase()),
            base,
            offsets: full_offsets,
            is_absolute: false,
        }
    }

    /// Accesseurs (pour sérialisation / tests).
    pub fn module(&self) -> Option<&str> { self.module.as_deref() }
    pub fn base(&self) -> i32 { self.base }
    pub fn offsets(&self) -> &[i32] { &self.offsets }

    /// Suit la chaîne de pointeurs et retourne l'adresse finale de la valeur.
    /// Retourne None si un pointeur intermédiaire est invalide ou null.
    pub fn deref_offsets(&self, proc: &dyn LecteurMemoire) -> Option<usize> {
        // Calculer l'adresse de base
        let mut ptr = if self.is_absolute {
            // Adresse absolue (ex: F.Addr(offset) = 0x400000 + offset) — ne PAS
            // ajouter module.base_address, base est déjà l'adresse finale.
            self.base as usize
        } else if let Some(ref module_name) = self.module {
            let module = proc.trouver_module(module_name)?;
            module.base_address.wrapping_add(self.base as usize)
        } else {
            // Sans module et non absolu : utiliser le module principal
            let main = proc.module_principal()?;
            main.base_address.wrapping_add(self.base as usize)
        };

        // Suivre les offsets intermédiaires (tous sauf le dernier)
        let n = self.offsets.len();
        if n == 0 {
            return Some(ptr);
        }

        for i in 0..n - 1 {
            let offset = self.offsets[i] as isize;
            ptr = proc.lire_pointeur((ptr as isize + offset) as usize)?;
            if ptr == 0 {
                return None;
            }
        }

        // Ajouter le dernier offset (sans déréférencer)
        let last_offset = self.offsets[n - 1] as isize;
        Some((ptr as isize + last_offset) as usize)
    }

    // --- Lectures typées (équivalent des méthodes Deref<T> en C#) ---

    pub fn deref_i32(&self, proc: &dyn LecteurMemoire) -> Option<i32> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_i32(addr)
    }

    pub fn deref_u32(&self, proc: &dyn LecteurMemoire) -> Option<u32> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_u32(addr)
    }

    pub fn deref_i64(&self, proc: &dyn LecteurMemoire) -> Option<i64> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_i64(addr)
    }

    pub fn deref_u64(&self, proc: &dyn LecteurMemoire) -> Option<u64> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_u64(addr)
    }

    pub fn deref_f32(&self, proc: &dyn LecteurMemoire) -> Option<f32> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_f32(addr)
    }

    pub fn deref_f64(&self, proc: &dyn LecteurMemoire) -> Option<f64> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_f64(addr)
    }

    pub fn deref_u8(&self, proc: &dyn LecteurMemoire) -> Option<u8> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_u8(addr)
    }

    pub fn deref_i8(&self, proc: &dyn LecteurMemoire) -> Option<i8> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_i8(addr)
    }

    pub fn deref_i16(&self, proc: &dyn LecteurMemoire) -> Option<i16> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_i16(addr)
    }

    pub fn deref_u16(&self, proc: &dyn LecteurMemoire) -> Option<u16> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_u16(addr)
    }

    pub fn deref_bool(&self, proc: &dyn LecteurMemoire) -> Option<bool> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_bool(addr)
    }

    pub fn deref_string(&self, proc: &dyn LecteurMemoire, max_bytes: usize) -> Option<String> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_string(addr, max_bytes)
    }

    pub fn deref_bytes(&self, proc: &dyn LecteurMemoire, count: usize) -> Option<Vec<u8>> {
        let addr = self.deref_offsets(proc)?;
        proc.lire_bytes(addr, count)
    }
}

// =============================================================================
// TESTS — new_state_var (RCA-1 + RCA-2 SoR/Fusion)
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::speedrun::memory::process::ProcessModule;

    /// Mock d'un processus simulant Kega Fusion : module principal "Fusion.exe"
    /// à la base 0x00400000. À l'adresse (base + 0x2A52D4) se trouve un pointeur
    /// vers la RAM émulée (0x00FF0000). Le byte gameState se trouve à
    /// 0x00FF0000 + 0xFF01 = 0x00FFFF01, valeur 0x16 (InGame).
    struct MockFusion {
        modules: Vec<ProcessModule>,
        mem: std::collections::HashMap<usize, Vec<u8>>,
    }

    impl MockFusion {
        fn new() -> Self {
            let mut mem = std::collections::HashMap::new();
            // Pointeur vers la RAM émulée à (module.base + 0x2A52D4).
            // LiveSplit deref la base en premier (prepend 0) → read_ptr(base+0).
            let ptr_addr = 0x00400000usize + 0x2A52D4;
            let ram_base = 0x00FF0000u32;
            mem.insert(ptr_addr, ram_base.to_le_bytes().to_vec());
            // gameState (byte) à ram_base + 0xFF01 = 0x00FFFF01, valeur 0x16.
            mem.insert((ram_base as usize) + 0xFF01, vec![0x16]);
            Self {
                modules: vec![ProcessModule {
                    base_address: 0x00400000,
                    module_memory_size: 0x800000,
                    entry_point: 0x00401000,
                    module_name: "Fusion.exe".to_string(),
                    file_name: "C:\\Fusion\\Fusion.exe".to_string(),
                }],
                mem,
            }
        }
    }

    impl LecteurMemoire for MockFusion {
        fn nom(&self) -> &str { "Fusion.exe" }
        fn pid(&self) -> u32 { 4321 }
        fn est_64_bit(&self) -> bool { false }
        fn est_vivant(&self) -> bool { true }
        fn modules(&self) -> &[ProcessModule] { &self.modules }
        fn lire_bytes(&self, addr: usize, count: usize) -> Option<Vec<u8>> {
            let data = self.mem.get(&addr)?;
            if data.len() >= count {
                Some(data[..count].to_vec())
            } else {
                let mut out = data.clone();
                out.resize(count, 0);
                Some(out)
            }
        }
        fn lire_pointeur(&self, addr: usize) -> Option<usize> {
            let b = self.lire_bytes(addr, 4)?;
            Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        }
        fn lire_i32(&self, addr: usize) -> Option<i32> {
            let b = self.lire_bytes(addr, 4)?;
            Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        }
        fn lire_u32(&self, addr: usize) -> Option<u32> {
            let b = self.lire_bytes(addr, 4)?;
            Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        }
        fn lire_i64(&self, addr: usize) -> Option<i64> {
            let b = self.lire_bytes(addr, 8)?;
            Some(i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        }
        fn lire_u64(&self, addr: usize) -> Option<u64> {
            let b = self.lire_bytes(addr, 8)?;
            Some(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        }
        fn lire_f32(&self, addr: usize) -> Option<f32> {
            let b = self.lire_bytes(addr, 4)?;
            Some(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        }
        fn lire_f64(&self, addr: usize) -> Option<f64> {
            let b = self.lire_bytes(addr, 8)?;
            Some(f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        }
        fn lire_u8(&self, addr: usize) -> Option<u8> {
            let b = self.lire_bytes(addr, 1)?;
            Some(b[0])
        }
        fn lire_i8(&self, addr: usize) -> Option<i8> {
            let b = self.lire_bytes(addr, 1)?;
            Some(b[0] as i8)
        }
        fn lire_i16(&self, addr: usize) -> Option<i16> {
            let b = self.lire_bytes(addr, 2)?;
            Some(i16::from_le_bytes([b[0], b[1]]))
        }
        fn lire_u16(&self, addr: usize) -> Option<u16> {
            let b = self.lire_bytes(addr, 2)?;
            Some(u16::from_le_bytes([b[0], b[1]]))
        }
        fn lire_bool(&self, addr: usize) -> Option<bool> {
            self.lire_u8(addr).map(|b| b != 0)
        }
        fn lire_string(&self, addr: usize, max_bytes: usize) -> Option<String> {
            let b = self.lire_bytes(addr, max_bytes)?;
            String::from_utf8(b).ok()
        }
    }

    /// RCA-1 + RCA-2 : `new_state_var(None, ...)` doit utiliser le module
    /// principal ("Fusion.exe") ET prépend 0 (deref base) → lire le byte à
    /// read_ptr(0x400000 + 0x2A52D4) + 0xFF01 = 0xFF0000 + 0xFF01 = 0x16.
    /// Avant le fix : `DeepPointer::new("Fusion", ...)` ne trouvait pas le
    /// module ("fusion" != "fusion.exe") → retournait None.
    #[test]
    fn test_new_state_var_module_vide_uses_main_module_and_prepends_zero() {
        let proc = MockFusion::new();
        // state("Fusion") { byte gameState : 0x2A52D4, 0xFF01; }
        let ptr = DeepPointer::new_state_var(None, 0x2A52D4, &[0xFF01]);
        let val = ptr.deref_u8(&proc);
        assert_eq!(val, Some(0x16), "gameState doit être lu via module principal + deref base");
    }

    /// RCA-1 (régression) : l'ancien chemin `DeepPointer::new("Fusion", ...)`
    /// ne trouve pas le module "Fusion.exe" → None. Ce test documente le bug
    /// pour empêcher toute régression vers process_name comme module.
    #[test]
    fn test_regression_ancien_chemin_new_avec_process_name_echoue() {
        let proc = MockFusion::new();
        // Ancien code : DeepPointer::new("Fusion", 0x2A52D4, &[0xFF01])
        // → module = "fusion" → trouver_module("fusion") ne match pas "Fusion.exe".
        let ptr = DeepPointer::new("Fusion", 0x2A52D4, &[0xFF01]);
        assert_eq!(
            ptr.deref_u8(&proc),
            None,
            "DeepPointer::new(\"Fusion\", ...) ne doit pas trouver le module (documente RCA-1)"
        );
    }

    /// RCA-2 (régression) : sans le prepend 0, `new_state_var` lirait à
    /// module.base + 0x2A52D4 + 0xFF01 (mauvaise adresse) au lieu de
    /// read_ptr(module.base + 0x2A52D4) + 0xFF01. Ce test vérifie que le
    /// prepend est bien actif en comparant avec un DeepPointer sans prepend.
    #[test]
    fn test_new_state_var_prepend_zero_vs_new_sans_prepend() {
        let proc = MockFusion::new();
        // new_state_var (avec prepend 0) → deref base → bonne adresse → 0x16.
        let ptr_fix = DeepPointer::new_state_var(Some("Fusion.exe"), 0x2A52D4, &[0xFF01]);
        assert_eq!(ptr_fix.deref_u8(&proc), Some(0x16));
        // new (sans prepend) → module.base + 0x2A52D4 + 0xFF01 = mauvaise adresse
        // (pas de valeur dans le mock à 0x400000 + 0x2A52D4 + 0xFF01) → None.
        let ptr_buggy = DeepPointer::new("Fusion.exe", 0x2A52D4, &[0xFF01]);
        assert_eq!(
            ptr_buggy.deref_u8(&proc),
            None,
            "Sans prepend 0, la lecture tombe à mauvaise adresse (documente RCA-2)"
        );
    }

    /// module explicite (ex: SEGAGameRoom "GenesisEmuWrapper.dll") → lookup par
    /// nom de module. Vérifie que new_state_var(Some(module)) utilise bien le
    /// module nommé et le prepend 0.
    #[test]
    fn test_new_state_var_module_explicite() {
        let mut proc = MockFusion::new();
        // Ajouter un 2e module "GenesisEmuWrapper.dll" à base 0x10000000.
        proc.modules.push(ProcessModule {
            base_address: 0x10000000,
            module_memory_size: 0x100000,
            entry_point: 0x10001000,
            module_name: "GenesisEmuWrapper.dll".to_string(),
            file_name: "C:\\SEGA\\GenesisEmuWrapper.dll".to_string(),
        });
        // Pointeur à module.base + 0xB677E8 → 0x20000000.
        let ptr_addr = 0x10000000usize + 0xB677E8;
        proc.mem.insert(ptr_addr, 0x20000000u32.to_le_bytes().to_vec());
        // byte à 0x20000000 + 0xFF00 = 0x2000FF00, valeur 0x16.
        proc.mem.insert(0x20000000usize + 0xFF00, vec![0x16]);
        // state("SEGAGameRoom") { byte gameState : "GenesisEmuWrapper.dll", 0xB677E8, 0xFF00; }
        let ptr = DeepPointer::new_state_var(Some("GenesisEmuWrapper.dll"), 0xB677E8, &[0xFF00]);
        assert_eq!(ptr.deref_u8(&proc), Some(0x16), "module explicite + prepend 0");
    }

    /// Zéro régression MGS : `from_absolute(base, [])` (chemin MGS via
    /// pont_memoire) doit rester inchangé — offsets vides → prepend n'a pas
    /// d'effet (new_state_var non utilisé par MGS, mais on vérifie que
    /// from_absolute n'est pas affecté).
    #[test]
    fn test_from_absolute_chemin_mgs_inchange() {
        let mut proc = MockFusion::new();
        // MGS : F.Addr(offset) = from_absolute(0x400000 + offset, []).
        // Valeur i32 à 0x400000 + 0x1234.
        let abs_addr = 0x00400000usize + 0x1234;
        proc.mem.insert(abs_addr, 42i32.to_le_bytes().to_vec());
        let ptr = DeepPointer::from_absolute((0x00400000 + 0x1234) as i32, &[]);
        assert_eq!(ptr.deref_i32(&proc), Some(42), "from_absolute chemin MGS inchangé");
    }
}
