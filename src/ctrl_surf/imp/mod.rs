mod mackie;
pub use mackie::Mackie;

pub trait ControlSurface: Send + 'static {
    fn start_identification(&mut self) -> Vec<super::Msg>;

    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Vec<super::Msg>;
    fn event_to_device(&mut self, event: super::event::Feedback) -> Vec<super::Msg>;

    fn is_connected(&self) -> bool;
    fn reset(&mut self) -> Vec<super::Msg>;
}
