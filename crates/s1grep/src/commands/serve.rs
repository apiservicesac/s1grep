use std::time::Instant;

use clap::Args;

use crate::models::ModelDirectory;
use crate::server::SearchServer;
use crate::service::SearchService;

#[derive(Args)]
pub struct ServeCommand {
    /// Port on 127.0.0.1 (default: any free port; searches find it on their own)
    #[arg(long, default_value_t = 0)]
    port: u16,
    #[arg(long, hide = true)]
    threads: Option<usize>,
    #[command(flatten)]
    models: ModelDirectory,
}

impl ServeCommand {
    /// Loads the models once and answers searches until stopped with Ctrl+C.
    pub fn run(self) -> anyhow::Result<()> {
        let started = Instant::now();
        let service = SearchService::load(&self.models, true, self.threads)?;
        let server = SearchServer::start(service, self.port)?;
        eprintln!(
            "s1grep serve: models loaded in {:.1} s, listening on 127.0.0.1:{}. Searches now use it; Ctrl+C stops it.",
            started.elapsed().as_secs_f64(),
            server.port()
        );
        server.run()
    }
}
