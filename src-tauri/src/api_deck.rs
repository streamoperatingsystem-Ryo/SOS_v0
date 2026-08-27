/// API HTTP Stream Deck (v0.14).
///
/// Réutilise le serveur :4321 existant — pas de 2e port, pas de process Elgato.
/// Localhost only (bind 127.0.0.1:4321). Pas d'auth en v0.14.
///
/// Endpoints :
///   GET  /api/scenes           → [{ id, nom }]
///   POST /api/scene/ouvrir     { id }   → 200 / 404
///   POST /api/scene/nouvelle   { nom }  → 200 + { id, nom } / 500
///   GET  /api/scene/courante   → { id, nom }
///
/// Mêmes fonctions que l'UI (scenes::ouvrir = save + load + snapshot).
/// Structuré pour étendre plus tard (widgets, fond, lecture) sans gonfler server.rs.
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::scenes::{self, SceneIndex};
use crate::server::ServerState;

/// Construit le routeur des endpoints /api/* (à merger dans le router principal).
pub fn routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/api/scenes", get(list_scenes))
        .route("/api/scene/ouvrir", post(ouvrir_scene))
        .route("/api/scene/nouvelle", post(nouvelle_scene))
        .route("/api/scene/courante", get(scene_courante))
}

// ===== Handlers =====

async fn list_scenes(State(st): State<Arc<ServerState>>) -> Result<Json<Vec<SceneIndex>>, (StatusCode, String)> {
    let idx = scenes::lister(&st.app).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(idx))
}

#[derive(Deserialize)]
struct OuvrirBody { id: String }

async fn ouvrir_scene(
    State(st): State<Arc<ServerState>>,
    Json(body): Json<OuvrirBody>,
) -> Result<Json<SceneIndex>, (StatusCode, String)> {
    let entry = scenes::ouvrir(&st.app, &st.scenes, &body.id)
        .map_err(|e| {
            // Erreur « introuvable » → 404, autre → 500
            if e.contains("introuvable") {
                (StatusCode::NOT_FOUND, e)
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e)
            }
        })?;
    Ok(Json(entry))
}

#[derive(Deserialize)]
struct NouvelleBody { nom: String }

#[derive(Serialize)]
struct NouvelleResp { id: String, nom: String }

async fn nouvelle_scene(
    State(st): State<Arc<ServerState>>,
    Json(body): Json<NouvelleBody>,
) -> Result<Json<NouvelleResp>, (StatusCode, String)> {
    let entry = scenes::creer(&st.app, &st.scenes, &body.nom)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(NouvelleResp { id: entry.id, nom: entry.nom }))
}

async fn scene_courante(State(st): State<Arc<ServerState>>) -> Result<Json<SceneIndex>, (StatusCode, String)> {
    let entry = scenes::courante(&st.app, &st.scenes)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(entry))
}
