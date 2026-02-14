use std::io;

/// Basic error type for the library. Expand as the rewrite progresses.
#[derive(Debug)]
pub enum MDictError {
    Io(io::Error),
    InvalidFormat(String),
    UnsupportedFeature(String),
}

impl From<io::Error> for MDictError {
    fn from(e: io::Error) -> Self {
        MDictError::Io(e)
    }
}

impl From<binrw::Error> for MDictError {
    fn from(e: binrw::Error) -> Self {
        MDictError::InvalidFormat(e.to_string())
    }
}

impl From<&'static str> for MDictError {
    fn from(s: &'static str) -> Self {
        MDictError::InvalidFormat(s.to_string())
    }
}

impl From<String> for MDictError {
    fn from(s: String) -> Self {
        MDictError::InvalidFormat(s)
    }
}

pub type Result<T> = std::result::Result<T, MDictError>;
