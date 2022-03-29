use eframe::egui;
use std::sync::Arc;

#[derive(Debug)]
pub enum Response {
    Use(Arc<str>),
    Discover,
}

pub struct ControlSurfaceWidget {
    list: Vec<Arc<str>>,
    pub cur: Arc<str>,
}

impl ControlSurfaceWidget {
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
}
