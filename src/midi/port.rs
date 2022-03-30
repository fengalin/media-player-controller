use std::{borrow::Borrow, collections::BTreeMap, sync::Arc};

use super::io;

pub type PortsIn = DirectionalPorts<midir::MidiInput, midir::MidiInputConnection<()>>;
pub type PortsOut = DirectionalPorts<midir::MidiOutput, midir::MidiOutputConnection>;

pub struct DirectionalPorts<IO: midir::MidiIO, Conn> {
    map: BTreeMap<Arc<str>, IO::Port>,
    cur: Option<Arc<str>>,
    midi_conn: io::DirectionalConnection<IO, Conn>,
    client_name: Arc<str>,
}

impl<IO: midir::MidiIO, Conn> DirectionalPorts<IO, Conn> {
    pub fn list(&self) -> impl Iterator<Item = &Arc<str>> {
        self.map.keys()
    }

    pub fn cur(&self) -> Option<&Arc<str>> {
        self.cur.as_ref()
    }

    fn refresh_from(&mut self, conn: IO) -> Result<(), super::Error> {
        self.map.clear();

        let mut prev = self.cur.take();
        for port in conn.ports().iter() {
            let name = conn.port_name(port)?;
            if !name.starts_with(self.client_name.as_ref()) {
                if let Some(ref prev_ref) = prev {
                    if prev_ref.as_ref() == name {
                        self.cur = prev.take();
                    }
                }

                self.map.insert(name.into(), port.clone());
            }
        }

        Ok(())
    }
}

impl PortsIn {
    pub fn try_new(client_name: Arc<str>) -> Result<Self, super::Error> {
        Ok(Self {
            map: BTreeMap::new(),
            cur: None,
            midi_conn: io::MidiIn::try_new(&client_name)?,
            client_name,
        })
    }

    pub fn refresh(&mut self) -> Result<(), super::Error> {
        let temp_conn = midir::MidiInput::new(&format!("{} referesh In ports", self.client_name,))?;

        self.refresh_from(temp_conn)?;

        Ok(())
    }

    pub fn connect<C>(&mut self, port_name: Arc<str>, mut callback: C) -> Result<(), super::Error>
    where
        C: FnMut(super::Msg) + Send + 'static,
    {
        let port = self
            .map
            .get(&port_name)
            .ok_or_else(|| super::Error::PortNotFound(port_name.clone()))?
            .clone();

        self.midi_conn
            .connect(
                port_name.clone(),
                &port,
                &self.client_name,
                move |_ts, buf| callback(buf.into()),
            )
            .map_err(|_| {
                self.cur = None;
                super::Error::PortConnection
            })?;

        log::info!("Connected for Input to {}", port_name);
        self.cur = Some(port_name);

        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.midi_conn.disconnect();

        if let Some(cur) = self.cur.take() {
            log::debug!("Disconnected Input from {}", cur);
        }
    }
}

impl PortsOut {
    pub fn try_new(client_name: Arc<str>) -> Result<Self, super::Error> {
        Ok(Self {
            map: BTreeMap::new(),
            cur: None,
            midi_conn: io::MidiOut::try_new(&client_name)?,
            client_name,
        })
    }

    pub fn refresh(&mut self) -> Result<(), super::Error> {
        let temp_conn =
            midir::MidiOutput::new(&format!("{} referesh Out ports", self.client_name,))?;

        self.refresh_from(temp_conn)?;

        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.midi_conn.is_connected()
    }

    pub fn connect(&mut self, port_name: Arc<str>) -> Result<(), super::Error> {
        let port = self
            .map
            .get(&port_name)
            .ok_or_else(|| super::Error::PortNotFound(port_name.clone()))?
            .clone();

        self.midi_conn
            .connect(port_name.clone(), &port, &self.client_name)
            .map_err(|_| {
                self.cur = None;
                super::Error::PortConnection
            })?;

        log::info!("Connected for Output to {}", port_name);
        self.cur = Some(port_name);

        Ok(())
    }

    pub fn send(&mut self, msg: impl Borrow<[u8]>) -> Result<(), super::Error> {
        self.midi_conn.send(msg.borrow())
    }

    pub fn disconnect(&mut self) {
        self.midi_conn.disconnect();

        if let Some(cur) = self.cur.take() {
            log::debug!("Disconnected Output from {}", cur);
        }
    }
}
