use crate::models::ModelDirectory;
use crate::progress::IndexEvent;
use crate::server::BackgroundServer;
use crate::service::{SearchRequest, SearchResponse, SearchService};

/// Where a search runs: in the background process, started on demand so the models stay loaded between searches, or
/// in this process when that is turned off with `--no-server` or cannot start.
pub struct SearchBackend {
    models: ModelDirectory,
    threads: Option<usize>,
    use_server: bool,
    local: Option<SearchService>,
}

/// Which side answered, for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answered {
    Server,
    Local,
}

impl SearchBackend {
    pub fn new(models: ModelDirectory, threads: Option<usize>, use_server: bool) -> Self {
        Self {
            models,
            threads,
            use_server,
            local: None,
        }
    }

    pub fn search(
        &mut self,
        request: &SearchRequest,
        progress: &mut dyn FnMut(IndexEvent),
    ) -> anyhow::Result<(SearchResponse, Answered)> {
        if self.use_server
            && let Some(client) = BackgroundServer::ensure(&self.models, progress)
        {
            return Ok((client.search(request, progress)?, Answered::Server));
        }
        if self.local.is_none() {
            progress(IndexEvent::LoadingModels);
            self.local = Some(SearchService::load(&self.models, true, self.threads)?);
        }
        let service = self.local.as_mut().expect("loaded above");
        Ok((service.search(request, progress)?, Answered::Local))
    }
}
