// =============================================================================
// Speedrun Splitter — Auto Splitter natif Rust (remplacement du sidecar .NET)
// -----------------------------------------------------------------------------
// Port du moteur ASL (Auto Split Language) de LiveSplit en Rust natif.
// Anciennement : autosplit-sidecar (.NET 8 + Roslyn + Irony) → crashes et
// consommation excessive. Maintenant : 100% Rust, aucun runtime .NET.
//
// Architecture :
//   memory/    — Scan mémoire processus jeu (ReadProcessMemory, DeepPointer,
//                MemoryWatcher, SignatureScanner). Port de ComponentUtil C#.
//   asl/       — Parser ASL (format LiveSplit) — parser manuel robuste.
//                Extrait : state definitions (adresses mémoire) + method bodies
//                (code JavaScript transpilé depuis C#).
//   engine/    — Moteur ASL : timer model, polling loop, intégration Boa JS
//                (moteur script pur Rust, remplace Roslyn C# compilation).
//   commands.rs — Commandes Tauri exposées au frontend Svelte.
//
// Le défi central : les fichiers ASL contiennent du code C# exécutable.
// Au lieu de compiler du C# à la volée (Roslyn — lourd et énergivore),
// on transpile le C# vers JavaScript et on l'exécute avec Boa (pur Rust).
//
// NOTE PLATFORME : Le scan mémoire processus (ReadProcessMemory) et le moteur
// Boa sont liés à l'API Win32 → le module complet est désactivé hors Windows.
// Sur Linux/macOS, des stubs renvoient une erreur explicite pour que le
// frontend puisse afficher un message au lieu de crasher.
//
// NOTE LINTS : `#[allow(dead_code)]` au niveau du module — de nombreuses
// fonctions/structs sont appelées indirectement (via Tauri commands, depuis
// JS via Boa, ou depuis le thread moteur) et le compilateur les voit comme
// inutilisées bien qu'elles le soient à l'exécution.
// =============================================================================
#![allow(dead_code)]
#![allow(clippy::all)]
#[cfg(windows)]
pub mod asl;
#[cfg(windows)]
pub mod engine;
#[cfg(windows)]
pub mod memory;

#[cfg(windows)]
pub mod commands;

// -----------------------------------------------------------------------------
// Stubs non-Windows — commandes Tauri renvoyant une erreur explicite.
// Le frontend reçoit l'erreur et peut afficher un message à l'utilisateur.
// -----------------------------------------------------------------------------
#[cfg(not(windows))]
pub mod commands {
    use tauri::AppHandle;

    const MSG_INDISPONIBLE: &str =
        "Speedrun Splitter non disponible sur cette plateforme (scan mémoire Win32 requis)";

    /// Stub non-Windows de AslLoadResult (le vrai type est dans engine/asl_engine.rs,
    /// module Windows-only). Sérialisé vide car la commande retourne toujours Err.
    #[derive(serde::Serialize)]
    pub struct AslLoadResultStub {
        pub settings: Vec<serde_json::Value>,
        pub settings_detailles: Vec<serde_json::Value>,
        pub noms_splits: Vec<serde_json::Value>,
    }

    #[tauri::command]
    pub fn speedrun_charger_asl(_app_handle: AppHandle, _path: String) -> Result<AslLoadResultStub, String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_charger_lss(_path: String) -> Result<String, String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_demarrer(
        _app_handle: AppHandle,
        _total_splits: i32,
        _start: bool,
        _split: bool,
        _reset: bool,
    ) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_arreter(_app_handle: AppHandle) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_maj_settings(
        _app_handle: AppHandle,
        _start: bool,
        _split: bool,
        _reset: bool,
    ) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_est_actif() -> bool {
        false
    }

    #[tauri::command]
    pub fn speedrun_action_manuelle(_app_handle: AppHandle, _action: String) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_lire_config(_app_handle: AppHandle) -> Result<crate::config::SpeedrunConfig, String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_sauver_config(
        _app_handle: AppHandle,
        _config: crate::config::SpeedrunConfig,
    ) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }

    #[tauri::command]
    pub fn speedrun_maj_settings_asl(
        _app_handle: AppHandle,
        _valeurs: std::collections::HashMap<String, bool>,
    ) -> Result<(), String> {
        Err(MSG_INDISPONIBLE.to_string())
    }
}
