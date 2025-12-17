use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("tim connect error: {0}")]
    TimConnect(#[from] tonic::transport::Error),

    #[error("tim grpc error: {0}")]
    TimGrpc(#[from] tonic::Status),

    #[error("missing session in response")]
    MissingSession,

    #[error("invalid session metadata: {0}")]
    SessionMetadata(#[from] tonic::metadata::errors::InvalidMetadataValue),
}

pub type Result<T> = std::result::Result<T, Error>;
