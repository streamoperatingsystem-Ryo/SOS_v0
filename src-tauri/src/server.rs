use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::Html,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tower_http::services::ServeDir;

use crate::api_deck;
use crate::scenes::ScenesState;

/// HTML embarqué (vanilla JS, zéro Tauri). Sert de page de diffusion pour OBS.
const DIFFUSION_HTML: &str = include_str!("../resources/diffusion.html");

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
        .nest_service("/medias", ServeDir::new(medias_dir))
        .merge(api_deck::routes())
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

    // Snapshot initial : envoyer l'état courant dès la connexion
    {
        let snapshot = {
            let scene = state.scenes.scene.lock().unwrap();
            serde_json::json!({
                "type": "snapshot",
                "scene": &*scene
            })
            .to_string()
        };
        let _ = sender.send(Message::Text(snapshot.into())).await;
    }

    // Tâche : pousser les snapshots ET les messages chat vers le client.
    // Les deux canaux sont fusionnés via select.
    let mut send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(snapshot) = rx.recv() => {
                    if sender.send(Message::Text(snapshot.into())).await.is_err() {
                        break;
                    }
                }
                Ok(chat) = chat_rx.recv() => {
                    if sender.send(Message::Text(chat.into())).await.is_err() {
                        break;
                    }
                    eprintln!("[Serveur] WS chat → client");
                }
                else => { break; }
            }
        }
    });

    // Tâche : lire les messages entrants (on ignore le contenu, v0)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {
            // On ne traite pas les messages entrants en v0
        }
    });

    // Si l'une des tâches se termine, on coupe l'autre
    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}
