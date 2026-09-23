use std::fmt;

/// Errors are kept as owned data so they stay `Send + Sync` on wasm32, where the
/// underlying `fetch` errors are not.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The request could not be sent or its reply could not be read.
    Http(String),
    /// The endpoint answered with a non-success status; `message` is its error message, or the start
    /// of the body when it has none.
    Status { status: u16, message: String },
    /// A request could not be encoded or a reply could not be decoded.
    Decode(String),
    /// The same question id was given twice, so one question would have replaced the other.
    DuplicateQuestion(String),
    /// A question cannot be answered as asked: too many options, an option twice, no levels.
    Invalid { id: String, why: String },
    /// The reply has no answer under this question id.
    MissingAnswer(String),
    /// The answer under `id` is of another type than the one asked for.
    WrongType { id: String, expected: &'static str, found: String },
    /// The answer under `id` gives no probability for one of its levels or options, which rules
    /// need: reading it as zero would be a confident no that nothing said.
    MissingProbability { id: String, label: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(message) => write!(f, "HTTP error: {message}"),
            Error::Status { status, message } => write!(f, "decisions endpoint returned HTTP {status}: {message}"),
            Error::Decode(message) => write!(f, "invalid message: {message}"),
            Error::DuplicateQuestion(id) => write!(f, "question `{id}` is asked twice"),
            Error::Invalid { id, why } => write!(f, "question `{id}`: {why}"),
            Error::MissingAnswer(id) => write!(f, "no answer for question `{id}`"),
            Error::WrongType { id, expected, found } => write!(f, "question `{id}` is a {found}, not a {expected}"),
            Error::MissingProbability { id, label } => write!(f, "question `{id}` gives no probability for `{label}`"),
        }
    }
}

impl std::error::Error for Error {}
