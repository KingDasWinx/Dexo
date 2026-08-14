use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use dexo_transport::{ProxyConfig, connect_direct, connect_proxy};

#[test]
fn rejects_proxy_without_port() {
    let config = ProxyConfig::http_connect("proxy.internal", 0);
    assert_eq!(
        config.validate().unwrap_err().to_string(),
        "proxy port must be between 1 and 65535"
    );
}

#[tokio::test]
async fn http_connect_accepts_2xx() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let n = socket.read(&mut buf).await.unwrap();
        assert!(
            std::str::from_utf8(&buf[..n])
                .unwrap()
                .starts_with("CONNECT ")
        );
        socket
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .unwrap();
        socket.write_all(b"pong").await.unwrap();
    });

    let config = ProxyConfig::http_connect("127.0.0.1", addr.port());
    let mut stream = connect_proxy(&config, "db.internal", 5432, None)
        .await
        .unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"pong");
}

#[tokio::test]
async fn http_connect_rejects_non_2xx() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 1024];
        let _ = socket.read(&mut buf).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let config = ProxyConfig::http_connect("127.0.0.1", addr.port());
    let error = connect_proxy(&config, "db.internal", 5432, None)
        .await
        .err()
        .expect("CONNECT should fail");
    assert!(error.to_string().contains("403"));
}

#[tokio::test]
async fn direct_tcp_roundtrip() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4];
        socket.read_exact(&mut buf).await.unwrap();
        socket.write_all(&buf).await.unwrap();
    });

    let mut stream = connect_direct("127.0.0.1", addr.port()).await.unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");
}

#[tokio::test]
async fn socks5_connect_tunnels() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut greeting = [0u8; 3];
        socket.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting[0], 0x05);
        socket.write_all(&[0x05, 0x00]).await.unwrap();
        let mut header = [0u8; 4];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(&header[..2], &[0x05, 0x01]);
        match header[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                socket.read_exact(&mut rest).await.unwrap();
            }
            0x03 => {
                let mut len = [0u8; 1];
                socket.read_exact(&mut len).await.unwrap();
                let mut rest = vec![0u8; len[0] as usize + 2];
                socket.read_exact(&mut rest).await.unwrap();
            }
            0x04 => {
                let mut rest = [0u8; 18];
                socket.read_exact(&mut rest).await.unwrap();
            }
            other => panic!("unexpected atyp {other}"),
        }
        socket
            .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        socket.write_all(b"pong").await.unwrap();
    });

    let config = ProxyConfig::socks5("127.0.0.1", addr.port());
    let mut stream = connect_proxy(&config, "db.internal", 5432, None)
        .await
        .unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"pong");
}
