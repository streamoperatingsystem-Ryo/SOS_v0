/// Input Viewer — capture globale clavier + souris → overlay diffusion.
///
/// Port de l'ancienne app RUST_SOS_2026 (features/input_viewer.rs +
/// capture_clavier.rs), simplifié pour l'architecture v0 :
///   - Clavier : poll `GetAsyncKeyState` 60Hz dans un thread dédié (l'ancienne
///     app pollait à 125Hz via un moteur à subscribers — la v0 n'a pas de
///     raccourcis clavier, un poll simple suffit). Émission UNIQUEMENT sur
///     changement d'état (front down/up).
///   - Souris : hook Win32 `WH_MOUSE_LL` (event-driven, zéro coût au repos) —
///     boutons G/M/D immédiats, déplacements throttlés 50ms (20Hz). Le hook
///     est installé et désinstallé sur le MÊME thread (boucle de messages
///     dédiée — leçon de l'ancienne app).
///   - Manette + pavé numérique : lot 2.
///
/// Émission : WS direct via chat_tx (`input-viewer-etat`) — throttle 30Hz +
/// skip si signature identique (le CEF d'OBS saccade si bombardé). Pas de
/// détour par le dashboard (l'ancienne app faisait Rust → event Tauri → store
/// → invoke → serveur : moins de copies, moins de latence ici).
///
/// Non énergivore : AUCUN thread ni hook quand la capture est OFF. Le poll
/// clavier est trivial (256 appels GetAsyncKeyState / 16ms). Le hook souris
/// est event-driven (la boucle GetMessageW dort quand il ne se passe rien).
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// Période du poll clavier (60Hz — 2× moins que les 125Hz de l'ancienne app,
/// imperceptible à l'affichage).
const PERIODE_POLL_MS: u64 = 16;
/// Throttle publication WS (30Hz max — le CEF d'OBS saccade si bombardé).
const THROTTLE_PUBLICATION_MS: u128 = 33;
/// Throttle maj position souris dans l'état (20Hz — leçon ancienne app).
const THROTTLE_SOURIS_MS: u128 = 50;

static DIRTY: AtomicBool = AtomicBool::new(false);
/// ID du thread souris (pour PostThreadMessageW WM_QUIT à l'arrêt).
static THREAD_SOURIS_ID: AtomicU32 = AtomicU32::new(0);

// --- Slots statiques (les callbacks Win32 n'ont pas de contexte Tauri) ---
fn touches_slot() -> &'static Mutex<BTreeSet<&'static str>> {
    static T: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(BTreeSet::new()))
}
fn souris_slot() -> &'static Mutex<EtatSouris> {
    static S: OnceLock<Mutex<EtatSouris>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(EtatSouris::default()))
}
fn derniere_maj_souris_slot() -> &'static Mutex<Instant> {
    static D: OnceLock<Mutex<Instant>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Instant::now() - Duration::from_secs(1)))
}
fn manette_slot() -> &'static Mutex<EtatManette> {
    static M: OnceLock<Mutex<EtatManette>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(EtatManette::default()))
}
fn chat_tx_slot() -> &'static Mutex<Option<broadcast::Sender<String>>> {
    static T: OnceLock<Mutex<Option<broadcast::Sender<String>>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

// =============================================================================
// STRUCTURES SÉRIALISABLES (payload WS)
// =============================================================================

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct EtatSouris {
    pub gauche: bool,
    pub milieu: bool,
    pub droite: bool,
    pub x: i32,
    pub y: i32,
}

/// État manette (20 boutons + 6 axes). Index boutons gilrs/XInput standard :
/// 0=South(A/✕), 1=East(B/○), 2=West(X/□), 3=North(Y/△),
/// 4=LB/L1, 5=RB/R1, 6=LT/L2, 7=RT/R2,
/// 8=Select/Share, 9=Start/Options, 10=L3, 11=R3,
/// 12=DPadUp, 13=DPadDown, 14=DPadLeft, 15=DPadRight.
/// Axes : 0=LeftX, 1=LeftY, 2=RightX, 3=RightY, 4=LeftZ(LT), 5=RightZ(RT).
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct EtatManette {
    pub connectee: bool,
    pub boutons: Vec<bool>,
    pub axes: Vec<f32>,
}

// =============================================================================
// MAPPING vkCode Windows → code HTML KeyboardEvent.code
// (porté de capture_clavier.rs — table statique, zéro allocation par event)
// =============================================================================

/// Retourne `None` pour les vkCodes non mappés.
/// NB : les modificateurs génériques (0x10/0x11/0x12) ne sont JAMAIS pollés —
/// on utilise les variantes L/R (0xA0-0xA5) pour la différenciation.
fn vk_vers_code_html(vk: u32) -> Option<&'static str> {
    // Lettres A-Z (0x41-0x5A) → KeyA-KeyZ
    if (0x41..=0x5A).contains(&vk) {
        let lettres = [
            "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI",
            "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR",
            "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
        ];
        return lettres.get((vk - 0x41) as usize).copied();
    }
    // Chiffres 0-9 (0x30-0x39) → Digit0-Digit9
    if (0x30..=0x39).contains(&vk) {
        let chiffres = [
            "Digit0", "Digit1", "Digit2", "Digit3", "Digit4",
            "Digit5", "Digit6", "Digit7", "Digit8", "Digit9",
        ];
        return chiffres.get((vk - 0x30) as usize).copied();
    }
    // Numpad 0-9 (0x60-0x69) → Numpad0-Numpad9
    if (0x60..=0x69).contains(&vk) {
        let numpad = [
            "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4",
            "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
        ];
        return numpad.get((vk - 0x60) as usize).copied();
    }
    // F1-F12 (vk 0x70-0x7B)
    if (0x70..=0x7B).contains(&vk) {
        let fkeys = [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
        ];
        return fkeys.get((vk - 0x70) as usize).copied();
    }
    // Touches spéciales (table statique)
    let specials: &[(u32, &str)] = &[
        (0x08, "Backspace"),
        (0x09, "Tab"),
        (0x0D, "Enter"),
        (0x14, "CapsLock"),
        (0x1B, "Escape"),
        (0x20, "Space"),
        (0x21, "PageUp"),
        (0x22, "PageDown"),
        (0x23, "End"),
        (0x24, "Home"),
        (0x25, "ArrowLeft"),
        (0x26, "ArrowUp"),
        (0x27, "ArrowRight"),
        (0x28, "ArrowDown"),
        (0x2C, "PrintScreen"),
        (0x2D, "Insert"),
        (0x2E, "Delete"),
        (0x90, "NumLock"),
        (0x91, "ScrollLock"),
        (0x5B, "MetaLeft"),
        (0x5C, "MetaRight"),
        (0x5D, "ContextMenu"),
        // Variantes L/R des modificateurs (les génériques 0x10-0x12 sont ignorés)
        (0xA0, "ShiftLeft"),
        (0xA1, "ShiftRight"),
        (0xA2, "ControlLeft"),
        (0xA3, "ControlRight"),
        (0xA4, "AltLeft"),
        (0xA5, "AltRight"),
        // Numpad opérateurs
        (0x6A, "NumpadMultiply"),
        (0x6B, "NumpadAdd"),
        (0x6D, "NumpadSubtract"),
        (0x6E, "NumpadDecimal"),
        (0x6F, "NumpadDivide"),
        // Ponctuation (labels AZERTY/QWERTY gérés côté rendu)
        (0xBA, "Semicolon"),
        (0xBB, "Equal"),
        (0xBC, "Comma"),
        (0xBD, "Minus"),
        (0xBE, "Period"),
        (0xBF, "Slash"),
        (0xC0, "Backquote"),
        (0xDB, "BracketLeft"),
        (0xDC, "Backslash"),
        (0xDD, "BracketRight"),
        (0xDE, "Quote"),
        (0xE2, "IntlBackslash"),
        (0x32, "IntlBackslash"), // VK_OEM_102 (alt) — fallback < sur certains claviers
    ];
    specials.iter().find(|(k, _)| *k == vk).map(|(_, v)| *v)
}

// =============================================================================
// CLAVIER — poll GetAsyncKeyState (une itération = scan des 256 vkCodes)
// =============================================================================

/// Un passage de poll : détecte les fronts (down/up) sur les 256 vkCodes, met
/// à jour l'ensemble des touches pressées. Retourne true si changement.
#[cfg(windows)]
fn poll_clavier_une_fois(etat_precedent: &mut [bool; 256]) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    let mut change = false;
    unsafe {
        for vk in 0x08u32..=0xFE {
            if vk == 0x10 || vk == 0x11 || vk == 0x12 {
                continue; // modificateurs génériques → variantes L/R à la place
            }
            let presse = GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0;
            let idx = vk as usize;
            if etat_precedent[idx] != presse {
                etat_precedent[idx] = presse;
                if let Some(code) = vk_vers_code_html(vk) {
                    let mut t = touches_slot().lock().unwrap();
                    if presse {
                        t.insert(code);
                    } else {
                        t.remove(code);
                    }
                    drop(t);
                    change = true;
                }
            }
        }
    }
    change
}

// =============================================================================
// SOURIS — hook WH_MOUSE_LL (event-driven, zéro coût au repos)
// =============================================================================
#[cfg(windows)]
mod souris_win32 {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM, HINSTANCE};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, MSG, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, HHOOK, MSLLHOOKSTRUCT, WH_MOUSE_LL,
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN,
        WM_RBUTTONUP,
    };

    const HC_ACTION: i32 = 0;

    fn est_bouton(msg: u32) -> bool {
        msg == WM_LBUTTONDOWN
            || msg == WM_LBUTTONUP
            || msg == WM_RBUTTONDOWN
            || msg == WM_RBUTTONUP
            || msg == WM_MBUTTONDOWN
            || msg == WM_MBUTTONUP
    }

    /// Callback hook souris : maj boutons (immédiat) + position (throttle
    /// 50ms). Doit rester ULTRA rapide (il bloque le curseur système) — on ne
    /// fait qu'écrire l'état + marquer DIRTY ; la publication WS est faite
    /// par le thread moteur (throttle 30Hz).
    pub unsafe extern "system" fn callback(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let msg = wparam.0 as u32;
            let mut s = super::souris_slot().lock().unwrap();
            let mut bouton_change = false;
            match msg {
                WM_LBUTTONDOWN if !s.gauche => { s.gauche = true; bouton_change = true; }
                WM_LBUTTONUP if s.gauche => { s.gauche = false; bouton_change = true; }
                WM_RBUTTONDOWN if !s.droite => { s.droite = true; bouton_change = true; }
                WM_RBUTTONUP if s.droite => { s.droite = false; bouton_change = true; }
                WM_MBUTTONDOWN if !s.milieu => { s.milieu = true; bouton_change = true; }
                WM_MBUTTONUP if s.milieu => { s.milieu = false; bouton_change = true; }
                _ => {}
            }
            // Position : maj d'état toujours (trivial), marquage DIRTY throttlé
            // 50ms (20Hz) pour les déplacements purs.
            s.x = ms.pt.x;
            s.y = ms.pt.y;
            let publier_position = !est_bouton(msg)
                && Instant::now()
                    .duration_since(*super::derniere_maj_souris_slot().lock().unwrap())
                    .as_millis()
                    >= THROTTLE_SOURIS_MS;
            if bouton_change || publier_position {
                *super::derniere_maj_souris_slot().lock().unwrap() = Instant::now();
                drop(s);
                DIRTY.store(true, Ordering::Relaxed);
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Installe le hook souris global. Doit être appelé sur le thread qui
    /// tournera la boucle de messages (contrainte SetWindowsHookExW).
    pub fn installer() -> Option<isize> {
        unsafe {
            let module = GetModuleHandleW(None).ok()?;
            // HINSTANCE(module.0) : hmod = handle du module (0 = processus courant).
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(callback), HINSTANCE(module.0), 0)
                .ok()?;
            Some(hook.0 as isize)
        }
    }

    /// Désinstalle le hook (MÊME thread que l'installation — obligatoire).
    pub fn desinstaller(handle: isize) {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(handle as *mut _));
        }
    }

    /// Boucle de messages — nécessaire pour que les hooks LL reçoivent les
    /// événements. GetMessageW est BLOQUANT (zéro CPU au repos) ; l'arrêt se
    /// fait via PostThreadMessageW(WM_QUIT) depuis set_actif(false).
    pub fn boucle_messages() {
        unsafe {
            let mut msg = MSG::default();
            // GetMessageW retourne FALSE sur WM_QUIT → sortie propre.
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

// =============================================================================
// MANETTE — XInput direct (Windows) + gilrs (fallback cross-platform)
// -----------------------------------------------------------------------------
// gilrs 0.11 a un bug avec certaines manettes XInput (ex: 8BitDo M30) : la
// manette est détectée mais aucun event n'est reçu. On utilise donc l'API
// XInput de Windows directement (priorité), avec gilrs en fallback pour les
// manettes non-XInput (ex: DualSense PS5 en D-Input).
// =============================================================================
#[cfg(windows)]
fn poll_xinput() -> Option<(Vec<bool>, Vec<f32>)> {
    use windows::Win32::UI::Input::XboxController::*;
    unsafe {
        for user_index in 0..4u32 {
            let mut state = XINPUT_STATE::default();
            if XInputGetState(user_index, &mut state) == 0 {
                let gp = state.Gamepad;
                let w: u16 = gp.wButtons.0;
                const DPAD_UP: u16 = 0x0001;
                const DPAD_DOWN: u16 = 0x0002;
                const DPAD_LEFT: u16 = 0x0004;
                const DPAD_RIGHT: u16 = 0x0008;
                const START_BTN: u16 = 0x0010;
                const BACK: u16 = 0x0020;
                const LEFT_THUMB: u16 = 0x0040;
                const RIGHT_THUMB: u16 = 0x0080;
                const LEFT_SHOULDER: u16 = 0x0100;
                const RIGHT_SHOULDER: u16 = 0x0200;
                const A: u16 = 0x1000;
                const B: u16 = 0x2000;
                const X: u16 = 0x4000;
                const Y: u16 = 0x8000;
                let mut boutons = vec![
                    w & A != 0,           // 0 = South
                    w & B != 0,           // 1 = East
                    w & X != 0,           // 2 = West
                    w & Y != 0,           // 3 = North
                    w & LEFT_SHOULDER != 0,  // 4 = LB
                    w & RIGHT_SHOULDER != 0, // 5 = RB
                    gp.bLeftTrigger > 30,    // 6 = LT (seuil 30/255)
                    gp.bRightTrigger > 30,   // 7 = RT
                    w & BACK != 0,        // 8 = Select/Back
                    w & START_BTN != 0,   // 9 = Start
                    w & LEFT_THUMB != 0,  // 10 = L3
                    w & RIGHT_THUMB != 0, // 11 = R3
                    w & DPAD_UP != 0,     // 12 = DPadUp
                    w & DPAD_DOWN != 0,   // 13 = DPadDown
                    w & DPAD_LEFT != 0,   // 14 = DPadLeft
                    w & DPAD_RIGHT != 0,  // 15 = DPadRight
                    false, false, false, false, // 16-19 = extra
                ];
                // XInput Y positif = haut ; gilrs/frontend : -1 = haut → nier Y.
                let mut lx = gp.sThumbLX as f32 / 32767.0;
                let mut ly = -(gp.sThumbLY as f32 / 32767.0);
                let mut rx = gp.sThumbRX as f32 / 32767.0;
                let mut ry = -(gp.sThumbRY as f32 / 32767.0);
                // Fallback D-pad sur axes (8BitDo M30/ProFight en mode XInput :
                // le D-pad analogique peut être reporté sur sThumbLX/sThumbLY
                // OU sThumbRX/sThumbRY, PAS sur wButtons). Si wButtons D-pad est
                // inactif mais les axes sont aux extrêmes, on mappe vers le D-pad.
                // Convention XInput : sThumbLY > 0 = haut → ly < 0 = haut.
                let dpad_wbuttons_actif = boutons[12] || boutons[13] || boutons[14] || boutons[15];
                if !dpad_wbuttons_actif {
                    let seuil = 0.5;
                    // Essai stick gauche d'abord
                    if ly < -seuil { boutons[12] = true; }
                    if ly > seuil { boutons[13] = true; }
                    if lx < -seuil { boutons[14] = true; }
                    if lx > seuil { boutons[15] = true; }
                    let dpad_via_l = boutons[12] || boutons[13] || boutons[14] || boutons[15];
                    if dpad_via_l {
                        lx = 0.0;
                        ly = 0.0;
                    } else {
                        // Essai stick droit si le gauche n'a rien détecté
                        if ry < -seuil { boutons[12] = true; }
                        if ry > seuil { boutons[13] = true; }
                        if rx < -seuil { boutons[14] = true; }
                        if rx > seuil { boutons[15] = true; }
                        if boutons[12] || boutons[13] || boutons[14] || boutons[15] {
                            rx = 0.0;
                            ry = 0.0;
                        }
                    }
                }
                let axes = vec![
                    lx,
                    ly,
                    rx,
                    ry,
                    gp.bLeftTrigger as f32 / 255.0,
                    gp.bRightTrigger as f32 / 255.0,
                ];
                return Some((boutons, axes));
            }
        }
    }
    None
}

/// Thread manette : poll XInput (priorité Windows) + gilrs (fallback) à 60Hz.
/// Émet l'état dans manette_slot() → publier_etat() le diffuse via WS.
pub fn demarrer_thread_manette(actif: Arc<AtomicBool>) {
    thread::spawn(move || {
        // gilrs init (fallback pour manettes non-XInput comme DualSense).
        // Sur Windows, XInput est prioritaire — gilrs peut échouer sans impact.
        let mut gilrs: Option<gilrs::Gilrs> = match gilrs::Gilrs::new() {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("[InputViewer] gilrs init échouée : {e}");
                None
            }
        };

        // Mapping index → gilrs::Button (gilrs 0.11).
        let boutons_gilrs: [gilrs::Button; 16] = [
            gilrs::Button::South,      // 0
            gilrs::Button::East,       // 1
            gilrs::Button::West,       // 2
            gilrs::Button::North,      // 3
            gilrs::Button::LeftTrigger,  // 4 (LB)
            gilrs::Button::RightTrigger, // 5 (RB)
            gilrs::Button::South, // 6 placeholder (LT = axe LeftZ)
            gilrs::Button::South, // 7 placeholder (RT = axe RightZ)
            gilrs::Button::Select,    // 8
            gilrs::Button::Start,     // 9
            gilrs::Button::LeftThumb,   // 10 (L3)
            gilrs::Button::RightThumb,  // 11 (R3)
            gilrs::Button::DPadUp,    // 12
            gilrs::Button::DPadDown,  // 13
            gilrs::Button::DPadLeft,  // 14
            gilrs::Button::DPadRight, // 15
        ];

        while actif.load(Ordering::Relaxed) {
            // Traiter les events gilrs (connexion/déconnexion).
            if let Some(ref mut gilrs) = gilrs {
                while gilrs.next_event().is_some() {
                    // Les events sont traités ci-dessous par poll direct.
                }
            }

            // Priorité XInput (Windows) ; fallback gilrs.
            let (boutons, axes, connectee, _chemin) = {
                #[cfg(windows)]
                {
                    if let Some((xb, xa)) = poll_xinput() {
                        (xb, xa, true, "XInput")
                    } else {
                        let (b, a, c) = poll_gilrs_fallback(gilrs.as_mut(), &boutons_gilrs);
                        (b, a, c, "gilrs")
                    }
                }
                #[cfg(not(windows))]
                {
                    let (b, a, c) = poll_gilrs_fallback(gilrs.as_mut(), &boutons_gilrs);
                    (b, a, c, "gilrs")
                }
            };

            let nouvel_etat = EtatManette { connectee, boutons, axes };
            let mut m = manette_slot().lock().unwrap();
            if *m != nouvel_etat {
                *m = nouvel_etat;
                DIRTY.store(true, Ordering::Relaxed);
            }
            drop(m);

            thread::sleep(Duration::from_millis(16)); // 60Hz
        }
    });
}

/// Fallback gilrs : lit la première manette connectée (manettes non-XInput
/// comme DualSense PS5 en D-Input). Inclut le fallback D-pad sur axes
/// (DPadX/DPadY puis LeftStickX/LeftStickY pour les manettes rétro 8BitDo).
fn poll_gilrs_fallback(
    gilrs: Option<&mut gilrs::Gilrs>,
    boutons_gilrs: &[gilrs::Button; 16],
) -> (Vec<bool>, Vec<f32>, bool) {
    let Some(gilrs) = gilrs else {
        return (vec![false; 20], vec![0.0; 6], false);
    };
    let manette = gilrs.gamepads().find(|(_, gp)| gp.is_connected());
    if let Some((_, gp)) = manette {
        let mut b = vec![false; 20];
        for i in 0..16usize {
            if i == 6 {
                let lt = gp.axis_data(gilrs::Axis::LeftZ).map(|d| d.value()).unwrap_or(0.0);
                b[i] = lt > 0.1;
            } else if i == 7 {
                let rt = gp.axis_data(gilrs::Axis::RightZ).map(|d| d.value()).unwrap_or(0.0);
                b[i] = rt > 0.1;
            } else {
                b[i] = gp.is_pressed(boutons_gilrs[i]);
            }
        }

        // Détection manette rétro 8BitDo (vendor ID 0x2dc8) ou par nom.
        let vid_8bitdo = gp.vendor_id() == Some(0x2dc8);
        let nom_manette = gp.name().to_lowercase();
        let est_retro = vid_8bitdo
            || ["8bitdo", "m30", "zero 2", "sfc30", "snes30", "fc30", "nes30", "gbros", "modkit", "arcade stick", "fight stick"]
                .iter()
                .any(|motif| nom_manette.contains(motif));

        // D-pad : on lit TOUJOURS les axes DPadX/DPadY (lecture directe, pas
        // d'événements) car gilrs a un bug d'événements avec 8BitDo — les
        // boutons D-pad ne sont JAMAIS mis à jour via is_pressed(), mais les
        // axes sont lus directement et fonctionnent. Priorité :
        //   1. Axes DPadX/DPadY (si mappés)
        //   2. Axes LeftStickX/LeftStickY (rétro 8BitDo M30 en D-Input)
        //   3. Boutons D-pad (fallback si pas d'axes du tout)
        let dpad_axes_mappes = gp.axis_code(gilrs::Axis::DPadX).is_some()
            || gp.axis_code(gilrs::Axis::DPadY).is_some();

        let mut lstick_x = gp.axis_data(gilrs::Axis::LeftStickX).map(|d| d.value()).unwrap_or(0.0);
        let mut lstick_y = gp.axis_data(gilrs::Axis::LeftStickY).map(|d| d.value()).unwrap_or(0.0);

        let mut dpad_via_axes = false;
        let seuil = 0.5;

        if dpad_axes_mappes {
            // Axes DPadX/DPadY disponibles — lecture directe (contourne le
            // bug d'événements gilrs). Convention gilrs : -1 = haut.
            let dpad_x = gp.axis_data(gilrs::Axis::DPadX).map(|d| d.value()).unwrap_or(0.0);
            let dpad_y = gp.axis_data(gilrs::Axis::DPadY).map(|d| d.value()).unwrap_or(0.0);
            if dpad_x < -seuil { b[14] = true; dpad_via_axes = true; }
            if dpad_x > seuil { b[15] = true; dpad_via_axes = true; }
            if dpad_y < -seuil { b[12] = true; dpad_via_axes = true; }
            if dpad_y > seuil { b[13] = true; dpad_via_axes = true; }
        } else if est_retro {
            // Fallback rétro : D-pad sur LeftStickX/Y (8BitDo M30 en D-Input).
            // Convention gilrs : -1 = haut.
            if lstick_x < -seuil { b[14] = true; dpad_via_axes = true; }
            if lstick_x > seuil { b[15] = true; dpad_via_axes = true; }
            if lstick_y < -seuil { b[12] = true; dpad_via_axes = true; }
            if lstick_y > seuil { b[13] = true; dpad_via_axes = true; }
        }

        // Si le D-pad a été lu via les axes LeftStick (rétro), on zéro les
        // axes pour éviter que le joystick fantôme ne bouge en même temps.
        if dpad_via_axes && est_retro && !dpad_axes_mappes {
            lstick_x = 0.0;
            lstick_y = 0.0;
        }

        let a = vec![
            lstick_x,
            lstick_y,
            gp.axis_data(gilrs::Axis::RightStickX).map(|d| d.value()).unwrap_or(0.0),
            gp.axis_data(gilrs::Axis::RightStickY).map(|d| d.value()).unwrap_or(0.0),
            gp.axis_data(gilrs::Axis::LeftZ).map(|d| d.value()).unwrap_or(0.0),
            gp.axis_data(gilrs::Axis::RightZ).map(|d| d.value()).unwrap_or(0.0),
        ];
        (b, a, true)
    } else {
        (vec![false; 20], vec![0.0; 6], false)
    }
}

// =============================================================================
// PUBLICATION WS — throttle 30Hz + skip si signature identique
// =============================================================================

/// Publie l'état complet sur le WS (touches + souris). Skip si la signature
/// est identique à la dernière publication (le CEF d'OBS saccade si bombardé).
/// `derniere_signature` : "" au boot → la première publication part toujours.
fn publier_etat(derniere_signature: &mut String) {
    let touches: Vec<String> = touches_slot()
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let souris = souris_slot().lock().unwrap().clone();
    let manette = manette_slot().lock().unwrap().clone();
    let msg = serde_json::json!({
        "type": "input-viewer-etat",
        "etat": {
            "touches": touches,
            "souris": souris,
            "manette": manette,
        },
    });
    let sig = msg.to_string();
    if sig == *derniere_signature {
        return;
    }
    *derniere_signature = sig;
    if let Ok(guard) = chat_tx_slot().lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(msg.to_string());
        }
    }
}

// =============================================================================
// ÉTAT TAURI
// =============================================================================

/// État partagé Input Viewer. La capture est GLOBALE (un seul jeu de threads
/// pour tous les widgets input-viewer) — démarrée/arrêtée par le bouton ON/OFF
/// des options du widget.
#[derive(Clone)]
pub struct InputViewerState {
    pub chat_tx: broadcast::Sender<String>,
    actif: Arc<AtomicBool>,
}

impl InputViewerState {
    /// Crée l'état (capture inactive au boot — démarrée par le bouton ON/OFF).
    pub fn new(chat_tx: broadcast::Sender<String>) -> Self {
        *chat_tx_slot().lock().unwrap() = Some(chat_tx.clone());
        Self {
            chat_tx,
            actif: Arc::new(AtomicBool::new(false)),
        }
    }

    /// true si la capture est active.
    pub fn est_actif(&self) -> bool {
        self.actif.load(Ordering::Relaxed)
    }

    /// Démarre ou arrête la capture globale (clavier + souris).
    /// ON  → thread moteur (poll clavier 60Hz + publication WS 30Hz) + hook
    ///       souris WH_MOUSE_LL (thread dédié + boucle de messages).
    /// OFF → threads stoppés + hook désinstallé + état réinitialisé + dernier
    ///       état vide publié (éteint les touches à l'écran).
    pub fn set_actif(&self, actif: bool) -> Result<(), String> {
        if actif == self.actif.load(Ordering::Relaxed) {
            return Ok(()); // déjà dans cet état
        }
        if actif {
            *chat_tx_slot().lock().unwrap() = Some(self.chat_tx.clone());
            self.actif.store(true, Ordering::Relaxed);

            // Thread moteur : poll clavier 60Hz + publication WS 30Hz.
            // Détaché — sort proprement quand ACTIF passe à false.
            let actif_flag = self.actif.clone();
            thread::spawn(move || {
                let mut etat_precedent = [false; 256];
                let mut derniere_publication = Instant::now() - Duration::from_secs(1);
                let mut derniere_signature = String::new();
                while actif_flag.load(Ordering::Relaxed) {
                    #[cfg(windows)]
                    let change_clavier = poll_clavier_une_fois(&mut etat_precedent);
                    #[cfg(not(windows))]
                    let change_clavier = false;
                    let dirty_souris = DIRTY.swap(false, Ordering::Relaxed);
                    if change_clavier || dirty_souris {
                        let maintenant = Instant::now();
                        if maintenant.duration_since(derniere_publication).as_millis()
                            >= THROTTLE_PUBLICATION_MS
                        {
                            publier_etat(&mut derniere_signature);
                            derniere_publication = maintenant;
                        } else if !dirty_souris {
                            // Changement clavier throttlé → re-marquer pour ne
                            // pas perdre le front (la prochaine itération, 16ms
                            // plus tard, publiera).
                            DIRTY.store(true, Ordering::Relaxed);
                        }
                    }
                    thread::sleep(Duration::from_millis(PERIODE_POLL_MS));
                }
            });

            // Thread souris : installe le hook WH_MOUSE_LL PUIS tourne la
            // boucle de messages — OBLIGATOIREMENT le même thread (leçon
            // ancienne app : SetWindowsHookExW exige que la boucle tourne sur
            // le thread du hook). Sortie propre via WM_QUIT posté à l'arrêt.
            #[cfg(windows)]
            {
                let actif_flag = self.actif.clone();
                thread::spawn(move || {
                    use windows::Win32::System::Threading::GetCurrentThreadId;
                    // Mémoriser l'ID du thread courant (pour le WM_QUIT d'arrêt).
                    let id = unsafe { GetCurrentThreadId() };
                    THREAD_SOURIS_ID.store(id, Ordering::Relaxed);

                    // Installer le hook (sur CE thread).
                    let Some(hook) = souris_win32::installer() else {
                        eprintln!("[InputViewer] échec installation hook souris");
                        actif_flag.store(false, Ordering::Relaxed);
                        return;
                    };

                    // Boucle de messages — bloque (zéro CPU au repos) jusqu'au
                    // WM_QUIT posté par set_actif(false).
                    souris_win32::boucle_messages();

                    // Désinstallation sur le MÊME thread que l'installation.
                    souris_win32::desinstaller(hook);
                });
            }

            // Thread manette : poll XInput (Windows) + gilrs (fallback) 60Hz.
            // Détaché — sort proprement quand ACTIF passe à false.
            demarrer_thread_manette(self.actif.clone());
        } else {
            self.actif.store(false, Ordering::Relaxed);
            // Réveiller la boucle de messages du thread souris (WM_QUIT →
            // GetMessageW retourne false → le thread désinstalle le hook).
            #[cfg(windows)]
            {
                let id = THREAD_SOURIS_ID.load(Ordering::Relaxed);
                if id != 0 {
                    use windows::Win32::Foundation::{LPARAM, WPARAM};
                    use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
                    unsafe {
                        let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
                    }
                }
            }
            // Réinitialiser l'état + publier un état vide (éteint les touches).
            touches_slot().lock().unwrap().clear();
            *souris_slot().lock().unwrap() = EtatSouris::default();
            *manette_slot().lock().unwrap() = EtatManette::default();
            let mut vide = String::new();
            publier_etat(&mut vide);
        }
        Ok(())
    }
}
