use std::sync::{Arc, Mutex};

pub trait Buildable {
    const NAME: &'static str;

    fn build() -> Arc<Mutex<dyn super::ControlSurface>>;
}

pub type ControlSurfaceArc = Arc<Mutex<dyn super::ControlSurface>>;

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
