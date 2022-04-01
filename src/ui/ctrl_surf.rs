use eframe::{egui, epi};
use once_cell::sync::Lazy;
use std::sync::Arc;

#[derive(Debug)]
pub enum Response {
    Use(Arc<str>),
    NoControlSurface,
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

        let cur = NO_CTRL_SURF.clone();

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
                    if ui
                        .selectable_value(
                            &mut self.cur,
                            NO_CTRL_SURF.clone(),
                            NO_CTRL_SURF.as_ref(),
                        )
                        .clicked()
                    {
                        resp = Some(NoControlSurface);
                    }

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
                if ctrl_surf == NO_CTRL_SURF.as_ref() {
                    return Some(NoControlSurface);
                }

                return Some(Use(ctrl_surf.into()));
            }
        }

        None
    }

    pub fn save(&self, storage: &mut dyn epi::Storage) {
        storage.set_string(STORAGE_CTRL_SURF, self.cur.to_string());
    }
}

impl ControlSurfacePanel {
    pub fn update(&mut self, ctrl_surf: impl Into<Option<Arc<str>>>) {
        self.cur = ctrl_surf.into().unwrap_or_else(|| NO_CTRL_SURF.clone());
    }
}
