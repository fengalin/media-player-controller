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

use crate::midi;

pub static FACTORY: Lazy<Arc<factory::Factory>> = Lazy::new(|| {
    factory::Factory::default()
        .with::<imp::XTouchOneMackie>()
        .into()
});

#[derive(Debug, Default)]
pub struct Response {
    pub event: Option<event::CtrlSurfEvent>,
    pub msg_list: midi::MsgList,
}

impl Response {
    pub fn none() -> Self {
        Self {
            event: None,
            msg_list: midi::MsgList::none(),
        }
    }

    pub fn from(
        event: impl Into<event::CtrlSurfEvent>,
        msg_list: impl Into<midi::MsgList>,
    ) -> Response {
        Response {
            event: Some(event.into()),
            msg_list: msg_list.into(),
        }
    }

    pub fn from_event(event: impl Into<event::CtrlSurfEvent>) -> Response {
        Response {
            event: Some(event.into()),
            msg_list: midi::MsgList::none(),
        }
    }

    pub fn from_msg_list(msg_list: impl Into<midi::MsgList>) -> Response {
        Response {
            event: None,
            msg_list: msg_list.into(),
        }
    }

    pub fn from_msg(msg: impl Into<midi::Msg>) -> Response {
        let msg: midi::Msg = msg.into();
        Response {
            event: None,
            msg_list: msg.into(),
        }
    }

    pub fn into_inner(self) -> (Option<event::CtrlSurfEvent>, midi::MsgList) {
        (self.event, self.msg_list)
    }
}

pub trait ControlSurface: Send + 'static {
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Response;
    fn event_to_device(&mut self, event: event::Feedback) -> Response;
    fn reset(&mut self) -> midi::MsgList;
}
