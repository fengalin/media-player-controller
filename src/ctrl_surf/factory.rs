use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};

use super::{device, ControlSurface};

pub static FACTORY: Lazy<Arc<Factory>> = Lazy::new(|| {
    Factory::default()
        .with::<device::XTouchMackie>()
        .with::<device::XTouchExtMackie>()
        .into()
});

pub trait Buildable {
    const NAME: &'static str;

    fn build() -> Arc<Mutex<dyn ControlSurface>>;
}

pub type ControlSurfaceArc = Arc<Mutex<dyn ControlSurface>>;

#[derive(Default)]
pub struct Factory(std::collections::BTreeMap<&'static str, fn() -> ControlSurfaceArc>);

impl Factory {
    pub(super) fn with<B: Buildable>(mut self) -> Self {
        self.0.insert(B::NAME, B::build);
        self
    }

    pub fn list(&self) -> impl Iterator<Item = &str> {
        self.0.keys().cloned()
    }

    pub fn build(&self, name: &str) -> Option<ControlSurfaceArc> {
        self.0.get(name).map(|build| build())
    }
}
