use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::cache::{AtomicFile, CacheDirectory};
use crate::hub::ModelInstaller;
use crate::models::ModelDirectory;
use crate::progress::IndexEvent;
use crate::service::{SearchRequest, SearchResponse, SearchService};
use crate::settings::ServerSettings;

/// What a running server leaves in the cache so that searches can find it: its port and a secret only this user can
/// read, so other accounts on the same machine cannot query the code it indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
    /// Minutes without searches before it stops on its own; `None` when started by hand.
    #[serde(default)]
    pub idle_minutes: Option<u64>,
}

impl ServerInfo {
    fn path() -> anyhow::Result<PathBuf> {
        Ok(CacheDirectory::root()?.join(ServerSettings::INFO_FILE))
    }

    pub fn read() -> Option<Self> {
        let text = std::fs::read_to_string(Self::path().ok()?).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write(&self) -> anyhow::Result<()> {
        AtomicFile::write(&Self::path()?, serde_json::to_string(self)?.as_bytes())
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
    #[serde(default)]
    shutdown: bool,
}

/// One line from the server: progress while it indexes, then the response or an error.
#[derive(Serialize, Deserialize)]
struct Reply {
    #[serde(default)]
    progress: Option<IndexEvent>,
    response: Option<SearchResponse>,
    error: Option<String>,
}

/// Keeps the models in memory and answers searches over a loopback connection, one at a time. Only one runs per
/// user: it holds a lock file for as long as it lives, released by the system even if it crashes.
pub struct SearchServer {
    service: SearchService,
    info: ServerInfo,
    listener: TcpListener,
    idle: Option<Duration>,
    /// Held while the server runs: a second server cannot start.
    #[expect(dead_code, reason = "held only to release the lock when the server stops")]
    server_lock: File,
}

impl SearchServer {
    /// Starts answering with `lock`, taken by `acquire_lock` before the models were loaded.
    pub fn start(service: SearchService, lock: File, port: u16, idle: Option<Duration>) -> anyhow::Result<Self> {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)).with_context(|| format!("listening on port {port}"))?;
        listener.set_nonblocking(true)?;
        let info = ServerInfo {
            port: listener.local_addr()?.port(),
            token: ServerInfo::new_token()?,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            idle_minutes: idle.map(|duration| duration.as_secs() / 60),
        };
        info.write()?;
        Ok(Self {
            service,
            info,
            listener,
            idle,
            server_lock: lock,
        })
    }

    /// The server lock, taken before loading the models, so that two searches starting at once never load them twice.
    pub fn acquire_lock() -> anyhow::Result<File> {
        let lock = Self::lock()?.context("an s1grep server is already running (`s1grep status` shows it)")?;
        // Whatever server.json says now was left by a server that is gone: nobody else can hold the lock.
        if let Ok(path) = ServerInfo::path() {
            let _ = std::fs::remove_file(path);
        }
        Ok(lock)
    }

    /// Whether a server holds the lock: running, or still loading its models.
    pub fn is_running() -> anyhow::Result<bool> {
        Ok(Self::lock()?.is_none())
    }

    /// The server lock, or `None` when another server holds it.
    fn lock() -> anyhow::Result<Option<File>> {
        let path = CacheDirectory::root()?.join(ServerSettings::LOCK_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        Ok(file.try_lock().ok().map(|()| file))
    }

    pub fn port(&self) -> u16 {
        self.info.port
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let mut last_activity = Instant::now();
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false)?;
                    match self.answer(stream) {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(error) => eprintln!("s1grep server: {error:#}"),
                    }
                    last_activity = Instant::now();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if self.service.has_indexing() {
                        // Background indexing counts as activity: the process stays until every project is indexed.
                        // A failed step pauses before the next try instead of spinning on the same error.
                        match self.service.index_step() {
                            Ok(()) => last_activity = Instant::now(),
                            Err(error) => {
                                eprintln!("s1grep server: indexing: {error:#}");
                                std::thread::sleep(ServerSettings::INDEX_RETRY_PAUSE);
                            }
                        }
                        continue;
                    }
                    if let Some(idle) = self.idle
                        && last_activity.elapsed() >= idle
                    {
                        eprintln!(
                            "s1grep server: stopping after {} min without searches",
                            idle.as_secs() / 60
                        );
                        break;
                    }
                    std::thread::sleep(ServerSettings::POLL_INTERVAL);
                }
                Err(error) => return Err(error.into()),
            }
        }
        self.info.remove();
        Ok(())
    }

    /// Answers one connection; `true` when it asked the server to stop.
    fn answer(&mut self, stream: TcpStream) -> anyhow::Result<bool> {
        stream.set_read_timeout(Some(ServerSettings::REQUEST_TIMEOUT))?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let envelope: Envelope = serde_json::from_str(&line).context("malformed request")?;
        let mut stop = false;
        let reply = if envelope.token != self.info.token {
            Reply {
                progress: None,
                response: None,
                error: Some("wrong token".to_string()),
            }
        } else if envelope.shutdown {
            stop = true;
            Reply {
                progress: None,
                response: None,
                error: None,
            }
        } else if let Some(request) = envelope.request {
            let started = Instant::now();
            let mut writer = &stream;
            let mut send_progress = |event: IndexEvent| {
                let line = Reply {
                    progress: Some(event),
                    response: None,
                    error: None,
                };
                if let Ok(text) = serde_json::to_string(&line) {
                    let _ = writer.write_all(text.as_bytes()).and_then(|()| writer.write_all(b"\n"));
                }
            };
            match self.service.search(&request, &mut send_progress) {
                Ok(response) => {
                    eprintln!(
                        "{:.2} s  {}  {:?}",
                        started.elapsed().as_secs_f64(),
                        request.target.display(),
                        request.query
                    );
                    Reply {
                        progress: None,
                        response: Some(response),
                        error: None,
                    }
                }
                Err(error) => Reply {
                    progress: None,
                    response: None,
                    error: Some(format!("{error:#}")),
                },
            }
        } else {
            Reply {
                progress: None,
                response: None,
                error: None,
            }
        };
        let mut writer = &stream;
        writer.write_all(serde_json::to_string(&reply)?.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(stop)
    }
}

impl Drop for SearchServer {
    fn drop(&mut self) {
        self.info.remove();
    }
}

/// A connection to a running server.
pub struct ServerClient {
    pub info: ServerInfo,
}

impl ServerClient {
    /// The running server of this version, if there is one and it answers.
    pub fn connect() -> Option<Self> {
        Self::any().filter(|client| client.info.version == env!("CARGO_PKG_VERSION"))
    }

    /// The running server of any version, if there is one and it answers within the ping timeout.
    pub fn any() -> Option<Self> {
        let client = Self {
            info: ServerInfo::read()?,
        };
        client
            .exchange(None, false, &mut |_| {}, Some(ServerSettings::PING_TIMEOUT))
            .ok()?;
        Some(client)
    }

    /// The server recorded in the cache, whether it answers or not, as long as it still holds the server lock.
    pub fn recorded() -> Option<Self> {
        let info = ServerInfo::read()?;
        (SearchServer::lock().ok()?.is_none()).then_some(Self { info })
    }

    pub fn search(
        &self,
        request: &SearchRequest,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<SearchResponse> {
        self.exchange(Some(request.clone()), false, progress, None)?
            .context("the server sent no results")
    }

    /// Asks the server to stop; if it does not within a few seconds (busy or hung), terminates it.
    pub fn stop(&self) -> anyhow::Result<()> {
        let _ = self.exchange(None, true, &mut |_| {}, Some(ServerSettings::PING_TIMEOUT));
        if Self::wait_released(ServerSettings::STOP_TIMEOUT)? {
            return Ok(());
        }
        Self::terminate(self.info.pid)?;
        if Self::wait_released(ServerSettings::KILL_TIMEOUT)? {
            return Ok(());
        }
        bail!("the s1grep background process (pid {}) did not stop", self.info.pid)
    }

    fn wait_released(timeout: Duration) -> anyhow::Result<bool> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if SearchServer::lock()?.is_some() {
                return Ok(true);
            }
            std::thread::sleep(ServerSettings::POLL_INTERVAL);
        }
        Ok(false)
    }

    #[cfg(unix)]
    fn terminate(pid: u32) -> anyhow::Result<()> {
        // SAFETY: kill only sends a signal to the process id the server recorded for itself.
        let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if result != 0 {
            bail!("could not terminate the background process (pid {pid})");
        }
        Ok(())
    }

    #[cfg(windows)]
    fn terminate(pid: u32) -> anyhow::Result<()> {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            bail!("could not terminate the background process (pid {pid})");
        }
        Ok(())
    }

    /// Sends one request and reads the replies; `reply_timeout` bounds each wait for a line (none for searches, which
    /// stream progress while they index).
    fn exchange(
        &self,
        request: Option<SearchRequest>,
        shutdown: bool,
        progress: &mut dyn FnMut(IndexEvent),
        reply_timeout: Option<Duration>,
    ) -> anyhow::Result<Option<SearchResponse>> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.info.port));
        let stream = TcpStream::connect_timeout(&address, ServerSettings::CONNECT_TIMEOUT)?;
        stream.set_read_timeout(reply_timeout)?;
        let mut writer = &stream;
        let envelope = Envelope {
            token: self.info.token.clone(),
            request,
            shutdown,
        };
        writer.write_all(serde_json::to_string(&envelope)?.as_bytes())?;
        writer.write_all(b"\n")?;
        for line in BufReader::new(&stream).lines() {
            let reply: Reply = serde_json::from_str(&line?).context("malformed reply from the s1grep server")?;
            if let Some(event) = reply.progress {
                progress(event);
                continue;
            }
            if let Some(error) = reply.error {
                bail!("{error}");
            }
            return Ok(reply.response);
        }
        bail!("the s1grep server closed the connection")
    }
}

/// Starts the server in the background when none is running, so that nobody has to start it by hand.
pub struct BackgroundServer;

impl BackgroundServer {
    /// A client of a running server of this version, starting one if needed. Fails at once, with the reason, when the
    /// models are missing or the new process exits instead of waiting for it.
    pub fn ensure(models: &ModelDirectory, progress: &mut dyn FnMut(IndexEvent)) -> anyhow::Result<ServerClient> {
        if let Some(client) = ServerClient::connect() {
            return Ok(client);
        }
        let folder = models.resolved()?;
        if !ModelInstaller::is_complete(&folder) {
            bail!(
                "the models are not in {}: run `s1grep setup` to download them (about 2.4 GB, once)",
                folder.display()
            );
        }
        if let Some(recorded) = ServerClient::recorded() {
            // Busy with a batch of background indexing: it answers right after it.
            if recorded.info.version == env!("CARGO_PKG_VERSION") {
                return Ok(recorded);
            }
            let _ = recorded.stop();
        }
        progress(IndexEvent::StartingServer);
        // A server holding the lock without server.json is still loading its models (another search started it).
        let mut child = if SearchServer::is_running()? {
            None
        } else {
            Some(Self::spawn(models)?)
        };
        let deadline = Instant::now() + ServerSettings::START_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(client) = ServerClient::connect() {
                return Ok(client);
            }
            if let Some(process) = child.as_mut()
                && process.try_wait()?.is_some()
            {
                // Lost the race to a server another search started at the same moment: wait for that one.
                if !SearchServer::is_running()? {
                    bail!(
                        "the background process stopped: {}",
                        Self::last_log_line().unwrap_or_else(|| "see the server log".to_string())
                    );
                }
                child = None;
            }
            std::thread::sleep(ServerSettings::POLL_INTERVAL);
        }
        bail!(
            "the background process did not start within {} s",
            ServerSettings::START_TIMEOUT.as_secs()
        )
    }

    /// The last line the background process wrote, usually the reason it stopped.
    fn last_log_line() -> Option<String> {
        let text = std::fs::read_to_string(CacheDirectory::root().ok()?.join(ServerSettings::LOG_FILE)).ok()?;
        text.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim_start_matches("Error: ").to_string())
    }

    fn spawn(models: &ModelDirectory) -> anyhow::Result<std::process::Child> {
        let log_path = CacheDirectory::root()?.join(ServerSettings::LOG_FILE);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = OpenOptions::new().create(true).append(true).open(&log_path)?;
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["serve", "--background"]);
        if let Some(folder) = models.explicit() {
            command.arg("--models").arg(folder);
        }
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own session, detached from the terminal: closing the terminal or Ctrl+C there does not stop it.
            // SAFETY: setsid is async-signal-safe and the closure touches nothing else between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(
                crate::settings::PlatformSettings::WINDOWS_DETACHED_PROCESS
                    | crate::settings::PlatformSettings::WINDOWS_NEW_PROCESS_GROUP,
            );
        }
        command.spawn().context("starting the s1grep background process")
    }
}
