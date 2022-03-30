use super::Error;
use crate::bytes;

pub type Result = std::result::Result<Msg, Error>;

#[derive(Debug, Default)]
pub struct Msg(Box<[u8]>);

impl Msg {
    pub fn inner(&self) -> &[u8] {
        self.0.as_ref()
    }

    pub fn display(&self) -> bytes::Displayable {
        bytes::Displayable::from(self.0.as_ref())
    }

    pub fn new_sysex(data: &[u8]) -> Self {
        use super::sysex;

        let mut buf = Vec::with_capacity(data.len() + 2);

        buf.push(sysex::TAG.into());
        buf.extend(data);
        buf.push(sysex::END_TAG.into());

        Self(buf.into())
    }

    pub fn try_get_sysex_data(&self) -> std::result::Result<&[u8], Error> {
        use super::sysex;

        if self.0.len() < 3 {
            return Err(Error::InvalidSysExInitTag(self.display().to_owned()));
        }

        if *self.0.first().unwrap() != sysex::TAG {
            return Err(Error::InvalidSysExInitTag(self.display().to_owned()));
        }

        if *self.0.last().unwrap() != sysex::END_TAG {
            return Err(Error::InvalidSysExFinalTag(self.display().to_owned()));
        }

        Ok(&self.0[1..self.0.len() - 1])
    }
}

impl<const S: usize> From<[u8; S]> for Msg {
    fn from(buf: [u8; S]) -> Self {
        Self(buf.into())
    }
}

impl From<&[u8]> for Msg {
    fn from(buf: &[u8]) -> Self {
        Self(buf.into())
    }
}

impl std::ops::Deref for Msg {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

#[derive(Debug, Default)]
pub struct MsgList(Vec<Msg>);

impl MsgList {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn none() -> Self {
        Self(Vec::with_capacity(0))
    }

    pub fn push(&mut self, msg: impl Into<Msg>) {
        self.0.push(msg.into())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl IntoIterator for MsgList {
    type Item = Msg;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: Into<Msg>> From<T> for MsgList {
    fn from(msg: T) -> Self {
        Self(vec![msg.into()])
    }
}
