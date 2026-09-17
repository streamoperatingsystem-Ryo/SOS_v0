// =============================================================================
// Process — Wrapper processus jeu pour lecture mémoire (port de ProcessExtensions.cs)
// -----------------------------------------------------------------------------
// Fournit :
//   - Ouverture de handle avec PROCESS_ALL_ACCESS (OpenProcess)
//   - Énumération des modules du processus (EnumProcessModulesEx)
//   - Lecture mémoire (ReadProcessMemory) : bytes, valeurs typées, pointeurs, strings
//   - Détection 32/64 bits (IsWow64Process)
//   - Vérification que le processus est vivant (GetExitCodeProcess)
//
// Toutes les API Windows sont appelées via le crate `windows`.
// =============================================================================
use std::ffi::c_void;
use std::mem;

use windows::Win32::Foundation::{CloseHandle, HMODULE, HANDLE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, ENUM_PROCESS_MODULES_EX_FLAGS, GetModuleBaseNameW,
    GetModuleFileNameExW, GetModuleInformation, MODULEINFO,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_ALL_ACCESS};

use super::LecteurMemoire;

/// Représente un module chargé dans le processus jeu.
#[derive(Clone, Debug)]
pub struct ProcessModule {
    pub base_address: usize,
    pub module_memory_size: u32,
    pub entry_point: usize,
    pub module_name: String,
    pub file_name: String,
}

/// Wrapper d'un processus Windows ouvert pour lecture mémoire.
pub struct Process {
    pub id: u32,
    handle: HANDLE,
    pub name: String,
    is_64_bit: bool,
    modules: Vec<ProcessModule>,
}

impl LecteurMemoire for Process {
    fn nom(&self) -> &str { &self.name }
    fn pid(&self) -> u32 { self.id }
    fn est_64_bit(&self) -> bool { self.is_64_bit }
    fn est_vivant(&self) -> bool { self.is_alive() }
    fn modules(&self) -> &[ProcessModule] { &self.modules }
    fn lire_bytes(&self, addr: usize, count: usize) -> Option<Vec<u8>> { self.read_bytes(addr, count) }
    fn lire_pointeur(&self, addr: usize) -> Option<usize> { self.read_pointer(addr) }
    fn lire_i32(&self, addr: usize) -> Option<i32> { self.read_i32(addr) }
    fn lire_u32(&self, addr: usize) -> Option<u32> { self.read_u32(addr) }
    fn lire_i64(&self, addr: usize) -> Option<i64> { self.read_value::<i64>(addr) }
    fn lire_u64(&self, addr: usize) -> Option<u64> { self.read_value::<u64>(addr) }
    fn lire_f32(&self, addr: usize) -> Option<f32> { self.read_f32(addr) }
    fn lire_f64(&self, addr: usize) -> Option<f64> { self.read_f64(addr) }
    fn lire_u8(&self, addr: usize) -> Option<u8> { self.read_u8(addr) }
    fn lire_i8(&self, addr: usize) -> Option<i8> { self.read_value::<i8>(addr) }
    fn lire_i16(&self, addr: usize) -> Option<i16> { self.read_value::<i16>(addr) }
    fn lire_u16(&self, addr: usize) -> Option<u16> { self.read_value::<u16>(addr) }
    fn lire_bool(&self, addr: usize) -> Option<bool> { self.read_bool(addr) }
    fn lire_string(&self, addr: usize, max_bytes: usize) -> Option<String> { self.read_string(addr, max_bytes) }
}

impl Process {
    /// Tente de trouver un processus par nom et d'ouvrir un handle avec
    /// PROCESS_ALL_ACCESS. Retourne None si le processus n'existe pas ou
    /// si l'ouverture du handle échoue (permissions insuffisantes).
    pub fn find_by_name(name: &str) -> Option<Self> {
        let snap = unsafe {
            windows::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot(
                windows::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPPROCESS,
                0,
            )
        };
        let snap = match snap {
            Ok(h) => h,
            Err(_) => return None,
        };

        let mut entry = windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W {
            dwSize: mem::size_of::<windows::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W>()
                as u32,
            ..Default::default()
        };

        let mut found: Option<(u32, String)> = None;

        let ok = unsafe {
            windows::Win32::System::Diagnostics::ToolHelp::Process32FirstW(snap, &mut entry)
        };
        if ok.is_ok() {
            loop {
                let proc_name = String::from_utf16_lossy(&entry.szExeFile);
                let proc_name = proc_name.trim_end_matches('\0').to_lowercase();
                // E8 Bug A — le script ASL référence le process sans extension
                // (state("mgsi") → cherche "mgsi", mais Windows liste "mgsi.exe").
                // On compare donc sans l'extension .exe. On garde aussi le nom
                // complet pour le log (proc.name = "mgsi.exe").
                let proc_name_sans_ext = proc_name
                    .strip_suffix(".exe")
                    .unwrap_or(&proc_name)
                    .to_string();
                if proc_name_sans_ext == name.to_lowercase() {
                    found = Some((entry.th32ProcessID, proc_name));
                    break;
                }
                let ok = unsafe {
                    windows::Win32::System::Diagnostics::ToolHelp::Process32NextW(snap, &mut entry)
                };
                if ok.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snap) };

        let (pid, proc_name) = found?;

        // Ouvrir le handle avec PROCESS_ALL_ACCESS
        let handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid) };
        let handle = match handle {
            Ok(h) => h,
            Err(e) => {
                log::warn!("[Speedrun] OpenProcess failed for PID {} ({}): {}", pid, name, e);
                return None;
            }
        };

        // Détecter 32 vs 64 bits
        let mut is_wow64 = windows::Win32::Foundation::BOOL::default();
        let _ = unsafe {
            windows::Win32::System::Threading::IsWow64Process(handle, &mut is_wow64)
        };
        let is_64_bit = !is_wow64.as_bool(); // Si ce n'est pas WoW64, c'est natif 64-bit

        let mut proc = Self {
            id: pid,
            handle,
            name: proc_name,
            is_64_bit,
            modules: Vec::new(),
        };

        // Pré-charger les modules
        proc.modules = proc.enum_modules_internal().unwrap_or_default();

        Some(proc)
    }

    /// Retourne true si le processus est 64-bit.
    pub fn is_64_bit(&self) -> bool {
        self.is_64_bit
    }

    /// Retourne true si le processus est encore en vie.
    pub fn is_alive(&self) -> bool {
        let mut exit_code: u32 = 0;
        let ok = unsafe {
            windows::Win32::System::Threading::GetExitCodeProcess(self.handle, &mut exit_code)
        };
        ok.is_ok() && exit_code == 259 // STILL_ACTIVE = 259
    }

    /// Énumère les modules chargés dans le processus.
    pub fn modules(&self) -> &[ProcessModule] {
        &self.modules
    }

    /// Recherche un module par nom (insensible à la casse).
    pub fn find_module(&self, name: &str) -> Option<&ProcessModule> {
        self.modules
            .iter()
            .find(|m| m.module_name.to_lowercase() == name.to_lowercase())
    }

    /// Retourne le module principal (le premier, généralement l'exécutable).
    pub fn main_module(&self) -> Option<&ProcessModule> {
        self.modules.first()
    }

    fn enum_modules_internal(&self) -> Option<Vec<ProcessModule>> {
        unsafe {
            // Premier appel pour obtenir la taille nécessaire
            let mut cb_needed: u32 = 0;
            let ok = EnumProcessModulesEx(
                self.handle,
                std::ptr::null_mut(),
                0,
                &mut cb_needed,
                ENUM_PROCESS_MODULES_EX_FLAGS(0x03), // LIST_MODULES_ALL
            );
            if ok.is_err() || cb_needed == 0 {
                return None;
            }

            let num_mods = (cb_needed / mem::size_of::<HMODULE>() as u32) as usize;
            let mut h_modules: Vec<HMODULE> = vec![HMODULE(std::ptr::null_mut()); num_mods];

            let ok = EnumProcessModulesEx(
                self.handle,
                h_modules.as_mut_ptr(),
                cb_needed,
                &mut cb_needed,
                ENUM_PROCESS_MODULES_EX_FLAGS(0x03),
            );
            if ok.is_err() {
                return None;
            }

            let mut result = Vec::with_capacity(num_mods);
            for h_mod in &h_modules {
                if h_mod.0.is_null() {
                    continue;
                }

                // Nom de base du module
                let mut base_name = [0u16; 260];
                let len = GetModuleBaseNameW(self.handle, *h_mod, &mut base_name);
                let module_name = if len > 0 {
                    String::from_utf16_lossy(&base_name[..len as usize])
                } else {
                    String::new()
                };

                // Chemin complet du module
                let mut file_name = [0u16; 260];
                let len = GetModuleFileNameExW(self.handle, *h_mod, &mut file_name);
                let file_name_str = if len > 0 {
                    String::from_utf16_lossy(&file_name[..len as usize])
                } else {
                    String::new()
                };

                // Informations du module (base, taille, entry point)
                let mut mod_info = MODULEINFO::default();
                let ok = GetModuleInformation(
                    self.handle,
                    *h_mod,
                    &mut mod_info,
                    mem::size_of::<MODULEINFO>() as u32,
                );
                if ok.is_err() {
                    continue;
                }

                result.push(ProcessModule {
                    base_address: mod_info.lpBaseOfDll as usize,
                    module_memory_size: mod_info.SizeOfImage,
                    entry_point: mod_info.EntryPoint as usize,
                    module_name,
                    file_name: file_name_str,
                });
            }

            Some(result)
        }
    }

    /// Lit `count` bytes à l'adresse `addr` dans le processus.
    /// Retourne None si la lecture échoue (adresse invalide, processus mort).
    pub fn read_bytes(&self, addr: usize, count: usize) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; count];
        let mut bytes_read: usize = 0;

        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buffer.as_mut_ptr() as *mut c_void,
                count,
                Some(&mut bytes_read),
            )
        };

        if ok.is_err() || bytes_read != count {
            return None;
        }

        Some(buffer)
    }

    /// Lit une valeur typée à l'adresse `addr` (little-endian).
    pub fn read_value<T: Copy + Default>(&self, addr: usize) -> Option<T> {
        let size = mem::size_of::<T>();
        let bytes = self.read_bytes(addr, size)?;
        let mut val = T::default();
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                &mut val as *mut T as *mut u8,
                size,
            );
        }
        Some(val)
    }

    /// Lit un pointeur (4 ou 8 bytes selon l'architecture du processus).
    pub fn read_pointer(&self, addr: usize) -> Option<usize> {
        if self.is_64_bit {
            self.read_value::<u64>(addr).map(|v| v as usize)
        } else {
            self.read_value::<u32>(addr).map(|v| v as usize)
        }
    }

    /// Lit un i32 à l'adresse donnée.
    pub fn read_i32(&self, addr: usize) -> Option<i32> {
        self.read_value::<i32>(addr)
    }

    /// Lit un u32 à l'adresse donnée.
    pub fn read_u32(&self, addr: usize) -> Option<u32> {
        self.read_value::<u32>(addr)
    }

    /// Lit un f32 à l'adresse donnée.
    pub fn read_f32(&self, addr: usize) -> Option<f32> {
        self.read_value::<f32>(addr)
    }

    /// Lit un f64 à l'adresse donnée.
    pub fn read_f64(&self, addr: usize) -> Option<f64> {
        self.read_value::<f64>(addr)
    }

    /// Lit un u8 à l'adresse donnée.
    pub fn read_u8(&self, addr: usize) -> Option<u8> {
        self.read_value::<u8>(addr)
    }

    /// Lit un bool à l'adresse donnée (1 byte, 0 = false).
    pub fn read_bool(&self, addr: usize) -> Option<bool> {
        self.read_u8(addr).map(|b| b != 0)
    }

    /// Lit une string à l'adresse donnée. Détection auto ASCII/UTF16.
    /// Lit jusqu'à `max_bytes` bytes ou jusqu'à un null terminator.
    pub fn read_string(&self, addr: usize, max_bytes: usize) -> Option<String> {
        let bytes = self.read_bytes(addr, max_bytes)?;

        // Détection : si le 2e byte est 0x00, c'est probablement de l'UTF16
        if bytes.len() >= 2 && bytes[1] == 0 {
            // UTF16-LE
            let utf16: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .take_while(|&c| c != 0)
                .collect();
            Some(String::from_utf16_lossy(&utf16))
        } else {
            // ASCII/UTF8
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from_utf8(bytes[..end].to_vec()).ok()
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}
