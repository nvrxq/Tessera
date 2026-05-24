use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid workspace name: {0}")]
    InvalidName(String),
}
