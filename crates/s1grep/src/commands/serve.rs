use std::time::Instant;

use clap::Args;

use crate::models::ModelDirectory;
use crate::server::SearchServer;
use crate::service::SearchService;
use crate::settings::ServerSettings;

#[derive(Args)]
pub struct ServeCommand {
    /// Port on 127.0.0.1 (default: any free port; searches find it on their own)
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Started by a search: no console, and it stops after 30 minutes without searches
    #[arg(long, hide = true)]
    background: bool,
    #[arg(long, hide = true)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl ServeCommand {
    /// Loads the models once and answers searches; in the foreground until Ctrl+C, in the background until idle.
    pub fn run(self) -> anyhow::Result<()> {
        let started = Instant::now();
        let lock = SearchServer::acquire_lock()?;
        let service = SearchService::load(&self.models, true, self.threads)?;
        let idle = self.background.then_some(ServerSettings::IDLE);
        let server = SearchServer::start(service, lock, self.port, idle)?;
        eprintln!(
            "s1grep: models loaded in {:.1} s, listening on 127.0.0.1:{}{}",
            started.elapsed().as_secs_f64(),
            server.port(),
            if self.background { "" } else { ". Ctrl+C stops it." }
        );
        server.run()
    }
}
