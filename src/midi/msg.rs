use std::{error, fmt};

pub type Result = std::result::Result<Msg, Error>;

impl TryFrom<(u64, &[u8])> for Msg {
    type Error = self::Error;

    fn try_from(ts_buf: (u64, &[u8])) -> Result {
        match midi_msg::MidiMsg::from_midi(ts_buf.1) {
            Ok((msg, _len)) => Ok(Msg {
                ts: ts_buf.0,
                inner: msg,
            }),
            Err(err) => Err(Error { ts: ts_buf.0, err }),
        }
    }
}

#[derive(Debug)]
pub struct Msg {
    pub ts: u64,
    pub inner: midi_msg::MidiMsg,
}

#[derive(Debug)]
pub struct Error {
    pub ts: u64,
    pub err: midi_msg::ParseError,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} @ {}", self.err, self.ts)
    }
}

impl error::Error for Error {}
