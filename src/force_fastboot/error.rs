use thiserror::Error;

/// Errors that can occur during force-fastboot operation.
#[derive(Error, Debug)]
pub enum Error {
    /// The serial port could not be opened at the given path.
    #[error("failed to open serial port `{port}`")]
    OpenSerialPort {
        /// Path to the serial port device.
        port: String,
        /// Underlying I/O or driver error.
        #[source]
        source: serialport::Error,
    },

    /// Enumeration of available serial ports failed.
    #[error("serial port enumeration failed: {0}")]
    PortEnumeration(#[from] serialport::Error),
}

/// Convenience alias for `Result<T, error::Error>`.
pub type Result<T> = std::result::Result<T, Error>;
