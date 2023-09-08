use ammonia::Builder;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::stream::StreamExt;
use futures::SinkExt;
use maplit::{hashmap, hashset};
use serde::Serialize;
// use serde_json::json;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tower_http::services::ServeDir;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::ClientConfig;
use twitch_irc::SecureTCPTransport;
use twitch_irc::TwitchIRCClient;

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "content", rename_all = "lowercase")]
enum BrowserMessage {
    Chat { sender: String, message: String },
}

struct AppState {
    twitch_tx: broadcast::Sender<String>,
}

#[tokio::main]
async fn main() {
    let (twitch_tx, _) = broadcast::channel::<String>(100);
    let mut set = JoinSet::new();
    set.spawn(twitch_bot(twitch_tx.clone()));
    set.spawn(web_page(twitch_tx.clone()));
    loop {}
}

async fn web_page(twitch_tx: broadcast::Sender<String>) {
    let app_state = Arc::new(AppState { twitch_tx });
    let addr = SocketAddr::from(([127, 0, 0, 1], 3314));
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .nest_service("/", ServeDir::new(Path::new("html")))
        .with_state(app_state);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    println!("Connection requested");
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    println!("Connection made");
    let mut state_rx = state.twitch_tx.subscribe();
    let (mut sender, mut _receiver) = stream.split();
    while let Ok(msg) = state_rx.recv().await {
        if sender.send(Message::Text(msg)).await.is_err() {
            break;
        }
    }
}

async fn twitch_bot(twitch_tx: broadcast::Sender<String>) {
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);
    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            match message {
                twitch_irc::message::ServerMessage::Privmsg(payload) => {
                    // let chat_message = BrowserMessage::Chat {
                    //     sender: payload.sender.name.to_string(),
                    //     message: payload.message_text.to_string(),
                    // };
                    let clean_html = sanitize_html(payload.message_text.to_string());
                    let _ = twitch_tx
                        // .send(sanatize_html(serde_json::to_string(&chat_message).unwrap()));
                        .send(format!(
                            r#"<div id="notifications" hx-swap-oob="beforeend"><div>{}</div><div>{}</div></div>"#,
                            payload.sender.name,
                            clean_html
                        ));
                }
                _ => {}
            }
        }
    });
    client.join("theidofalan".to_owned()).unwrap();
    join_handle.await.unwrap();
}

fn sanitize_html(source: String) -> String {
    let tags = hashset!["sup", "sub", "marquee", "b", "i", "strong", "em"];
    let tag_attrs = hashmap![
        // "span" => hashset!["id"]
    ];
    Builder::new()
        .tags(tags)
        .tag_attributes(tag_attrs)
        .clean(source.as_str())
        .to_string()
}
