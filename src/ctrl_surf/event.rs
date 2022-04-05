use std::sync::Arc;

#[derive(Debug)]
pub enum CtrlSurfEvent {
    Transport(Transport),
    Mixer(Mixer),
    DataRequest,
}

#[derive(Debug)]
pub enum AppEvent {
    Transport(Transport),
    Mixer(Mixer),
    Data(Data),
    NewApp(Arc<str>),
}

#[derive(Debug)]
pub enum Transport {
    Play,
    Pause,
    PlayPause,
    Stop,
    Previous,
    Next,
    StepForward,
    StepBackward,
}

impl From<Transport> for CtrlSurfEvent {
    fn from(evt: Transport) -> Self {
        Self::Transport(evt)
    }
}

impl From<Transport> for AppEvent {
    fn from(evt: Transport) -> Self {
        Self::Transport(evt)
    }
}

#[derive(Debug)]
pub enum Mixer {
    Volume(f64),
    Mute,
}

impl From<Mixer> for CtrlSurfEvent {
    fn from(evt: Mixer) -> Self {
        Self::Mixer(evt)
    }
}

impl From<Mixer> for AppEvent {
    fn from(evt: Mixer) -> Self {
        Self::Mixer(evt)
    }
}

#[derive(Debug)]
pub enum Data {
    Track(super::Track),
    Timecode(super::Timecode),
}

impl From<Data> for AppEvent {
    fn from(evt: Data) -> Self {
        Self::Data(evt)
    }
}

impl From<super::Track> for Data {
    fn from(track: super::Track) -> Self {
        Self::Track(track)
    }
}

impl From<super::Track> for AppEvent {
    fn from(track: super::Track) -> Self {
        AppEvent::from(Data::from(track))
    }
}

impl From<super::Timecode> for Data {
    fn from(tc: super::Timecode) -> Self {
        Self::Timecode(tc)
    }
}

impl From<super::Timecode> for AppEvent {
    fn from(tc: super::Timecode) -> Self {
        AppEvent::from(Data::from(tc))
    }
}
