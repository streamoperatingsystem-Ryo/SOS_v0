//! Énumération native PC (SANS OBS) : caméras (PnP/DirectShow via PowerShell),
//! fenêtres visibles (EnumWindows), processus. Retourne des listes vivantes
//! du PC pour le dialog « créer la capture depuis SOS ».
//!
//! Windows-only. Sur autres plateformes → listes vides (compilation OK).
#[cfg(windows)]
mod imp {
    use serde::Serialize;
    use std::collections::HashMap;
    use std::process::Command;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    /// Fenêtre visible : titre + exe + valeur OBS ("title:class:exe").
    #[derive(Debug, Clone, Serialize)]
    pub struct WindowEntry {
        pub title: String,
        pub exe: String,
        pub obs_value: String,
    }

    /// Énumère les caméras du PC via PowerShell (Get-PnpDevice -Class Camera).
    /// Retourne les FriendlyName = valeurs video_device_id pour OBS dshow_input.
    /// Fallback WMI si Get-PnpDevice indisponible (Windows 7/8).
    pub fn enumerate_cameras() -> Vec<String> {
        // Essayer Get-PnpDevice (Windows 10+), fallback WMI.
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "try { Get-PnpDevice -Class Camera -Status OK | Select-Object -ExpandProperty FriendlyName } catch { Get-WmiObject Win32_PnPEntity | Where-Object { $_.PNPClass -eq 'Camera' -or $_.PNPClass -eq 'Image' } | Where-Object { $_.Status -eq 'OK' } | Select-Object -ExpandProperty Name }",
            ])
            .output();

        let cameras = match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
            _ => Vec::new(),
        };

        log::info!("[PC] caméras → {} device(s)", cameras.len());
        cameras
    }

    /// Récupère le nom de l'exe d'un process via son PID (QueryFullProcessImageName).
    unsafe fn exe_name_from_pid(pid: u32) -> String {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        let Ok(h) = handle else {
            return String::new();
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = h; // CloseHandle via Drop
        if ok.is_ok() && len > 0 {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit('\\').next().unwrap_or("").to_string()
        } else {
            String::new()
        }
    }

    /// Construit une carte PID → nom d'exe via CreateToolhelp32Snapshot.
    /// Plus permissive que OpenProcess : fonctionne pour les process protégés
    /// (anti-cheat, DRM, process élevés) où OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) échoue.
    fn build_pid_exe_map() -> HashMap<u32, String> {
        let mut map = HashMap::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                log::warn!("[PC] CreateToolhelp32Snapshot échec — fallback OpenProcess");
                return map;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_err() {
                log::warn!("[PC] Process32FirstW échec");
                let _ = snap;
                return map;
            }
            loop {
                let exe = String::from_utf16_lossy(&entry.szExeFile);
                let exe = exe.trim_end_matches('\0').to_string();
                if !exe.is_empty() {
                    let name = exe.rsplit('\\').next().unwrap_or(&exe).to_string();
                    map.insert(entry.th32ProcessID, name);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
            let _ = snap; // CloseHandle via Drop
        }
        log::info!("[PC] toolhelp snapshot → {} process(s)", map.len());
        map
    }

    /// Callback EnumWindows : collecte les fenêtres visibles avec titre.
    /// LPARAM = *mut (&mut Vec<WindowEntry>, &HashMap<u32, String>).
    /// Utilise la carte toolhelp pour l'exe (permissif), fallback OpenProcess.
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (list, pid_map) = &mut *(lparam.0 as *mut (&mut Vec<WindowEntry>, &HashMap<u32, String>));

        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let title_len = GetWindowTextLengthW(hwnd);
        if title_len == 0 {
            return BOOL(1);
        }

        let mut title_buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut title_buf);
        if n == 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&title_buf[..n as usize]);

        let mut class_buf = [0u16; 256];
        let cn = GetClassNameW(hwnd, &mut class_buf);
        let class = String::from_utf16_lossy(&class_buf[..cn as usize]);

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        // 1) toolhelp snapshot (permissif, marche pour process protégés)
        // 2) fallback OpenProcess si absent de la carte
        let exe = if pid > 0 {
            pid_map.get(&pid).cloned().unwrap_or_else(|| exe_name_from_pid(pid))
        } else {
            String::new()
        };

        // Format OBS window property : "title:class:exe"
        let obs_value = format!("{}:{}:{}", title, class, exe);

        list.push(WindowEntry { title, exe, obs_value });
        BOOL(1)
    }

    /// Énumère les fenêtres visibles du PC (EnumWindows + titre + class + exe).
    pub fn enumerate_windows() -> Vec<WindowEntry> {
        let pid_map = build_pid_exe_map();
        let mut list: Vec<WindowEntry> = Vec::new();
        unsafe {
            let mut ctx = (&mut list, &pid_map);
            let _ = EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut ctx as *mut _ as isize),
            );
        }
        // Filtrer : titre non vide seulement. L'exe peut être vide pour les
        // process protégés (anti-cheat/DRM) — on garde la fenêtre, OBS match
        // sur title:class même si exe est vide.
        let before = list.len();
        list.retain(|w| !w.title.is_empty());
        let no_exe = list.iter().filter(|w| w.exe.is_empty()).count();
        log::info!(
            "[PC] fenêtres visibles → {} fenêtre(s) ({} sans exe, {} avant filtre titre)",
            list.len(), no_exe, before
        );
        list
    }

    /// Énumère les jeux = fenêtres visibles (game_capture capture_specific_window
    /// utilise le même format "title:class:exe" que window_capture).
    pub fn enumerate_games() -> Vec<WindowEntry> {
        let list = enumerate_windows();
        log::info!("[PC] jeux/fenêtres → {} entrée(s)", list.len());
        list
    }
}

#[cfg(not(windows))]
mod imp {
    use serde::Serialize;

    #[derive(Debug, Clone, Serialize)]
    pub struct WindowEntry {
        pub title: String,
        pub exe: String,
        pub obs_value: String,
    }

    pub fn enumerate_cameras() -> Vec<String> {
        Vec::new()
    }
    pub fn enumerate_windows() -> Vec<WindowEntry> {
        Vec::new()
    }
    pub fn enumerate_games() -> Vec<WindowEntry> {
        Vec::new()
    }
}

pub use imp::*;
