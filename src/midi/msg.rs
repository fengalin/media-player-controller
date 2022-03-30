use std::{borrow::Cow, fmt};

use super::Error;

pub type Result = std::result::Result<Msg, Error>;

#[derive(Debug, Default)]
pub struct Msg(Box<[u8]>);

impl Msg {
    pub fn into_inner(self) -> Box<[u8]> {
        self.0
    }

    pub fn display(&self) -> Displayable {
        Displayable::from(self.0.as_ref())
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

#[derive(Debug)]
pub struct Displayable<'a>(Cow<'a, [u8]>);

impl<'a> From<&'a [u8]> for Displayable<'a> {
    fn from(msg: &'a [u8]) -> Self {
        Self(Cow::Borrowed(msg))
    }
}

impl From<Box<[u8]>> for Displayable<'static> {
    fn from(msg: Box<[u8]>) -> Self {
        Self(Cow::Owned(msg.into()))
    }
}

impl<'a> Displayable<'a> {
    pub fn to_owned(&self) -> Displayable<'static> {
        Displayable::from(Box::<[u8]>::from(self.0.as_ref()))
    }
}

impl<'a> fmt::Display for Displayable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut iter = self.0.iter();

        match iter.next() {
            Some(first) => write!(f, "(hex): {:02x}", first)?,
            None => return Ok(()),
        };

        for val in iter {
            write!(f, ", {:02x}", val)?;
        }

        Ok(())
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
