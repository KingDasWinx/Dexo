use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;

use crate::{
    BoxStream, ProxyConfig, TransportError,
    config::{validate_host, validate_port},
};

pub struct ProxyCredentials {
    pub username: String,
    password: SecretString,
}

impl std::fmt::Debug for ProxyCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyCredentials")
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

impl ProxyCredentials {
    pub fn new(username: impl Into<String>, password: SecretString) -> Self {
        Self {
            username: username.into(),
            password,
        }
    }
}

pub async fn connect_proxy(
    proxy: &ProxyConfig,
    target_host: &str,
    target_port: u16,
    auth: Option<&ProxyCredentials>,
) -> Result<BoxStream, TransportError> {
    proxy.validate()?;
    validate_host(target_host)?;
    validate_port(target_port, "target port must be between 1 and 65535")?;
    match proxy {
        ProxyConfig::Socks5 { host, port } => {
            connect_socks5(host, *port, target_host, target_port, auth).await
        }
        ProxyConfig::HttpConnect { host, port } => {
            connect_http(host, *port, target_host, target_port, auth).await
        }
    }
}

async fn connect_socks5(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    auth: Option<&ProxyCredentials>,
) -> Result<BoxStream, TransportError> {
    let proxy = (proxy_host, proxy_port);
    let target = (target_host, target_port);
    let stream = if let Some(auth) = auth {
        Socks5Stream::connect_with_password(
            proxy,
            target,
            &auth.username,
            auth.password.expose_secret(),
        )
        .await
    } else {
        Socks5Stream::connect(proxy, target).await
    }
    .map_err(|error| TransportError::Proxy(error.to_string()))?;
    Ok(Box::new(stream))
}

async fn connect_http(
    proxy_host: &str,
    proxy_port: u16,
    target_host: &str,
    target_port: u16,
    auth: Option<&ProxyCredentials>,
) -> Result<BoxStream, TransportError> {
    let mut stream = TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|error| TransportError::Io(error.to_string()))?;
    let authority = connect_authority(target_host, target_port);
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(auth) = auth {
        let token = base64_basic(&auth.username, auth.password.expose_secret());
        request.push_str("Proxy-Authorization: Basic ");
        request.push_str(&token);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    if request.len() > 2048 {
        return Err(TransportError::InvalidConfig(
            "proxy CONNECT request too large".into(),
        ));
    }
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|error| TransportError::Io(error.to_string()))?;
    let headers = read_http_headers(&mut stream).await?;
    accept_2xx(&headers)?;
    Ok(Box::new(stream))
}

fn connect_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn accept_2xx(headers: &[u8]) -> Result<(), TransportError> {
    let text = std::str::from_utf8(headers)
        .map_err(|_| TransportError::Proxy("proxy response was not valid UTF-8".into()))?;
    let status_line = text.lines().next().unwrap_or("");
    let code = status_line.split_whitespace().nth(1).unwrap_or("");
    let parsed = code.parse::<u16>().unwrap_or(0);
    if (200..300).contains(&parsed) {
        Ok(())
    } else {
        Err(TransportError::Proxy(format!(
            "proxy CONNECT failed with status {code}"
        )))
    }
}

async fn read_http_headers(stream: &mut TcpStream) -> Result<Vec<u8>, TransportError> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < 8192 {
        stream
            .read_exact(&mut byte)
            .await
            .map_err(|error| TransportError::Io(error.to_string()))?;
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            return Ok(buf);
        }
    }
    Err(TransportError::Proxy(
        "proxy response headers too large".into(),
    ))
}

// ponytail: tiny Basic-auth encoder; replace with a crate if we add more HTTP auth schemes
fn base64_basic(username: &str, password: &str) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!("{username}:{password}");
    let mut out = String::new();
    for chunk in raw.as_bytes().chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
        match b1 {
            Some(b1) => {
                out.push(TABLE[(((b1 & 0x0f) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char)
            }
            None => out.push('='),
        }
        match b2 {
            Some(b2) => out.push(TABLE[(b2 & 0x3f) as usize] as char),
            None => out.push('='),
        }
    }
    out
}
