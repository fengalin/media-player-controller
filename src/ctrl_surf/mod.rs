pub mod data;
pub use data::{Timecode, Track};

mod device;

pub mod error;
pub use error::Error;

pub mod event;
pub use event::{AppEvent, CtrlSurfEvent};

mod factory;
use factory::Buildable;
pub use factory::{ControlSurfaceArc, FACTORY};

pub mod msg;
pub use msg::Msg;

mod protocol;

pub trait ControlSurface: Send + 'static {
    #[must_use]
    fn start_connection(&mut self) -> Vec<Msg>;

    #[must_use]
    fn abort_connection(&mut self) -> Vec<Msg>;

    #[must_use]
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Vec<Msg>;

    #[must_use]
    fn event_from_app(&mut self, event: AppEvent) -> Vec<Msg>;

    fn is_connected(&self) -> bool;

    #[must_use]
    fn reset(&mut self) -> Vec<Msg>;
}
