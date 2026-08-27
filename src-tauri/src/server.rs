use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::Html,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast;
use tower_http::services::ServeDir;

use crate::scene::Scene;

/// HTML embarqué (vanilla JS, zéro Tauri). Sert de page de diffusion pour OBS.
const DIFFUSION_HTML: &str = include_str!("../resources/diffusion.html");

/// Canal de diffusion des snapshots scène vers tous les clients WS connectés.
pub type SnapshotTx = broadcast::Sender<String>;

pub struct ServerState {
    pub snapshot_tx: SnapshotTx,
    pub scene: Arc<Mutex<Scene>>,
}

/// Démarre le serveur HTTP+WS sur 127.0.0.1:4321.
/// En cas d'échec du bind (port occupé), émet `server_error` et retourne l'erreur.
/// Ne JAMAIS fallback sur un autre port — OBS pointe sur :4321.
pub async fn run_server(
    app: AppHandle,
    snapshot_tx: SnapshotTx,
    scene: Arc<Mutex<Scene>>,
) -> Result<(), String> {
    let addr: SocketAddr = "127.0.0.1:4321"
        .parse()
        .map_err(|e| format!("Adresse invalide: {}", e))?;

    let state = Arc::new(ServerState {
        snapshot_tx: snapshot_tx.clone(),
        scene,
    });

    // Dossier medias servi sur /medias pour diffusion.html (chemin relatif).
    let medias_dir = crate::config::data_dir(&app)?.join("medias");

    let app_router = Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_handler))
        .nest_service("/medias", ServeDir::new(medias_dir))
        .with_state(state);

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
    let mut rx = state.snapshot_tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Snapshot initial : envoyer l'état courant dès la connexion
    {
        let snapshot = {
            let scene = state.scene.lock().unwrap();
            serde_json::json!({
                "type": "snapshot",
                "scene": &*scene
            })
            .to_string()
        };
        let _ = sender.send(Message::Text(snapshot.into())).await;
    }

    // Tâche : pousser les snapshots vers le client
    let mut send_task = tokio::spawn(async move {
        while let Ok(snapshot) = rx.recv().await {
            if sender
                .send(Message::Text(snapshot.into()))
                .await
                .is_err()
            {
                break;
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
