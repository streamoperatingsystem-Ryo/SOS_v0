// TEST JETABLE (diagnostic Kick 2026-09-22) — vérifie la chaîne complète
// realtime Kick avec la même pile isolée que kick.rs : hyper+hyper-tls
// (Schannel) pour HTTP, TcpStream+tokio-native-tls+client_async pour le WS.
// Usage : cargo test --test kick_ws_probe -- --nocapture
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

async fn realtime_post(path: &str, body: serde_json::Value) -> serde_json::Value {
    let client = hyper_util::client::legacy::Client::builder(
        hyper_util::rt::TokioExecutor::new(),
    )
    .build::<_, http_body_util::Full<hyper::body::Bytes>>(hyper_tls::HttpsConnector::new());
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(format!("https://web.kick.com/api/v1/realtime{}", path))
        .header("x-app-platform", "web")
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body.to_string(),
        )))
        .unwrap();
    let resp = client.request(req).await.unwrap();
    assert!(resp.status().is_success(), "statut {}", resp.status());
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn kick_centrifugo_ws_schannel() {
    // 1. URL Centrifugo via l'API web anonyme
    let client_id = uuid::Uuid::new_v4().to_string();
    let conf = realtime_post(
        "/channels/668/chat/connection",
        serde_json::json!({
            "client": {"id": client_id, "type": "web"},
            "capabilities": {"accepted_providers": [{"provider": "pusher"}, {"provider": "centrifugo"}]}
        }),
    )
    .await;
    println!("conf: {}", conf);
    let url = conf["data"]["connections"][0]["credentials"]["url"]
        .as_str()
        .unwrap()
        .to_string();

    // 1b. Jeton invité « guest » — requis par Centrifugo (« bad request » sans)
    let auth = realtime_post(
        "/auth/connection",
        serde_json::json!({"client_id": client_id}),
    )
    .await;
    let token = auth["data"]["token"].as_str().unwrap().to_string();
    println!("token: {}...", &token[..30]);

    // 2. Poignée de main WS : TcpStream + TLS Schannel + client_async
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://kick.com".parse().unwrap());
    let host = request.uri().host().unwrap().to_string();
    let port = request.uri().port_u16().unwrap_or(443);
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .unwrap();
    let connector =
        tokio_native_tls::TlsConnector::from(native_tls::TlsConnector::new().unwrap());
    let tls = connector.connect(&host, tcp).await.unwrap();
    let (mut ws, resp) = client_async(request, tls).await.expect("WS connect a échoué");
    println!("WS OK: {}", resp.status());

    ws.send(Message::Text(
        serde_json::json!({"id":1,"connect":{"token":token,"name":"js"}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();
    let first = ws.next().await.unwrap().unwrap();
    println!("ack connect: {}", first);

    ws.send(Message::Text(
        serde_json::json!({"id":2,"subscribe":{"channel":"chatrooms.668.v2"}})
            .to_string()
            .into(),
    ))
    .await
    .unwrap();

    // 15s max pour recevoir au moins un push
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut pushes = 0;
    while std::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(3), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
                if v.get("push").is_some() {
                    pushes += 1;
                    if pushes <= 2 {
                        println!("push: {}", &t[..t.len().min(200)]);
                    }
                }
            }
            _ => {}
        }
    }
    println!("pushes reçus: {}", pushes);
    assert!(pushes > 0, "aucun push reçu");
}
