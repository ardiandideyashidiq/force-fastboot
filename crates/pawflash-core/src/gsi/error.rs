use thiserror::Error;

#[derive(Error, Debug)]
pub enum GsiError {
    #[error("{0}")]
    Flash(#[from] crate::flash::error::FlashError),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("image check failed: {0}")]
    ImageCheck(String),

    #[error("format tools: {0}")]
    FormatTools(String),

    #[error("GSI flash cancelled by user")]
    Cancelled,

    #[error("partition resolution: {0}")]
    PartitionResolution(String),

    #[error("mode transition failed: {0}")]
    Transition(String),

    #[error("sparse header: {0}")]
    SparseHeader(String),
}

pub type Result<T> = std::result::Result<T, GsiError>;
