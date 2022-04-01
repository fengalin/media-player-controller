use super::CtrlSurfEvent;
use crate::midi;

#[derive(Debug)]
pub enum Msg {
    ToApp(CtrlSurfEvent),
    ToDevice(midi::Msg),
    ConnectionStatus(ConnectionStatus),
}

impl Msg {
    pub fn none() -> Vec<Msg> {
        Vec::with_capacity(0)
    }

    pub fn connetion_in_progress() -> Self {
        Self::ConnectionStatus(ConnectionStatus::InProgress)
    }

    pub fn from_connection_result(res: Result<(), super::Error>) -> Self {
        Self::ConnectionStatus(ConnectionStatus::Result(res))
    }
}

impl super::event::CtrlSurfEvent {
    pub fn to_app(self) -> Msg {
        Msg::ToApp(self)
    }
}

impl super::event::Transport {
    pub fn to_app(self) -> Msg {
        Msg::ToApp(self.into())
    }
}

impl super::event::Mixer {
    pub fn to_app(self) -> Msg {
        Msg::ToApp(self.into())
    }
}

impl midi::Msg {
    pub fn to_device(self) -> Msg {
        Msg::ToDevice(self)
    }
}

impl From<Msg> for Vec<Msg> {
    fn from(msg: Msg) -> Vec<Msg> {
        vec![msg]
    }
}

impl<T: Into<CtrlSurfEvent>> From<T> for Msg {
    fn from(event: T) -> Self {
        Self::ToApp(event.into())
    }
}

impl<const S: usize> From<[u8; S]> for Msg {
    fn from(msg: [u8; S]) -> Self {
        Self::ToDevice(msg.into())
    }
}

#[derive(Debug)]
pub enum ConnectionStatus {
    InProgress,
    Result(Result<(), super::Error>),
}

impl ConnectionStatus {
    pub fn in_progres() -> Self {
        Self::InProgress
    }

    pub fn from_res(res: Result<(), super::Error>) -> Self {
        Self::Result(res)
    }
}
