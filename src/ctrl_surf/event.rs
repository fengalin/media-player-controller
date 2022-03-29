use std::sync::Arc;

#[derive(Debug)]
pub enum CtrlSurfEvent {
    Transport(Transport),
    Mixer(Mixer),
}

#[derive(Debug)]
pub enum Feedback {
    Transport(Transport),
    Mixer(Mixer),
    Data(Data),
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

impl From<Transport> for Feedback {
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

impl From<Mixer> for Feedback {
    fn from(evt: Mixer) -> Self {
        Self::Mixer(evt)
    }
}

#[derive(Debug)]
pub enum Data {
    Track(super::Track),
    Timecode(super::Timecode),
    Player(Arc<str>),
}

impl From<Data> for Feedback {
    fn from(evt: Data) -> Self {
        Self::Data(evt)
    }
}

impl From<super::Track> for Data {
    fn from(track: super::Track) -> Self {
        Self::Track(track)
    }
}

impl From<super::Track> for Feedback {
    fn from(track: super::Track) -> Self {
        Feedback::from(Data::from(track))
    }
}

impl From<super::Timecode> for Data {
    fn from(tc: super::Timecode) -> Self {
        Self::Timecode(tc)
    }
}

impl From<super::Timecode> for Feedback {
    fn from(tc: super::Timecode) -> Self {
        Feedback::from(Data::from(tc))
    }
}
