use crate::{bytes, midi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Device identification failed: {}", .0)]
    InitFailure(#[from] midi::Error),

    #[error("Unexpected device response: {}", .0)]
    UnexpectedDeviceResponse(bytes::Displayable<'static>),

    #[error("Manufacturer id mismatch expected {expected}, found {found}")]
    ManufacturerMismatch {
        expected: bytes::Displayable<'static>,
        found: bytes::Displayable<'static>,
    },

    #[error("Device mismatch expected id {expected:02x}, found {found:02x}")]
    DeviceMismatch { expected: u8, found: u8 },

    #[error("Unexpected device status expected {expected:02x}, found {found:02x}")]
    UnexpectedDeviceStatus { expected: u8, found: u8 },
}
