//! Terminal traffic is many tiny writes: one keystroke, one echoed byte, one
//! cursor move. With Nagle's algorithm on, a small write waits while an
//! earlier one is unacknowledged, which together with delayed ACKs holds a
//! keystroke or its echo back by 40–200 ms. Every socket that carries
//! terminal bytes turns it off.

use axum::serve::{ListenerExt, TapIo};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{client::IntoClientRequest, handshake::client::Response, Error},
    MaybeTlsStream, WebSocketStream,
};

/// `listener`, with Nagle's algorithm off on every accepted connection. The
/// concrete type keeps axum's `ConnectInfo<SocketAddr>` working.
pub fn listener(listener: TcpListener) -> TapIo<TcpListener, fn(&mut TcpStream)> {
    listener.tap_io(disable_nagle as fn(&mut TcpStream))
}

fn disable_nagle(stream: &mut TcpStream) {
    if let Err(error) = stream.set_nodelay(true) {
        tracing::debug!(%error, "could not disable Nagle's algorithm on a connection");
    }
}

/// Open a WebSocket with Nagle's algorithm off.
pub async fn connect_websocket(
    request: impl IntoClientRequest + Unpin,
) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, Response), Error> {
    connect_async_with_config(request, None, true).await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use axum::serve::Listener;

    #[tokio::test]
    async fn accepted_connections_have_nagle_off() {
        let bound = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = bound.local_addr().unwrap();
        let mut listener = listener(bound);
        let client = tokio::spawn(TcpStream::connect(address));
        let (accepted, _) = listener.accept().await;
        assert!(accepted.nodelay().unwrap());
        drop(client.await.unwrap().unwrap());
    }

    #[tokio::test]
    async fn websocket_clients_have_nagle_off() {
        let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = server.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = server.accept().await.unwrap();
            let _socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            std::future::pending::<()>().await;
        });
        let (socket, _) = connect_websocket(format!("ws://{address}/")).await.unwrap();
        match socket.get_ref() {
            MaybeTlsStream::Plain(stream) => assert!(stream.nodelay().unwrap()),
            _ => panic!("expected a plain TCP stream"),
        }
    }
}
