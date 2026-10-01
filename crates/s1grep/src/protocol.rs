use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::progress::IndexEvent;
use crate::service::{ProjectProgress, SearchRequest, SearchResponse};

/// What a client asks the background process; one per connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    /// Answered at once, even while a search or an indexing step runs.
    Ping,
    /// Stops the process after the request in hand.
    Shutdown,
    Search(SearchRequest),
    /// Reads the project and hands what is missing to background indexing; answers with its progress at once.
    Index {
        target: PathBuf,
    },
    /// How far a project is indexed.
    Progress {
        target: PathBuf,
    },
}

/// The first line of every connection: who asks, in which protocol version, and what.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub token: String,
    pub protocol: u32,
    pub request: Request,
}

/// One line from the background process. Progress lines may come first; exactly one other line ends the exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Progress { event: IndexEvent },
    Pong,
    Searched { response: Box<SearchResponse> },
    Indexing { progress: ProjectProgress },
    Done,
    Failed { error: ServerError },
}

/// Why a request failed, so that clients can say what to do about it instead of passing on a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The token does not match: the client read another user's or a stale `server.json`.
    Unauthorized,
    /// Client and process speak different protocol versions; the client restarts the process.
    ProtocolMismatch,
    /// The request could not be read.
    InvalidRequest,
    /// The searched folder does not exist or is not a folder.
    NotFound,
    /// A model bundle is missing; `s1grep setup` downloads it.
    ModelsMissing,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ServerError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The kind of a failure inside the process, from the typed errors it may carry.
    pub fn classify(error: &anyhow::Error) -> Self {
        let kind = if error.downcast_ref::<crate::project::MissingFolder>().is_some() {
            ErrorKind::NotFound
        } else if error.downcast_ref::<crate::models::MissingModel>().is_some() {
            ErrorKind::ModelsMissing
        } else {
            ErrorKind::Internal
        };
        Self::new(kind, format!("{error:#}"))
    }
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ServerError {}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Envelope, ErrorKind, Reply, Request, ServerError};
    use crate::project::MissingFolder;

    #[test]
    fn requests_and_replies_round_trip_as_tagged_json() {
        let envelope = Envelope {
            token: "secret".to_string(),
            protocol: 2,
            request: Request::Progress {
                target: PathBuf::from("/work/shop"),
            },
        };
        let line = serde_json::to_string(&envelope).unwrap();
        assert!(line.contains(r#""kind":"progress""#));
        let read: Envelope = serde_json::from_str(&line).unwrap();
        assert!(matches!(read.request, Request::Progress { target } if target == Path::new("/work/shop")));
        let reply = Reply::Failed {
            error: ServerError::new(ErrorKind::NotFound, "gone"),
        };
        let line = serde_json::to_string(&reply).unwrap();
        assert!(line.contains(r#""kind":"not_found""#));
        assert!(
            matches!(serde_json::from_str::<Reply>(&line).unwrap(), Reply::Failed { error } if error.kind == ErrorKind::NotFound)
        );
    }

    #[test]
    fn a_missing_folder_is_reported_as_not_found() {
        let error = MissingFolder::resolve(&PathBuf::from("/no/such/folder/for/s1grep")).unwrap_err();
        assert_eq!(ServerError::classify(&error).kind, ErrorKind::NotFound);
    }
}
