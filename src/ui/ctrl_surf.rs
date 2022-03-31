use eframe::{egui, epi};
use once_cell::sync::Lazy;
use std::sync::Arc;

#[derive(Debug)]
pub enum Response {
    Use(Arc<str>),
    Discover,
}

static NO_CTRL_SURF: Lazy<Arc<str>> = Lazy::new(|| "No Control Surface".into());
const STORAGE_CTRL_SURF: &str = "control_surface";

pub struct ControlSurfacePanel {
    list: Vec<Arc<str>>,
    pub cur: Arc<str>,
}

impl ControlSurfacePanel {
    pub fn new() -> Self {
        let mut list: Vec<Arc<str>> = crate::ctrl_surf::FACTORY.list().map(Arc::from).collect();
        list.sort();

        // FIXME initialize with the last Control Surface used
        // or try auto-detecting?
        let cur = list.first().unwrap().clone();

        Self { list, cur }
    }

    #[must_use]
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<Response> {
        ui.horizontal(|ui| {
            use Response::*;

            let mut resp = None;

            egui::ComboBox::from_label("Control Surface")
                .selected_text(self.cur.as_ref())
                .show_ui(ui, |ui| {
                    for ctrl_surf in self.list.iter() {
                        if ui
                            .selectable_value(&mut self.cur, ctrl_surf.clone(), ctrl_surf.as_ref())
                            .clicked()
                        {
                            resp = Some(Use(ctrl_surf.clone()));
                        }
                    }
                });

            ui.add_space(20f32);
            if ui.button("Discover").clicked() {
                resp = Some(Discover)
            }

            resp
        })
        .inner
    }

    pub fn setup(&mut self, storage: Option<&dyn epi::Storage>) -> Option<Response> {
        use Response::*;

        if let Some(storage) = storage {
            if let Some(ctrl_surf) = storage.get_string(STORAGE_CTRL_SURF) {
                return Some(Use(ctrl_surf.into()));
            }
        }

        None
    }

    pub fn save(&self, storage: &mut dyn epi::Storage) {
        if self.cur != *NO_CTRL_SURF {
            storage.set_string(STORAGE_CTRL_SURF, self.cur.to_string());
        }
    }
}
