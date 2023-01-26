use std::{borrow::Cow, fmt};

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
            Some(first) => write!(f, "(hex): {first:02x}")?,
            None => return Ok(()),
        };

        for val in iter {
            write!(f, ", {val:02x}")?;
        }

        Ok(())
    }
}
