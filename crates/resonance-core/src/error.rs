//! The error type shared by the core library, the CLI and the MCP server.

use std::fmt;

/// Result alias used throughout `resonance-core`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong in Resonance, phrased for someone who does not know its internals.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Not inside a git repository (MVP-009).
    #[error(
        "not inside a git repository: {0}\nResonance runs inside a git repository; run it from within one, or pass -C <path>."
    )]
    NotARepository(String),

    /// The repository has no `.resonance/` yet.
    #[error("Resonance is not set up in this repository yet; run `resonance init` first")]
    NotInitialized,

    /// The store was written by a newer Resonance (FR-258).
    #[error(
        "this repository's graph uses schema version {found}, but this resonance binary only understands up to version {supported}; install a newer resonance to use it"
    )]
    NewerSchema { found: u32, supported: u32 },

    /// A reference matched nothing.
    #[error("nothing matches {0}")]
    NotFound(String),

    /// A reference matched more than one thing.
    #[error("{reference} is ambiguous; it could mean:\n{}", .candidates.iter().map(|c| format!("  {c}")).collect::<Vec<_>>().join("\n"))]
    Ambiguous {
        reference: String,
        candidates: Vec<String>,
    },

    /// The request is well-formed but not allowed or not meaningful.
    #[error("{0}")]
    Invalid(String),

    /// The request itself is malformed (exit code 2).
    #[error("{0}")]
    Usage(String),

    /// A git command failed.
    #[error("git: {0}")]
    Git(String),

    /// A storage engine failure, already translated into engine-neutral terms (FR-161).
    #[error("store: {0}")]
    Store(String),

    /// A file system failure.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    /// Configuration could not be read.
    #[error("configuration: {0}")]
    Config(String),

    /// An error reported by the daemon on behalf of a request, with the exit code it maps to.
    #[error("{message}")]
    Remote { message: String, code: i32 },

    /// The daemon could not be reached, started or kept running.
    #[error("daemon: {0}")]
    Daemon(String),

    /// Another daemon already serves this repository (FR-082).
    #[error("a daemon is already running for this repository (pid {0})")]
    AlreadyRunning(String),
}

impl Error {
    /// Wrap an I/O error with what was being attempted.
    pub fn io(context: impl fmt::Display, source: std::io::Error) -> Self {
        Error::Io {
            context: context.to_string(),
            source,
        }
    }

    /// The process exit code for this error (FR-253): 2 for usage errors, 1 otherwise.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) => 2,
            Error::Remote { code, .. } => *code,
            _ => 1,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Store(format!("malformed JSON: {e}"))
    }
}
