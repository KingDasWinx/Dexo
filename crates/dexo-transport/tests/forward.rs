use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use dexo_transport::{ProxyConfig, TransportLease};

struct EchoServer {
    addr: std::net::SocketAddr,
    _task: tokio::task::JoinHandle<()>,
}

impl EchoServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    loop {
                        match socket.read(&mut buf).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if socket.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                });
            }
        });
        Self { addr, _task }
    }

    fn address(&self) -> std::net::SocketAddr {
        self.addr
    }
}

async fn roundtrip(endpoint: std::net::SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(endpoint).await.unwrap();
    stream.write_all(payload).await.unwrap();
    let mut buf = vec![0u8; payload.len()];
    stream.read_exact(&mut buf).await.unwrap();
    buf
}

#[tokio::test]
async fn lease_forwards_multiple_connections_and_stops_on_drop() {
    let target = EchoServer::start().await;
    let lease = TransportLease::direct(target.address()).await.unwrap();
    assert_eq!(roundtrip(lease.endpoint(), b"one").await, b"one");
    assert_eq!(roundtrip(lease.endpoint(), b"two").await, b"two");
    let endpoint = lease.endpoint();
    lease.close().await;
    assert!(TcpStream::connect(endpoint).await.is_err());
}

#[tokio::test]
async fn lease_forwards_through_http_connect_proxy() {
    let target = EchoServer::start().await;
    let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    let echo_addr = target.address();
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = proxy_listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 1024];
                let n = client.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                assert!(
                    std::str::from_utf8(&buf[..n])
                        .unwrap_or_default()
                        .starts_with("CONNECT ")
                );
                if client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .await
                    .is_err()
                {
                    return;
                }
                if let Ok(mut upstream) = TcpStream::connect(echo_addr).await {
                    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
                }
            });
        }
    });

    let lease = TransportLease::proxy(
        ProxyConfig::http_connect("127.0.0.1", proxy_addr.port()),
        echo_addr.ip().to_string(),
        echo_addr.port(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(roundtrip(lease.endpoint(), b"proxied").await, b"proxied");
}
