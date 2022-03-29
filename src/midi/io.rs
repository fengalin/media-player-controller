use std::sync::Arc;

pub type MidiIn = DirectionalConnection<midir::MidiInput, midir::MidiInputConnection<()>>;
pub type MidiOut = DirectionalConnection<midir::MidiOutput, midir::MidiOutputConnection>;

pub enum DirectionalConnection<IO: midir::MidiIO, C> {
    Connected(C),
    Disconnected(IO),
    None,
}

impl<IO: midir::MidiIO, C> Default for DirectionalConnection<IO, C> {
    fn default() -> Self {
        Self::None
    }
}

impl<IO: midir::MidiIO, C> DirectionalConnection<IO, C> {
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

impl MidiIn {
    pub fn try_new(client_name: &str) -> Result<Self, super::Error> {
        Ok(Self::Disconnected(midir::MidiInput::new(client_name)?))
    }

    pub fn connect<C>(
        &mut self,
        port_name: Arc<str>,
        port: &midir::MidiInputPort,
        client_port_name: &str,
        mut callback: C,
    ) -> Result<(), super::Error>
    where
        C: FnMut(u64, &[u8]) + Send + 'static,
    {
        self.disconnect();
        match std::mem::take(self) {
            Self::Disconnected(midi_input) => {
                match midi_input.connect(
                    port,
                    client_port_name,
                    move |ts, buf, _| callback(ts, buf),
                    (),
                ) {
                    Ok(conn) => {
                        *self = Self::Connected(conn);
                    }
                    Err(err) => {
                        *self = Self::Disconnected(err.into_inner());
                        let err = super::Error::Connection(port_name);
                        log::error!("{}", err);
                        return Err(err);
                    }
                };
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn disconnect(&mut self) {
        if self.is_connected() {
            match std::mem::take(self) {
                Self::Connected(conn) => {
                    let (io, _) = conn.close();
                    *self = Self::Disconnected(io);
                }
                _ => unreachable!(),
            }
        }
    }
}

impl MidiOut {
    pub fn try_new(client_name: &str) -> Result<Self, super::Error> {
        Ok(Self::Disconnected(midir::MidiOutput::new(client_name)?))
    }

    pub fn connect(
        &mut self,
        port_name: Arc<str>,
        port: &midir::MidiOutputPort,
        client_port_name: &str,
    ) -> Result<(), super::Error> {
        self.disconnect();
        match std::mem::take(self) {
            Self::Disconnected(midi_output) => {
                match midi_output.connect(port, client_port_name) {
                    Ok(conn) => {
                        *self = Self::Connected(conn);
                    }
                    Err(err) => {
                        *self = Self::Disconnected(err.into_inner());
                        let err = super::Error::Connection(port_name);
                        log::error!("{}", err);
                        return Err(err);
                    }
                };
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn send(&mut self, message: &[u8]) -> Result<(), super::Error> {
        match self {
            Self::Connected(conn) => {
                conn.send(message).map_err(|err| {
                    log::error!("Failed to send a MIDI message");
                    err
                })?;
            }
            _ => {
                log::warn!("Attempt to send a message, but MIDI Out is not connected");
                return Err(super::Error::NotConnected);
            }
        }

        Ok(())
    }

    pub fn disconnect(&mut self) {
        if self.is_connected() {
            match std::mem::take(self) {
                Self::Connected(conn) => {
                    let io = conn.close();
                    *self = Self::Disconnected(io);
                }
                _ => unreachable!(),
            }
        }
    }
}
