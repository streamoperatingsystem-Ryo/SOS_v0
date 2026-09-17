use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::api_deck;
use crate::scenes::ScenesState;

/// HTML embarqué (vanilla JS, zéro Tauri). Sert de page de diffusion pour OBS.
const DIFFUSION_HTML: &str = include_str!("../resources/diffusion.html");

/// Polices Google Font embarquées (woff2, subset latin) pour les titres de
/// widgets. Servies sur /fonts/{name} — chargées par le dashboard ET la
/// diffusion (même origine :4321).
fn font_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "Anton" => Some(include_bytes!("../resources/fonts/Anton.woff2")),
        "Audiowide" => Some(include_bytes!("../resources/fonts/Audiowide.woff2")),
        "BebasNeue" => Some(include_bytes!("../resources/fonts/BebasNeue.woff2")),
        "BlackOpsOne" => Some(include_bytes!("../resources/fonts/BlackOpsOne.woff2")),
        "Bungee" => Some(include_bytes!("../resources/fonts/Bungee.woff2")),
        "Caveat" => Some(include_bytes!("../resources/fonts/Caveat.woff2")),
        "Creepster" => Some(include_bytes!("../resources/fonts/Creepster.woff2")),
        "Fredoka" => Some(include_bytes!("../resources/fonts/Fredoka.woff2")),
        "Lobster" => Some(include_bytes!("../resources/fonts/Lobster.woff2")),
        "Montserrat" => Some(include_bytes!("../resources/fonts/Montserrat.woff2")),
        "Orbitron" => Some(include_bytes!("../resources/fonts/Orbitron.woff2")),
        "Oswald" => Some(include_bytes!("../resources/fonts/Oswald.woff2")),
        "Pacifico" => Some(include_bytes!("../resources/fonts/Pacifico.woff2")),
        "PermanentMarker" => Some(include_bytes!("../resources/fonts/PermanentMarker.woff2")),
        "PressStart2P" => Some(include_bytes!("../resources/fonts/PressStart2P.woff2")),
        "Rajdhani" => Some(include_bytes!("../resources/fonts/Rajdhani.woff2")),
        "Righteous" => Some(include_bytes!("../resources/fonts/Righteous.woff2")),
        "RussoOne" => Some(include_bytes!("../resources/fonts/RussoOne.woff2")),
        "Satisfy" => Some(include_bytes!("../resources/fonts/Satisfy.woff2")),
        "Teko" => Some(include_bytes!("../resources/fonts/Teko.woff2")),
        _ => None,
    }
}

/// Handler GET /fonts/{name} — sert une police woff2 embarquée.
/// Le paramètre {name} capture le segment complet (ex: "BebasNeue.woff2"),
/// on strip l'extension .woff2 avant de chercher dans font_bytes.
async fn font_handler(Path(name): Path<String>) -> Response {
    let key = name
        .strip_suffix(".woff2")
        .map(|s| s.to_string())
        .unwrap_or(name);
    match font_bytes(&key) {
        Some(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "font/woff2"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")],
            bytes,
        ).into_response(),
        None => (StatusCode::NOT_FOUND, "Police introuvable").into_response(),
    }
}

/// État du serveur :4321. Contient l'AppHandle + ScenesState (scène + id + canal).
/// Les handlers /api/* et WS extraient st.scenes pour appeler scenes::*.
pub struct ServerState {
    pub app: AppHandle,
    pub scenes: ScenesState,
}

/// Démarre le serveur HTTP+WS sur 127.0.0.1:4321.
/// En cas d'échec du bind (port occupé), émet `server_error` et retourne l'erreur.
/// Ne JAMAIS fallback sur un autre port — OBS pointe sur :4321.
pub async fn run_server(
    app: AppHandle,
    state: ScenesState,
) -> Result<(), String> {
    let addr: SocketAddr = "127.0.0.1:4321"
        .parse()
        .map_err(|e| format!("Adresse invalide: {}", e))?;

    let server_state = Arc::new(ServerState {
        app: app.clone(),
        scenes: state,
    });

    // Dossier medias servi sur /medias pour diffusion.html (chemin relatif).
    let medias_dir = crate::config::data_dir(&app)?.join("medias");

    let app_router = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .route("/fonts/{name}", get(font_handler))
        .nest_service("/medias", ServeDir::new(medias_dir))
        .merge(api_deck::routes())
        // CORS permissif : le dashboard (localhost:1420 / tauri.localhost) charge
        // les médias de :4321 en cross-origin — requis pour texImage2D WebGL
        // (morphing) sans tainted canvas. Localhost only, pas de risque.
        .layer(CorsLayer::permissive())
        .with_state(server_state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            let msg = format!("Port :4321 indisponible — {}", e);
            let _ = app.emit("server_error", &msg);
            msg
        })?;

    // Bind réussi → notifier le dashboard
    let _ = app.emit("server_ready", "127.0.0.1:4321");
    log::info!("Serveur diffusion sur http://{}", addr);

    axum::serve(listener, app_router)
        .await
        .map_err(|e| format!("Erreur serveur: {}", e))?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(DIFFUSION_HTML)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: Arc<ServerState>) {
    let mut rx = state.scenes.snapshot_tx.subscribe();
    let mut chat_rx = state.scenes.chat_tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Snapshot initial : envoyer l'état courant dès la connexion.
    // `sceneId` inclus (même contrat que scenes::push_snapshot) — la
    // diffusion l'initialise SANS déclencher le fondu au noir.
    {
        let snapshot = {
            // Ordre des locks : scene PUIS current_id (convention scenes.rs).
            let scene = state.scenes.scene.lock().unwrap();
            let id = state.scenes.current_id.lock().unwrap();
            serde_json::json!({
                "type": "snapshot",
                "sceneId": &*id,
                "scene": &*scene
            })
            .to_string()
        };
        let _ = sender.send(Message::Text(snapshot.into())).await;
    }

    // Tâche : pousser les snapshots ET les messages chat vers le client.
    // Les deux canaux sont fusionnés via select. Gère Lagged (broadcast
    // channel saturé → messages perdus) sans déconnecter le WS : on log et
    // on continue. Seul Closed (émetteur droppé) déconnecte.
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                snap_result = rx.recv() => {
                    match snap_result {
                        Ok(snapshot) => {
                            if sender.send(Message::Text(snapshot.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("[Serveur] WS snapshot lagged ({} perdus) — continue", n);
                        }
                        Err(_) => break,
                    }
                }
                chat_result = chat_rx.recv() => {
                    match chat_result {
                        Ok(chat) => {
                            if sender.send(Message::Text(chat.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            eprintln!("[Serveur] WS chat lagged ({} perdus) — continue", n);
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    // Tâche : lire les messages entrants. La diffusion peut demander un
    // resync de l'état speedrun (LSS) à la (re)connexion — on re-émet la
    // dernière run LSS chargée vers ce client via le canal chat_tx.
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(txt) = msg {
                // Resync speedrun : la diffusion demande l'état LSS courant.
                // Non-fatal si aucune run chargée (reemit_lss est un no-op).
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&txt) {
                    if parsed.get("type").and_then(|v| v.as_str()) == Some("speedrun-resync") {
                        crate::speedrun::commands::reemit_lss();
                    }
                }
            }
        }
    });

    // Si l'une des tâches se termine, on coupe l'autre
    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}
