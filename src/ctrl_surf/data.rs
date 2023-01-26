use std::{fmt, io::Write, sync::Arc, time::Duration};

#[derive(Debug)]
pub struct Track {
    pub artist: Option<Arc<str>>,
    pub album: Option<Arc<str>>,
    pub title: Option<Arc<str>>,
    pub duration: Option<Duration>,
    pub image_url: Option<Arc<str>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timecode {
    pub h: u16,
    pub m: u8,
    pub s: u8,
    pub ms: u16,
}

impl From<std::time::Duration> for Timecode {
    fn from(dur: std::time::Duration) -> Self {
        let ms_total = dur.as_nanos() / 1_000_000;
        let s_total = ms_total / 1_000;
        let m_total = s_total / 60;

        Self {
            ms: (ms_total % 1_000) as u16,
            s: (s_total % 60) as u8,
            m: (m_total % 60) as u8,
            h: (m_total / 60) as u16,
        }
    }
}

struct TimecodeUnpadded(Timecode);

impl fmt::Display for TimecodeUnpadded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        if self.0.h > 0 {
            write!(f, "{}:{:02}:{:02}", self.0.h, self.0.m, self.0.s)?;
        } else {
            write!(f, "{}:{:02}", self.0.m, self.0.s)?;
        }

        let p = f.precision().unwrap_or(0);
        if p > 0 {
            let mut buf = [b'0'; 3];
            write!(buf.as_mut_slice(), "{:03}", self.0.ms).unwrap();
            let buf_str = std::str::from_utf8(&buf[..p]).unwrap();
            write!(f, ".{buf_str}")?;
        }

        Ok(())
    }
}

impl fmt::Display for Timecode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let width = match f.width() {
            Some(width) => width,
            None => {
                return TimecodeUnpadded(*self).fmt(f);
            }
        };

        const MAX_SIZE: usize = "65535:59:59.999".len();
        let mut buf = [0u8; MAX_SIZE];
        let mut cur = std::io::Cursor::new(buf.as_mut_slice());

        write!(
            cur,
            "{:.p$}",
            TimecodeUnpadded(*self),
            p = f.precision().unwrap_or(0),
        )
        .unwrap();

        let len = cur.position() as usize;
        let buf_str = std::str::from_utf8(&buf[..len]).unwrap();

        for _ in 0..width.saturating_sub(len) {
            write!(f, "{}", f.fill())?;
        }

        write!(f, "{buf_str}")
    }
}

#[derive(Clone, Copy, Debug)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

impl PlaybackStatus {
    pub fn is_playing(self) -> bool {
        matches!(self, Self::Playing)
    }
}
