use std::io;

use tim_client::client::TimClientError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error(transparent)]
    Client(#[from] TimClientError),
}

pub type Result<T> = std::result::Result<T, Error>;
