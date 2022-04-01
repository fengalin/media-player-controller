use crate::{bytes, midi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Device identification failed: {}", .0)]
    InitFailure(#[from] midi::Error),

    #[error("Unexpected device message: {}", .0)]
    UnexpectedDeviceMsg(bytes::Displayable<'static>),

    #[error("Manufacturer id mismatch expected {expected}, found {found}")]
    ManufacturerMismatch {
        expected: bytes::Displayable<'static>,
        found: bytes::Displayable<'static>,
    },

    #[error("Device reported connection error")]
    ConnectionError,
}
