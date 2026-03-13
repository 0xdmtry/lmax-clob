use actix_web::{get, web, HttpRequest, HttpResponse};
use actix_ws::Message;
use metrics::gauge;
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::metrics::WS_CLIENT_COUNT;
use crate::ws::broadcast::FillBroadcast;

const SLOW_CLIENT_TIMEOUT_MS: u64 = 100;

#[get("/ws/markets/{id}")]
pub async fn ws_market(
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Payload,
    broadcast: web::Data<FillBroadcast>,
) -> actix_web::Result<HttpResponse> {
    let market_id = path.into_inner();
    let (response, mut session, mut msg_stream) = actix_ws::handle(&req, body)?;

    let mut rx = broadcast.subscribe();

    gauge!(WS_CLIENT_COUNT).increment(1.0);
    info!(market_id = %market_id, "ws client connected");

    actix_web::rt::spawn(async move {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(msg) => {
                            let send_result = timeout(
                                Duration::from_millis(SLOW_CLIENT_TIMEOUT_MS),
                                session.text(msg),
                            )
                            .await;

                            match send_result {
                                Ok(Ok(_))  => {}
                                Ok(Err(e)) => {
                                    warn!("ws send error, dropping client: {}", e);
                                    let _ = session.close(None).await;
                                    gauge!(WS_CLIENT_COUNT).decrement(1.0);
                                    return;
                                }
                                Err(_) => {
                                    warn!("ws slow client timeout, dropping");
                                    let _ = session.close(None).await;
                                    gauge!(WS_CLIENT_COUNT).decrement(1.0);
                                    return;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!("ws client lagged {} messages, dropping", n);
                            let _ = session.close(None).await;
                            gauge!(WS_CLIENT_COUNT).decrement(1.0);
                            return;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            gauge!(WS_CLIENT_COUNT).decrement(1.0);
                            return;
                        }
                    }
                }

                msg = msg_stream.recv() => {
                    match msg {
                        Some(Ok(Message::Ping(bytes))) => {
                            if session.pong(&bytes).await.is_err() {
                                gauge!(WS_CLIENT_COUNT).decrement(1.0);
                                return;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            info!(market_id = %market_id, "ws client disconnected");
                            gauge!(WS_CLIENT_COUNT).decrement(1.0);
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    Ok(response)
}
