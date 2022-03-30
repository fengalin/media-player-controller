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

#[inline]
pub fn be_to_u16(buf: &[u8]) -> Result<u16, Error> {
    if buf.len() != 2 {
        return Err(Error::InvalidValue);
    }

    let (lsb, msb) = (buf[0], buf[1]);
    if lsb > 0x7f || msb > 0x7f {
        return Err(Error::InvalidValue);
    }

    Ok(lsb as u16 + ((msb as u16) << 7))
}

#[inline]
pub fn u16_to_be(val: u16) -> [u8; 2] {
    if val > 0x3fff {
        return [0x7f, 0x7f];
    }

    [val as u8 & 0x7f, (val >> 7) as u8]
}
