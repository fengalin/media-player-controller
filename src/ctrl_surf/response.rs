use super::CtrlSurfEvent;
use crate::midi;

#[derive(Debug, Default)]
pub struct Response {
    pub event: Option<CtrlSurfEvent>,
    pub msg_list: midi::MsgList,
}

impl Response {
    pub fn none() -> Self {
        Self {
            event: None,
            msg_list: midi::MsgList::none(),
        }
    }

    pub fn from(event: impl Into<CtrlSurfEvent>, msg_list: impl Into<midi::MsgList>) -> Response {
        Response {
            event: Some(event.into()),
            msg_list: msg_list.into(),
        }
    }

    pub fn from_event(event: impl Into<CtrlSurfEvent>) -> Response {
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

    pub fn into_inner(self) -> (Option<CtrlSurfEvent>, midi::MsgList) {
        (self.event, self.msg_list)
    }
}
