//! One error type for the engine.

use crate::backend::BackendError;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EngineError {
    #[error("secret store: {0}")]
    Secret(String),
    #[error(transparent)]
    Net(#[from] scrin_net::NetError),
    #[error(transparent)]
    Crypto(#[from] scrin_crypto::Error),
    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error("frame sharding: {0}")]
    Fec(#[from] scrin_media::fec::FecError),
    #[error("could not resolve scrin id {0}")]
    Resolve(String),
    #[error("invalid connect target: {0}")]
    InvalidTarget(&'static str),
    #[error("no session {0}")]
    UnknownSession(String),
    #[error("invalid argument: {0}")]
    Invalid(&'static str),
    #[error("protocol violation: {0}")]
    Protocol(&'static str),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("timed out")]
    Timeout,
    #[error("the engine has stopped")]
    Stopped,
}

pub type Result<T> = std::result::Result<T, EngineError>;
