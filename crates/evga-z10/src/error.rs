use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the EVGA Z10 RGB is not connected")]
    NotFound,
    #[cfg(feature = "hid")]
    #[error("could not talk to the keyboard: {0}")]
    Hid(#[from] hidapi::HidError),
    /// as Hid, from a connection the caller supplies
    #[error("could not talk to the keyboard: {0}")]
    Link(String),
    #[error("the keyboard answered {status:#04x} to {what}")]
    Refused { what: String, status: u8 },
    #[error("profile checksum mismatch: the layout is wrong")]
    Checksum,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::Invalid(msg.into())
}
