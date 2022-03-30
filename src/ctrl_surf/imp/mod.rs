mod xtouch_one_mackie;
pub use xtouch_one_mackie::XTouchOneMackie;

pub trait ControlSurface: Send + 'static {
    fn start_identification(&mut self) -> super::Response;
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> super::Response;
    fn event_to_device(&mut self, event: super::event::Feedback) -> super::Response;
    fn reset(&mut self) -> crate::midi::MsgList;
}
