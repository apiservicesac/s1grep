use crate::models::ModelDirectory;
use crate::server::ServerClient;
use crate::service::{SearchRequest, SearchResponse, SearchService};

/// Where a search runs: in a running `s1grep serve` when there is one, otherwise in this process, loading the models
/// on first use.
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

    pub fn search(&mut self, request: &SearchRequest) -> anyhow::Result<(SearchResponse, Answered)> {
        if self.use_server
            && let Some(client) = ServerClient::connect()
        {
            return Ok((client.search(request)?, Answered::Server));
        }
        if self.local.is_none() {
            self.local = Some(SearchService::load(&self.models, true, self.threads)?);
        }
        let service = self.local.as_mut().expect("loaded above");
        Ok((service.search(request)?, Answered::Local))
    }
}
