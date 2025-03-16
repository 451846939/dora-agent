use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolsError {
    #[error("search error: {0}")]
    SearchError(String),
    #[error("cursor error: {0}")]
    CursorError(String),
    #[error("not found")]
    NotFound,
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("unknown error")]
    Unknown,
}