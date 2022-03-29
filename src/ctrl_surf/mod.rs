pub mod data;
pub use data::{Timecode, Track};

pub mod event;
pub use event::CtrlSurfEvent;

mod factory;
use factory::Buildable;
pub use factory::ControlSurfaceArc;

mod imp;

use once_cell::sync::Lazy;
use std::sync::Arc;

pub static FACTORY: Lazy<Arc<factory::Factory>> = Lazy::new(|| {
    factory::Factory::default()
        .with::<imp::XTouchOneMackie>()
        .into()
});

#[derive(Debug, Default)]
pub struct MidiMsgList(Vec<Vec<u8>>);

impl MidiMsgList {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn none() -> Self {
        Self(Vec::with_capacity(0))
    }

    pub fn push(&mut self, msg: impl Into<Vec<u8>>) {
        self.0.push(msg.into())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl IntoIterator for MidiMsgList {
    type Item = Vec<u8>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T: Into<Vec<u8>>> From<T> for MidiMsgList {
    fn from(msg: T) -> Self {
        Self(vec![msg.into()])
    }
}

#[derive(Debug, Default)]
pub struct Response {
    pub event: Option<event::CtrlSurfEvent>,
    pub msg_list: MidiMsgList,
}

impl Response {
    pub fn none() -> Self {
        Self {
            event: None,
            msg_list: MidiMsgList::none(),
        }
    }

    pub fn from(
        event: impl Into<event::CtrlSurfEvent>,
        msg_list: impl Into<MidiMsgList>,
    ) -> Response {
        Response {
            event: Some(event.into()),
            msg_list: msg_list.into(),
        }
    }

    pub fn from_event(event: impl Into<event::CtrlSurfEvent>) -> Response {
        Response {
            event: Some(event.into()),
            msg_list: MidiMsgList::none(),
        }
    }

    pub fn from_msg_list(msg_list: impl Into<MidiMsgList>) -> Response {
        Response {
            event: None,
            msg_list: msg_list.into(),
        }
    }

    pub fn into_inner(self) -> (Option<event::CtrlSurfEvent>, MidiMsgList) {
        (self.event, self.msg_list)
    }
}

pub trait ControlSurface: Send + 'static {
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Response;
    fn event_to_device(&mut self, event: event::Feedback) -> Response;
    fn reset(&mut self) -> MidiMsgList;
}
