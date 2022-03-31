use std::sync::Arc;

pub type MidiIn<D> = DirectionalConnection<midir::MidiInput, midir::MidiInputConnection<D>, D>;
pub type MidiOut = DirectionalConnection<midir::MidiOutput, midir::MidiOutputConnection, ()>;

pub enum DirectionalConnection<IO: midir::MidiIO, C, D> {
    Connected(C),
    Disconnected((IO, D)),
    None,
}

impl<IO: midir::MidiIO, C, D> Default for DirectionalConnection<IO, C, D> {
    fn default() -> Self {
        Self::None
    }
}

impl<IO: midir::MidiIO, C, D> DirectionalConnection<IO, C, D> {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

impl<D: Send + Clone> MidiIn<D> {
    pub fn try_new(client_name: &str, data: D) -> Result<Self, super::Error> {
        Ok(Self::Disconnected((
            midir::MidiInput::new(client_name)?,
            data,
        )))
    }

    pub fn connect<C>(
        &mut self,
        port_name: Arc<str>,
        port: &midir::MidiInputPort,
        client_port_name: &str,
        callback: C,
    ) -> Result<(), super::Error>
    where
        C: FnMut(u64, &[u8], &mut D) + Send + 'static,
    {
        self.disconnect();
        match std::mem::take(self) {
            Self::Disconnected((midi_input, data)) => {
                match midi_input.connect(port, client_port_name, callback, data.clone()) {
                    Ok(conn) => {
                        *self = Self::Connected(conn);
                    }
                    Err(err) => {
                        // Unfortunately, err.into_inner() doesn't contain
                        // data, hence the need for a Clone bound on D.
                        *self = Self::Disconnected((err.into_inner(), data));
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
                    let (io, data) = conn.close();
                    *self = Self::Disconnected((io, data));
                }
                _ => unreachable!(),
            }
        }
    }
}

impl MidiOut {
    pub fn try_new(client_name: &str) -> Result<Self, super::Error> {
        Ok(Self::Disconnected((
            midir::MidiOutput::new(client_name)?,
            (),
        )))
    }

    pub fn connect(
        &mut self,
        port_name: Arc<str>,
        port: &midir::MidiOutputPort,
        client_port_name: &str,
    ) -> Result<(), super::Error> {
        self.disconnect();
        match std::mem::take(self) {
            Self::Disconnected((midi_output, ())) => {
                match midi_output.connect(port, client_port_name) {
                    Ok(conn) => {
                        *self = Self::Connected(conn);
                    }
                    Err(err) => {
                        *self = Self::Disconnected((err.into_inner(), ()));
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

    pub fn send(&mut self, msg: &[u8]) -> Result<(), super::Error> {
        match self {
            Self::Connected(conn) => {
                conn.send(msg).map_err(|err| {
                    log::error!(
                        "Failed to send MIDI msg {}: {err}",
                        crate::bytes::Displayable::from(msg)
                    );
                    err
                })?;
            }
            _ => {
                log::warn!("Attempt to send a msg, but MIDI Out is not connected");
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
                    *self = Self::Disconnected((io, ()));
                }
                _ => unreachable!(),
            }
        }
    }
}
