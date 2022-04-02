use std::sync::{Arc, Mutex};

use crate::ctrl_surf::protocol::Mackie;

pub const XTOUCH_ID: u8 = 0x14;
pub const XTOUCH_EXT_ID: u8 = 0x15;

pub struct XTouchMackie;

impl crate::ctrl_surf::Buildable for XTouchMackie {
    const NAME: &'static str = "X-Touch (Mackie)";

    fn build() -> crate::ctrl_surf::ControlSurfaceArc {
        Arc::new(Mutex::new(Mackie::new(XTOUCH_ID)))
    }
}

pub struct XTouchExtMackie;

impl crate::ctrl_surf::Buildable for XTouchExtMackie {
    const NAME: &'static str = "X-Touch Extension (Mackie)";

    fn build() -> crate::ctrl_surf::ControlSurfaceArc {
        Arc::new(Mutex::new(Mackie::new(XTOUCH_EXT_ID)))
    }
}
