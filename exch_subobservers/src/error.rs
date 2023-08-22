use std::fmt::Display;
use exch_observer_types::{
    ExchangeKind
};
use thiserror::Error;

/// Enum of errors that can occur in the Observer
#[derive(Error, Debug)]
pub enum ObserverError {
    #[error("Exchange not found: {0}")]
    ExchangeNotFound(ExchangeKind),

    #[error("Symbol not found: {0} {1}")]
    SymbolNotFound(ExchangeKind, String),
}

/// Result type for Observer
pub type ObserverResult<T> = Result<T, ObserverError>;
