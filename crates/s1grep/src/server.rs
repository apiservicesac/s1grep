use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::models::CacheDirectory;
use crate::service::{SearchRequest, SearchResponse, SearchService};

/// What a running server leaves in the cache so that searches can find it: its port and a secret only this user can
/// read, so other accounts on the same machine cannot query the code it indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
}

impl ServerInfo {
    fn path() -> anyhow::Result<PathBuf> {
        Ok(CacheDirectory::root()?.join("server.json"))
    }

    pub fn read() -> Option<Self> {
        let text = std::fs::read_to_string(Self::path().ok()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write(&self) -> anyhow::Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn remove(&self) {
        if let Ok(path) = Self::path()
            && Self::read().is_some_and(|current| current.pid == self.pid)
        {
            let _ = std::fs::remove_file(path);
        }
    }

    fn new_token() -> anyhow::Result<String> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness for the server token: {error}"))?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    request: Option<SearchRequest>,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    response: Option<SearchResponse>,
    error: Option<String>,
}

/// Keeps the models in memory and answers searches over a loopback connection, one at a time.
pub struct SearchServer {
    service: SearchService,
    info: ServerInfo,
    listener: TcpListener,
}

impl SearchServer {
    pub fn start(service: SearchService, port: u16) -> anyhow::Result<Self> {
        if let Some(running) = ServerClient::connect() {
            bail!(
                "s1grep serve is already running (pid {}, port {})",
                running.info.pid,
                running.info.port
            );
        }
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).with_context(|| format!("listening on port {port}"))?;
        let info = ServerInfo {
            port: listener.local_addr()?.port(),
            token: ServerInfo::new_token()?,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        info.write()?;
        Ok(Self {
            service,
            info,
            listener,
        })
    }

    pub fn port(&self) -> u16 {
        self.info.port
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        for stream in self.listener.try_clone()?.incoming() {
            let Ok(stream) = stream else { continue };
            if let Err(error) = self.answer(stream) {
                eprintln!("s1grep serve: {error:#}");
            }
        }
        self.info.remove();
        Ok(())
    }

    fn answer(&mut self, stream: TcpStream) -> anyhow::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let envelope: Envelope = serde_json::from_str(&line).context("malformed request")?;
        let reply = if envelope.token != self.info.token {
            Reply {
                response: None,
                error: Some("wrong token".to_string()),
            }
        } else if let Some(request) = envelope.request {
            let started = std::time::Instant::now();
            match self.service.search(&request) {
                Ok(response) => {
                    eprintln!(
                        "{:.2} s  {}  {:?}",
                        started.elapsed().as_secs_f64(),
                        request.root.display(),
                        request.query
                    );
                    Reply {
                        response: Some(response),
                        error: None,
                    }
                }
                Err(error) => Reply {
                    response: None,
                    error: Some(format!("{error:#}")),
                },
            }
        } else {
            Reply {
                response: None,
                error: None,
            }
        };
        let mut writer = &stream;
        writer.write_all(serde_json::to_string(&reply)?.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }
}

impl Drop for SearchServer {
    fn drop(&mut self) {
        self.info.remove();
    }
}

/// A connection to a running `s1grep serve`, when there is one.
pub struct ServerClient {
    pub info: ServerInfo,
}

impl ServerClient {
    const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);

    /// The running server, if its file exists, it answers, and it is the same version as this binary.
    pub fn connect() -> Option<Self> {
        let info = ServerInfo::read()?;
        let client = Self { info };
        client.exchange(None).ok()?;
        (client.info.version == env!("CARGO_PKG_VERSION")).then_some(client)
    }

    pub fn search(&self, request: &SearchRequest) -> anyhow::Result<SearchResponse> {
        self.exchange(Some(request.clone()))?
            .context("the server sent no results")
    }

    fn exchange(&self, request: Option<SearchRequest>) -> anyhow::Result<Option<SearchResponse>> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.info.port));
        let stream = TcpStream::connect_timeout(&address, Self::CONNECT_TIMEOUT)?;
        let mut writer = &stream;
        let envelope = Envelope {
            token: self.info.token.clone(),
            request,
        };
        writer.write_all(serde_json::to_string(&envelope)?.as_bytes())?;
        writer.write_all(b"\n")?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let reply: Reply = serde_json::from_str(&line).context("malformed reply from s1grep serve")?;
        if let Some(error) = reply.error {
            bail!("{error}");
        }
        Ok(reply.response)
    }
}
