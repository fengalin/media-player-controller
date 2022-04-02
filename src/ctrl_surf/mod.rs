pub mod data;
pub use data::{Timecode, Track};

mod device;

pub mod error;
pub use error::Error;

pub mod event;
pub use event::CtrlSurfEvent;

mod factory;
use factory::Buildable;
pub use factory::{ControlSurfaceArc, FACTORY};

pub mod msg;
pub use msg::Msg;

mod protocol;

pub trait ControlSurface: Send + 'static {
    fn start_identification(&mut self) -> Vec<Msg>;

    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Vec<Msg>;
    fn event_to_device(&mut self, event: event::Feedback) -> Vec<Msg>;

    fn is_connected(&self) -> bool;
    fn reset(&mut self) -> Vec<Msg>;
}
