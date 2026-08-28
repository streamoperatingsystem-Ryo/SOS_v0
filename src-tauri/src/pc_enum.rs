//! Énumération native PC (SANS OBS) : caméras (PnP/DirectShow via PowerShell),
//! fenêtres visibles (EnumWindows), processus. Retourne des listes vivantes
//! du PC pour le dialog « créer la capture depuis SOS ».
//!
//! Windows-only. Sur autres plateformes → listes vides (compilation OK).
#[cfg(windows)]
mod imp {
    use serde::Serialize;
    use std::process::Command;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
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

    /// Callback EnumWindows : collecte les fenêtres visibles avec titre.
    /// LPARAM = *mut Vec<WindowEntry>.
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<WindowEntry>);

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
        let exe = if pid > 0 { exe_name_from_pid(pid) } else { String::new() };

        // Format OBS window property : "title:class:exe"
        let obs_value = format!("{}:{}:{}", title, class, exe);

        list.push(WindowEntry { title, exe, obs_value });
        BOOL(1)
    }

    /// Énumère les fenêtres visibles du PC (EnumWindows + titre + class + exe).
    pub fn enumerate_windows() -> Vec<WindowEntry> {
        let mut list: Vec<WindowEntry> = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut list as *mut Vec<WindowEntry> as isize),
            );
        }
        // Filtrer : titre + exe non vides (évite fenêtres système sans exe).
        list.retain(|w| !w.title.is_empty() && !w.exe.is_empty());
        log::info!("[PC] fenêtres visibles → {} fenêtre(s)", list.len());
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
