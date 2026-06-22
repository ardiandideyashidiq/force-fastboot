use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to open serial port `{port}`")]
    OpenSerialPort {
        port: String,
        #[source]
        source: tokio_serial::Error,
    },

    #[error("serial port enumeration failed: {0}")]
    PortEnumeration(#[from] tokio_serial::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
