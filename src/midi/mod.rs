mod error;
pub use error::Error;

mod io;

pub mod msg;
pub use msg::{Msg, MsgList};

pub mod port;
pub use port::{DirectionalPorts, PortsIn, PortsOut};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tag(u8);

impl Tag {
    pub const fn from(byte: u8) -> Self {
        Self(byte & 0xf0)
    }
}

impl From<Tag> for u8 {
    fn from(tag: Tag) -> u8 {
        tag.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Channel(u8);

impl Channel {
    pub const fn from(byte: u8) -> Self {
        Self(byte & 0x0f)
    }
}

impl From<Channel> for u8 {
    fn from(chan: Channel) -> u8 {
        chan.0
    }
}

impl std::ops::BitOr<Channel> for Tag {
    type Output = u8;

    fn bitor(self, chan: Channel) -> Self::Output {
        self.0 | chan.0
    }
}

pub mod u14 {
    use super::{msg, Error};

    pub const MAX: u16 = 0x3fff;

    #[inline]
    pub fn from_be(buf: &[u8]) -> Result<u16, Error> {
        if buf.len() != 2 {
            return Err(Error::InvalidTwoBytesValue(
                msg::Displayable::from(buf).to_owned(),
            ));
        }

        let (lsb, msb) = (buf[0], buf[1]);
        if lsb > 0x7f || msb > 0x7f {
            return Err(Error::InvalidTwoBytesValue(
                msg::Displayable::from(buf).to_owned(),
            ));
        }

        Ok(lsb as u16 + ((msb as u16) << 7))
    }

    #[inline]
    pub fn to_be(val: u16) -> Result<[u8; 2], Error> {
        if val > MAX {
            return Err(Error::InvalidU14(val));
        }

        Ok([val as u8 & 0x7f, (val >> 7) as u8])
    }
}

pub mod normalized_f64 {
    use super::Error;

    pub const MAX: f64 = 1f64;
    pub const QUANTUM: f64 = 1f64 / super::u14::MAX as f64;

    #[inline]
    pub fn from_be(buf: &[u8]) -> Result<f64, Error> {
        let val = super::u14::from_be(buf)?;

        Ok(val as f64 * QUANTUM)
    }

    #[inline]
    pub fn to_be(val: f64) -> Result<[u8; 2], Error> {
        if val > MAX {
            return Err(Error::InvalidNormalizedFloat(val));
        }

        let val = (super::u14::MAX as f64 * val) as u16;

        Ok([val as u8 & 0x7f, (val >> 7) as u8])
    }
}
