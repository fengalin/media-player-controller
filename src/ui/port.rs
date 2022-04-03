use eframe::{egui, epi};
use once_cell::sync::Lazy;
use std::sync::Arc;

use crate::midi::{self, port::Direction};

static DISCONNECTED: Lazy<Arc<str>> = Lazy::new(|| "Disconnected".into());
const STORAGE_PORT_IN: &str = "port_in";
const STORAGE_PORT_OUT: &str = "port_out";

#[derive(Debug)]
pub struct DirectionalPorts {
    pub list: Vec<Arc<str>>,
    pub cur: Arc<str>,
}

impl DirectionalPorts {
    fn update_from<IO: midir::MidiIO, Conn, D>(
        &mut self,
        ports: &midi::DirectionalPorts<IO, Conn, D>,
    ) {
        self.list.clear();
        self.list.extend(ports.list());

        self.cur = ports.cur().unwrap_or_else(|| DISCONNECTED.clone());
    }
}

impl Default for DirectionalPorts {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            cur: DISCONNECTED.clone(),
        }
    }
}

#[derive(Debug)]
pub enum Response {
    Connect((Direction, Arc<str>)),
    Disconnect(Direction),
    CheckingList,
}

pub struct PortsPanel {
    ports: [DirectionalPorts; 2],
}

impl PortsPanel {
    pub fn new() -> Self {
        PortsPanel {
            ports: [DirectionalPorts::default(), DirectionalPorts::default()],
        }
    }

    #[must_use]
    pub fn show(&mut self, direction: Direction, ui: &mut egui::Ui) -> Option<Response> {
        use Response::*;

        let dir_port = &mut self.ports[direction.idx()];

        let resp = egui::ComboBox::from_label(direction.as_str())
            .selected_text(dir_port.cur.as_ref())
            .show_ui(ui, |ui| {
                let mut resp = None;

                if ui
                    .selectable_value(
                        &mut dir_port.cur,
                        DISCONNECTED.clone(),
                        DISCONNECTED.as_ref(),
                    )
                    .clicked()
                {
                    resp = Some(Disconnect(direction));
                }

                for port in dir_port.list.iter() {
                    if ui
                        .selectable_value(&mut dir_port.cur, port.clone(), port.as_ref())
                        .clicked()
                    {
                        resp = Some(Connect((direction, port.clone())));
                    }
                }

                resp
            })
            .inner;

        if let Some(None) = resp {
            Some(CheckingList)
        } else {
            resp.flatten()
        }
    }

    pub fn setup(&mut self, storage: Option<&dyn epi::Storage>) -> impl Iterator<Item = Response> {
        use Response::*;

        let mut resp = Vec::new();
        if let Some(storage) = storage {
            if let Some(port) = storage.get_string(STORAGE_PORT_IN) {
                if port != DISCONNECTED.as_ref() {
                    resp.push(Connect((Direction::In, port.into())));
                }
            }
            if let Some(port) = storage.get_string(STORAGE_PORT_OUT) {
                if port != DISCONNECTED.as_ref() {
                    resp.push(Connect((Direction::Out, port.into())));
                }
            }
        }

        resp.into_iter()
    }

    pub fn save(&self, storage: &mut dyn epi::Storage) {
        storage.set_string(
            STORAGE_PORT_IN,
            self.ports[Direction::In.idx()].cur.to_string(),
        );
        storage.set_string(
            STORAGE_PORT_OUT,
            self.ports[Direction::Out.idx()].cur.to_string(),
        );
    }
}

/// The following functions must be called from the AppController thread,
/// not the UI update thread.
impl PortsPanel {
    pub fn update(&mut self, midi_ports: &midi::port::InOutManager) {
        self.ports[Direction::In.idx()].update_from(&midi_ports.ins);
        self.ports[Direction::Out.idx()].update_from(&midi_ports.outs);
    }
}
