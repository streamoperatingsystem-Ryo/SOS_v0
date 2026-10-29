mod api_deck;
mod alertes;
mod cadres_cmd;
mod commandes;
mod config;
mod ecran_attente;
mod fenetre;
mod fight;
mod just_chatting;
mod pad_numerique;
mod input_viewer;
mod equaliseur;
mod kick;
mod mask_png;
mod musique;
mod moteur;
mod obs;
mod obs_trou;
mod pc_enum;
mod pc_thumb;
mod presets;
mod scene;
mod scenes;
mod server;
mod twitch_auth;
mod twitch_chat;
mod twitch_emotes;
mod twitch_eventsub;
mod viewers;
mod twitch_helix;
mod youtube_auth;
mod youtube_chat;
mod youtube_data;
mod tiktok_chat;
mod twitch_clips;
mod welcome;
mod bandeau;
mod position_overlay;
mod presentation;
mod raccourcis;
mod speedrun;
mod splash;
mod telemetrie;
mod conso;
mod zip_store;
// Les commandes speedrun sont définies dans speedrun::commands (réelles sur
// Windows, stubs sur les autres plateformes). Importées dans le scope pour
// generate_handler!.
use conso::{boite_noire_etat, conso_etat};
use fenetre::{fenetre_appliquer, fenetre_etat};
use raccourcis::{
    raccourcis_appliquer, raccourcis_etat, raccourcis_masquer, raccourcis_moniteurs,
};
use speedrun::commands::{
    speedrun_action_manuelle, speedrun_aller_split, speedrun_arreter,
    speedrun_charger_asl, speedrun_charger_lss,
    speedrun_demarrer, speedrun_est_actif, speedrun_exporter_splits,
    speedrun_lire_config, speedrun_maj_settings, speedrun_maj_settings_asl,
    speedrun_sauver_config,
};
use telemetrie::commands::{
    audience_session, audience_sessions, telemetrie_derniers, telemetrie_etat,
};

use scenes::{ScenesState, SceneIndex};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast;

// ===== Commandes scène (lecture/écriture) =====

#[tauri::command]
fn get_scene(state: tauri::State<ScenesState>) -> scene::Scene {
    state.scene.lock().unwrap().clone()
}

#[tauri::command]
async fn update_scene(app: AppHandle, state: tauri::State<'_, ScenesState>, scene: scene::Scene) -> Result<(), String> {
    // 1. Muter l'état unique. Un cadreWidget différent = édition du cadre
    //    par le streamer → purge de l'override « !cadres » (couleur viewer
    //    éphémère) : la couleur du streamer prime toujours.
    {
        let mut current = state.scene.lock().unwrap();
        if current.cadreWidget != scene.cadreWidget {
            *state.cadres_chat.lock().unwrap() = None;
        }
        *current = scene;
    }
    // 2. Sauver <current_id>.json + snapshot WS (chaîne unique)
    scenes::save_current(&app, &state)?;
    Ok(())
}

// ===== Commandes scènes (v0.14) =====

#[tauri::command]
fn scenes_lister(app: AppHandle) -> Result<Vec<SceneIndex>, String> {
    scenes::lister(&app)
}

/// Résumé de toutes les scènes (nb widgets, types, modules, mini-rects) —
/// alimente le tableau enrichi « Vos scènes » (mini-map procédurale).
#[tauri::command]
fn scenes_resumer(app: AppHandle) -> Result<Vec<scenes::SceneResume>, String> {
    scenes::resumer(&app)
}

#[tauri::command]
fn scene_creer(app: AppHandle, state: tauri::State<ScenesState>, nom: String) -> Result<SceneIndex, String> {
    scenes::creer(&app, &state, &nom)
}

#[tauri::command]
fn scene_ouvrir(app: AppHandle, state: tauri::State<ScenesState>, id: String) -> Result<SceneIndex, String> {
    scenes::ouvrir(&app, &state, &id)
}

#[tauri::command]
fn scene_renommer(app: AppHandle, id: String, nom: String) -> Result<(), String> {
    scenes::renommer(&app, &id, &nom)
}

/// Déplace une scène dans l'index (glisser-déposer barre « Vos scènes »).
#[tauri::command]
fn scene_deplacer(app: AppHandle, id: String, position: usize) -> Result<(), String> {
    scenes::deplacer(&app, &id, position)
}

/// Masque/affiche le titre d'une scène dans sa pastille (œil, onglet orange).
#[tauri::command]
fn scene_masquer_nom(app: AppHandle, id: String, masque: bool) -> Result<(), String> {
    scenes::masquer_nom(&app, &id, masque)
}

#[tauri::command]
fn scene_supprimer(app: AppHandle, state: tauri::State<ScenesState>, id: String) -> Result<(), String> {
    scenes::supprimer(&app, &state, &id)
}

/// Duplique une scène : <id>.json → copie (ids widgets régénérés, obsSource
/// conservé — source OBS partagée), ajoutée en fin de liste, bascule dessus.
#[tauri::command]
fn scene_dupliquer(app: AppHandle, state: tauri::State<ScenesState>, id: String, nom: String) -> Result<SceneIndex, String> {
    scenes::dupliquer(&app, &state, &id, &nom)
}

#[tauri::command]
fn scene_courante(app: AppHandle, state: tauri::State<ScenesState>) -> Result<SceneIndex, String> {
    scenes::courante(&app, &state)
}

// ===== Commandes presets de scènes (modèles embarqués) =====

/// Liste les métadonnées des presets embarqués (galerie). Parse non-fatal :
/// un JSON cassé est loggé + absent de la liste.
#[tauri::command]
fn presets_lister(app: AppHandle) -> Vec<presets::PresetInfo> {
    presets::lister(&app)
}

/// Crée une NOUVELLE scène depuis un preset embarqué (bouton « + Scène » →
/// galerie). L'existante n'est jamais touchée. Ids régénérés + canvas OBS.
#[tauri::command]
fn scene_creer_depuis_preset(
    app: AppHandle,
    state: tauri::State<ScenesState>,
    nom: String,
    preset_id: String,
) -> Result<SceneIndex, String> {
    let preset = presets::charger(&preset_id)?;
    let scene = presets::instancier(&app, &preset);
    scenes::creer_avec(&app, &state, &nom, scene)
}

/// ONBOARDING : initialise la scène de départ (courante, vide au 1er boot)
/// depuis un preset — remplace son contenu + renomme. Contrat : appelée
/// uniquement au premier lancement (voir scenes::initialiser_depuis_preset).
#[tauri::command]
fn scene_initialiser_preset(
    app: AppHandle,
    state: tauri::State<ScenesState>,
    preset_id: String,
) -> Result<SceneIndex, String> {
    let preset = presets::charger(&preset_id)?;
    scenes::initialiser_depuis_preset(&app, &state, &preset)
}

/// État de l'onboarding : true = déjà fait (ne pas montrer la galerie).
#[tauri::command]
fn onboarding_etat(app: AppHandle) -> bool {
    config::lire_onboarding_fait(&app)
}

/// Marque l'onboarding comme fait (choix preset OU page vierge).
#[tauri::command]
fn onboarding_marquer_fait(app: AppHandle) -> Result<(), String> {
    config::marquer_onboarding_fait(&app)
}

/// Langue de l'UI persistée (langue.json). None = jamais choisie → le front
/// détecte navigator.language (fallback "fr").
#[tauri::command]
fn langue_etat(app: AppHandle) -> Option<String> {
    config::lire_langue(&app)
}

/// Persiste la langue de l'UI (langue.json). Garde-fou minimal : code court
/// non vide — la whitelist des langues vit côté front (stores/langue.ts).
#[tauri::command]
fn langue_sauver(app: AppHandle, langue: String) -> Result<(), String> {
    let l = langue.trim();
    if l.is_empty() || l.len() > 10 {
        return Err("code langue invalide".to_string());
    }
    config::sauver_langue(&app, l)
}

#[tauri::command]
fn scene_exporter(app: AppHandle, state: tauri::State<ScenesState>, nom_pack: String) -> Result<bool, String> {
    scenes::exporter(&app, &state, &nom_pack)
}

#[tauri::command]
fn scene_importer(app: AppHandle, state: tauri::State<ScenesState>) -> Result<Option<SceneIndex>, String> {
    scenes::importer(&app, &state)
}

/// Exporte TOUTES les scènes + TOUS les médias dans un seul .zip (dialog
/// « Enregistrer sous » — dossier + nom choisis, un .zip existant est
/// écrasé). Retourne Some(chemin du zip), None si annulé.
#[tauri::command]
async fn scenes_exporter_zip(
    app: AppHandle,
    state: tauri::State<'_, ScenesState>,
) -> Result<Option<String>, String> {
    // Dialog natif + écriture du zip (potentiellement des centaines de Mo)
    // sur le pool bloquant Tokio — jamais sur le thread UI (même recette
    // que les commandes d'import média).
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || scenes::exporter_tout_zip(&app, &state))
        .await
        .map_err(|e| format!("scenes_exporter_zip: {}", e))?
}

/// Phase 1 de l'import « tout en .zip » : dialog « Ouvrir » + analyse de
/// l'archive SANS écriture. Retourne Some({ zipPath, scenes, conflits }) —
/// le frontend tranche chaque conflit (« remplacer ou passer ? ») puis
/// appelle scenes_importer_zip_appliquer — ou None si dialog annulé.
#[tauri::command]
async fn scenes_importer_zip(
    app: AppHandle,
) -> Result<Option<scenes::AnalyseZipImport>, String> {
    tauri::async_runtime::spawn_blocking(move || scenes::choisir_et_analyser_zip(&app))
        .await
        .map_err(|e| format!("scenes_importer_zip: {}", e))?
}

/// Phase 2 : applique les décisions de l'UI — `remplacer` = ids de scènes
/// locales acceptées à l'écrasement (les autres conflits sont passés).
/// Retourne le bilan { scenesImportees, mediasRestaures, scenesSautees }.
#[tauri::command]
async fn scenes_importer_zip_appliquer(
    app: AppHandle,
    state: tauri::State<'_, ScenesState>,
    zip_path: String,
    remplacer: Vec<String>,
) -> Result<scenes::BilanImportZip, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        scenes::appliquer_zip_import(
            &app,
            &state,
            std::path::Path::new(&zip_path),
            &remplacer,
        )
    })
    .await
    .map_err(|e| format!("scenes_importer_zip_appliquer: {}", e))?
}

// ===== Import média (widget + fond) =====

const MAX_IMG_BYTES: u64 = 10 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 80 * 1024 * 1024;
const MAX_AUDIO_BYTES: u64 = 10 * 1024 * 1024;
const IMG_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO_EXT: &[&str] = &["mp4", "webm", "mkv"];
const AUDIO_EXT: &[&str] = &["mp3", "ogg", "wav"];

/// Vérifie les magic bytes pour PNG/JPEG/GIF/WebP.
fn check_magic_image(bytes: &[u8]) -> bool {
    let p = |pre: &[u8]| bytes.starts_with(pre);
    p(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) // PNG
        || p(&[0xFF, 0xD8, 0xFF]) // JPEG
        || p(&[0x47, 0x49, 0x46, 0x38]) // GIF8
        || (bytes.len() >= 12
            && p(&[0x52, 0x49, 0x46, 0x46])
            && bytes[8..12] == [0x57, 0x45, 0x42, 0x50]) // RIFF...WEBP
}

/// Vérifie les magic bytes pour MP4 (boîte `ftyp` à l'offset 4) et WebM/MKV
/// (EBML). MKV et WebM partagent le format conteneur Matroska (magic EBML
/// `1A 45 DF A3`) — la distinction se fait par extension, pas par magic bytes.
/// NB : Chromium ne lit pas le MKV dans un <video> → remux MKV→MP4 à l'import
/// (validate_and_copy_media) via le sidecar ffmpeg.
fn check_magic_video(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp" // MP4 / ISOBMFF
        || bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) // WebM / MKV / EBML
}

/// Vérifie les magic bytes pour MP3 (tag ID3 ou frame sync 0xFFEx), OGG
/// (OggS) et WAV (RIFF…WAVE).
fn check_magic_audio(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") // MP3 + tag ID3
        || (bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0) // frame MP3
        || bytes.starts_with(b"OggS") // OGG
        || (bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE") // WAV
}

/// (kind, extensions autorisées, taille max, validateur magic bytes) d'un média.
type MediaSpec = (&'static str, &'static [&'static str], u64, fn(&[u8]) -> bool);

/// Validation + copie d'un média vers AppData/StreamOS/medias/<uuid>.<ext>
/// (extension + taille + magic bytes). Retourne `(rel, kind)`. Utilisé par
/// pick_and_copy_media et import_alerte_media — aucune mutation de scène ici.
///
/// MKV : Chromium ne lit pas le MKV dans un <video> → remux en MP4 via le
/// sidecar ffmpeg (`-c copy -sn -movflags +faststart`, sans réencodage). Le
/// fichier stocké est `medias/<uuid>.mp4` (kind "video"). Échoue si un flux
/// n'est pas compatible MP4 (ex. HEVC vidéo, audio Opus hors profil) —
/// l'erreur ffmpeg est retournée telle quelle.
fn validate_and_copy_media(app: &AppHandle, src: &std::path::Path) -> Result<(String, String, String), String> {
    validate_and_copy_media_limite(app, src, false)
}

/// Variante interne : `video_sans_limite` = true → pas de cap de taille sur
/// les vidéos (import écran d'attente UNIQUEMENT — la vidéo BRB peut être
/// longue ; les images gardent leur cap 10 Mo). Extension, magic bytes et
/// remux MKV→MP4 inchangés.
fn validate_and_copy_media_limite(
    app: &AppHandle,
    src: &std::path::Path,
    video_sans_limite: bool,
) -> Result<(String, String, String), String> {
    use std::fs;
    use std::io::Read;

    // 0. Nom original du fichier (pour affichage dans la carte d'édition).
    let nom_original = src
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("média")
        .to_string();

    // 1. Extension → kind + liste autorisée
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .ok_or_else(|| "Extension manquante".to_string())?;

    let (kind, allowed_ext, max_bytes, magic_ok): MediaSpec =
        if IMG_EXT.contains(&ext.as_str()) {
            ("image", IMG_EXT, MAX_IMG_BYTES, check_magic_image)
        } else if VIDEO_EXT.contains(&ext.as_str()) {
            ("video", VIDEO_EXT, MAX_VIDEO_BYTES, check_magic_video)
        } else {
            return Err(format!("Extension .{} non autorisée", ext));
        };

    // 2. Taille ≤ limite du kind (sauf vidéo écran d'attente : pas de cap —
    //    le média BRB peut durer plusieurs minutes sans saturer le quota).
    let meta = fs::metadata(src).map_err(|e| format!("metadata: {}", e))?;
    if meta.len() > max_bytes && !(video_sans_limite && kind == "video") {
        return Err(format!(
            "Fichier trop volumineux ({} octets > {} Mo)",
            meta.len(),
            max_bytes / 1024 / 1024
        ));
    }

    // 3. Magic bytes (selon le kind)
    let mut f = fs::File::open(src).map_err(|e| format!("open: {}", e))?;
    let mut head = [0u8; 32];
    let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
    if !magic_ok(&head[..n]) {
        return Err("Format non supporté (magic bytes invalides)".into());
    }

    // 4. (re-vérif extension déjà faite au §1 — allowed_ext cohérent avec kind)
    let _ = allowed_ext;

    // 5. Copie (ou remux MKV→MP4) vers AppData/StreamOS/medias/<uuid>.<dest_ext>
    let dir = config::data_dir(app)?;
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    // MKV → remux en MP4 (lisible navigateur) ; autres extensions → copie brute.
    let dest_ext: &str = if ext == "mkv" { "mp4" } else { ext.as_str() };
    let dest_name = format!("{}.{}", uuid, dest_ext);
    let dest = dir.join("medias").join(&dest_name);
    if ext == "mkv" {
        remux_to_mp4(app, src, &dest)?;
    } else {
        fs::copy(src, &dest).map_err(|e| format!("copy: {}", e))?;
    }

    let rel = format!("medias/{}", dest_name);
    Ok((rel, kind.to_string(), nom_original))
}

/// Remuxe un MKV en MP4 via le sidecar ffmpeg (`-c copy -sn -movflags
/// +faststart`). Sans réencodage — rapide, préserve la qualité. Échoue si un
/// flux n'est pas compatible MP4 (ex. HEVC, Opus hors profil). Bloquant :
/// atteint uniquement via les commandes `async` + `spawn_blocking` des imports
/// — JAMAIS sur le thread UI (gel « Ne répond plus », corrigé 2026-09-27).
fn remux_to_mp4(app: &AppHandle, src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    let sidecar = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| format!("ffmpeg sidecar indisponible: {}", e))?;
    let mut cmd = std::process::Command::from(sidecar);
    // CREATE_NO_WINDOW : ffmpeg est un exe console — sans ce flag, Windows
    // crée une console visible à chaque remux en build GUI installée
    // (flash + vol de focus, même classe de bug que le watch caméra).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd
        .arg("-y")
        .arg("-i")
        .arg(src)
        .arg("-c")
        .arg("copy")
        .arg("-sn")
        .arg("-movflags")
        .arg("+faststart")
        .arg(dest)
        .output()
        .map_err(|e| format!("ffmpeg spawn: {}", e))?;
    if !out.status.success() {
        // Nettoyer le fichier partiel éventuel.
        let _ = std::fs::remove_file(dest);
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Remux MKV→MP4 échoué: {}", stderr.trim()));
    }
    Ok(())
}

/// Helper commun d'import média (widget OU fond de scène) : dialog fichier
/// « Médias » → validation (extension + taille + magic bytes) → copie (ou
/// remux MKV→MP4) vers AppData/StreamOS/medias/<uuid>.<ext>. Retourne
/// `Some((rel, kind))` si importé, `None` si dialog annulé.
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo. Vidéo : mp4/webm/mkv ≤ 80 Mo
/// (mkv remuxé en mp4 via sidecar ffmpeg) — sauf `video_sans_limite`
/// (écran d'attente : pas de cap vidéo). Aucune mutation de la scène —
/// l'appelant mutera le widget ciblé ou les champs fond après coup.
/// ⚠️ BLOQUANT (dialog natif + I/O disque + remux) : n'appeler que depuis
/// `spawn_blocking` dans une commande `async` — jamais sur le thread UI.
fn pick_and_copy_media(
    app: &AppHandle,
    video_sans_limite: bool,
) -> Result<Option<(String, String, String)>, String> {
    use tauri_plugin_dialog::DialogExt;

    // 1. Dialog fichier (un seul filtre « Médias » : images + vidéos)
    let mut all_ext: Vec<&str> = IMG_EXT.to_vec();
    all_ext.extend_from_slice(VIDEO_EXT);
    let file = app
        .dialog()
        .file()
        .add_filter("Médias", &all_ext)
        .blocking_pick_file();
    let Some(file) = file else {
        return Ok(None); // dialog annulé
    };
    let src: std::path::PathBuf = file
        .into_path()
        .map_err(|e| format!("Chemin invalide: {}", e))?;

    // 2-5. Validation + copie (helper commun).
    validate_and_copy_media_limite(app, &src, video_sans_limite).map(Some)
}

/// Importe un média (image OU vidéo) pour un widget : dialog → validation
/// (extension + taille + magic bytes) → copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some((media_rel_path, nom))`
/// si importé, `None` si dialog annulé. AUCUNE mutation de scène ici : le
/// frontend pose le média dans son store puis commit via update_scene (un
/// seul écrivain — l'ancienne mutation RAM + save créait une course avec
/// update_scene : une duplication pendant l'import en vol sauvait la copie
/// SANS son média, perdu au redémarrage).
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo. Vidéo : mp4/webm/mkv ≤ 80 Mo
/// (mkv remuxé en mp4 via sidecar ffmpeg).
#[tauri::command]
async fn import_media(app: AppHandle, _widget_id: String) -> Result<Option<(String, String)>, String> {
    // Dialog natif + copie/remux sur le pool bloquant Tokio — jamais sur le
    // thread UI (la fenêtre reste déplaçable pendant l'import).
    tauri::async_runtime::spawn_blocking(move || {
        // Helper commun : dialog + validation + copie (pas de mutation scène).
        let Some((rel, _kind, nom)) = pick_and_copy_media(&app, false)? else {
            return Ok(None); // dialog annulé
        };
        Ok(Some((rel, nom)))
    })
    .await
    .map_err(|e| format!("import_media: {}", e))?
}

/// Résultat d'un import multi-fichiers : succès + erreurs concaténées.
/// Un échec sur UN fichier n'annule pas les autres (comportement drop).
#[derive(serde::Serialize)]
struct ImportCheminsResult {
    /// (rel, kind, nom_original) pour chaque fichier importé avec succès.
    ok: Vec<(String, String, String)>,
    /// Messages d'erreur préfixés du nom de fichier (affichage direct).
    erreurs: Vec<String>,
}

/// Importe des médias depuis des chemins disque (drop Tauri — pas de dialog).
/// Mêmes règles que validate_and_copy_media : extension + taille + magic
/// bytes + copie/remux MKV→MP4 vers medias/. AUCUNE mutation de scène —
/// le frontend range les résultats dans la bibliothèque médias.
#[tauri::command]
async fn import_media_from_paths(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<ImportCheminsResult, String> {
    // Boucle de copie/remux (jusqu'à N fichiers) sur le pool bloquant Tokio —
    // jamais sur le thread UI.
    tauri::async_runtime::spawn_blocking(move || {
        let mut ok = Vec::new();
        let mut erreurs = Vec::new();
        for p in &paths {
            let src = std::path::Path::new(p);
            let nom = src
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(p.as_str())
                .to_string();
            match validate_and_copy_media(&app, src) {
                Ok(t) => ok.push(t),
                Err(e) => erreurs.push(format!("{} : {}", nom, e)),
            }
        }
        Ok(ImportCheminsResult { ok, erreurs })
    })
    .await
    .map_err(|e| format!("import_media_from_paths: {}", e))?
}

/// Importe PLUSIEURS images en un seul dialog multi-sélection (carrousels :
/// mediaSlides widget + bgSlides fond — images uniquement, pas de vidéos).
/// Mêmes règles que validate_and_copy_media (extension + taille + magic
/// bytes) → copie vers medias/. AUCUNE mutation de scène — le frontend range
/// les résultats dans les listes de slides puis commit via update_scene.
/// Un échec sur UN fichier n'annule pas les autres (erreurs concaténées).
/// Image : png/jpg/jpeg/gif/webp ≤ 10 Mo.
#[tauri::command]
async fn import_medias_multi(app: AppHandle) -> Result<ImportCheminsResult, String> {
    // Dialog natif + copie sur le pool bloquant Tokio — jamais sur le
    // thread UI.
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        // 1. Dialog multi-fichiers (filtre « Images » uniquement — les
        //    carrousels n'acceptent pas de vidéos en v1).
        let files = app
            .dialog()
            .file()
            .add_filter("Images", IMG_EXT)
            .blocking_pick_files();
        let Some(files) = files else {
            // Dialog annulé → résultat vide (ok=[], erreurs=[]).
            return Ok(ImportCheminsResult {
                ok: Vec::new(),
                erreurs: Vec::new(),
            });
        };

        // 2. Validation + copie par fichier (helper commun), erreurs par
        //    fichier concaténées — comportement import_media_from_paths.
        let mut ok = Vec::new();
        let mut erreurs = Vec::new();
        for f in &files {
            let src: std::path::PathBuf = match f.clone().into_path() {
                Ok(p) => p,
                Err(e) => {
                    erreurs.push(format!("chemin invalide : {}", e));
                    continue;
                }
            };
            let nom = src
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("média")
                .to_string();
            match validate_and_copy_media(&app, &src) {
                Ok(t) => ok.push(t),
                Err(e) => erreurs.push(format!("{} : {}", nom, e)),
            }
        }
        Ok(ImportCheminsResult { ok, erreurs })
    })
    .await
    .map_err(|e| format!("import_medias_multi: {}", e))?
}

/// Importe un média (image OU vidéo) comme fond de scène : dialog → validation
/// → copie vers medias/. Retourne `Some(media_rel_path)` si importé, `None` si
/// dialog annulé. AUCUNE mutation de scène : le frontend pose bgMedia dans son
/// store puis commit via update_scene (un seul écrivain, cf. import_media).
/// Mêmes limites que import_media (helper commun pick_and_copy_media).
#[tauri::command]
async fn import_fond(app: AppHandle) -> Result<Option<String>, String> {
    // Dialog + copie/remux sur le pool bloquant Tokio (pas le thread UI).
    tauri::async_runtime::spawn_blocking(move || {
        // Helper commun : dialog + validation + copie (pas de mutation scène).
        let Some((rel, _kind, _nom)) = pick_and_copy_media(&app, false)? else {
            return Ok(None); // dialog annulé
        };
        Ok(Some(rel))
    })
    .await
    .map_err(|e| format!("import_fond: {}", e))?
}

/// Importe un média (image OU vidéo) pour une alerte : dialog filtré selon le
/// kind attendu ("image" → Images, "video" → Vidéos) → validation (extension +
/// taille + magic bytes) → copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Le kind est vérifié contre `kind_attendu`
/// (l'appelant connaît déjà le kind demandé — pas besoin de le retourner).
/// Le média est servi par :4321 sur /medias/. Vidéo mkv acceptée (remuxée en
/// mp4 via sidecar ffmpeg).
#[tauri::command]
async fn import_alerte_media(
    app: AppHandle,
    kind_attendu: String,
) -> Result<Option<String>, String> {
    // Dialog natif + copie/remux sur le pool bloquant Tokio (pas le thread UI).
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        // 1. Dialog fichier filtré selon le kind attendu.
        let (filtre_nom, exts): (&str, &[&str]) = if kind_attendu == "video" {
            ("Vidéos", VIDEO_EXT)
        } else {
            ("Images", IMG_EXT)
        };
        let file = app
            .dialog()
            .file()
            .add_filter(filtre_nom, exts)
            .blocking_pick_file();
        let Some(file) = file else {
            return Ok(None); // dialog annulé
        };
        let src: std::path::PathBuf = file
            .into_path()
            .map_err(|e| format!("Chemin invalide: {}", e))?;

        // 2. Validation + copie (helper commun) + garde-fou kind.
        let (rel, kind, _nom) = validate_and_copy_media(&app, &src)?;
        if kind != kind_attendu {
            return Err(format!(
                "Type de fichier inattendu : {} attendu, {} reçu",
                kind_attendu, kind
            ));
        }
        Ok(Some(rel))
    })
    .await
    .map_err(|e| format!("import_alerte_media: {}", e))?
}

/// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une alerte : dialog fichier
/// « Sons » → validation (extension + taille + magic bytes) → copie vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Le son est servi par :4321 sur /medias/ (le même
/// dossier que les médias de scène).
#[tauri::command]
async fn import_son(app: AppHandle) -> Result<Option<String>, String> {
    // Dialog natif + copie sur le pool bloquant Tokio (pas le thread UI).
    tauri::async_runtime::spawn_blocking(move || {
        use std::fs;
        use std::io::Read;
        use tauri_plugin_dialog::DialogExt;

        // 1. Dialog fichier (filtre « Sons »)
        let file = app
            .dialog()
            .file()
            .add_filter("Sons", AUDIO_EXT)
            .blocking_pick_file();
        let Some(file) = file else {
            return Ok(None); // dialog annulé
        };
        let src: std::path::PathBuf = file
            .into_path()
            .map_err(|e| format!("Chemin invalide: {}", e))?;

        // 2. Extension autorisée
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .ok_or_else(|| "Extension manquante".to_string())?;
        if !AUDIO_EXT.contains(&ext.as_str()) {
            return Err(format!("Extension .{} non autorisée", ext));
        }

        // 3. Taille ≤ limite
        let meta = fs::metadata(&src).map_err(|e| format!("metadata: {}", e))?;
        if meta.len() > MAX_AUDIO_BYTES {
            return Err(format!(
                "Fichier trop volumineux ({} octets > {} Mo)",
                meta.len(),
                MAX_AUDIO_BYTES / 1024 / 1024
            ));
        }

        // 4. Magic bytes
        let mut f = fs::File::open(&src).map_err(|e| format!("open: {}", e))?;
        let mut head = [0u8; 32];
        let n = f.read(&mut head).map_err(|e| format!("read: {}", e))?;
        if !check_magic_audio(&head[..n]) {
            return Err("Format audio non supporté (magic bytes invalides)".into());
        }

        // 5. Copie vers AppData/StreamOS/medias/<uuid>.<ext>
        let dir = config::data_dir(&app)?;
        let uuid = uuid::Uuid::new_v4().simple().to_string();
        let dest_name = format!("{}.{}", uuid, ext);
        let dest = dir.join("medias").join(&dest_name);
        fs::copy(&src, &dest).map_err(|e| format!("copy: {}", e))?;

        Ok(Some(format!("medias/{}", dest_name)))
    })
    .await
    .map_err(|e| format!("import_son: {}", e))?
}

/// Connecte à OBS WebSocket (host:port, password), authentifie, lit la
/// résolution canvas OBS (GetVideoSettings → baseWidth/baseHeight, fallback
/// 1920×1080), s'assure que la scène "SOS" + source navigateur "SOS-Diffusion"
/// (dims = résolution OBS, URL :4321) existent sans doublon — one-shot.
/// Puis mute la scène (canvasW/canvasH) + save <current_id>.json + snapshot WS.
/// Pas de poll, pas de rescale des widgets existants.
#[tauri::command]
async fn obs_connect(
    app: AppHandle,
    state: tauri::State<'_, ScenesState>,
    host: String,
    port: u16,
    password: String,
    origine: Option<String>,
) -> Result<(), String> {
    // Traçage boîte noire : chaque obs_connect bascule la scène OBS sur
    // "SOS" hors live — on veut savoir QUI appelle et à quelle cadence
    // (2026-10-27 : 24 bascules sans intervention de l'utilisateur).
    conso::marquer_obs_connect(origine.as_deref().unwrap_or("inconnu"));
    let (w, h) = obs::connect_and_setup(&host, port, &password).await?;

    // Mémoriser la résolution OBS globalement (obs_canvas.json) : appliquée à
    // TOUTES les scènes au chargement (boot, création, bascule, import) — pas
    // seulement à la scène courante. Non-fatal si écriture échoue.
    if let Err(e) = config::sauver_obs_canvas(&app, w, h) {
        log::warn!("sauver_obs_canvas: {}", e);
    }

    // Muter canvasW/canvasH si changement → save + snapshot (chaîne unique).
    // La géométrie suit un vrai changement de résolution (facteur uniforme +
    // letterbox, seuil SEUIL_CANVAS) — sinon les widgets restent aux
    // coordonnées de l'ancien canvas et débordent (cf. scenes::adapter_canvas).
    let changed = {
        let mut current = state.scene.lock().unwrap();
        let (avant_w, avant_h) = (current.canvasW, current.canvasH);
        let change = scenes::adapter_canvas(&mut current, w, h);
        if change {
            log::info!("Canvas mis à jour : {}×{} → {}×{}", avant_w, avant_h, w, h);
        }
        change
    };

    if changed {
        scenes::save_current(&app, &state)?;
    }

    Ok(())
}

/// Refresh standalone de la source navigateur "SOS-Diffusion" (refreshnocache).
/// Appelé par le frontend quand serveur :4321 ready + OBS connecté.
/// One-shot : connect → PressInputPropertiesButton → close. Non-fatal.
#[tauri::command]
async fn obs_refresh_diffusion(
    host: String,
    port: u16,
    password: String,
) -> Result<(), String> {
    obs::refresh_diffusion(&host, port, &password).await
}

/// Met le streaming OBS dans l'état demandé (Start/StopStream explicite,
/// idempotent — jamais ToggleStream : la direction est celle confirmée dans
/// l'UI, pas l'inverse d'un état réel potentiellement désynchronisé).
/// One-shot connect → statut → action si besoin → re-statut → close.
/// Retourne l'état réel relu (true = en live).
#[tauri::command]
async fn obs_set_stream(
    host: String,
    port: u16,
    password: String,
    actif: bool,
) -> Result<bool, String> {
    obs::set_stream(&host, port, &password, actif).await
}

/// Met l'enregistrement OBS dans l'état demandé. Même contrat que
/// obs_set_stream (Start/StopRecord explicite, idempotent, état relu).
#[tauri::command]
async fn obs_set_record(
    host: String,
    port: u16,
    password: String,
    actif: bool,
) -> Result<bool, String> {
    obs::set_record(&host, port, &password, actif).await
}

/// Lit l'état des sorties OBS (GetStreamStatus + GetRecordStatus, une
/// connexion). Retourne { stream, record }. Appelé au connect + après chaque
/// toggle — PAS de polling (connexion OBS = one-shot, pas persistante).
#[tauri::command]
async fn obs_outputs_status(
    host: String,
    port: u16,
    password: String,
) -> Result<serde_json::Value, String> {
    let (stream, record) = obs::outputs_status(&host, port, &password).await?;
    Ok(serde_json::json!({ "stream": stream, "record": record }))
}

/// Liste les moniteurs vus par OBS (GetMonitorList) — sélecteur de la section
/// Prévisualisation. One-shot, non-fatal côté frontend ([] en cas d'échec).
#[tauri::command]
async fn obs_projecteur_moniteurs(
    host: String,
    port: u16,
    password: String,
) -> Result<Vec<serde_json::Value>, String> {
    obs::moniteurs_liste(&host, port, &password).await
}

/// Ouvre le projecteur OBS « prévisualisation » (OpenVideoMixProjector) —
/// équivalent du clic droit OBS. monitor_index : index OBS, -1 = fenêtré.
/// La nouvelle fenêtre OBSProjector est remontée au premier plan (Windows).
#[tauri::command]
async fn obs_projecteur_preview(
    host: String,
    port: u16,
    password: String,
    monitor_index: i32,
) -> Result<(), String> {
    obs::ouvrir_projecteur_preview(&host, port, &password, monitor_index).await
}

/// Ferme les fenêtres projecteur OBS (WM_CLOSE, Win32 local — aucune
/// connexion WebSocket nécessaire). Appelée à la connexion OBS quand
/// l'option « projecteur au démarrage » est OFF : OBS restaure ses
/// projecteurs d'une session à l'autre, StreamOS impose « fermé » par
/// défaut. Retourne le nombre de fenêtres fermées.
#[tauri::command]
async fn obs_projecteur_fermer() -> u32 {
    obs::fermer_projecteurs()
}

// ===== Pop-out chat (Lot B) =====

/// HTML embarqué du pop-out chat (include_str! — recompile si le fichier change).
const CHAT_POPOUT_HTML: &str = include_str!("../resources/chat-popout.html");

/// HTML embarqué du projecteur natif (include_str!) — fenêtre d'aperçu du
/// moteur Rust servie par le protocole `streamos-apercu://`.
const APERCU_HTML: &str = include_str!("../resources/apercu.html");

/// Ouvre ou ferme la fenêtre pop-out chat (toggle).
/// - Si fermée → ouvre (always_on_top, 360×520, décorations simples).
/// - Si ouverte → ferme (destroy). IRC inchangé.
/// Retourne true si maintenant ouverte, false si fermée.
/// Quand l'utilisateur ferme via la croix → emit "popout-closed" au frontend
/// pour que le bouton sidebar redevienne « Détacher ».
///
/// Navigation : custom protocol streamos-chat://localhost/chat → HTML embarqué
/// servi par register_uri_scheme_protocol (vraie origine, IPC + WS fiables).
/// PAS d'about:blank + eval (webview pas prête → fenêtre blanche).
/// PAS d'URL externe localhost:4321 (Webview2 peut bloquer la navigation).
#[tauri::command]
async fn chat_popout_toggle(app: AppHandle) -> Result<bool, String> {
    use tauri::webview::WebviewWindowBuilder;

    const LABEL: &str = "chat-popout";

    // Si la fenêtre existe déjà → fermer (réattacher).
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
        return Ok(false);
    }

    // Création : custom protocol streamos-chat://localhost/chat.
    // Le HTML est servi par register_uri_scheme_protocol (déclaré dans run()).
    // ASYNC obligatoire : sur Windows, créer une webview dans une commande sync
    // deadlock (Webview2) → fenêtre blanche, navigation/protocol jamais déclenché.
    // always_on_top, 360×520, décorations simples (croix native), resizable.
    let url = tauri::Url::parse("streamos-chat://localhost/chat")
        .map_err(|e| format!("URL invalide: {}", e))?;
    let win = WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("Chat pop-out")
        .inner_size(360.0, 520.0)
        .always_on_top(true)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Création fenêtre pop-out: {}", e))?;

    // CloseRequested : la croix native déclenche cet event.
    // On NE PASSE PAS prevent_close() → la fenêtre se ferme normalement.
    // On emit "popout-closed" pour que le bouton sidebar redevienne « Détacher ».
    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = app_clone.emit("popout-closed", ());
        }
    });

    Ok(true)
}

/// Ferme explicitement la fenêtre pop-out chat si elle est ouverte.
/// Non-fatal si déjà fermée. IRC inchangé.
#[tauri::command]
fn chat_popout_fermer(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "chat-popout";
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
    Ok(())
}

// ===== Pop-out speedrun (2026-10-19) =====

/// HTML embarqué du pop-out speedrun (include_str! — recompile si le fichier change).
const SPEEDRUN_POPOUT_HTML: &str = include_str!("../resources/speedrun-popout.html");

/// Police par défaut du splitter (Rajdhani), servie par le protocol du
/// pop-out (same-origin). Un @font-face cross-origin sur :4321 serait
/// bloqué : les polices web exigent un Access-Control-Allow-Origin que le
/// serveur n'envoie pas — on ne touche pas à server.rs.
const SPEEDRUN_POPOUT_RAJDHANI: &[u8] = include_bytes!("../resources/fonts/Rajdhani.woff2");

/// Ouvre ou ferme la fenêtre pop-out speedrun (toggle).
/// - Si fermée → ouvre (always_on_top, 420×640, décorations simples).
/// - Si ouverte → ferme (destroy). Moteur ASL inchangé.
/// Retourne true si maintenant ouverte, false si fermée.
/// Quand l'utilisateur ferme via la croix → emit "speedrun-popout-closed"
/// au frontend pour que le bouton de la carte speedrun redevienne « Détacher ».
///
/// Navigation : custom protocol streamos-speedrun://localhost/splitter → HTML
/// embarqué servi par register_uri_scheme_protocol. Même recette que le
/// pop-out chat (streamos-chat) — PAS d'about:blank, PAS d'URL externe.
/// Contrôles de run : la page émet des events Tauri (speedrun-popout:action)
/// que le dashboard exécute (pattern raccourcis — pas de logique métier
/// dans les fenêtres satellite). L'affichage vient du WS :4321 (frames
/// speedrun-etat) + resync à l'ouverture (speedrun-resync), comme diffusion.html.
#[tauri::command]
async fn speedrun_popout_toggle(app: AppHandle) -> Result<bool, String> {
    use tauri::webview::WebviewWindowBuilder;

    const LABEL: &str = "speedrun-popout";

    // Si la fenêtre existe déjà → fermer (réattacher).
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
        return Ok(false);
    }

    // ASYNC obligatoire (même piège que le pop-out chat) : sur Windows,
    // créer une webview dans une commande sync deadlock Webview2.
    let url = tauri::Url::parse("streamos-speedrun://localhost/splitter")
        .map_err(|e| format!("URL invalide: {}", e))?;
    let win = WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::CustomProtocol(url))
        .title("Splitter pop-out")
        .inner_size(420.0, 640.0)
        .always_on_top(true)
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Création fenêtre pop-out speedrun: {}", e))?;

    // Croix native → event pour remettre le bouton de la carte sur « Détacher ».
    // Event distinct de "popout-closed" du chat (les deux boutons ne doivent
    // pas se réinitialiser mutuellement).
    let app_clone = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = app_clone.emit("speedrun-popout-closed", ());
        }
    });

    Ok(true)
}

/// Ferme explicitement la fenêtre pop-out speedrun si elle est ouverte.
/// Non-fatal si déjà fermée. Moteur ASL inchangé.
#[tauri::command]
fn speedrun_popout_fermer(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "speedrun-popout";
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.close();
    }
    Ok(())
}

// ===== Contrôle de l'app (topbar) =====

/// Arrête l'application (quitte proprement).
#[tauri::command]
fn app_arreter(app: AppHandle) -> Result<(), String> {
    // Sauver la géométrie fenêtrée : app.exit() ne déclenche pas forcément
    // le hook CloseRequested de la fenêtre main.
    fenetre::sauver_geo(&app);
    app.exit(0);
    Ok(())
}

/// Redémarre l'application (relance le processus puis quitte).
/// NB : `app.restart()` ne retourne jamais (`!`) — coercé en Result.
#[tauri::command]
fn app_redemarrer(app: AppHandle) -> Result<(), String> {
    fenetre::sauver_geo(&app);
    app.restart()
}

// ===== Captures liées à la sauvegarde (Lot C) =====

/// Synchronise les captures SOS-Trou-* et SOS-Fond-* d'OBS avec la scène
/// chargée. Active + sync transform les items liés à un widget.obsSource de
/// la scène + la capture de fond (bgObsSource, plein canvas). Cache
/// (SetSceneItemEnabled false) les items orphelins des autres scènes.
/// Non-fatal : si OBS offline → erreur remontée (pas de crash).
/// `scene` optionnel : présent (mode Just Chatting) → synchroniser avec la
/// scène FOURNIE (copie dérivée non persistée : caméra/chat au layout JC,
/// autres widgets absents, bgObsSource vidé) ; absent → scène chargée,
/// comportement historique. La copie n'est JAMAIS écrite dans state.scene
/// ni passée à save_current/update_scene.
#[tauri::command]
async fn scene_sync_captures(
    state: tauri::State<'_, ScenesState>,
    host: String,
    port: u16,
    password: String,
    scene: Option<scene::Scene>,
) -> Result<(), String> {
    // Scène COMPLÈTE (widgets + bgObsSource + canvas) — un seul lock, clone.
    let scene: scene::Scene = match scene {
        Some(s) => s,
        None => {
            let s = state.scene.lock().unwrap();
            s.clone()
        }
    };
    obs_trou::sync_scene_captures(&host, port, &password, &scene).await
}

// ===== Sources OBS "trou" (Lot 2) =====

/// Énumère les cibles de capture OBS pour un type (camera/window/game).
/// Jeu → Vec vide (capture_any_foreground_window, pas de cible).
/// One-shot : connect OBS → CreateInput temp → GetInputPropertiesList →
/// DeleteInput temp → close. Erreur remontée au dashboard si OBS offline.
#[tauri::command]
async fn obs_enumerate_targets(
    host: String,
    port: u16,
    password: String,
    kind: obs_trou::CaptureKind,
) -> Result<Vec<String>, String> {
    obs_trou::enumerate_targets(&host, port, &password, kind).await
}

/// Crée une source OBS de capture sous SOS-Diffusion, calée sur le widget
/// (x y w h). Retourne le nom de la source créée. One-shot connect/disconnect.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_create_trou_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    kind: obs_trou::CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    obs_trou::create_trou_source(
        &host, port, &password, &source_name, kind, target, x, y, w, h,
    )
    .await
}

/// Synchronise les transforms OBS de toutes les sources trou en une connexion.
/// Appelée au commitScene (pointerup) pour les widgets avec obsSource=Some.
#[tauri::command]
async fn obs_sync_trous(
    host: String,
    port: u16,
    password: String,
    items: Vec<obs_trou::SyncItem>,
) -> Result<(), String> {
    obs_trou::sync_trous(&host, port, &password, items).await
}

/// Supprime une source trou d'OBS (RemoveInput). Idempotent : si la source
/// n'existe pas → Ok (considérée comme déjà supprimée).
#[tauri::command]
async fn obs_delete_trou_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
) -> Result<(), String> {
    obs_trou::delete_trou_source(&host, port, &password, &source_name).await
}

/// Énumère les inputs OBS (GetInputList) filtrés par type de capture.
/// Caméra → dshow_input ; Fenêtre → window_capture ; Jeu → game_capture.
/// Exclut SOS-Diffusion, browser_source, audio, scènes, trous liés.
#[tauri::command]
async fn obs_enumerate_inputs_by_kind(
    host: String,
    port: u16,
    password: String,
    kind: obs_trou::CaptureKind,
) -> Result<Vec<obs_trou::InputEntry>, String> {
    obs_trou::enumerate_inputs_by_kind(&host, port, &password, kind).await
}

/// Lie une source OBS existante au widget trou (pas de création, juste
/// transform + reorder sous SOS-Diffusion).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_link_existing_source(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    obs_trou::link_existing_source(&host, port, &password, &source_name, x, y, w, h).await
}

// ===== Widget caméra (source OBS "SOS-Caméra" sous SOS-Diffusion) =====

/// Crée / met à jour la source "SOS-Caméra" (dshow_input) dans la scène « SOS »,
/// sous SOS-Diffusion, calée sur le widget (transform = sémantique sync_trous :
/// centre + OBS_BOUNDS_SCALE_OUTER + alignment 0).
/// device = Some(FriendlyName) → SetInputSettings video_device_id sur l'input
/// existant (changement de device explicite) ; None → device par défaut d'OBS.
/// fondVert = flag cameraFondVert du widget : plaque SOS-Caméra-Fond masquée +
/// filtre background_removal (ré)asserté (chaque chemin caméra est atomique).
/// visible = inverse de cameraCachee : false → items caméra + plaque
/// désactivés (la même commande masque ET réaffiche — bouton œil du widget).
/// Singleton : 1 widget caméra / scène → 1 input "SOS-Caméra".
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn camera_sync(
    host: String,
    port: u16,
    password: String,
    device: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fond_vert: bool,
    visible: bool,
) -> Result<(), String> {
    obs_trou::camera_sync(&host, port, &password, device.as_deref(), x, y, w, h, fond_vert, visible).await
}

/// Toggle « fond vert virtuel » : filtre plugin background_removal
/// (occ-ai/obs-backgroundremoval, 100% local) sur SOS-Caméra + plaque
/// SOS-Caméra-Fond masquée (jamais RemoveInput) — la scène OBS apparaît
/// derrière la personne. Plugin absent → INSTALLATION AUTOMATIQUE (GitHub,
/// SHA256 vérifié, extraction per-user — aucun exe/admin). Retour = statut :
/// "applique" | "desactive" | "redemarrer-obs" (plugin posé, OBS doit être
/// redémarré pour le charger ; l'état persisté s'applique à la reconnexion).
/// One-shot : l'état reste persisté dans le widget (cameraFondVert),
/// scene_sync_captures le ré-applique (auto-guérison).
#[tauri::command]
async fn camera_fond_vert(
    host: String,
    port: u16,
    password: String,
    actif: bool,
    visible: bool,
) -> Result<String, String> {
    obs_trou::camera_fond_vert(&host, port, &password, actif, visible).await
}

/// Cache l'item de scène "SOS-Caméra" (SetSceneItemEnabled false) à la
/// suppression du widget caméra. L'input OBS est CONSERVÉ (jamais RemoveInput
/// — la recréation du widget le réutilisera). Non-fatal si l'item est absent.
#[tauri::command]
async fn camera_hide(host: String, port: u16, password: String) -> Result<(), String> {
    obs_trou::camera_hide(&host, port, &password).await
}

/// Découpe les sources OBS "SOS-Caméra" et "SOS-Caméra-Fond" à la forme du
/// cadre SVG du widget caméra (filtre « Image mask/Blend » + PNG régénéré
/// dans <data>/obs, nom versionné par hash du contenu). points = polygone
/// widget-local (null/vide = cadre sans forme → filtres retirés, caméra
/// rectangulaire). Retourne false si la source n'était pas encore prête
/// (capteur en démarrage) — le dashboard ne met pas en cache et retente au
/// prochain commit. One-shot, non-fatal si OBS offline.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn camera_mask_sync(
    app: AppHandle,
    host: String,
    port: u16,
    password: String,
    points: Option<Vec<(f64, f64)>>,
    w: f64,
    h: f64,
    fit: String,
    zoom: f64,
) -> Result<bool, String> {
    let dossier = config::data_dir(&app)?.join("obs");
    obs_trou::camera_mask_sync(&host, port, &password, points, w, h, &fit, zoom, &dossier).await
}

// ===== Connexions réseau (Lot réseau) =====

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;
use twitch_auth::{Cancel, new_cancel};

/// État Twitch partagé : handle de la tâche IRC + jeton d'annulation (swappable)
/// + drapeau connecté + login/user_id/access token du compte connecté.
///
/// Géré via tauri::State. Le keyring reste le store persistant ; TwitchState
/// est la référence en mémoire pour la session courante (évite de relire le
/// keyring à chaque appel Helix).
#[derive(Clone)]
pub struct TwitchState {
    pub irc_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Cancel>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub login: Arc<std::sync::Mutex<Option<String>>>,
    pub user_id: Arc<std::sync::Mutex<Option<String>>>,
    pub access: Arc<std::sync::Mutex<Option<String>>>,
    /// Canal d'envoi de commandes IRC (ex: "/clear") vers la tâche IRC.
    /// None si l'IRC n'est pas démarré. Remplacé à chaque démarrage IRC.
    pub irc_cmd_tx: Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Sender<String>>>>,
    /// Handle de la tâche EventSub (follows temps réel). Même cancel que
    /// l'IRC — démarré/arrêté avec la connexion Twitch.
    pub eventsub_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl TwitchState {
    fn new() -> Self {
        Self {
            irc_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(new_cancel())),
            connected: Arc::new(std::sync::Mutex::new(false)),
            login: Arc::new(std::sync::Mutex::new(None)),
            user_id: Arc::new(std::sync::Mutex::new(None)),
            access: Arc::new(std::sync::Mutex::new(None)),
            irc_cmd_tx: Arc::new(std::sync::Mutex::new(None)),
            eventsub_handle: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_login(&self, v: Option<String>) {
        *self.login.lock().unwrap() = v;
    }
    fn login_courant(&self) -> Option<String> {
        self.login.lock().unwrap().clone()
    }
    fn set_user_id(&self, v: Option<String>) {
        *self.user_id.lock().unwrap() = v;
    }
    fn user_id_courant(&self) -> Option<String> {
        self.user_id.lock().unwrap().clone()
    }
    fn set_access(&self, v: Option<String>) {
        *self.access.lock().unwrap() = v;
    }
    fn access_courant(&self) -> Option<String> {
        self.access.lock().unwrap().clone()
    }
    /// Arrête l'IRC + l'EventSub en cours (si actifs) : flag cancel + abort
    /// handles + clear canal cmd. Le cancel étant partagé, les deux tâches
    /// s'arrêtent — l'abort est ceinture+bretelles (même style que l'IRC).
    fn stop_irc(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.irc_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
        let mut es = self.eventsub_handle.lock().unwrap();
        if let Some(handle) = es.take() {
            handle.abort();
        }
        // Vider le canal de commandes : la tâche IRC qui le consommait est arrêtée.
        *self.irc_cmd_tx.lock().unwrap() = None;
    }
    /// Prépare un nouveau cancel (fresh) pour un prochain démarrage IRC/flow.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = new_cancel();
    }
    /// Récupère un clone du cancel courant.
    fn cancel_clone(&self) -> Cancel {
        self.cancel.lock().unwrap().clone()
    }
    /// Envoie une commande IRC (ex: "/clear") vers la tâche IRC.
    /// Retourne false si l'IRC n'est pas démarré (canal absent).
    fn envoyer_commande_irc(&self, commande: &str) -> bool {
        let tx = self.irc_cmd_tx.lock().unwrap();
        match tx.as_ref() {
            Some(sender) => {
                let _ = sender.try_send(commande.to_string());
                true
            }
            None => false,
        }
    }
    /// Envoie un message dans le chat Twitch (PRIVMSG) — utilisé par les
    /// modules qui produisent des messages asynchrones (arène d'emojis :
    /// annonce de tour, expiration de lobby, fin AFK).
    pub fn envoyer_message_chat(&self, texte: &str) -> bool {
        self.envoyer_commande_irc(texte)
    }
}

/// État Kick partagé : drapeau connecté + slug du canal + handle de la tâche
/// WS + jeton d'annulation (swappable). Le client WebSocket Pusher vit côté
/// Rust (tokio-tungstenite avec Origin: https://kick.com — nécessaire car le
/// webview est rejeté par CORS avec Origin: localhost:1420).
/// Géré via tauri::State. Clonable (Arc internes).
#[derive(Clone)]
pub struct KickState {
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub slug: Arc<std::sync::Mutex<Option<String>>>,
    pub ws_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Arc<AtomicBool>>>,
}

impl KickState {
    fn new() -> Self {
        Self {
            connected: Arc::new(std::sync::Mutex::new(false)),
            slug: Arc::new(std::sync::Mutex::new(None)),
            ws_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(Arc::new(AtomicBool::new(false)))),
        }
    }
    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_slug(&self, v: Option<String>) {
        *self.slug.lock().unwrap() = v;
    }
    /// Slug du canal Kick connecté (ex: "xqc"). Utilisé par le hook musique
    /// dans kick.rs pour détecter si l'expéditeur est le streamer.
    pub(crate) fn slug_courant(&self) -> Option<String> {
        self.slug.lock().unwrap().clone()
    }
    /// Arrête le WS en cours (si actif) : flag cancel + abort handle.
    fn stop_ws(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.ws_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel frais pour la prochaine connexion WS.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = Arc::new(AtomicBool::new(false));
    }
    /// Clone du cancel courant.
    fn cancel_clone(&self) -> Arc<AtomicBool> {
        self.cancel.lock().unwrap().clone()
    }
}

/// État YouTube partagé : handle de la tâche chat polling + jeton d'annulation
/// (swappable) + drapeau connecté + login/channel_id/access token du compte.
/// Géré via tauri::State. Même pattern que TwitchState.
#[derive(Clone)]
pub struct YoutubeState {
    pub chat_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<youtube_auth::Cancel>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub login: Arc<std::sync::Mutex<Option<String>>>,
    pub channel_id: Arc<std::sync::Mutex<Option<String>>>,
    pub access: Arc<std::sync::Mutex<Option<String>>>,
}

impl YoutubeState {
    fn new() -> Self {
        Self {
            chat_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(youtube_auth::new_cancel())),
            connected: Arc::new(std::sync::Mutex::new(false)),
            login: Arc::new(std::sync::Mutex::new(None)),
            channel_id: Arc::new(std::sync::Mutex::new(None)),
            access: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_login(&self, v: Option<String>) {
        *self.login.lock().unwrap() = v;
    }
    fn login_courant(&self) -> Option<String> {
        self.login.lock().unwrap().clone()
    }
    fn set_channel_id(&self, v: Option<String>) {
        *self.channel_id.lock().unwrap() = v;
    }
    fn set_access(&self, v: Option<String>) {
        *self.access.lock().unwrap() = v;
    }
    fn access_courant(&self) -> Option<String> {
        self.access.lock().unwrap().clone()
    }
    /// Arrête le chat polling en cours (si actif) : flag cancel + abort handle.
    fn stop_chat(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.chat_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel (fresh) pour un prochain démarrage/flow.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = youtube_auth::new_cancel();
    }
    /// Récupère un clone du cancel courant.
    fn cancel_clone(&self) -> youtube_auth::Cancel {
        self.cancel.lock().unwrap().clone()
    }
}

/// État TikTok partagé : handle de la tâche chat + jeton d'annulation
/// (swappable) + drapeau connecté + username du streamer suivi.
/// Géré via tauri::State. Même pattern que KickState.
#[derive(Clone)]
pub struct TiktokState {
    pub chat_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub cancel: Arc<std::sync::Mutex<Arc<AtomicBool>>>,
    pub connected: Arc<std::sync::Mutex<bool>>,
    pub username: Arc<std::sync::Mutex<Option<String>>>,
}

impl TiktokState {
    fn new() -> Self {
        Self {
            chat_handle: Arc::new(std::sync::Mutex::new(None)),
            cancel: Arc::new(std::sync::Mutex::new(Arc::new(AtomicBool::new(false)))),
            connected: Arc::new(std::sync::Mutex::new(false)),
            username: Arc::new(std::sync::Mutex::new(None)),
        }
    }
    fn set_connected(&self, v: bool) {
        *self.connected.lock().unwrap() = v;
    }
    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }
    fn set_username(&self, v: Option<String>) {
        *self.username.lock().unwrap() = v;
    }
    /// Arrête le chat en cours (si actif) : flag cancel + abort handle.
    fn stop_chat(&self) {
        {
            let c = self.cancel.lock().unwrap();
            c.store(true, Ordering::SeqCst);
        }
        let mut h = self.chat_handle.lock().unwrap();
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }
    /// Prépare un nouveau cancel frais pour la prochaine connexion.
    fn reset_cancel(&self) {
        *self.cancel.lock().unwrap() = Arc::new(AtomicBool::new(false));
    }
    /// Clone du cancel courant.
    fn cancel_clone(&self) -> Arc<AtomicBool> {
        self.cancel.lock().unwrap().clone()
    }
}

/// Démarre la connexion Twitch. Si un token valide existe en coffre →
/// valider/refresh + IRC + emit `twitch:connecte`. Sinon → Device Code Flow :
/// emit `twitch:device` (user_code + uri) → spawn poll cancellable → succès :
/// save + IRC + emit `twitch:connecte` ; expire/erreur : emit `twitch:erreur`.
#[tauri::command]
async fn twitch_connecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // 1. Token en coffre ? → valider (refresh si 401) + IRC direct.
    //    Si échec (token/refresh invalide, révoqué, expiré) : le coffre a été
    //    effacé dans connect_with_tokens, on enchaîne sur le Device Code Flow
    //    pour que l'utilisateur puisse se ré-authentifier (au lieu de rester
    //    bloqué sur un token mort).
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if connect_with_tokens(&app, state.inner(), tokens).await.is_ok() {
            return Ok(());
        }
    }

    // 2. Pas de token (ou token invalide) → Device Code Flow.
    let flow = match twitch_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    };

    // Préparer un cancel frais pour ce flow.
    state.reset_cancel();
    let cancel = state.cancel_clone();

    // Émettre les infos device vers le frontend (modal).
    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_uri,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("twitch:device", &device_info);

    // Poll en arrière-plan (cancellable). Sur succès → save + IRC + connecte.
    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match twitch_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(tokens) => {
                if let Err(e) = twitch_auth::sauver_tokens(&tokens) {
                    eprintln!("[Twitch] ERR coffre save: {}", e);
                }
                let _ = connect_with_tokens(&app2, &state2, tokens).await;
            }
            Err(e) => {
                let _ = app2.emit("twitch:erreur", &e);
            }
        }
    });

    Ok(())
}

/// Helper : valide/refresh les tokens, démarre IRC, emit `twitch:connecte`.
async fn connect_with_tokens(
    app: &AppHandle,
    state: &TwitchState,
    mut tokens: twitch_auth::Tokens,
) -> Result<(), String> {
    // Valider le token ; si 401 → refresh.
    match twitch_auth::valider_token(&tokens.access).await {
        Ok(v) => {
            tokens.login = v.login;
            tokens.user_id = v.user_id;
        }
        Err(e) if e.contains("401") => {
            match twitch_auth::refresh_token(&tokens.refresh).await {
                Ok(t) => {
                    tokens = t;
                    twitch_auth::sauver_tokens(&tokens)
                        .map_err(|e| format!("coffre save: {}", e))?;
                }
                // Refresh impossible (refresh token révoqué/expiré/tourné) :
                // on efface le coffre pour ne pas rester bloqué sur un token
                // mort, et on propage l'erreur. L'appelant (twitch_connecter)
                // peut alors enchaîner sur un nouveau Device Code Flow.
                Err(e) => {
                    let _ = twitch_auth::effacer_tokens();
                    let _ = app.emit("twitch:erreur", &e);
                    return Err(e);
                }
            }
        }
        Err(e) => {
            // Token invalide et refresh impossible → effacer + erreur.
            let _ = twitch_auth::effacer_tokens();
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    }

    // Démarrer IRC.
    state.reset_cancel();
    let cancel = state.cancel_clone();
    let chat_tx = app
        .try_state::<ScenesState>()
        .ok_or("ScenesState absent")?
        .chat_tx
        .clone();
    // Créer le canal de commandes IRC (ex: "/clear" depuis la modale de modération).
    let (irc_cmd_tx, irc_cmd_rx) = tokio::sync::mpsc::channel::<String>(32);
    {
        *state.irc_cmd_tx.lock().unwrap() = Some(irc_cmd_tx);
    }
    let handle = twitch_chat::demarrer(
        app.clone(),
        chat_tx,
        tokens.access.clone(),
        tokens.login.clone(),
        tokens.user_id.clone(),
        cancel.clone(),
        Some(irc_cmd_rx),
    );
    {
        let mut h = state.irc_handle.lock().unwrap();
        *h = Some(handle);
    }
    // EventSub WebSocket : follows temps réel (remplace le poll Helix 60s).
    // Même cancel que l'IRC — arrêt groupé dans stop_irc. Abonnement
    // channel.follow v2 créé dans la tâche après le session_welcome.
    let eventsub = twitch_eventsub::demarrer(
        app.clone(),
        tokens.access.clone(),
        tokens.user_id.clone(),
        cancel,
    );
    {
        let mut es = state.eventsub_handle.lock().unwrap();
        *es = Some(eventsub);
    }
    state.set_connected(true);
    state.set_login(Some(tokens.login.clone()));
    state.set_user_id(Some(tokens.user_id.clone()));
    state.set_access(Some(tokens.access.clone()));
    let _ = app.emit("twitch:connecte", &tokens.login);
    Ok(())
}

/// Annule le Device Code Flow en cours (bouton Annuler du modal).
#[tauri::command]
fn twitch_annuler_device_flow(state: tauri::State<'_, TwitchState>) -> Result<(), String> {
    let c = state.cancel.lock().unwrap();
    c.store(true, Ordering::SeqCst);
    Ok(())
}

/// Déconnecte Twitch : arrête IRC + révoque le token (best-effort) + efface le
/// coffre + emit `twitch:deconnecte`. Revoke non-fatal (réseau) — on efface
/// le coffre quoi qu'il arrive.
#[tauri::command]
async fn twitch_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    state.stop_irc();
    state.set_connected(false);
    state.set_login(None);
    state.set_user_id(None);
    state.set_access(None);
    // Revoke best-effort : lire le token, révoquer, puis effacer le coffre.
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if let Err(e) = twitch_auth::revoke_token(&tokens.access).await {
            eprintln!("[Twitch] WARN revoke échoué (on efface quand même): {}", e);
        }
    }
    let _ = twitch_auth::effacer_tokens();
    let _ = app.emit("twitch:deconnecte", ());
    Ok(())
}

/// Retourne l'état de connexion Twitch (true/false).
#[tauri::command]
fn twitch_etat(state: tauri::State<'_, TwitchState>) -> bool {
    state.is_connected()
}

/// Retourne le login du compte Twitch connecté (ou null si déconnecté).
#[tauri::command]
fn twitch_login_courant(state: tauri::State<'_, TwitchState>) -> Option<String> {
    state.login_courant()
}

/// Force une reconnexion Twitch : arrête IRC + révoque le token + efface le
/// coffre + emit `twitch:deconnecte`, puis enchaîne directement le Device Code
/// Flow (même client_id, même coffre, scopes étendus). Utilisé quand les
/// tokens existants n'ont pas les scopes communauté (Helix 403).
#[tauri::command]
async fn twitch_reconnecter(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    // 1. Déconnexion propre : stop IRC + revoke + efface coffre.
    state.stop_irc();
    state.set_connected(false);
    state.set_login(None);
    state.set_user_id(None);
    state.set_access(None);
    if let Ok(Some(tokens)) = twitch_auth::lire_tokens() {
        if let Err(e) = twitch_auth::revoke_token(&tokens.access).await {
            eprintln!("[Twitch] WARN revoke (reconnect): {}", e);
        }
    }
    let _ = twitch_auth::effacer_tokens();
    let _ = app.emit("twitch:deconnecte", ());

    // 2. Device Code Flow avec les scopes étendus (même logique que twitch_connecter).
    let flow = match twitch_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("twitch:erreur", &e);
            return Err(e);
        }
    };

    state.reset_cancel();
    let cancel = state.cancel_clone();

    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_uri,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("twitch:device", &device_info);

    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match twitch_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(tokens) => {
                if let Err(e) = twitch_auth::sauver_tokens(&tokens) {
                    eprintln!("[Twitch] ERR coffre save (reconnect): {}", e);
                }
                let _ = connect_with_tokens(&app2, &state2, tokens).await;
            }
            Err(e) => {
                let _ = app2.emit("twitch:erreur", &e);
            }
        }
    });

    Ok(())
}

// ===== Communauté lecture (Helix) =====

/// Mappe HelixError → String pour le frontend. "need_reauth" est distinct
/// pour que le frontend affiche le banner de reconnexion.
/// pub(crate) : réutilisée par presentation.rs (fetch Helix profil/chaîne).
pub(crate) fn helix_err_to_string(e: twitch_helix::HelixError) -> String {
    match e {
        twitch_helix::HelixError::NeedReauth => "need_reauth".to_string(),
        twitch_helix::HelixError::Deconnecte => "Pas connecté".to_string(),
        twitch_helix::HelixError::Autre(s) => s,
    }
}

/// Followers : liste + total (pagination curseur, max 10 pages).
/// Enrichit avec photos de profil (batch /users) et détecte les unfollows
/// par comparaison avec le snapshot précédent (sauvé sur disque).
#[tauri::command]
async fn twitch_communaute_followers(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::FollowersResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut resp = twitch_helix::followers(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;

    // Enrichir avec photos de profil (batch /users, max 100 par requête).
    // Non-fatal : si ça échoue, on garde les followers sans avatar.
    if let Err(e) = twitch_helix::enrichir_avatars(&mut resp.liste, &access).await {
        eprintln!("[Communauté] enrichir_avatars: {:?} (non-fatal)", e);
    }

    // Détection des unfollows : comparaison avec le snapshot précédent.
    let maintenant = chrono::Utc::now().to_rfc3339();
    let snap_precedent = match config::lire_followers_snapshot(&app) {
        Ok(Some(s)) => Some(s),
        Ok(None) => None,
        Err(e) => {
            eprintln!("[Communauté] lire_followers_snapshot: {} (non-fatal)", e);
            None
        }
    };

    // Si on a un snapshot précédent (non vide), détecter les manquants.
    if let Some(ref snap) = snap_precedent {
        if !snap.liste.is_empty() {
            let courants: std::collections::HashSet<String> =
                resp.liste.iter().map(|f| f.user_id.clone()).collect();
            let unfollows: Vec<config::UnfollowEntry> = snap
                .liste
                .iter()
                .filter(|f| !courants.contains(&f.user_id))
                .map(|f| config::UnfollowEntry {
                    user_id: f.user_id.clone(),
                    login: f.login.clone(),
                    date_unfollow: maintenant.clone(),
                    display_name: f.display_name.clone(),
                    profile_image_url: f.profile_image_url.clone(),
                })
                .collect();
            if !unfollows.is_empty() {
                if let Err(e) = config::ajouter_unfollows(&app, &unfollows) {
                    eprintln!("[Communauté] ajouter_unfollows: {} (non-fatal)", e);
                }
            }
        }
    }

    // Sauver le nouveau snapshot (écrase l'ancien).
    let snap = config::FollowersSnapshot {
        timestamp: maintenant,
        liste: resp
            .liste
            .iter()
            .map(|f| config::SnapshotFollower {
                user_id: f.user_id.clone(),
                login: f.login.clone(),
                followed_at: f.followed_at.clone(),
                display_name: f.display_name.clone(),
                profile_image_url: f.profile_image_url.clone(),
            })
            .collect(),
    };
    if let Err(e) = config::sauver_followers_snapshot(&app, &snap) {
        eprintln!("[Communauté] sauver_followers_snapshot: {} (non-fatal)", e);
    }

    Ok(resp)
}

/// Subs : liste + total + points (pagination, max 10 pages).
#[tauri::command]
async fn twitch_communaute_subs(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::SubsResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut resp = twitch_helix::subs(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    // Enrichir avec photos de profil (non-fatal).
    let ids: Vec<String> = resp.liste.iter().map(|s| s.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for s in &mut resp.liste {
            if let Some((dn, url)) = map.get(&s.user_id) {
                if !dn.is_empty() {
                    s.display_name = dn.clone();
                }
                s.profile_image_url = url.clone();
            }
        }
    }
    Ok(resp)
}

/// Viewers live : chiffre si en live, None si hors-ligne.
#[tauri::command]
async fn twitch_communaute_viewers(
    state: tauri::State<'_, TwitchState>,
) -> Result<Option<u32>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::stream_viewers(&uid, &access)
        .await
        .map(|o| o.map(|s| s.viewers))
        .map_err(helix_err_to_string)
}

/// Broadcaster : display_name, avatar, type, description.
#[tauri::command]
async fn twitch_broadcaster(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::BroadcasterInfo, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::broadcaster(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Unfollows : historique des unfollows détectés au démarrage (lecture disque).
#[tauri::command]
fn twitch_communaute_unfollows(
    app: AppHandle,
) -> Result<Vec<config::UnfollowEntry>, String> {
    let hist = config::lire_unfollows(&app)?;
    // Tri : plus récent en premier.
    let mut liste = hist.liste;
    liste.sort_by(|a, b| b.date_unfollow.cmp(&a.date_unfollow));
    Ok(liste)
}

// ===== Modération Twitch (Helix lecture + écriture) =====

/// Liste les VIPs de la chaîne.
#[tauri::command]
async fn twitch_lister_vips(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::VipEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_vips(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|v| v.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for v in &mut liste {
            if let Some((_, url)) = map.get(&v.user_id) {
                v.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Liste les modérateurs de la chaîne.
#[tauri::command]
async fn twitch_lister_moderateurs(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::ModEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_moderators(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|m| m.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for m in &mut liste {
            if let Some((_, url)) = map.get(&m.user_id) {
                m.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Liste les utilisateurs bannis/timeout de la chaîne.
#[tauri::command]
async fn twitch_lister_bannis(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::BannedEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let mut liste = twitch_helix::list_banned(&uid, &access)
        .await
        .map_err(helix_err_to_string)?;
    let ids: Vec<String> = liste.iter().map(|b| b.user_id.clone()).collect();
    if let Ok(map) = twitch_helix::fetch_users_batch(&ids, &access).await {
        for b in &mut liste {
            if let Some((dn, url)) = map.get(&b.user_id) {
                if !dn.is_empty() {
                    b.display_name = dn.clone();
                }
                b.profile_image_url = url.clone();
            }
        }
    }
    Ok(liste)
}

/// Résout un login Twitch en user_id (pour la recherche par pseudo).
#[tauri::command]
async fn twitch_resoudre_user(
    state: tauri::State<'_, TwitchState>,
    login: String,
) -> Result<Option<twitch_helix::ResolvedUser>, String> {
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::resolve_user_id(&login, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Bannir ou timeout un utilisateur. `duree` = None → ban permanent,
/// Some(n) → timeout de n secondes.
#[tauri::command]
async fn twitch_bannir(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
    raison: String,
    duree: Option<u32>,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::ban_user(&uid, &access, &user_id, &raison, duree)
        .await
        .map_err(helix_err_to_string)
}

/// Débannir un utilisateur.
#[tauri::command]
async fn twitch_debannir(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::unban_user(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Ajouter un VIP.
#[tauri::command]
async fn twitch_ajouter_vip(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::add_vip(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Retirer un VIP.
#[tauri::command]
async fn twitch_retirer_vip(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::remove_vip(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Ajouter un modérateur.
#[tauri::command]
async fn twitch_ajouter_moderateur(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::add_moderator(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Retirer un modérateur.
#[tauri::command]
async fn twitch_retirer_moderateur(
    state: tauri::State<'_, TwitchState>,
    user_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::remove_moderator(&uid, &access, &user_id)
        .await
        .map_err(helix_err_to_string)
}

/// Supprimer un message de chat (par message_id).
#[tauri::command]
async fn twitch_supprimer_message(
    state: tauri::State<'_, TwitchState>,
    message_id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::delete_chat_message(&uid, &access, &message_id)
        .await
        .map_err(helix_err_to_string)
}

/// Envoie une commande IRC au chat Twitch (ex: "/clear", "/ban user").
/// Non-fatal si l'IRC n'est pas démarré (retourne false).
#[tauri::command]
fn twitch_envoyer_commande_chat(
    state: tauri::State<'_, TwitchState>,
    commande: String,
) -> bool {
    state.envoyer_commande_irc(&commande)
}

// ===== Contrôles en direct Twitch (Helix — section « En direct ») =====

/// Infos de la chaîne (titre, jeu, tags) : GET /channels.
#[tauri::command]
async fn twitch_live_channel_info(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::ChannelInfo, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::channel_info(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Modifier titre/catégorie/tags : PATCH /channels. Corps partiel —
/// un champ None n'est pas sérialisé vers Helix.
#[tauri::command]
async fn twitch_live_maj_channel(
    state: tauri::State<'_, TwitchState>,
    title: Option<String>,
    game_id: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::maj_channel(&uid, &access, title.as_deref(), game_id.as_deref(), tags)
        .await
        .map_err(helix_err_to_string)
}

/// Recherche de catégorie (jeu) : GET /search/categories.
#[tauri::command]
async fn twitch_live_search_categories(
    state: tauri::State<'_, TwitchState>,
    query: String,
) -> Result<Vec<twitch_helix::CategoryEntry>, String> {
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::search_categories(&query, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Infos d'un jeu par id (jacquette) : GET /games?id=<id>. Relaie
/// twitch_helix::game_info — box_art_url quand la section s'ouvre avec
/// un game_id déjà connu. `None` si l'id n'existe plus.
#[tauri::command]
async fn twitch_live_game_info(
    state: tauri::State<'_, TwitchState>,
    game_id: String,
) -> Result<Option<twitch_helix::CategoryEntry>, String> {
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::game_info(&game_id, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Recherche de chaîne (suggestions raid) : GET /search/channels.
#[tauri::command]
async fn twitch_live_search_channels(
    state: tauri::State<'_, TwitchState>,
    query: String,
) -> Result<Vec<twitch_helix::ChannelSearchEntry>, String> {
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::search_channels(&query, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Créer un marqueur de stream : POST /streams/markers. Hors-ligne →
/// message Helix remonté (pas de retry).
#[tauri::command]
async fn twitch_live_marker(
    state: tauri::State<'_, TwitchState>,
    description: Option<String>,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::create_marker(&uid, &access, description.as_deref())
        .await
        .map_err(helix_err_to_string)
}

/// Lancer une pub : POST /channels/commercial. La durée est relayée —
/// le refus hors {30,60,90,120,150,180} est dans start_commercial.
#[tauri::command]
async fn twitch_live_pub(
    state: tauri::State<'_, TwitchState>,
    duree: u32,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::start_commercial(&uid, &access, duree)
        .await
        .map_err(helix_err_to_string)
}

/// Lancer un raid vers un login : résolution (resolve_user_id) puis
/// POST /raids. Chaîne introuvable ou pas en live → message clair.
#[tauri::command]
async fn twitch_live_raid_lancer(
    state: tauri::State<'_, TwitchState>,
    login: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    let cible = twitch_helix::resolve_user_id(&login, &access)
        .await
        .map_err(helix_err_to_string)?
        .ok_or_else(|| format!("Chaîne introuvable : {}", login))?;
    twitch_helix::start_raid(&uid, &cible.user_id, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Annuler le raid en cours : DELETE /raids.
#[tauri::command]
async fn twitch_live_raid_annuler(
    state: tauri::State<'_, TwitchState>,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::cancel_raid(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Lire les modes de chat : GET /chat/settings.
#[tauri::command]
async fn twitch_live_chat_settings(
    state: tauri::State<'_, TwitchState>,
) -> Result<twitch_helix::ChatSettings, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::chat_settings(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Modifier les modes de chat : PATCH /chat/settings. Corps partiel —
/// les champs None ne sont pas envoyés à Helix.
#[tauri::command]
async fn twitch_live_chat_settings_maj(
    state: tauri::State<'_, TwitchState>,
    settings: twitch_helix::ChatSettingsPatch,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::patch_chat_settings(&uid, &access, &settings)
        .await
        .map_err(helix_err_to_string)
}

/// Envoyer une annonce colorée : POST /chat/announcements.
#[tauri::command]
async fn twitch_live_annonce(
    state: tauri::State<'_, TwitchState>,
    message: String,
    couleur: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::send_announcement(&uid, &access, &message, &couleur)
        .await
        .map_err(helix_err_to_string)
}

/// Shield Mode actif ? : GET /moderation/shield_mode.
#[tauri::command]
async fn twitch_live_shield_status(
    state: tauri::State<'_, TwitchState>,
) -> Result<bool, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::shield_status(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Activer/désactiver le Shield Mode : PUT /moderation/shield_mode.
/// La confirmation avant activation est côté frontend.
#[tauri::command]
async fn twitch_live_shield_set(
    state: tauri::State<'_, TwitchState>,
    actif: bool,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::set_shield(&uid, &access, actif)
        .await
        .map_err(helix_err_to_string)
}

/// Demandes d'unban en attente : GET /moderation/unban_requests.
#[tauri::command]
async fn twitch_live_unban_requests(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::UnbanRequestEntry>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::unban_requests(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Approuver/refuser une demande d'unban : PATCH /moderation/unban_requests.
#[tauri::command]
async fn twitch_live_unban_resoudre(
    state: tauri::State<'_, TwitchState>,
    unban_request_id: String,
    approuver: bool,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::resolve_unban_request(&uid, &access, &unban_request_id, approuver)
        .await
        .map_err(helix_err_to_string)
}

/// Autoriser/refuser un message retenu par AutoMod : POST
/// /moderation/automod/message. La file arrive via EventSub.
#[tauri::command]
async fn twitch_live_automod_resoudre(
    state: tauri::State<'_, TwitchState>,
    msg_id: String,
    autoriser: bool,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::automod_resolve(&uid, &access, &msg_id, autoriser)
        .await
        .map_err(helix_err_to_string)
}

/// Télécharge la jacquette du jeu dans medias/ : `jacquette-<game_id>.jpg`,
/// écrasé à chaque pose. Commande dédiée — pas d'import par URL générique.
/// Gardes AVANT toute requête : schéma https + hôte exactement
/// static-cdn.jtvnw.net (CDN box art Twitch), game_id alphanumérique
/// (jamais de séparateur dans un nom de fichier). Corps plafonné à
/// MAX_IMG_BYTES et JPEG vérifié par magic bytes (FF D8 FF) — sinon rien
/// n'est écrit. Retourne le chemin relatif "medias/jacquette-<game_id>.jpg" ;
/// la pose du widget et le commitScene relèvent du store scène.
#[tauri::command]
async fn twitch_live_poser_jacquette(
    app: AppHandle,
    box_art_url: String,
    game_id: String,
) -> Result<String, String> {
    if game_id.is_empty() || !game_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("game_id invalide".into());
    }
    // URL à trous reconstruite à 285x380 puis hôte figé : refus avant requête.
    let url = box_art_url.replace("{width}", "285").replace("{height}", "380");
    let u = reqwest::Url::parse(&url).map_err(|e| format!("URL jacquette: {}", e))?;
    if u.scheme() != "https" || u.host_str() != Some("static-cdn.jtvnw.net") {
        return Err("URL jacquette refusée (hôte non Twitch)".into());
    }
    let resp = reqwest::get(u)
        .await
        .map_err(|e| format!("GET jacquette: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Jacquette HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Lecture jacquette: {}", e))?;
    if bytes.len() > MAX_IMG_BYTES as usize {
        return Err(format!("Jacquette trop volumineuse ({} octets)", bytes.len()));
    }
    if !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err("Jacquette non-JPEG (magic bytes invalides)".into());
    }
    let dest = config::data_dir(&app)?
        .join("medias")
        .join(format!("jacquette-{}.jpg", game_id));
    std::fs::write(&dest, &bytes).map_err(|e| format!("write jacquette: {}", e))?;
    Ok(format!("medias/jacquette-{}.jpg", game_id))
}

/// Creator Goals Twitch déclarés : GET /goals (objectifs ACTIFS seulement —
/// un par type max, déclarés dans le Creator Dashboard ; aucune API
/// d'écriture). Appelée à l'ouverture de la carte d'édition d'un widget
/// goal (choix du goal à lier) — zéro persistance, Helix relu à chaque
/// ouverture. Scope channel:read:goals ; 403 → helix_err_to_string mappe
/// NeedReauth → l'UI propose la reconnexion.
#[tauri::command]
async fn twitch_goals_lister(
    state: tauri::State<'_, TwitchState>,
) -> Result<Vec<twitch_helix::CreatorGoal>, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::creator_goals(&uid, &access)
        .await
        .map_err(helix_err_to_string)
}

// ===== Planning Twitch (schedule Helix — onglet Planning, Interactions Live) =====

/// Lire le planning Twitch : GET /schedule (segments + vacances).
/// `start_time` = borne basse RFC3339 optionnelle (début de la vue courante).
/// Zéro persistance : Helix est relu à chaque ouverture/navigation de l'onglet.
#[tauri::command]
async fn twitch_planning_schedule(
    state: tauri::State<'_, TwitchState>,
    start_time: Option<String>,
) -> Result<twitch_helix::ScheduleResp, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::schedule_get(&uid, start_time.as_deref(), &access)
        .await
        .map_err(helix_err_to_string)
}

/// Créer un segment : POST /schedule/segment. `params.is_recurring`=false
/// tente un ponctuel ; si Twitch refuse (chaîne non affiliée → 403
/// "non-recurring", distingué de NeedReauth dans helix_write) on retombe
/// sur récurrent — le choix produit est « ponctuel avec repli récurrent ».
#[tauri::command]
async fn twitch_planning_segment_creer(
    state: tauri::State<'_, TwitchState>,
    params: twitch_helix::ScheduleSegmentNew,
) -> Result<twitch_helix::ScheduleSegment, String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    match twitch_helix::schedule_segment_create(&uid, &access, &params).await {
        Err(twitch_helix::HelixError::Autre(m))
            if !params.is_recurring && m.contains("non-recurring") =>
        {
            // Chaîne non affiliée : Twitch n'accepte que les récurrents → repli.
            let repli = twitch_helix::ScheduleSegmentNew {
                is_recurring: true,
                ..params
            };
            twitch_helix::schedule_segment_create(&uid, &access, &repli)
                .await
                .map_err(helix_err_to_string)
        }
        r => r.map_err(helix_err_to_string),
    }
}

/// Modifier un segment : PATCH /schedule/segment (corps partiel — les champs
/// None ne sont pas envoyés). ⚠️ Sur un récurrent, les changements
/// s'appliquent à toute la série (confirmé côté UI).
#[tauri::command]
async fn twitch_planning_segment_maj(
    state: tauri::State<'_, TwitchState>,
    id: String,
    patch: twitch_helix::ScheduleSegmentPatch,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::schedule_segment_update(&uid, &access, &id, &patch)
        .await
        .map_err(helix_err_to_string)
}

/// Supprimer un segment : DELETE /schedule/segment. ⚠️ Sur un récurrent,
/// TOUTE la série est supprimée (confirmé côté UI).
#[tauri::command]
async fn twitch_planning_segment_supprimer(
    state: tauri::State<'_, TwitchState>,
    id: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::schedule_segment_delete(&uid, &access, &id)
        .await
        .map_err(helix_err_to_string)
}

/// Définir/désactiver les vacances du planning : PATCH /schedule/settings.
#[tauri::command]
async fn twitch_planning_vacances(
    state: tauri::State<'_, TwitchState>,
    actif: bool,
    debut: Option<String>,
    fin: Option<String>,
    timezone: String,
) -> Result<(), String> {
    let uid = state.user_id_courant().ok_or("Pas connecté")?;
    let access = state.access_courant().ok_or("Pas connecté")?;
    twitch_helix::schedule_settings_vacances(
        &uid,
        &access,
        actif,
        debut.as_deref(),
        fin.as_deref(),
        &timezone,
    )
    .await
    .map_err(helix_err_to_string)
}

// ===== Kick (lecture seule, WS côté Rust) =====

/// Connecte au chat Kick : résout le slug → chatroom ID, fetch un viewer token,
/// ouvre le WebSocket Pusher (côté Rust avec Origin: https://kick.com), subscribe
/// au channel chatrooms.<id>.v2. Le WS tourne en arrière-plan (tokio::spawn).
/// emit `kick:connecte` quand subscribed, `kick:deconnecte` sur close/error.
#[tauri::command]
async fn kick_connecter(
    app: AppHandle,
    state: tauri::State<'_, KickState>,
    slug: String,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // Arrêter un éventuel WS précédent.
    state.stop_ws();
    state.reset_cancel();

    // 1. Résoudre slug → (channel ID, chatroom ID)
    let (channel_id, chatroom_id) = kick::resolve_channel(&slug).await?;

    // 2. Démarrer le WS Centrifuge en arrière-plan
    //    (connexion anonyme : UUID éphémère, jeton « guest » — pas de compte)
    let cancel = state.cancel_clone();
    let handle = kick::run_ws(app.clone(), channel_id, chatroom_id, cancel);

    {
        let mut h = state.ws_handle.lock().unwrap();
        *h = Some(handle);
    }
    state.set_slug(Some(slug.clone()));
    // Persister le slug pour l'auto-resume au prochain boot.
    if let Err(e) = crate::config::sauver_kick_slug(&app, &slug) {
        eprintln!("[Kick] WARN sauver slug: {}", e);
    }
    // set_connected(true) se fait dans run_ws quand subscription_succeeded.
    Ok(())
}

/// Déconnecte Kick : arrête le WS (cancel + abort) + emit `kick:deconnecte`.
#[tauri::command]
fn kick_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, KickState>,
) -> Result<(), String> {
    state.stop_ws();
    state.set_connected(false);
    state.set_slug(None);
    // Effacer le slug persisté (déconnexion volontaire).
    if let Err(e) = crate::config::effacer_kick_slug(&app) {
        eprintln!("[Kick] WARN effacer slug: {}", e);
    }
    let _ = app.emit("kick:deconnecte", ());
    Ok(())
}

/// Retourne l'état de connexion Kick (true/false).
#[tauri::command]
fn kick_etat(state: tauri::State<'_, KickState>) -> bool {
    state.is_connected()
}

/// Retourne le slug Kick sauvegardé (pour pré-remplir l'input).
/// None si pas de slug sauvegardé.
#[tauri::command]
fn kick_slug_courant(app: AppHandle) -> Option<String> {
    crate::config::lire_kick_slug(&app).ok().flatten()
}

// ===== YouTube (chat polling + communauté Data API v3) =====

/// Connecte YouTube : si token valide en coffre → valider + resolve channel +
/// start chat polling + emit `youtube:connecte`. Sinon → Device Code Flow :
/// emit `youtube:device` (user_code + uri) → spawn poll cancellable → succès :
/// save + resolve channel + start chat + emit `youtube:connecte`.
#[tauri::command]
async fn youtube_connecter(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // 1. Token en coffre ? → valider (refresh si 401) + chat direct.
    if let Ok(Some(tokens)) = youtube_auth::lire_tokens() {
        return connect_with_tokens_youtube(&app, state.inner(), tokens).await;
    }

    // 2. Pas de token → Device Code Flow.
    let flow = match youtube_auth::demarrer_device_flow().await {
        Ok(f) => f,
        Err(e) => {
            let _ = app.emit("youtube:erreur", &e);
            return Err(e);
        }
    };

    // Préparer un cancel frais pour ce flow.
    state.reset_cancel();
    let cancel = state.cancel_clone();

    // Émettre les infos device vers le frontend (modal).
    let device_info = serde_json::json!({
        "user_code": flow.user_code,
        "verification_uri": flow.verification_url,
        "expires_in": flow.expires_in,
    });
    let _ = app.emit("youtube:device", &device_info);

    // Poll en arrière-plan (cancellable). Sur succès → save + chat + connecte.
    let app2 = app.clone();
    let state2 = state.inner().clone();
    let device_code = flow.device_code;
    let interval = flow.interval;
    let expires_in = flow.expires_in;
    tauri::async_runtime::spawn(async move {
        match youtube_auth::poll_token(device_code, interval, expires_in, cancel).await {
            Ok(mut tokens) => {
                // Resolve channel_id + login via Data API.
                match youtube_data::resolve_channel(&tokens.access).await {
                    Ok((channel_id, login)) => {
                        tokens.channel_id = channel_id;
                        tokens.login = login;
                    }
                    Err(e) => {
                        eprintln!("[YouTube] WARN resolve channel échoué: {}", e);
                        // Non-fatal : on continue sans channel_id/login.
                    }
                }
                if let Err(e) = youtube_auth::sauver_tokens(&tokens) {
                    eprintln!("[YouTube] ERR coffre save: {}", e);
                }
                let _ = connect_with_tokens_youtube(&app2, &state2, tokens).await;
            }
            Err(e) => {
                let _ = app2.emit("youtube:erreur", &e);
            }
        }
    });

    Ok(())
}

/// Helper : valide/refresh les tokens, resolve channel, démarre chat polling,
/// emit `youtube:connecte`.
async fn connect_with_tokens_youtube(
    app: &AppHandle,
    state: &YoutubeState,
    mut tokens: youtube_auth::Tokens,
) -> Result<(), String> {
    // Valider le token via tokeninfo (non-fatal si échec).
    if tokens.user_id.is_empty() {
        if let Ok(uid) = youtube_auth::valider_token(&tokens.access).await {
            tokens.user_id = uid;
        }
    }

    // Resolve channel_id + login si manquants.
    if tokens.channel_id.is_empty() || tokens.login.is_empty() {
        match youtube_data::resolve_channel(&tokens.access).await {
            Ok((channel_id, login)) => {
                tokens.channel_id = channel_id;
                tokens.login = login;
                // Re-sauver avec les infos complètes.
                if let Err(e) = youtube_auth::sauver_tokens(&tokens) {
                    eprintln!("[YouTube] WARN coffre save (resolve): {}", e);
                }
            }
            Err(e) => {
                eprintln!("[YouTube] WARN resolve channel: {}", e);
                // Non-fatal : on continue sans channel_id/login.
            }
        }
    }

    // NE PAS démarrer le chat polling automatiquement — l'utilisateur doit
    // cliquer sur "Chat live ON" manuellement (bouton dans la Toolbar).
    // Le chat polling consomme de l'API quota YouTube, on évite le polling
    // inutile quand l'utilisateur ne stream pas.
    state.set_connected(true);
    state.set_login(Some(tokens.login.clone()));
    state.set_channel_id(Some(tokens.channel_id.clone()));
    state.set_access(Some(tokens.access.clone()));
    let _ = app.emit("youtube:connecte", &tokens.login);
    Ok(())
}

/// Démarre manuellement le chat polling YouTube Live. Appelé par le bouton
/// "Chat live ON" dans la Toolbar. Si pas de live actif → emit youtube:pas-de-live
/// et la tâche s'arrête immédiatement (l'utilisateur réessaiera quand il stream).
#[tauri::command]
fn youtube_demarrer_chat(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    if !state.is_connected() {
        return Err("YouTube non connecté".to_string());
    }
    // Arrêter un éventuel chat précédent.
    state.stop_chat();
    state.reset_cancel();
    let cancel = state.cancel_clone();
    let access = state.access_courant().ok_or("Pas de token access")?;
    let channel_id = state
        .login
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|_| state.channel_id.lock().unwrap().clone())
        .ok_or("Pas de channel_id")?;
    let chat_tx = app
        .try_state::<ScenesState>()
        .ok_or("ScenesState absent")?
        .chat_tx
        .clone();
    let handle = youtube_chat::demarrer(
        app.clone(),
        chat_tx,
        access,
        channel_id,
        cancel,
    );
    {
        let mut h = state.chat_handle.lock().unwrap();
        *h = Some(handle);
    }
    Ok(())
}

/// Arrête le chat polling YouTube Live sans déconnecter le compte
/// (bouton "Chat live OFF"). Le token et les infos chaîne restent en mémoire.
#[tauri::command]
fn youtube_arreter_chat(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    state.stop_chat();
    state.reset_cancel();
    let _ = app.emit("youtube:pas-de-live", ());
    Ok(())
}

/// Annule le Device Code Flow YouTube en cours.
#[tauri::command]
fn youtube_annuler_device_flow(state: tauri::State<'_, YoutubeState>) -> Result<(), String> {
    let c = state.cancel.lock().unwrap();
    c.store(true, Ordering::SeqCst);
    Ok(())
}

/// Déconnecte YouTube : arrête chat + révoque token (best-effort) + efface coffre
/// + emit `youtube:deconnecte`.
#[tauri::command]
async fn youtube_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, YoutubeState>,
) -> Result<(), String> {
    state.stop_chat();
    state.set_connected(false);
    state.set_login(None);
    state.set_channel_id(None);
    state.set_access(None);
    // Revoke best-effort.
    if let Ok(Some(tokens)) = youtube_auth::lire_tokens() {
        if let Err(e) = youtube_auth::revoke_token(&tokens.access).await {
            eprintln!("[YouTube] WARN revoke échoué (on efface quand même): {}", e);
        }
    }
    let _ = youtube_auth::effacer_tokens();
    let _ = app.emit("youtube:deconnecte", ());
    Ok(())
}

/// Retourne l'état de connexion YouTube (true/false).
#[tauri::command]
fn youtube_etat(state: tauri::State<'_, YoutubeState>) -> bool {
    state.is_connected()
}

/// Retourne le login de la chaîne YouTube connectée (ou null si déconnecté).
#[tauri::command]
fn youtube_login_courant(state: tauri::State<'_, YoutubeState>) -> Option<String> {
    state.login_courant()
}

/// Channel info : GET /channels?part=snippet,statistics&mine=true.
#[tauri::command]
async fn youtube_communaute_channel(
    state: tauri::State<'_, YoutubeState>,
) -> Result<youtube_data::ChannelInfo, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    youtube_data::channel_info(&access)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

/// Members : GET /members?part=snippet.
#[tauri::command]
async fn youtube_communaute_members(
    state: tauri::State<'_, YoutubeState>,
) -> Result<youtube_data::MembersResp, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    youtube_data::members(&access)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

/// Live viewers : search?channelId=...&eventType=live → /videos → concurrentViewers.
#[tauri::command]
async fn youtube_communaute_viewers(
    state: tauri::State<'_, YoutubeState>,
) -> Result<Option<u32>, String> {
    let access = state
        .access_courant()
        .ok_or("YouTube non connecté")?;
    let channel_id = state
        .channel_id
        .lock().unwrap()
        .clone()
        .ok_or("YouTube non connecté")?;
    youtube_data::live_viewers(&access, &channel_id)
        .await
        .map_err(|e| match e {
            youtube_data::YoutubeError::Deconnecte => "YouTube non connecté".to_string(),
            youtube_data::YoutubeError::Autre(s) => s,
        })
}

// ===== TikTok (chat live via PirateTok — reverse engineering Webcast) =====

/// Connecte au chat TikTok Live d'un streamer (username sans @).
/// Démarre le WebSocket PirateTok en arrière-plan (tokio::spawn).
/// emit `tiktok:connecte` quand connecté, `tiktok:deconnecte` sur close/error,
/// `chat:message` pour chaque message chat, `tiktok:viewers` pour le count.
#[tauri::command]
async fn tiktok_connecter(
    app: AppHandle,
    state: tauri::State<'_, TiktokState>,
    username: String,
) -> Result<(), String> {
    if state.is_connected() {
        return Ok(()); // déjà connecté
    }

    // Arrêter un éventuel chat précédent.
    state.stop_chat();
    state.reset_cancel();

    // Récupérer le chat_tx (broadcast vers :4321).
    let chat_tx = app
        .try_state::<ScenesState>()
        .map(|s| s.chat_tx.clone())
        .ok_or("ScenesState non initialisé")?;

    // Démarrer le WebSocket PirateTok en arrière-plan.
    let cancel = state.cancel_clone();
    let handle = tiktok_chat::demarrer(app.clone(), chat_tx, username.clone(), cancel);

    {
        let mut h = state.chat_handle.lock().unwrap();
        *h = Some(handle);
    }
    state.set_username(Some(username.clone()));
    // Persister le username pour l'auto-resume au prochain boot.
    if let Err(e) = crate::config::sauver_tiktok_username(&app, &username) {
        eprintln!("[TikTok] WARN sauver username: {}", e);
    }
    // set_connected(true) se fait via l'event tiktok:connecte (frontend listener).
    // Mais on le set aussi ici car l'event arrive async.
    Ok(())
}

/// Déconnecte TikTok : arrête le chat (cancel + abort) + emit `tiktok:deconnecte`.
#[tauri::command]
fn tiktok_deconnecter(
    app: AppHandle,
    state: tauri::State<'_, TiktokState>,
) -> Result<(), String> {
    state.stop_chat();
    state.set_connected(false);
    state.set_username(None);
    if let Err(e) = crate::config::effacer_tiktok_username(&app) {
        eprintln!("[TikTok] WARN effacer username: {}", e);
    }
    let _ = app.emit("tiktok:deconnecte", ());
    Ok(())
}

/// Retourne l'état de connexion TikTok (true/false).
#[tauri::command]
fn tiktok_etat(state: tauri::State<'_, TiktokState>) -> bool {
    state.is_connected()
}

/// Retourne le username TikTok sauvegardé (pour pré-remplir l'input).
/// None si pas de username sauvegardé.
#[tauri::command]
fn tiktok_username_courant(app: AppHandle) -> Option<String> {
    crate::config::lire_tiktok_username(&app).ok().flatten()
}

// ===== Clips de bienvenue (welcome) =====

/// Retourne l'état courant de la file d'attente welcome (clip en cours + queue).
#[tauri::command]
fn welcome_etat(state: tauri::State<'_, welcome::WelcomeState>) -> welcome::QueueEtat {
    state.etat()
}

/// Retourne le registre Twitch complet (login → config viewer).
#[tauri::command]
fn welcome_registre_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
) -> Vec<(String, welcome::ViewerConfig)> {
    state.lire_registre_twitch()
}

/// Sauvegarde la config d'un viewer Twitch dans le registre + disque.
#[tauri::command]
fn welcome_sauver_viewer_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
    login: String,
    config: welcome::ViewerConfig,
) -> Result<(), String> {
    state.sauver_viewer_twitch(&login, config)
}

/// Supprime un viewer Twitch du registre + disque.
#[tauri::command]
fn welcome_supprimer_viewer_twitch(
    state: tauri::State<'_, welcome::WelcomeState>,
    login: String,
) -> Result<(), String> {
    state.supprimer_viewer_twitch(&login)
}

/// Active/désactive la config globale des clips de bienvenue.
#[tauri::command]
fn welcome_config_globale_actif(
    state: tauri::State<'_, welcome::WelcomeState>,
    actif: bool,
) -> Result<(), String> {
    state.set_config_globale_actif(actif)
}

/// Met à jour la config de l'overlay welcome (position/taille côté diffusion).
#[tauri::command]
fn welcome_set_overlay_config(
    state: tauri::State<'_, welcome::WelcomeState>,
    config: welcome::OverlayConfig,
) -> Result<(), String> {
    state.set_overlay_config(config)
}

/// Test manuel : lance un clip côté diffusion (bypass queue). Résout MP4 +
/// émet welcome-clip-play. Utilisé par la modale Interaction viewer pour
/// tester un clip au clic.
#[tauri::command]
async fn welcome_tester_clip(
    state: tauri::State<'_, welcome::WelcomeState>,
    twitch: tauri::State<'_, TwitchState>,
    clip_id: String,
    clip_titre: String,
    clip_duree_ms: u64,
    display_name: String,
) -> Result<(), String> {
    // Avatar + bio + bannière via Helix /users (best-effort : None si Twitch
    // déconnecté ou fetch échoué — la carte s'affiche alors sans).
    let access_opt = { twitch.access.lock().unwrap().clone() };
    let (avatar, bio, banniere) = match access_opt {
        Some(access) => {
            let client = reqwest::Client::new();
            match twitch_chat::fetch_user_infos(&client, &access, &display_name.to_lowercase()).await {
                Ok((av, b, ba)) => (
                    Some(av).filter(|s| !s.is_empty()),
                    Some(b).filter(|s| !s.is_empty()),
                    Some(ba).filter(|s| !s.is_empty()),
                ),
                Err(e) => {
                    eprintln!("[Welcome] tester_clip: fetch avatar/bio échoué : {}", e);
                    (None, None, None)
                }
            }
        }
        None => (None, None, None),
    };
    state
        .tester_clip(&clip_id, &clip_titre, clip_duree_ms, &display_name, avatar, bio, banniere)
        .await
}

/// Définit la durée d'affichage globale des clips (0 = durée naturelle).
#[tauri::command]
fn welcome_set_duree_affichage(
    state: tauri::State<'_, welcome::WelcomeState>,
    ms: u64,
) -> Result<(), String> {
    state.set_duree_affichage(ms)
}

/// Bascule le mode d'affichage welcome : true = squelette de position unifié
/// (défaut), false = spotlight plein écran (voile noir + carte 85 % + intro
/// avatar 3 s).
#[tauri::command]
fn welcome_set_utiliser_squelette(
    state: tauri::State<'_, welcome::WelcomeState>,
    actif: bool,
) -> Result<(), String> {
    state.set_utiliser_squelette(actif)
}

/// Stop le clip courant + vide la queue.
#[tauri::command]
fn welcome_stop(state: tauri::State<'_, welcome::WelcomeState>) {
    state.stop();
}

/// Skip le clip courant → passe au suivant.
#[tauri::command]
fn welcome_skip(state: tauri::State<'_, welcome::WelcomeState>) {
    state.skip();
}

/// Retire un item spécifique de la queue (par id).
#[tauri::command]
fn welcome_retirer(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.retirer(&id);
}

/// Remonte un item dans la queue (vers le début).
#[tauri::command]
fn welcome_remonter(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.remonter(&id);
}

/// Descend un item dans la queue (vers la fin).
#[tauri::command]
fn welcome_descendre(
    state: tauri::State<'_, welcome::WelcomeState>,
    id: String,
) {
    state.descendre(&id);
}

/// Vide la queue (sans stopper le clip courant).
#[tauri::command]
fn welcome_vider(state: tauri::State<'_, welcome::WelcomeState>) {
    state.vider();
}

/// Reset le seen set (nouveau stream → tous les streamers redeviennent éligibles).
#[tauri::command]
fn welcome_reset_session(state: tauri::State<'_, welcome::WelcomeState>) {
    state.reset_session();
}

/// Liste les clips récents d'un broadcaster Twitch (GET /clips Helix).
/// `broadcaster_id` = user_id du streamer. `first` = nombre de clips (max 100).
#[tauri::command]
async fn welcome_lister_clips_streamer(
    state: tauri::State<'_, TwitchState>,
    broadcaster_id: String,
    first: u32,
) -> Result<Vec<twitch_clips::ClipInfo>, String> {
    let access = state
        .access
        .lock()
        .unwrap()
        .clone()
        .ok_or("Twitch non connecté")?;
    twitch_clips::liste_clips(&broadcaster_id, &access, first).await
}

/// Résout l'URL MP4 signée d'un clip Twitch (slug) via GQL.
#[tauri::command]
async fn welcome_resoudre_mp4(slug: String) -> Result<String, String> {
    // Preview dashboard (~300 px dans la modale) → 360 suffit.
    twitch_clips::resoudre_mp4(&slug, 360).await
}

/// Attribution automatique : pour chaque follower, fetch ses clips → clip aléatoire
/// → résout MP4 → sauve config. Retourne { succes, echecs }.
/// `followers` = liste de { login, user_id, display_name }.
#[tauri::command]
async fn welcome_attribuer_auto(
    app: AppHandle,
    state: tauri::State<'_, TwitchState>,
    welcome_state: tauri::State<'_, welcome::WelcomeState>,
    followers: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let access = state
        .access
        .lock()
        .unwrap()
        .clone()
        .ok_or("Twitch non connecté")?;

    let mut succes = 0u32;
    let mut echecs = 0u32;

    // Même cible que la queue welcome : 480 squelette / 720 spotlight.
    let qualite_cible = if welcome_state.etat().utiliser_squelette {
        480
    } else {
        720
    };

    for f in &followers {
        let user_id = f["user_id"].as_str().unwrap_or("");
        let login = f["login"].as_str().unwrap_or("").to_lowercase();
        let display_name = f["display_name"]
            .as_str()
            .or_else(|| f["login"].as_str())
            .unwrap_or("")
            .to_string();

        if user_id.is_empty() || login.is_empty() {
            echecs += 1;
            continue;
        }

        match twitch_clips::clip_aleatoire_resolu(user_id, &access, qualite_cible).await {
            Ok(Some(clip)) => {
                let config = welcome::ViewerConfig {
                    actif: true,
                    clip_id: clip.info.id.clone(),
                    clip_titre: clip.info.title.clone(),
                    clip_thumbnail: clip.info.thumbnail_url.clone(),
                    clip_mp4_url: clip.mp4_url.clone(),
                    clip_duree_ms: clip.duree_ms,
                    message: format!("Bienvenue {} !", display_name),
                    display_name: display_name.clone(),
                    avatar: None,
                    bio: None,
                    banniere: None,
                };
                if welcome_state.sauver_viewer_twitch(&login, config).is_err() {
                    echecs += 1;
                } else {
                    succes += 1;
                }
            }
            Ok(None) => {
                echecs += 1;
            }
            Err(_) => {
                echecs += 1;
            }
        }
        // Émettre progression vers le dashboard (non-fatal).
        let _ = app.emit(
            "welcome:attribution-progress",
            serde_json::json!({ "succes": succes, "echecs": echecs, "login": login }),
        );
    }

    Ok(serde_json::json!({ "succes": succes, "echecs": echecs }))
}

// ===== Présentation streamer (portage « streamers favoris » ancienne app) =====

/// État courant : favoris persistés + présentation en cours (contrôles dashboard).
#[tauri::command]
fn presentation_etat(
    state: tauri::State<'_, presentation::PresentationState>,
) -> presentation::PresentationEtat {
    state.etat()
}

/// Ajoute/remplace un favori (upsert par login) + persiste + émet état.
#[tauri::command]
fn presentation_favori_sauver(
    state: tauri::State<'_, presentation::PresentationState>,
    favori: presentation::FavoriConfig,
) -> Result<(), String> {
    state.sauver_favori(favori)
}

/// Retire un favori + persiste + émet état.
#[tauri::command]
fn presentation_favori_supprimer(
    state: tauri::State<'_, presentation::PresentationState>,
    login: String,
) -> Result<(), String> {
    state.supprimer_favori(&login)
}

/// Affecte une piste audio à un favori ("voix"|"musique", src = medias/… ou
/// null pour retirer). Persiste + émet état.
#[tauri::command]
fn presentation_favori_set_son(
    state: tauri::State<'_, presentation::PresentationState>,
    login: String,
    piste: String,
    src: Option<String>,
) -> Result<(), String> {
    state.favori_set_son(&login, &piste, src)
}

/// Change le mode d'un favori ("theatrical"|"card") — bascule à chaud côté
/// diffusion si c'est le streamer actuellement présenté.
#[tauri::command]
fn presentation_favori_set_mode(
    state: tauri::State<'_, presentation::PresentationState>,
    login: String,
    mode: String,
) -> Result<(), String> {
    state.favori_set_mode(&login, &mode)
}

/// Recherche de chaînes Twitch (GET /search/channels — même endpoint que la
/// recherche de raid). Pour ajouter un favori ou présenter sans l'enregistrer.
#[tauri::command]
async fn presentation_rechercher(
    state: tauri::State<'_, TwitchState>,
    query: String,
) -> Result<Vec<twitch_helix::ChannelSearchEntry>, String> {
    let access = state.access_courant().ok_or("Twitch non connecté")?;
    twitch_helix::search_channels(&query, &access)
        .await
        .map_err(helix_err_to_string)
}

/// Lance la présentation d'un streamer : fetch Helix complet (profil, chaîne,
/// live, clips, jeux) → émet presentation-show → retourne les données pour
/// les contrôles live du dashboard.
#[tauri::command]
async fn presentation_lancer(
    state: tauri::State<'_, presentation::PresentationState>,
    twitch: tauri::State<'_, TwitchState>,
    user_id: String,
    mode: Option<String>,
) -> Result<presentation::PresentationDonnees, String> {
    let access = twitch.access_courant().ok_or("Twitch non connecté")?;
    state.lancer(&user_id, mode, &access).await
}

/// Ferme la présentation côté diffusion (rideaux se referment). Idempotent.
#[tauri::command]
fn presentation_fermer(state: tauri::State<'_, presentation::PresentationState>) {
    state.fermer();
}

/// Joue un clip sur la diffusion : résout l'URL MP4 signée au clic (les URL
/// Twitch expirent — jamais d'URL persistée) → émet presentation-clip-play.
#[tauri::command]
async fn presentation_clip_jouer(
    state: tauri::State<'_, presentation::PresentationState>,
    slug: String,
) -> Result<(), String> {
    state.clip_jouer(&slug).await
}

/// Arrête le clip en cours côté diffusion (retour à la carte présentation).
#[tauri::command]
fn presentation_clip_stopper(state: tauri::State<'_, presentation::PresentationState>) {
    state.clip_stopper();
}

/// Joue une piste audio ("voix"|"musique") sur la diffusion.
/// `src` = chemin relatif medias/<uuid>.<ext> (issu d'import_son).
#[tauri::command]
fn presentation_audio_jouer(
    state: tauri::State<'_, presentation::PresentationState>,
    piste: String,
    src: String,
) -> Result<(), String> {
    state.audio_jouer(&piste, &src)
}

/// Stoppe une piste audio ("voix"|"musique"|"toutes") sur la diffusion.
#[tauri::command]
fn presentation_audio_stopper(
    state: tauri::State<'_, presentation::PresentationState>,
    piste: String,
) -> Result<(), String> {
    state.audio_stopper(&piste)
}

// ===== Bandeau premier message =====

/// Retourne l'état courant du bandeau (config : actif, duree, position).
#[tauri::command]
fn bandeau_etat(state: tauri::State<'_, bandeau::BandeauState>) -> bandeau::BandeauEtat {
    state.etat()
}

/// Met à jour la config du bandeau (merge partiel : actif/duree_ms/position/
/// police). Persiste + émet la config vers diffusion.html + l'état dashboard.
#[tauri::command]
fn bandeau_set_config(
    state: tauri::State<'_, bandeau::BandeauState>,
    actif: Option<bool>,
    duree_ms: Option<u64>,
    position: Option<String>,
    police: Option<String>,
) -> Result<(), String> {
    state.set_config(actif, duree_ms, position, police)
}

/// Stop immédiat du bandeau courant (annule timer + émet stop vers diffusion).
#[tauri::command]
fn bandeau_stop(state: tauri::State<'_, bandeau::BandeauState>) {
    state.stop();
}

/// Reset le seen set (nouveau stream → tous les viewers redeviennent éligibles).
#[tauri::command]
fn bandeau_reset_session(state: tauri::State<'_, bandeau::BandeauState>) {
    state.reset_session();
}

/// Test manuel : lance un bandeau côté diffusion (bypass détection).
#[tauri::command]
fn bandeau_tester(
    state: tauri::State<'_, bandeau::BandeauState>,
    display_name: String,
    message: String,
) -> Result<(), String> {
    state.tester(&display_name, &message)
}

// ===== Commandes chat (!commande → overlay diffusion) =====

/// Liste complète des commandes chat (pour l'UI dashboard).
#[tauri::command]
fn commandes_etat(
    state: tauri::State<'_, commandes::CommandesState>,
) -> Vec<commandes::CommandeConfig> {
    state.etat()
}

/// Ajoute/remplace une commande chat (upsert par id) + persiste + push WS.
/// Retourne la config stockée (id généré si vide).
#[tauri::command]
fn commande_set_config(
    state: tauri::State<'_, commandes::CommandesState>,
    config: commandes::CommandeConfig,
) -> Result<commandes::CommandeConfig, String> {
    state.set_config(config)
}

/// Supprime une commande chat + persiste + push WS diffusion.
#[tauri::command]
fn commande_supprimer(
    state: tauri::State<'_, commandes::CommandesState>,
    id: String,
) -> Result<(), String> {
    state.supprimer(&id)
}

/// Test manuel : déclenche une commande côté diffusion (bypass cooldowns).
#[tauri::command]
fn commande_tester(
    state: tauri::State<'_, commandes::CommandesState>,
    id: String,
) -> Result<(), String> {
    state.tester(&id)
}

/// Exporte les commandes chat comme pack dossier portable (commandes.json +
/// medias/). Dialog = dossier PARENT. Retourne true si exporté, false si annulé.
#[tauri::command]
fn commande_exporter(
    state: tauri::State<'_, commandes::CommandesState>,
    nom_pack: String,
) -> Result<bool, String> {
    state.exporter(&nom_pack)
}

/// Importe les commandes depuis un dossier pack (dialog dossier). Importe
/// directement les noms sans conflit ; retourne conflits + réservés à trancher.
#[tauri::command]
fn commande_importer(
    state: tauri::State<'_, commandes::CommandesState>,
) -> Result<commandes::ImportCommandesResultat, String> {
    state.importer()
}

/// Phase conflits de l'import : les commandes acceptées par l'utilisateur
/// remplacent les existantes de même nom. Retourne le nb remplacé.
#[tauri::command]
fn commande_importer_ecraser(
    state: tauri::State<'_, commandes::CommandesState>,
    pack_dir: String,
    commandes: Vec<commandes::CommandeConfig>,
) -> Result<u32, String> {
    state.importer_ecraser(&pack_dir, commandes)
}

// ===== Musique (!rs → file de lecture bibliothèque locale → diffusion) =====

/// État complet musique : config + piste en cours + file + bibliothèque.
#[tauri::command]
fn musique_etat(state: tauri::State<'_, musique::MusiqueState>) -> serde_json::Value {
    state.etat()
}

/// Sauvegarde la config musique (noms de commandes normalisés + distincts).
#[tauri::command]
fn musique_set_config(
    state: tauri::State<'_, musique::MusiqueState>,
    config: musique::MusiqueConfig,
) -> Result<musique::MusiqueConfig, String> {
    state.set_config(config)
}

/// Importe des fichiers audio (multi-sélection) dans medias/musique/ — le
/// nom lisible est conservé (c'est le titre demandé par les viewers).
/// Retourne les entrées créées (les erreurs par fichier sont ignorées —
/// un fichier invalide n'invalide pas le lot).
#[tauri::command]
async fn musique_importer(
    app: AppHandle,
    state: tauri::State<'_, musique::MusiqueState>,
) -> Result<Vec<musique::MusiqueFichier>, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        let files = app
            .dialog()
            .file()
            .add_filter("Musiques", &["mp3", "ogg", "wav", "m4a", "flac"])
            .blocking_pick_files();
        let Some(files) = files else {
            return Ok(Vec::new()); // dialog annulé
        };

        let mut importes = Vec::new();
        for file in files {
            let Ok(src) = file.into_path() else {
                continue;
            };
            if let Ok(f) = musique::valider_et_copier(&app, &src) {
                importes.push(f);
            }
        }
        st.emit_update();
        Ok(importes)
    })
    .await
    .map_err(|e| format!("musique_importer: {}", e))?
}

/// Supprime un fichier de la bibliothèque (par nom de fichier uniquement).
#[tauri::command]
fn musique_supprimer_fichier(
    app: AppHandle,
    state: tauri::State<'_, musique::MusiqueState>,
    nom: String,
) -> Result<(), String> {
    musique::supprimer_fichier(&app, &nom)?;
    state.emit_update();
    Ok(())
}

/// Skip la piste courante → la suivante démarre (contrôle dashboard).
#[tauri::command]
fn musique_skip(state: tauri::State<'_, musique::MusiqueState>) {
    state.skip();
}

/// Pause ou reprend la piste courante côté diffusion.
#[tauri::command]
fn musique_pause(state: tauri::State<'_, musique::MusiqueState>, pause: bool) {
    state.pause(pause);
}

/// Stoppe la lecture ET vide la file (arrêt complet).
#[tauri::command]
fn musique_stop(state: tauri::State<'_, musique::MusiqueState>) {
    state.stop_tout();
}

/// Vide la file d'attente (la piste courante continue).
#[tauri::command]
fn musique_vider(state: tauri::State<'_, musique::MusiqueState>) {
    state.vider_file();
}

/// Retire une entrée de la file par id.
#[tauri::command]
fn musique_retirer(
    state: tauri::State<'_, musique::MusiqueState>,
    id: String,
) -> Result<(), String> {
    state.retirer_file(&id)
}

/// Déplace une entrée dans la file (delta = -1 monter, +1 descendre).
#[tauri::command]
fn musique_deplacer(
    state: tauri::State<'_, musique::MusiqueState>,
    id: String,
    delta: i32,
) -> Result<(), String> {
    state.deplacer_file(&id, delta as isize)
}

/// Ajoute des fichiers de la bibliothèque à la file (sélection streamer :
/// sans limites viewer, en fin de file ; 1re piste direct si rien ne joue).
#[tauri::command]
fn musique_file_ajouter(
    state: tauri::State<'_, musique::MusiqueState>,
    srcs: Vec<String>,
) -> Result<usize, String> {
    state.ajouter_file(&srcs)
}

/// Test manuel : joue un fichier de la bibliothèque côté diffusion.
#[tauri::command]
fn musique_tester(
    state: tauri::State<'_, musique::MusiqueState>,
    src: String,
) -> Result<(), String> {
    state.tester(&src)
}

/// Change le volume de lecture côté diffusion (persisté en config).
#[tauri::command]
fn musique_volume(
    state: tauri::State<'_, musique::MusiqueState>,
    volume: f64,
) -> Result<(), String> {
    state.set_volume(volume as f32)
}

// ===== Emoji Arena (combat d'emojis 1v1 piloté par le chat) =====

/// État complet : config + partie courante (phase lobby/combat/fin) + cooldowns.
#[tauri::command]
fn fight_etat(state: tauri::State<'_, fight::FightState>) -> serde_json::Value {
    state.etat()
}

// ===== Cadres chat (!cadres <couleur> → couleur vive du cadre unifié) =====

/// Config de la commande cadres (actif, nom, accusés chat).
#[tauri::command]
fn cadres_chat_etat(
    state: tauri::State<'_, cadres_cmd::CadresChatState>,
) -> cadres_cmd::CadresChatConfig {
    state.etat()
}

/// Sauvegarde la config (nom normalisé + refus de collision commandes/
/// musique/arène) + persiste + emit dashboard.
#[tauri::command]
fn cadres_chat_set_config(
    state: tauri::State<'_, cadres_cmd::CadresChatState>,
    config: cadres_cmd::CadresChatConfig,
) -> Result<cadres_cmd::CadresChatConfig, String> {
    state.set_config(config)
}

/// Palette fermée de couleurs nommées (grille affichée dans l'onglet Cadres).
#[tauri::command]
fn cadres_chat_palette() -> Vec<cadres_cmd::PaletteCouleur> {
    cadres_cmd::palette()
}

/// Sauvegarde la config (noms de commandes normalisés + distincts + sans
/// collision avec les commandes chat et la musique).
#[tauri::command]
fn fight_set_config(
    state: tauri::State<'_, fight::FightState>,
    config: fight::FightConfig,
) -> Result<fight::FightConfig, String> {
    state.set_config(config)
}

/// Test manuel : lance un combat factice « Streamy » vs BOT côté diffusion
/// (le pipeline complet — timers, coups du CPU, animations — est le réel).
#[tauri::command]
fn fight_tester(state: tauri::State<'_, fight::FightState>) -> Result<(), String> {
    state.tester()
}

/// Force l'arrêt : lobby → annulé, combat → fin "stop", bannière → Idle.
#[tauri::command]
fn fight_stop(state: tauri::State<'_, fight::FightState>) {
    state.stop()
}

/// Bouton dashboard : prévisualise la carte d'aide sur la diffusion.
#[tauri::command]
fn fight_previsualiser_aide(state: tauri::State<'_, fight::FightState>) {
    state.previsualiser_aide()
}

// ===== Input Viewer (capture globale clavier + souris → overlay diffusion) =====

/// Active/désactive la capture globale des entrées (clavier + souris).
/// ON → thread poll clavier 60Hz + hook souris WH_MOUSE_LL (zéro coût quand
/// OFF). L'état est émis directement sur le WS :4321 (input-viewer-etat).
#[tauri::command]
fn input_viewer_set_actif(
    state: tauri::State<'_, input_viewer::InputViewerState>,
    actif: bool,
) -> Result<(), String> {
    state.set_actif(actif)
}

/// true si la capture Input Viewer est active (pour l'état du bouton ON/OFF).
#[tauri::command]
fn input_viewer_etat_actif(state: tauri::State<'_, input_viewer::InputViewerState>) -> bool {
    state.est_actif()
}

// ===== Pad numérique (16 touches Numpad × 3 plages → overlay diffusion) =====

/// Retourne la config complète du pad (actif, plage, touches).
#[tauri::command]
fn pad_etat(state: tauri::State<'_, pad_numerique::PadNumeriqueState>) -> pad_numerique::PadConfig {
    state.etat()
}

/// Active/désactive le pad + démarre/arrête la capture clavier globale + persiste.
#[tauri::command]
fn pad_set_actif(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    actif: bool,
) -> Result<(), String> {
    state.set_actif(actif)
}

/// Remplace la config d'une touche (upsert) + persiste + émet état.
#[tauri::command]
fn pad_set_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
    config: pad_numerique::ToucheConfig,
) -> Result<(), String> {
    state.set_touche(&code, plage, config)
}

/// Supprime (reset) la config d'une touche + persiste + émet état.
#[tauri::command]
fn pad_supprimer_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
) -> Result<(), String> {
    state.supprimer_touche(&code, plage)
}

/// Change la plage courante (0-2) + persiste + émet état.
#[tauri::command]
fn pad_set_plage(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    plage: u32,
) -> Result<(), String> {
    state.set_plage(plage)
}

/// Test manuel : déclenche une touche côté diffusion (bypass capture).
#[tauri::command]
fn pad_tester_touche(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
    code: String,
    plage: u32,
) -> Result<(), String> {
    state.tester_touche(&code, plage)
}

/// Importe un média (image OU vidéo) pour une touche du pad : dialog filtré
/// selon le kind attendu → validation + copie (ou remux MKV→MP4) vers
/// AppData/StreamOS/medias/<uuid>.<ext>. Retourne `Some(rel)` si importé,
/// `None` si dialog annulé. Même mécanique que `import_alerte_media`.
#[tauri::command]
async fn pad_importer_media(
    app: AppHandle,
    kind_attendu: String,
) -> Result<Option<String>, String> {
    // Dialog natif + copie/remux sur le pool bloquant Tokio (pas le thread UI).
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;

        let (filtre_nom, exts): (&str, &[&str]) = if kind_attendu == "video" {
            ("Vidéos", VIDEO_EXT)
        } else {
            ("Images", IMG_EXT)
        };
        let file = app
            .dialog()
            .file()
            .add_filter(filtre_nom, exts)
            .blocking_pick_file();
        let Some(file) = file else {
            return Ok(None);
        };
        let src: std::path::PathBuf = file
            .into_path()
            .map_err(|e| format!("Chemin invalide: {}", e))?;

        let (rel, kind, _nom) = validate_and_copy_media(&app, &src)?;
        if kind != kind_attendu {
            return Err(format!(
                "Type de fichier inattendu : {} attendu, {} reçu",
                kind_attendu, kind
            ));
        }
        Ok(Some(rel))
    })
    .await
    .map_err(|e| format!("pad_importer_media: {}", e))?
}

/// Importe un son (mp3/ogg/wav ≤ 10 Mo) pour une touche du pad : dialog
/// « Sons » → validation + copie vers medias/. Retourne `Some(rel)` ou
/// `None` si dialog annulé. Réutilise `import_son` (même mécanique).
/// NB : on expose une commande dédiée pour la clarté de l'API frontend,
/// mais elle délègue à la logique commune de `import_son`.
#[tauri::command]
async fn pad_importer_son(app: AppHandle) -> Result<Option<String>, String> {
    // Délègue à import_son (même logique de validation + copie).
    import_son(app).await
}

// ===== Écran d'attente (BRB — overlay plein écran diffusion :4321) =====

/// État complet de l'écran d'attente (config + runtime) pour l'UI dashboard.
#[tauri::command]
fn ecran_attente_etat(
    state: tauri::State<'_, ecran_attente::EcranAttenteState>,
) -> ecran_attente::EcranAttenteEtat {
    state.etat_complet()
}

/// Remplace la config + persiste + re-émet la frame WS si l'écran est actif
/// (média/titre/son/volume appliqués en direct côté diffusion).
#[tauri::command]
fn ecran_attente_set_config(
    state: tauri::State<'_, ecran_attente::EcranAttenteState>,
    config: ecran_attente::EcranAttenteConfig,
) -> Result<(), String> {
    state.set_config(config)
}

/// Lance l'écran d'attente : chrono 1 Hz + frame WS + routage audio OBS
/// (connexion one-shot, non-fatal si OBS absent — creds des stores OBS).
#[tauri::command]
fn ecran_attente_lancer(
    state: tauri::State<'_, ecran_attente::EcranAttenteState>,
    host: String,
    port: u16,
    password: String,
) {
    state.lancer(&host, port, &password)
}

/// Bascule pause/reprise du chrono. No-op si l'écran est inactif.
#[tauri::command]
fn ecran_attente_pause_toggle(state: tauri::State<'_, ecran_attente::EcranAttenteState>) {
    state.toggle_pause()
}

/// Arrête l'écran d'attente + restaure l'audio bureau OBS (non-fatal).
#[tauri::command]
fn ecran_attente_arreter(
    state: tauri::State<'_, ecran_attente::EcranAttenteState>,
    host: String,
    port: u16,
    password: String,
) {
    state.arreter(&host, port, &password)
}

/// Importe un média (image OU vidéo) pour l'écran d'attente : dialog
/// « Médias » (images + vidéos) → validation + copie (ou remux MKV→MP4)
/// vers medias/. Retourne `Some((rel, kind, nom))`, `None` si annulé.
/// Même mécanique que `pad_importer_media` (helper pick_and_copy_media),
/// SAUF la limite de taille : pas de cap sur les vidéos (consigne
/// utilisateur — la vidéo BRB peut être longue ; les images gardent 10 Mo).
#[tauri::command]
async fn ecran_attente_importer_media(
    app: AppHandle,
) -> Result<Option<(String, String, String)>, String> {
    // Vidéo BRB sans cap de taille : la copie/remux peut être LONGUE →
    // impérativement sur le pool bloquant Tokio, jamais sur le thread UI.
    tauri::async_runtime::spawn_blocking(move || pick_and_copy_media(&app, true))
        .await
        .map_err(|e| format!("ecran_attente_importer_media: {}", e))?
}

/// Liste les sources audio de sortie OBS (`wasapi_output_capture`) pour le
/// picker de la modale. Connexion one-shot (creds des stores OBS).
#[tauri::command]
async fn ecran_attente_lister_sources_audio(
    host: String,
    port: u16,
    password: String,
) -> Result<Vec<String>, String> {
    ecran_attente::lister_sources_audio(&host, port, &password).await
}

// ===== Just Chatting (overlay non-destructif, état runtime non persisté) =====

/// Snapshot (actif + layouts paysage/portrait). OFF au boot.
#[tauri::command]
fn just_chatting_etat(
    state: tauri::State<'_, just_chatting::JustChattingState>,
) -> just_chatting::JustChattingEtat {
    state.etat()
}

/// Active ou coupe le mode. N'écrit pas la scène, pas de JSON.
#[tauri::command]
fn just_chatting_set_actif(
    state: tauri::State<'_, just_chatting::JustChattingState>,
    actif: bool,
) {
    state.set_actif(actif);
}

// ===== Alertes (follow/raid/sub/resub/subgift/bits) =====

/// Config complète des alertes (pour l'UI dashboard).
#[tauri::command]
fn alertes_etat(state: tauri::State<'_, alertes::AlertesState>) -> alertes::AlertesConfig {
    state.etat()
}

/// Remplace la config d'un type d'alerte + persiste + push WS diffusion.
#[tauri::command]
fn alertes_set_config(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
    config: alertes::AlerteTypeConfig,
) -> Result<(), String> {
    state.set_config_type(&type_alerte, config)
}

/// Test manuel : déclenche une alerte avec données factices (bypass cooldowns).
#[tauri::command]
fn alertes_tester(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
) -> Result<(), String> {
    state.tester(&type_alerte)
}

/// Remplace la position/taille de l'overlay alerte (même mécanique que
/// welcome_set_overlay_config) + persiste + push WS diffusion.
#[tauri::command]
fn alertes_set_overlay_config(
    state: tauri::State<'_, alertes::AlertesState>,
    x: f64,
    y: f64,
    largeur: f64,
    hauteur: f64,
) -> Result<(), String> {
    state.set_overlay_config(x, y, largeur, hauteur)
}

/// Retourne la config du squelette de position unifié (source de vérité
/// partagée par les overlays "clip de bienvenue" et "alertes").
#[tauri::command]
fn position_overlay_etat(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
) -> position_overlay::PositionOverlayConfig {
    state.etat()
}

/// Remplace la config du squelette de position unifié + persiste
/// (position_overlay.json) + push WS `position-overlay-config` vers la
/// diffusion (applique aux deux overlays en live).
#[tauri::command]
fn position_overlay_set(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
    config: position_overlay::PositionOverlayConfig,
) -> Result<(), String> {
    state.set(config)
}

/// Retourne la config du squelette DÉDIÉ du chrono 2 (indépendant du
/// squelette partagé — persistée `position_chrono.json`).
#[tauri::command]
fn position_chrono_etat(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
) -> position_overlay::PositionOverlayConfig {
    state.etat_chrono()
}

/// Remplace la config du squelette chrono 2 + persiste
/// (position_chrono.json) + push WS `chrono2-pos` vers la diffusion
/// (repositionnement live, même pendant que le chrono tourne).
#[tauri::command]
fn position_chrono_set(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
    config: position_overlay::PositionOverlayConfig,
) -> Result<(), String> {
    state.set_chrono(config)
}

/// Retourne la config du squelette DÉDIÉ du pop de validation des splits
/// (speedrun) — indépendant du squelette partagé, persistée
/// `position_splitpop.json`.
#[tauri::command]
fn position_splitpop_etat(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
) -> position_overlay::PositionOverlayConfig {
    state.etat_splitpop()
}

/// Remplace la config du squelette splitpop + persiste
/// (position_splitpop.json) + push WS `splitpop-pos` vers la diffusion
/// (repositionnement live, pop visible ou non).
#[tauri::command]
fn position_splitpop_set(
    state: tauri::State<'_, position_overlay::PositionOverlayState>,
    config: position_overlay::PositionOverlayConfig,
) -> Result<(), String> {
    state.set_splitpop(config)
}

/// Test manuel du squelette splitpop (bouton « Tester le rendu » de
/// l'onglet Position) : frame WS `splitpop-test` → la diffusion affiche un
/// pop factice au squelette dédié (même pipeline visuel qu'un vrai split).
#[tauri::command]
fn position_splitpop_test(app: AppHandle) {
    position_overlay::tester_splitpop(&app);
}

/// Test manuel du squelette chrono 2 (bouton « Tester le rendu » de
/// l'onglet Position) : démarre un chrono factice de 10 s — frames
/// `chrono2-etat` réelles, squelette dédié relu à chaque frame.
#[tauri::command]
fn position_chrono_test(
    state: tauri::State<'_, pad_numerique::PadNumeriqueState>,
) {
    state.tester_chrono();
}

/// Déclenche une alerte depuis le frontend. Utilisé par la diff des follows
/// (pas d'event IRC pour les follows) — et c'est le point d'extension prévu
/// pour les futures plateformes (Kick/YouTube/TikTok appelleront la même
/// commande avec leurs propres événements).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn alerte_declencher(
    state: tauri::State<'_, alertes::AlertesState>,
    type_alerte: String,
    pseudo: String,
    user_id: String,
    nb_viewers: i64,
    nb_bits: i64,
    nb_mois: i64,
    destinataire: String,
) -> Result<(), String> {
    if !alertes::TYPES_ALERTE.contains(&type_alerte.as_str()) {
        return Err(format!("Type d'alerte inconnu : {}", type_alerte));
    }
    state.on_event(alertes::EventTwitch {
        type_alerte,
        plateforme: String::new(),
        login: pseudo.to_lowercase(),
        pseudo,
        user_id,
        nb_viewers,
        nb_bits,
        nb_mois,
        destinataire,
        niveau_sub: String::new(),
        system_msg: String::new(),
        montant: String::new(),
        objet: String::new(),
        nb_objet: 0,
    });
    Ok(())
}

// ===== Énumération PC + création capture depuis SOS (Lot 3) =====

/// Énumère les caméras du PC (PnP/DirectShow, SANS OBS).
#[tauri::command]
fn pc_enumerate_cameras() -> Vec<String> {
    pc_enum::enumerate_cameras()
}

/// Énumère les fenêtres visibles du PC (EnumWindows, SANS OBS).
#[tauri::command]
fn pc_enumerate_windows() -> Vec<pc_enum::WindowEntry> {
    pc_enum::enumerate_windows()
}

/// Énumère les jeux/fenêtres du PC (EnumWindows, SANS OBS).
#[tauri::command]
fn pc_enumerate_games() -> Vec<pc_enum::WindowEntry> {
    pc_enum::enumerate_games()
}

/// Énumère les moniteurs du PC (EnumDisplayMonitors, SANS OBS) — type
/// « Écran » du sélecteur de capture (monitor_id au format OBS).
#[tauri::command]
fn pc_enumerate_monitors() -> Vec<pc_enum::MonitorEntry> {
    pc_enum::enumerate_monitors()
}

/// Miniature WGC d'une fenêtre (data URL PNG, fallback icône fenêtre).
/// None si fenêtre disparue et sans icône — le front affiche l'emoji.
/// À la demande : un invoke par carte de la grille du sélecteur.
#[tauri::command]
fn pc_window_thumbnail(hwnd: isize) -> Option<String> {
    pc_thumb::window_thumbnail(hwnd)
}

/// Miniature WGC d'un écran (data URL PNG). None → emoji côté front.
#[tauri::command]
fn pc_monitor_thumbnail(hmon: isize) -> Option<String> {
    pc_thumb::monitor_thumbnail(hmon)
}

/// Crée une source OBS de capture depuis une cible PC (caméra/fenêtre/jeu)
/// dans la scène contenant SOS-Diffusion. Si SOS-Trou-<id> existe déjà →
/// SetInputSettings + transform (pas de doublon). Sinon → CreateInput.
/// Puis SetSceneItemIndex sous SOS + transform = widget.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn obs_create_trou_from_pc(
    host: String,
    port: u16,
    password: String,
    source_name: String,
    kind: obs_trou::CaptureKind,
    target: Option<String>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<String, String> {
    obs_trou::create_trou_from_pc(
        &host, port, &password, &source_name, kind, target, x, y, w, h,
    )
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        // Sidecar ffmpeg : remux MKV→MP4 à l'import média (lecture navigateur).
        .plugin(tauri_plugin_shell::init())
        // Pop-out chat : custom protocol servant le HTML embarqué (include_str!).
        // Toute URL streamos-chat://localhost/* → CHAT_POPOUT_HTML. Vraie origine
        // (http://streamos-chat.localhost sur Windows) → invoke + WS fiables.
        .register_uri_scheme_protocol("streamos-chat", |_ctx, _request| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(CHAT_POPOUT_HTML.as_bytes().to_vec())
                .unwrap()
        })
        // Pop-out speedrun : custom protocol servant le HTML embarqué
        // (include_str!) + la police par défaut (Rajdhani, same-origin —
        // pas de blocage CORS ; routage par path comme splash::repondre).
        // Toute URL streamos-speedrun://localhost/* : /splitter → HTML,
        // /fonts/Rajdhani.woff2 → woff2 embarqué.
        .register_uri_scheme_protocol("streamos-speedrun", |_ctx, request| {
            let path = request.uri().path();
            if path.starts_with("/fonts/") && path.ends_with("Rajdhani.woff2") {
                return tauri::http::Response::builder()
                    .header("Content-Type", "font/woff2")
                    .body(SPEEDRUN_POPOUT_RAJDHANI.to_vec())
                    .unwrap();
            }
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(SPEEDRUN_POPOUT_HTML.as_bytes().to_vec())
                .unwrap()
        })
        // Barre de raccourcis : custom protocol servant le HTML embarqué.
        // Toute URL streamos-raccourcis://localhost/* → RACCOURCIS_HTML
        // (?bord= dans la query est lu par le HTML, pas par le handler).
        .register_uri_scheme_protocol("streamos-raccourcis", |_ctx, _request| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(raccourcis::RACCOURCIS_HTML.as_bytes().to_vec())
                .unwrap()
        })
        // Projecteur natif : custom protocol servant le HTML embarqué.
        // Toute URL streamos-apercu://localhost/* → APERCU_HTML (même
        // recette que le pop-out chat — vraie origine, invoke fiable).
        .register_uri_scheme_protocol("streamos-apercu", |_ctx, _request| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(APERCU_HTML.as_bytes().to_vec())
                .unwrap()
        })
        // Splash de démarrage : custom protocol servant le HTML embarqué
        // (/) + le wallpaper Windows (/wallpaper) + la mascotte (/mascotte).
        // Routage par path dans splash::repondre.
        .register_uri_scheme_protocol("streamos-splash", |ctx, request| {
            splash::repondre(ctx.app_handle(), request.uri().path())
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // Splash de démarrage EN PREMIER : fenêtre topmost façon Discord
            // (wallpaper Windows + mascotte + « Bienvenue <pseudo> ») visible
            // pendant que tout le reste s'initialise. Non-fatal : sans elle,
            // armer_affichage montre quand même main au bout du compte.
            splash::demarrer(&handle);

            // Boot scènes : migration + chargement initial (UNE scène en RAM).
            let (scene, current_id) = scenes::boot_scenes(&handle).unwrap_or_else(|e| {
                log::error!("Erreur boot_scenes: {}", e);
                (scene::Scene::new(), "defaut".to_string())
            });

            let scene = Arc::new(std::sync::Mutex::new(scene));
            let current_id = Arc::new(std::sync::Mutex::new(current_id));

            // Canal broadcast pour les snapshots WS
            let (snapshot_tx, _snapshot_rx) = broadcast::channel::<String>(64);
            // Canal broadcast pour les messages chat (IRC → diffusion :4321)
            let (chat_tx, _chat_rx) = broadcast::channel::<String>(256);

            let state = ScenesState {
                scene: scene.clone(),
                current_id: current_id.clone(),
                // Override « !cadres » : vide au boot (jamais persisté).
                cadres_chat: Arc::new(std::sync::Mutex::new(None)),
                snapshot_tx: snapshot_tx.clone(),
                chat_tx: chat_tx.clone(),
                eq: equaliseur::EqualiseurState::new(chat_tx.clone()),
            };
            app.manage(state.clone());

            // Registre moteur — mode actif persisté (obs | natif). Les deux
            // moteurs sont branchés : OBS (adaptateur obs-websocket) et le
            // moteur natif (capture/composition/encodage sans OBS).
            let mode_moteur = config::lire_mode_moteur(app.handle())
                .ok()
                .flatten()
                .and_then(|m| moteur::mode::Mode::depuis_nom(&m))
                .unwrap_or(moteur::mode::Mode::Obs);
            app.manage(moteur::RegistreMoteur::nouveau(mode_moteur));
            // Pont chat → moteur natif : le crate ne connaît pas tokio, l'app
            // écoute `chat_tx` et pousse dans le buffer borné du moteur.
            moteur::demarrer_ecoute_chat(app.handle().clone());
            // Veille aperçu : une mutation de scène (snapshot_tx) arrête la
            // session aperçu — le poll du projecteur la relance sur la
            // scène fraîche.
            moteur::demarrer_veille_apercu(app.handle().clone());

            // État Équaliseur — sync initiale au boot : si la scène chargée a
            // le module déjà actif (persisté), la capture démarre dès
            // maintenant (push_snapshot ne rejoue que sur mutation de scène).
            {
                let s = state.scene.lock().unwrap();
                state
                    .eq
                    .sync(s.modules.as_ref().and_then(|m| m.equaliseur.as_ref()));
            }

            // État Squelette de position unifié (source de vérité unique pour
            // la position/taille des overlays "clip de bienvenue" et "alertes").
            // Doit être enregistré AVANT welcome_state et alertes_state car ils
            // y accèdent via app.state::<PositionOverlayState>() au boot.
            let position_state =
                position_overlay::PositionOverlayState::new(handle.clone(), chat_tx.clone());
            app.manage(position_state.clone());

            // État Welcome (clips de bienvenue : registre + seen + queue).
            // Récupère chat_tx depuis ScenesState pour émettre vers :4321.
            let welcome_state = welcome::WelcomeState::new(handle.clone(), chat_tx.clone());
            app.manage(welcome_state.clone());

            // État Présentation streamer (favoris persistés + présentation
            // courante). Frames WS presentation-* sur chat_tx → diffusion
            // :4321. Déclenchement manuel uniquement (onglet Interactions).
            let presentation_state =
                presentation::PresentationState::new(handle.clone(), chat_tx.clone());
            app.manage(presentation_state.clone());

            // État Bandeau premier message (détection 1er message tous viewers).
            // Récupère chat_tx pour émettre vers diffusion :4321.
            let bandeau_state = bandeau::BandeauState::new(handle.clone(), chat_tx.clone());
            app.manage(bandeau_state.clone());

            // État Commandes chat ("!commande" tapée par un viewer → overlay
            // diffusion, même moteur que les alertes : file + cooldowns + timer).
            let commandes_state = commandes::CommandesState::new(handle.clone(), chat_tx.clone());
            app.manage(commandes_state.clone());

            // État Musique ("!rs <titre>" → file de lecture côté diffusion,
            // bibliothèque locale medias/musique/). Hooks on_message dans les
            // 4 modules chat + remontée "musique-ended" via server.rs.
            let musique_state = musique::MusiqueState::new(handle.clone(), chat_tx.clone());
            app.manage(musique_state.clone());

            // État Emoji Arena (combats d'emojis "!fight" pilotés par le
            // chat — tour par tour, vs viewer ou BOT). Hooks on_message
            // dans les 4 modules chat à côté du hook musique ; snapshot
            // complet fight-state → diffusion (resync via fight-resync).
            let fight_state = fight::FightState::new(handle.clone(), chat_tx.clone());
            app.manage(fight_state.clone());

            // État Cadres chat ("!cadres <couleur>" → couleurFin du cadre
            // unifié). Hooks on_message dans les 4 modules chat ; la mutation
            // passe par le dashboard (event "cadres:couleur") — écrivain
            // unique de la scène via update_scene.
            let cadres_chat_state = cadres_cmd::CadresChatState::new(handle.clone());
            app.manage(cadres_chat_state.clone());

            // État Input Viewer (capture globale clavier + souris → overlay
            // diffusion). Inactif au boot — démarré par le bouton ON/OFF des
            // options du widget. Émission WS directe (input-viewer-etat).
            let input_viewer_state =
                input_viewer::InputViewerState::new(chat_tx.clone());
            app.manage(input_viewer_state.clone());

            // État Pad numérique (16 touches Numpad × 3 plages → overlay
            // diffusion). Capture clavier globale dédiée (GetAsyncKeyState).
            // Démarré au boot si la config était active au dernier arrêt.
            let pad_state =
                pad_numerique::PadNumeriqueState::new(handle.clone(), chat_tx.clone());
            app.manage(pad_state.clone());

            // État Écran d'attente (BRB — overlay plein écran diffusion).
            // Config persistée ecran_attente.json + chrono tokio 1 Hz +
            // frames WS `ecran-attente-etat` sur chat_tx.
            let ecran_attente_state =
                ecran_attente::EcranAttenteState::new(handle.clone(), chat_tx.clone());
            app.manage(ecran_attente_state.clone());

            // Just Chatting — état runtime pur (OFF au boot, non persisté).
            // Frames WS `just-chatting-etat` sur chat_tx. Ne touche pas la scène.
            let just_chatting_state =
                just_chatting::JustChattingState::new(handle.clone(), chat_tx.clone());
            app.manage(just_chatting_state.clone());

            // Speedrun Splitter — initialise le canal WS pour la diffusion.
            // Le moteur émet l'état du timer via chat_tx (speedrun-etat) vers
            // diffusion.html (maj DOM directe, pattern input-viewer).
            speedrun::commands::set_chat_tx(chat_tx.clone());

            // État Alertes (follow/raid/sub/resub/subgift/bits → overlay diffusion).
            // Récupère chat_tx pour émettre vers diffusion :4321. Les événements
            // arrivent de twitch_chat.rs (USERNOTICE + bits) et de la commande
            // alerte_declencher (diff follows frontend + futures plateformes).
            let alertes_state = alertes::AlertesState::new(handle.clone(), chat_tx.clone());
            app.manage(alertes_state.clone());

            // État Twitch (handle IRC + cancel + drapeau connecté).
            let twitch_state = TwitchState::new();
            app.manage(twitch_state.clone());

            // État Kick (drapeau connecté + slug canal). Le WS est côté frontend (LOT 2).
            let kick_state = KickState::new();
            app.manage(kick_state.clone());

            // État YouTube (handle chat polling + cancel + drapeau connecté + login).
            let youtube_state = YoutubeState::new();
            app.manage(youtube_state.clone());

            // État TikTok (handle chat + cancel + drapeau connecté + username).
            let tiktok_state = TiktokState::new();
            app.manage(tiktok_state.clone());

            // Télémétrie multiplateforme — collecte passive (listeners events
            // + tap chat_tx). Enregistrée AVANT viewers::demarrer : la frame
            // stream-info n'est émise qu'au changement de contexte — si la
            // télémétrie n'est pas encore abonnée à chat_tx, la 1re frame est
            // perdue et tout le live reste sans contexte jeu/titre. Et avant
            // les auto-resume pour capter leurs connexions. Non-fatal : l'app
            // continue sans si l'init du fichier échoue.
            match telemetrie::TelemetrieState::new(&handle) {
                Ok(telemetrie_state) => {
                    telemetrie_state.demarrer(&handle, chat_tx.clone());
                    app.manage(telemetrie_state);
                }
                Err(e) => eprintln!("[Telemetrie] ERR init: {}", e),
            }

            // Compteur viewers — UNE tâche pour toutes les plateformes
            // (tick 15s, fetch 60s live / 180s off, dedup, push via chat_tx).
            viewers::demarrer(handle.clone(), chat_tx.clone());

            // Diagnostic consommation (1Hz) — CPU par sous-système (threads
            // nommés : splitter, input viewer…), par processus (WebView2,
            // OBS, jeu), compteurs d'activité. Tap passif sur chat_tx +
            // échantillonneur dédié — non énergivore, historique en mémoire.
            // + boîte noire : journal persistant 5 s (survit à un gel).
            let boite_noire = match conso::chemin_boite_noire(&handle) {
                Ok(p) => p,
                Err(e) => {
                    // Dégradé : chemin vide → la boucle mesure mais n'écrit
                    // rien. Ne bloque JAMAIS le boot.
                    eprintln!("[Conso] ERR chemin boîte noire: {}", e);
                    std::path::PathBuf::new()
                }
            };
            conso::demarrer(chat_tx.clone(), boite_noire);

            // Fenêtre principale : applique écran + mode persistés
            // (fenetre.json — section Ergonomie) SANS la montrer
            // (visible:false dans tauri.conf.json — anti-flash). Enregistre
            // le hook de sauvegarde de la géométrie fenêtrée à la fermeture.
            fenetre::appliquer_au_boot(&handle);

            // Affichage différé de main : après ≥4 s de splash ET l'event
            // "frontend:pret" émis par App.svelte en fin d'onMount → show()
            // main + close splash. Garde-fou timeout si le frontend est mort.
            splash::armer_affichage(&handle);

            // Auto-resume : si token valide en coffre → IRC + point vert.
            // Pas de modal. Si token absent/invalide → reste déconnecté.
            let resume_app = handle.clone();
            let resume_state = twitch_state.clone();
            tauri::async_runtime::spawn(async move {
                match twitch_auth::lire_tokens() {
                    Ok(Some(tokens)) => {
                        if let Err(e) =
                            connect_with_tokens(&resume_app, &resume_state, tokens).await
                        {
                            eprintln!("[Twitch] ERR auto-resume échoué: {}", e);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[Twitch] ERR lecture coffre: {}", e);
                    }
                }
            });

            // Auto-resume Kick : si slug sauvegardé → reconnecter automatiquement.
            // Non-fatal si échec (slug invalide, réseau) — l'utilisateur reconnectera.
            let kick_app = handle.clone();
            let kick_state_clone = kick_state.clone();
            tauri::async_runtime::spawn(async move {
                match crate::config::lire_kick_slug(&kick_app) {
                    Ok(Some(slug)) => {
                        match kick::resolve_channel(&slug).await {
                            Ok((channel_id, chatroom_id)) => {
                                kick_state_clone.reset_cancel();
                                let cancel = kick_state_clone.cancel_clone();
                                let handle =
                                    kick::run_ws(kick_app.clone(), channel_id, chatroom_id, cancel);
                                {
                                    let mut h = kick_state_clone.ws_handle.lock().unwrap();
                                    *h = Some(handle);
                                }
                                kick_state_clone.set_slug(Some(slug));
                                // set_connected(true) se fait dans run_ws quand subscribed.
                            }
                            Err(e) => {
                                eprintln!("[Kick] ERR auto-resume resolve: {}", e);
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[Kick] ERR lecture slug: {}", e);
                    }
                }
            });

            // Auto-resume YouTube : si token valide en coffre → valider + chat polling.
            // Non-fatal si échec (token expiré, réseau) — l'utilisateur reconnectera.
            let yt_app = handle.clone();
            let yt_state = youtube_state.clone();
            tauri::async_runtime::spawn(async move {
                match youtube_auth::lire_tokens() {
                    Ok(Some(tokens)) => {
                        if let Err(e) =
                            connect_with_tokens_youtube(&yt_app, &yt_state, tokens).await
                        {
                            eprintln!("[YouTube] ERR auto-resume échoué: {}", e);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[YouTube] ERR lecture coffre: {}", e);
                    }
                }
            });

            // Auto-resume TikTok : si username sauvegardé → reconnecter le chat.
            // Non-fatal si échec (streamer hors-ligne, réseau) — l'utilisateur reconnectera.
            let tt_app = handle.clone();
            let tt_state = tiktok_state.clone();
            tauri::async_runtime::spawn(async move {
                match crate::config::lire_tiktok_username(&tt_app) {
                    Ok(Some(username)) => {
                        let chat_tx = tt_app
                            .try_state::<ScenesState>()
                            .map(|s| s.chat_tx.clone());
                        if let Some(chat_tx) = chat_tx {
                            tt_state.stop_chat();
                            tt_state.reset_cancel();
                            let cancel = tt_state.cancel_clone();
                            let handle = tiktok_chat::demarrer(
                                tt_app.clone(),
                                chat_tx,
                                username.clone(),
                                cancel,
                            );
                            {
                                let mut h = tt_state.chat_handle.lock().unwrap();
                                *h = Some(handle);
                            }
                            tt_state.set_username(Some(username));
                        } else {
                            eprintln!("[TikTok] ERR auto-resume: ScenesState non trouvé");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[TikTok] ERR lecture username: {}", e);
                    }
                }
            });

            // Restore barre de raccourcis : si visible:true persisté → recrée la
            // fenêtre dockée au bord/moniteur sauvegardé. Non-fatal (moniteur
            // débranché → fallback primaire dans creer_ou_appliquer).
            let racc_app = handle.clone();
            tauri::async_runtime::spawn(async move {
                match crate::config::lire_raccourcis_config(&racc_app) {
                    Ok(Some(cfg)) if cfg.visible => {
                        if let Err(e) = raccourcis::creer_ou_appliquer(
                            &racc_app,
                            &cfg.bord,
                            cfg.monitor_index,
                        )
                        .await
                        {
                            eprintln!("[Raccourcis] ERR restore: {}", e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("[Raccourcis] ERR lecture config: {}", e);
                    }
                }
            });

            // Hot-plug caméras : tant que la scène a un widget caméra,
            // ré-énumère les devices PnP toutes les 3 s et émet
            // "camera:devices" (Vec<String>) au dashboard quand la liste
            // change — un Android branché en mode webcam USB (UVC) ou une
            // caméra virtuelle apparaît sans rouvrir la carte d'édition.
            // Zéro coût sans widget caméra (le spawn PowerShell est sauté).
            let cam_app = handle.clone();
            let cam_scene = scene.clone();
            tauri::async_runtime::spawn(async move {
                let mut precedente: Vec<String> = Vec::new();
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    let a_camera = {
                        let s = cam_scene.lock().unwrap();
                        s.widgets.iter().any(|w| w.widget_type == "camera")
                    };
                    if !a_camera {
                        continue;
                    }
                    let devices =
                        tauri::async_runtime::spawn_blocking(pc_enum::enumerate_cameras)
                            .await
                            .unwrap_or_default();
                    if devices != precedente {
                        precedente = devices.clone();
                        let _ = cam_app.emit("camera:devices", devices);
                    }
                }
            });

            // Démarre le serveur :4321 en arrière-plan.
            // run_server émet server_ready (bind OK) ou server_error (port pris)
            // directement — pas de probe externe, pas de fallback port.
            let server_handle = handle.clone();
            let server_state = state.clone();
            tauri::async_runtime::spawn(async move {
                match server::run_server(server_handle, server_state).await {
                    Ok(()) => {
                        log::info!("Serveur :4321 arrêté normalement");
                    }
                    Err(e) => {
                        log::error!("Erreur serveur :4321: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            moteur::moteur_etat,
            moteur::moteur_diag,
            moteur::moteur_basculer,
            moteur::moteur_obs_connexion,
            moteur::moteur_sortie_demarrer,
            moteur::moteur_sortie_arreter,
            moteur::moteur_sortie_etat,
            moteur::moteur_chat_message,
            moteur::moteur_apercu,
            moteur::apercu_fenetre_toggle,
            get_scene,
            update_scene,
            import_media,
            import_media_from_paths,
            import_medias_multi,
            import_fond,
            import_son,
            import_alerte_media,
            obs_connect,
            obs_refresh_diffusion,
            obs_set_stream,
            obs_set_record,
            obs_outputs_status,
            obs_projecteur_moniteurs,
            obs_projecteur_preview,
            obs_projecteur_fermer,
            chat_popout_toggle,
            chat_popout_fermer,
            speedrun_popout_toggle,
            speedrun_popout_fermer,
            raccourcis_moniteurs,
            raccourcis_etat,
            raccourcis_appliquer,
            raccourcis_masquer,
            fenetre_etat,
            fenetre_appliquer,
            app_arreter,
            app_redemarrer,
            scene_sync_captures,
            scenes_lister,
            scenes_resumer,
            scene_creer,
            scene_ouvrir,
            scene_renommer,
            scene_deplacer,
            scene_masquer_nom,
            scene_supprimer,
            scene_dupliquer,
            scene_courante,
            presets_lister,
            scene_creer_depuis_preset,
            scene_initialiser_preset,
            onboarding_etat,
            onboarding_marquer_fait,
            langue_etat,
            langue_sauver,
            scene_exporter,
            scene_importer,
            scenes_exporter_zip,
            scenes_importer_zip,
            scenes_importer_zip_appliquer,
            obs_enumerate_targets,
            obs_create_trou_source,
            obs_sync_trous,
            obs_delete_trou_source,
            obs_enumerate_inputs_by_kind,
            obs_link_existing_source,
            camera_sync,
            camera_hide,
            camera_mask_sync,
            camera_fond_vert,
            pc_enumerate_cameras,
            pc_enumerate_windows,
            pc_enumerate_games,
            pc_enumerate_monitors,
            pc_window_thumbnail,
            pc_monitor_thumbnail,
            obs_create_trou_from_pc,
            twitch_connecter,
            twitch_annuler_device_flow,
            twitch_deconnecter,
            twitch_etat,
            twitch_login_courant,
            twitch_reconnecter,
            twitch_communaute_followers,
            twitch_communaute_subs,
            twitch_communaute_viewers,
            twitch_communaute_unfollows,
            twitch_broadcaster,
            twitch_lister_vips,
            twitch_lister_moderateurs,
            twitch_lister_bannis,
            twitch_resoudre_user,
            twitch_bannir,
            twitch_debannir,
            twitch_ajouter_vip,
            twitch_retirer_vip,
            twitch_ajouter_moderateur,
            twitch_retirer_moderateur,
            twitch_supprimer_message,
            twitch_envoyer_commande_chat,
            twitch_live_channel_info,
            twitch_live_maj_channel,
            twitch_live_search_categories,
            twitch_live_game_info,
            twitch_live_search_channels,
            twitch_live_marker,
            twitch_live_pub,
            twitch_live_raid_lancer,
            twitch_live_raid_annuler,
            twitch_live_chat_settings,
            twitch_live_chat_settings_maj,
            twitch_live_annonce,
            twitch_live_shield_status,
            twitch_live_shield_set,
            twitch_live_unban_requests,
            twitch_live_unban_resoudre,
            twitch_live_automod_resoudre,
            twitch_live_poser_jacquette,
            twitch_planning_schedule,
            twitch_planning_segment_creer,
            twitch_planning_segment_maj,
            twitch_planning_segment_supprimer,
            twitch_planning_vacances,
            twitch_goals_lister,
            kick_connecter,
            kick_deconnecter,
            kick_etat,
            kick_slug_courant,
            youtube_connecter,
            youtube_deconnecter,
            youtube_etat,
            youtube_login_courant,
            youtube_annuler_device_flow,
            youtube_demarrer_chat,
            youtube_arreter_chat,
            youtube_communaute_channel,
            youtube_communaute_members,
            youtube_communaute_viewers,
            tiktok_connecter,
            tiktok_deconnecter,
            tiktok_etat,
            tiktok_username_courant,
            welcome_etat,
            welcome_registre_twitch,
            welcome_sauver_viewer_twitch,
            welcome_supprimer_viewer_twitch,
            welcome_config_globale_actif,
            welcome_set_overlay_config,
            welcome_tester_clip,
            welcome_set_duree_affichage,
            welcome_set_utiliser_squelette,
            welcome_stop,
            welcome_skip,
            welcome_retirer,
            welcome_remonter,
            welcome_descendre,
            welcome_vider,
            welcome_reset_session,
            welcome_lister_clips_streamer,
            welcome_resoudre_mp4,
            welcome_attribuer_auto,
            presentation_etat,
            presentation_favori_sauver,
            presentation_favori_supprimer,
            presentation_favori_set_son,
            presentation_favori_set_mode,
            presentation_rechercher,
            presentation_lancer,
            presentation_fermer,
            presentation_clip_jouer,
            presentation_clip_stopper,
            presentation_audio_jouer,
            presentation_audio_stopper,
            bandeau_etat,
            bandeau_set_config,
            bandeau_stop,
            bandeau_reset_session,
            bandeau_tester,
            commandes_etat,
            commande_set_config,
            commande_supprimer,
            commande_tester,
            commande_exporter,
            commande_importer,
            commande_importer_ecraser,
            musique_etat,
            musique_set_config,
            musique_importer,
            musique_supprimer_fichier,
            musique_skip,
            musique_pause,
            musique_stop,
            musique_vider,
            musique_retirer,
            musique_deplacer,
            musique_file_ajouter,
            musique_tester,
            musique_volume,
            fight_etat,
            fight_set_config,
            fight_tester,
            fight_stop,
            fight_previsualiser_aide,
            cadres_chat_etat,
            cadres_chat_set_config,
            cadres_chat_palette,
            input_viewer_set_actif,
            input_viewer_etat_actif,
            pad_etat,
            pad_set_actif,
            pad_set_touche,
            pad_supprimer_touche,
            pad_set_plage,
            pad_tester_touche,
            pad_importer_media,
            pad_importer_son,
            ecran_attente_etat,
            ecran_attente_set_config,
            ecran_attente_lancer,
            ecran_attente_pause_toggle,
            ecran_attente_arreter,
            ecran_attente_importer_media,
            ecran_attente_lister_sources_audio,
            just_chatting_etat,
            just_chatting_set_actif,
            alertes_etat,
            alertes_set_config,
            alertes_set_overlay_config,
            alertes_tester,
            alerte_declencher,
            position_overlay_etat,
            position_overlay_set,
            position_chrono_etat,
            position_chrono_set,
            position_splitpop_etat,
            position_splitpop_set,
            position_splitpop_test,
            position_chrono_test,
            speedrun_charger_asl,
            speedrun_charger_lss,
            speedrun_demarrer,
            speedrun_arreter,
            speedrun_maj_settings,
            speedrun_est_actif,
            speedrun_action_manuelle,
            speedrun_lire_config,
            speedrun_sauver_config,
            speedrun_maj_settings_asl,
            speedrun_aller_split,
            speedrun_exporter_splits,
            telemetrie_etat,
            telemetrie_derniers,
            audience_sessions,
            audience_session,
            conso_etat,
            boite_noire_etat
        ])
        .build(tauri::generate_context!())
        .expect("erreur lors du lancement de StreamOS v0")
        .run(|app, event| {
            // Teardown du moteur natif : joint le thread moteur et libère
            // NVENC. Sans cela, fermer l'app en cours d'enregistrement
            // laisserait le thread et l'encodeur vivants, MP4 non finalisé.
            if let tauri::RunEvent::Exit = event {
                if let Some(registre) = app.try_state::<moteur::RegistreMoteur>() {
                    registre.arreter_natif();
                }
            }
        });
}
