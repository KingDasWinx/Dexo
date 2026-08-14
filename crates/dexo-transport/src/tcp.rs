use tokio::net::TcpStream;

use crate::{
    BoxStream, TransportError,
    config::{validate_host, validate_port},
};

pub async fn connect_direct(host: &str, port: u16) -> Result<BoxStream, TransportError> {
    validate_host(host)?;
    validate_port(port, "port must be between 1 and 65535")?;
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(|error| TransportError::Io(error.to_string()))?;
    Ok(Box::new(stream))
}
