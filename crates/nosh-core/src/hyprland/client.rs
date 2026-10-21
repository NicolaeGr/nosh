//! Client for the Hyprland command socket.
//!
//! Hyprland 0.56.2 closes the command socket after one response and does not
//! send the `0x04` terminator described in the spec. The client opens one
//! connection per request and accepts either framing: a `0x04` terminator or
//! end of stream. One connection per request also keeps concurrent callers
//! from interleaving on the same socket.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::error::BackendError;

pub const RESPONSE_END: u8 = 0x04;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);

pub struct HyprlandClient {
    socket: PathBuf,
    request_timeout: Duration,
}

impl HyprlandClient {
    pub fn new(socket: PathBuf) -> Self {
        Self::with_timeout(socket, DEFAULT_TIMEOUT)
    }

    pub fn with_timeout(socket: PathBuf, request_timeout: Duration) -> Self {
        Self {
            socket,
            request_timeout,
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub async fn query(&self, request: &str) -> Result<String, BackendError> {
        let mut stream = self.connect().await?;

        let response = timeout(self.request_timeout, async {
            stream.write_all(request.as_bytes()).await?;
            stream.flush().await?;

            let mut response = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await?;
                if read == 0 {
                    if response.is_empty() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "compositor closed the command socket without a response",
                        ));
                    }
                    break;
                }
                if let Some(end) = buffer[..read].iter().position(|byte| *byte == RESPONSE_END) {
                    response.extend_from_slice(&buffer[..end]);
                    break;
                }
                response.extend_from_slice(&buffer[..read]);
            }

            Ok::<Vec<u8>, std::io::Error>(response)
        })
        .await
        .map_err(|_| {
            BackendError::Request(format!("request to {} timed out", self.socket.display()))
        })?
        .map_err(|error| BackendError::Request(error.to_string()))?;

        String::from_utf8(response)
            .map_err(|error| BackendError::Request(format!("response is not UTF-8: {error}")))
    }

    pub async fn query_json<T>(&self, request: &str) -> Result<T, BackendError>
    where
        T: DeserializeOwned,
    {
        let response = self.query(request).await?;
        serde_json::from_str(&response).map_err(|error| BackendError::Json(error.to_string()))
    }

    async fn connect(&self) -> Result<UnixStream, BackendError> {
        timeout(self.request_timeout, UnixStream::connect(&self.socket))
            .await
            .map_err(|_| {
                BackendError::Unavailable(format!(
                    "timed out connecting to {}",
                    self.socket.display()
                ))
            })?
            .map_err(|error| {
                BackendError::Unavailable(format!(
                    "cannot connect to {}: {error}",
                    self.socket.display()
                ))
            })
    }
}
