//! s1grep as a library: the search service, the background process and the commands, shared by the `s1grep`
//! binary and the `s1-lab` development tools.

pub mod backend;
pub mod cache;
pub mod commands;
pub mod hub;
pub mod indexer;
pub mod models;
pub mod progress;
pub mod project;
pub mod protocol;
pub mod query;
pub mod report;
pub mod runtime_library;
pub mod searcher;
pub mod server;
pub mod service;
pub mod session;
pub mod settings;
