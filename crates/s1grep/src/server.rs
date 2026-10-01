use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::cache::{AtomicFile, CacheDirectory};
use crate::hub::ModelInstaller;
use crate::models::ModelDirectory;
use crate::progress::IndexEvent;
use crate::protocol::{Envelope, ErrorKind, Reply, Request, ServerError};
use crate::service::{ProjectProgress, SearchCancelled, SearchRequest, SearchResponse, SearchService};
use crate::settings::ServerSettings;

/// What a running server leaves in the cache so that searches can find it: its port and a secret only this user can
/// read, so other accounts on the same machine cannot query the code it indexes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub version: String,
    /// Protocol version of the process; 0 for processes older than the field.
    #[serde(default)]
    pub protocol: u32,
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

/// A request the connection thread hands to the worker, with the connection to answer on.
struct Job {
    request: Request,
    connection: TcpStream,
}

/// Keeps the models in memory and answers over a loopback connection. Two threads: this one accepts connections and
/// answers pings and stops at once; a worker owns the models and runs searches, indexing requests and, between them,
/// background indexing steps, searches first. Only one runs per user: it holds a lock file for as long as it lives,
/// released by the system even if it crashes.
pub struct SearchServer {
    service: SearchService,
    info: ServerInfo,
    listener: TcpListener,
    idle: Option<Duration>,
    /// Held while the server runs: a second server cannot start.
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
            protocol: ServerSettings::PROTOCOL_VERSION,
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

    /// Accepts connections until the worker stops (asked to, idle, or failed), then cleans up.
    pub fn run(self) -> anyhow::Result<()> {
        let Self {
            service,
            info,
            listener,
            idle,
            server_lock,
        } = self;
        let stopping = Arc::new(AtomicBool::new(false));
        let (jobs, inbox) = mpsc::channel::<Job>();
        let worker = {
            let stopping = Arc::clone(&stopping);
            std::thread::Builder::new()
                .name("s1grep-worker".to_string())
                .spawn(move || Worker { service, idle }.run(&inbox, &stopping))?
        };
        while !stopping.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = Self::receive(stream, &info, &jobs, &stopping) {
                        eprintln!("s1grep server: {error:#}");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ServerSettings::ACCEPT_INTERVAL);
                }
                Err(error) => {
                    stopping.store(true, Ordering::SeqCst);
                    return Err(error.into());
                }
            }
        }
        drop(jobs);
        if worker.join().is_err() {
            eprintln!("s1grep server: the worker thread failed");
        }
        info.remove();
        drop(server_lock);
        eprintln!("s1grep server: process {} stopped", std::process::id());
        Ok(())
    }

    /// Reads one request; answers pings and stops here, hands the rest to the worker.
    fn receive(stream: TcpStream, info: &ServerInfo, jobs: &Sender<Job>, stopping: &AtomicBool) -> anyhow::Result<()> {
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(ServerSettings::REQUEST_TIMEOUT))?;
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line)?;
        let connection = Connection { stream };
        let envelope: Envelope = match serde_json::from_str(&line) {
            Ok(envelope) => envelope,
            Err(error) => {
                return connection.send(&Reply::Failed {
                    error: ServerError::new(ErrorKind::InvalidRequest, format!("malformed request: {error}")),
                });
            }
        };
        if envelope.token != info.token {
            return connection.send(&Reply::Failed {
                error: ServerError::new(ErrorKind::Unauthorized, "wrong token"),
            });
        }
        if envelope.protocol != ServerSettings::PROTOCOL_VERSION {
            return connection.send(&Reply::Failed {
                error: ServerError::new(
                    ErrorKind::ProtocolMismatch,
                    format!(
                        "this background process speaks protocol {}, the client {}",
                        ServerSettings::PROTOCOL_VERSION,
                        envelope.protocol
                    ),
                ),
            });
        }
        match envelope.request {
            Request::Ping => connection.send(&Reply::Pong),
            Request::Shutdown => {
                stopping.store(true, Ordering::SeqCst);
                connection.send(&Reply::Done)
            }
            request => jobs
                .send(Job {
                    request,
                    connection: connection.stream,
                })
                .map_err(|_| anyhow::anyhow!("the worker has stopped")),
        }
    }
}

/// One client connection seen from the process: replies go out as JSON lines.
struct Connection {
    stream: TcpStream,
}

impl Connection {
    fn send(&self, reply: &Reply) -> anyhow::Result<()> {
        let mut writer = &self.stream;
        writer.write_all(serde_json::to_string(reply)?.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }

    /// Whether the client has gone away (closed the connection or stopped the command): nothing is waiting to be
    /// read from it, and a read finds the end of the stream.
    fn is_closed(&self) -> bool {
        if self.stream.set_nonblocking(true).is_err() {
            return true;
        }
        let mut byte = [0_u8; 1];
        let closed = match self.stream.peek(&mut byte) {
            // The end of the stream: the client closed its side.
            Ok(0) => true,
            Ok(_) => false,
            // Nothing to read yet means the client is still there.
            Err(error) => error.kind() != std::io::ErrorKind::WouldBlock,
        };
        let _ = self.stream.set_nonblocking(false);
        closed
    }
}

/// Owns the models and does the work: requests as they come, background indexing steps in between.
struct Worker {
    service: SearchService,
    idle: Option<Duration>,
}

impl Worker {
    fn run(mut self, inbox: &Receiver<Job>, stopping: &AtomicBool) {
        let mut last_activity = Instant::now();
        let mut last_request = Instant::now();
        while !stopping.load(Ordering::SeqCst) {
            let indexing_due =
                self.service.has_indexing() && last_request.elapsed() >= ServerSettings::YIELD_AFTER_REQUEST;
            let wait = if indexing_due {
                Duration::ZERO
            } else {
                ServerSettings::POLL_INTERVAL
            };
            match inbox.recv_timeout(wait) {
                Ok(job) => {
                    // Only a search makes indexing wait: progress questions come every second while `s1grep index`
                    // follows a job, and pausing for them would stop the very job being followed.
                    let search = matches!(job.request, Request::Search(_));
                    self.answer(job);
                    last_activity = Instant::now();
                    if search {
                        last_request = last_activity;
                    }
                }
                Err(RecvTimeoutError::Timeout) if indexing_due => {
                    // Background indexing counts as activity: the process stays until every project is indexed.
                    // A failed step pauses before the next try instead of spinning on the same error.
                    match self.service.index_step() {
                        Ok(()) => last_activity = Instant::now(),
                        Err(error) => {
                            eprintln!("s1grep server: indexing: {error:#}");
                            std::thread::sleep(ServerSettings::INDEX_RETRY_PAUSE);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !self.service.has_indexing()
                        && let Some(idle) = self.idle
                        && last_activity.elapsed() >= idle
                    {
                        eprintln!(
                            "s1grep server: stopping after {} min without searches",
                            idle.as_secs() / 60
                        );
                        stopping.store(true, Ordering::SeqCst);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn answer(&mut self, job: Job) {
        let connection = Connection { stream: job.connection };
        if connection.is_closed() {
            return;
        }
        let started = Instant::now();
        let mut send_progress = |event: IndexEvent| {
            let _ = connection.send(&Reply::Progress { event });
        };
        let outcome = match &job.request {
            Request::Search(request) => self
                .service
                .search(request, &mut send_progress, &|| connection.is_closed())
                .map(|response| {
                    eprintln!(
                        "{:.2} s  {}  {:?}",
                        started.elapsed().as_secs_f64(),
                        request.target.display(),
                        request.query
                    );
                    Reply::Searched {
                        response: Box::new(response),
                    }
                }),
            Request::Index { target } => self
                .service
                .start_indexing(target, &mut send_progress)
                .map(|progress| Reply::Indexing { progress }),
            Request::Progress { target } => self
                .service
                .project_progress(target)
                .map(|progress| Reply::Indexing { progress }),
            Request::Ping | Request::Shutdown => Ok(Reply::Done),
        };
        let reply = match outcome {
            Ok(reply) => reply,
            Err(error) if error.downcast_ref::<SearchCancelled>().is_some() => return,
            Err(error) => Reply::Failed {
                error: ServerError::classify(&error),
            },
        };
        if let Err(error) = connection.send(&reply) {
            eprintln!("s1grep server: could not answer: {error:#}");
        }
    }
}

/// A connection to a running server.
pub struct ServerClient {
    pub info: ServerInfo,
}

impl ServerClient {
    /// The running server of this version, if there is one and it answers.
    pub fn connect() -> Option<Self> {
        Self::any().filter(|client| {
            client.info.version == env!("CARGO_PKG_VERSION") && client.info.protocol == ServerSettings::PROTOCOL_VERSION
        })
    }

    /// The running server of any version, if there is one and it answers within the ping timeout. It answers pings at
    /// once even while it searches or indexes.
    pub fn any() -> Option<Self> {
        let client = Self {
            info: ServerInfo::read()?,
        };
        match client.exchange(&Request::Ping, &mut |_| {}, Some(ServerSettings::PING_TIMEOUT)) {
            Ok(Reply::Pong) => Some(client),
            // An older process does not know this protocol but is alive: report it, so it can be stopped.
            Err(error) if error.downcast_ref::<ServerError>().is_some() => Some(client),
            _ => None,
        }
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
        match self.exchange(
            &Request::Search(request.clone()),
            progress,
            Some(ServerSettings::REPLY_TIMEOUT),
        )? {
            Reply::Searched { response } => Ok(*response),
            _ => bail!("the s1grep server sent an unexpected reply"),
        }
    }

    /// Asks the process to read a project and index what is missing in the background.
    pub fn index(&self, target: &Path, progress: &mut dyn FnMut(IndexEvent)) -> anyhow::Result<ProjectProgress> {
        let request = Request::Index {
            target: target.to_path_buf(),
        };
        match self.exchange(&request, progress, Some(ServerSettings::REPLY_TIMEOUT))? {
            Reply::Indexing { progress } => Ok(progress),
            _ => bail!("the s1grep server sent an unexpected reply"),
        }
    }

    /// How far a project is indexed.
    pub fn progress(&self, target: &Path) -> anyhow::Result<ProjectProgress> {
        let request = Request::Progress {
            target: target.to_path_buf(),
        };
        match self.exchange(&request, &mut |_| {}, Some(ServerSettings::REPLY_TIMEOUT))? {
            Reply::Indexing { progress } => Ok(progress),
            _ => bail!("the s1grep server sent an unexpected reply"),
        }
    }

    /// Asks the server to stop; if it does not within a few seconds (hung), terminates it.
    pub fn stop(&self) -> anyhow::Result<()> {
        let _ = self.exchange(&Request::Shutdown, &mut |_| {}, Some(ServerSettings::PING_TIMEOUT));
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

    /// Sends one request and reads the replies: progress lines go to `progress`, the last line is returned, and a
    /// failure comes back as a `ServerError`. `reply_timeout` bounds each wait for a line.
    fn exchange(
        &self,
        request: &Request,
        progress: &mut dyn FnMut(IndexEvent),
        reply_timeout: Option<Duration>,
    ) -> anyhow::Result<Reply> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.info.port));
        let stream = TcpStream::connect_timeout(&address, ServerSettings::CONNECT_TIMEOUT)?;
        stream.set_read_timeout(reply_timeout)?;
        let mut writer = &stream;
        let envelope = Envelope {
            token: self.info.token.clone(),
            protocol: ServerSettings::PROTOCOL_VERSION,
            request: request.clone(),
        };
        writer.write_all(serde_json::to_string(&envelope)?.as_bytes())?;
        writer.write_all(b"\n")?;
        for line in BufReader::new(&stream).lines() {
            match serde_json::from_str::<Reply>(&line?) {
                Ok(Reply::Progress { event }) => progress(event),
                Ok(Reply::Failed { error }) => return Err(error.into()),
                Ok(reply) => return Ok(reply),
                // An older process answers in its own format: it is alive but speaks another protocol.
                Err(_) => {
                    return Err(ServerError::new(
                        ErrorKind::ProtocolMismatch,
                        "the background process speaks an older protocol",
                    )
                    .into());
                }
            }
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
                "the models are not in {}: run `s1grep setup` to download them (about 2.9 GB, once)",
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
        // One older log is kept; a log over the limit becomes it, so the log never grows without bound.
        if std::fs::metadata(&log_path).is_ok_and(|metadata| metadata.len() > ServerSettings::LOG_MAXIMUM_BYTES) {
            let _ = std::fs::rename(&log_path, log_path.with_extension("log.1"));
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
