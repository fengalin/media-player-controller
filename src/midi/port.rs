use crossbeam_channel as channel;
use std::{collections::BTreeMap, fmt, sync::Arc};

use super::{io, Error, Msg};

pub type PortsIn<D> = DirectionalPorts<midir::MidiInput, midir::MidiInputConnection<D>, D>;
pub type PortsOut = DirectionalPorts<midir::MidiOutput, midir::MidiOutputConnection, ()>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    In,
    Out,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Direction {
    pub fn idx(self) -> usize {
        match self {
            Direction::In => 0,
            Direction::Out => 1,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Direction::In => "In Port",
            Direction::Out => "Out Port",
        }
    }
}

pub struct DirectionalPorts<IO: midir::MidiIO, Conn, D> {
    map: BTreeMap<Arc<str>, IO::Port>,
    cur: Option<Arc<str>>,
    midi_conn: io::DirectionalConnection<IO, Conn, D>,
    client_name: Arc<str>,
}

impl<IO: midir::MidiIO, Conn, D> DirectionalPorts<IO, Conn, D> {
    pub fn list(&self) -> impl Iterator<Item = Arc<str>> + '_ {
        self.map.keys().cloned()
    }

    pub fn cur(&self) -> Option<Arc<str>> {
        self.cur.as_ref().cloned()
    }

    pub fn is_connected(&self) -> bool {
        self.midi_conn.is_connected()
    }

    fn refresh_from(&mut self, conn: IO) -> Result<(), Error> {
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

impl<D: Send + Clone> PortsIn<D> {
    pub fn try_new(client_name: Arc<str>, data: D) -> Result<Self, Error> {
        Ok(Self {
            map: BTreeMap::new(),
            cur: None,
            midi_conn: io::MidiIn::<D>::try_new(&client_name, data)?,
            client_name,
        })
    }

    pub fn refresh(&mut self) -> Result<(), Error> {
        let temp_conn = midir::MidiInput::new(&format!("{} referesh In ports", self.client_name,))?;

        self.refresh_from(temp_conn)?;

        Ok(())
    }

    pub fn connect<C>(&mut self, port_name: Arc<str>, callback: C) -> Result<(), Error>
    where
        C: FnMut(u64, &[u8], &mut D) + Send + 'static,
    {
        let port = self
            .map
            .get(&port_name)
            .ok_or_else(|| Error::PortNotFound(port_name.clone()))?
            .clone();

        self.midi_conn
            .connect(port_name.clone(), &port, &self.client_name, callback)
            .map_err(|_| {
                self.cur = None;
                Error::PortConnection
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
    pub fn try_new(client_name: Arc<str>) -> Result<Self, Error> {
        Ok(Self {
            map: BTreeMap::new(),
            cur: None,
            midi_conn: io::MidiOut::try_new(&client_name)?,
            client_name,
        })
    }

    pub fn refresh(&mut self) -> Result<(), Error> {
        let temp_conn =
            midir::MidiOutput::new(&format!("{} referesh Out ports", self.client_name,))?;

        self.refresh_from(temp_conn)?;

        Ok(())
    }

    pub fn connect(&mut self, port_name: Arc<str>) -> Result<(), Error> {
        let port = self
            .map
            .get(&port_name)
            .ok_or_else(|| Error::PortNotFound(port_name.clone()))?
            .clone();

        self.midi_conn
            .connect(port_name.clone(), &port, &self.client_name)
            .map_err(|_| {
                self.cur = None;
                Error::PortConnection
            })?;

        log::info!("Connected for Output to {}", port_name);
        self.cur = Some(port_name);

        Ok(())
    }

    pub fn send(&mut self, msg: Msg) -> Result<(), Error> {
        self.midi_conn.send(&msg)
    }

    pub fn disconnect(&mut self) {
        self.midi_conn.disconnect();

        if let Some(cur) = self.cur.take() {
            log::debug!("Disconnected Output from {}", cur);
        }
    }
}

enum State {
    Static,
    Scanning {
        iter: Box<dyn Iterator<Item = Arc<str>>>,
    },
}

#[derive(Debug)]
pub enum ScannerStatus {
    Connected,
    Completed,
}

pub struct InOutManager {
    pub ins: PortsIn<channel::Sender<Msg>>,
    pub outs: PortsOut,
    state: State,
}

impl InOutManager {
    pub fn try_new(client_name: Arc<str>, msg_tx: channel::Sender<Msg>) -> Result<Self, Error> {
        let ins = PortsIn::try_new(client_name.clone(), msg_tx)?;
        let outs = PortsOut::try_new(client_name)?;

        Ok(Self {
            ins,
            outs,
            state: State::Static,
        })
    }

    pub fn connect(&mut self, direction: Direction, port_name: Arc<str>) -> Result<(), Error> {
        use Direction::*;
        match direction {
            In => {
                self.ins.connect(port_name, |_ts, msg, msg_tx| {
                    let _ = msg_tx.send(msg.into());
                })?;
            }
            Out => {
                self.outs.connect(port_name)?;
            }
        }

        Ok(())
    }

    pub fn disconnect(&mut self, direction: Direction) -> Result<(), Error> {
        use Direction::*;
        match direction {
            In => self.ins.disconnect(),
            Out => self.outs.disconnect(),
        }

        Ok(())
    }

    pub fn are_connected(&self) -> bool {
        self.ins.is_connected() && self.outs.is_connected()
    }

    pub fn is_scanning(&self) -> bool {
        matches!(self.state, State::Scanning { .. })
    }

    pub fn send(&mut self, msg: Msg) -> Result<(), Error> {
        self.outs.send(msg)
    }

    pub fn refresh(&mut self) -> Result<(), Error> {
        if self.is_scanning() {
            return Err(Error::ScanningPorts);
        }

        self.ins.refresh()?;
        self.outs.refresh()?;

        Ok(())
    }

    /// Returns the next port name for scanning.
    ///
    /// If the [`InOutManager`] is in `Static` mode, this will
    /// attempt to connect in and out ports with the same name
    /// and switch to scanning mode if a connection could be established.
    ///
    /// If the [`InOutManager`] is already in `Scanning` mode,
    /// this will attempt to connect the next ports with the same
    /// name.
    ///
    /// # Returns
    ///
    /// - `Some(port_name)` if in & out `port_name` ports could be connected.
    /// - `None` if no more ports were available or could be connected.
    pub fn scanner_next(&mut self) -> Option<Arc<str>> {
        use State::*;

        // Take ownership of the state so that we can get `iter`
        // as mutable while using `self` as mutable too.
        let state = std::mem::replace(&mut self.state, Static);
        let mut iter = match state {
            Static => Box::new(self.ins.list().collect::<Vec<Arc<str>>>().into_iter()),
            Scanning { iter } => iter,
        };

        let mut cur = None;
        for port_name in iter.by_ref() {
            let could_connect_both = self.connect(Direction::In, port_name.clone()).is_ok()
                && self.connect(Direction::Out, port_name.clone()).is_ok();

            if could_connect_both {
                cur = Some(port_name);
                break;
            }
        }

        if cur.is_none() {
            log::debug!("No more ports to scan");
            return None;
        }

        self.state = Scanning { iter };

        cur
    }

    /// Aborts the `Scanner` mode.
    ///
    /// This allows switching back to `Static` mode,
    /// e.g. when a device could be found on currently
    /// connected ports.
    pub fn abort_scanner(&mut self) {
        self.state = State::Static;
    }
}
