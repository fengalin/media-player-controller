use crossbeam_channel as channel;
use eframe::epi;
use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

use super::app::{self, Error};
use crate::{
    ctrl_surf::{self, event::CtrlSurfEvent},
    midi, mpris,
};

pub struct Spawner {
    pub req_rx: channel::Receiver<app::Request>,
    pub err_tx: channel::Sender<Error>,
    pub ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
    pub client_name: Arc<str>,
    pub ports_panel: Arc<Mutex<super::PortsPanel>>,
    pub player_panel: Arc<Mutex<super::PlayerPanel>>,
}

impl Spawner {
    pub fn spawn(self) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let _ = Controller::run(
                self.req_rx,
                self.err_tx,
                self.ctrl_surf_panel,
                self.client_name,
                self.ports_panel,
                self.player_panel,
            );
        })
    }
}

struct Controller<'a> {
    err_tx: channel::Sender<Error>,

    ctrl_surf: Option<ctrl_surf::ControlSurfaceArc>,
    ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,

    midi_ports_in: midi::PortsIn<channel::Sender<midi::Msg>>,
    midi_ports_out: midi::PortsOut,
    ports_panel: Arc<Mutex<super::PortsPanel>>,

    players: mpris::Players<'a>,
    player_panel: Arc<Mutex<super::PlayerPanel>>,

    must_repaint: bool,
    frame: Option<epi::Frame>,
}

// Important: panels Mutexes must be released as soon as possible.

impl<'a> Controller<'a> {
    fn run(
        req_rx: channel::Receiver<app::Request>,
        err_tx: channel::Sender<Error>,
        ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
        client_name: Arc<str>,
        ports_panel: Arc<Mutex<super::PortsPanel>>,
        player_panel: Arc<Mutex<super::PlayerPanel>>,
    ) -> Result<(), ()> {
        let (midi_tx, midi_rx) = channel::unbounded();

        let midi_ports_in =
            midi::PortsIn::try_new(client_name.clone(), midi_tx).map_err(|err| {
                log::error!("Error creating Controller: {}", err);
                let _ = err_tx.send(err.into());
            })?;

        let midi_ports_out = midi::PortsOut::try_new(client_name).map_err(|err| {
            log::error!("Error creating Controller: {}", err);
            let _ = err_tx.send(err.into());
        })?;

        let (players, evt_rx) = mpris::Players::new();

        Self {
            err_tx,

            ctrl_surf: None,
            ctrl_surf_panel,

            midi_ports_in,
            midi_ports_out,

            ports_panel,
            players,
            player_panel,

            must_repaint: false,
            frame: None,
        }
        .run_loop(req_rx, evt_rx, midi_rx);

        Ok(())
    }

    fn handle_request(&mut self, request: app::Request) -> Result<ControlFlow<(), ()>, Error> {
        use app::Request::*;

        match request {
            Connect((direction, port_name)) => self.connect_port(direction, port_name)?,
            Disconnect(direction) => {
                self.feed_ctrl_surf_back(ctrl_surf::event::Transport::Stop);
                self.disconnect_port(direction)?;
            }
            RefreshPorts => self.refresh_ports()?,
            UseControlSurface(ctrl_surf) => self.use_ctrl_surf(ctrl_surf)?,
            NoControlSurface => {
                self.feed_ctrl_surf_back(ctrl_surf::event::Transport::Stop);
                self.ctrl_surf = None;
                log::info!("Control Surface not used");
            }
            ResetControlSurface => {
                self.feed_ctrl_surf_back(ctrl_surf::event::Transport::Stop);
            }
            UsePlayer(player_name) => {
                self.players.set_cur(player_name).unwrap();
                {
                    let mut player_panel = self.player_panel.lock().unwrap();
                    player_panel.update_players(&self.players);
                    player_panel.reset_data();
                }
            }
            RefreshPlayers => self.refresh_players()?,
            Shutdown => return Ok(ControlFlow::Break(())),
            HaveFrame(egui_frame) => {
                self.frame = Some(egui_frame);
            }
            HaveContext(egui_ctx) => {
                self.player_panel.lock().unwrap().have_context(egui_ctx);
            }
        }

        Ok(ControlFlow::Continue(()))
    }

    fn connect_port(
        &mut self,
        direction: super::port::Direction,
        port_name: Arc<str>,
    ) -> Result<(), Error> {
        use super::port::Direction;
        match direction {
            Direction::In => {
                self.midi_ports_in.connect(port_name, |_ts, msg, midi_tx| {
                    let _ = midi_tx.send(msg.into());
                })?;
                self.try_ctrl_surf_identification()?;
            }
            Direction::Out => {
                self.midi_ports_out.connect(port_name)?;
                self.try_ctrl_surf_identification()?;
            }
        }

        self.refresh_ports()?;

        Ok(())
    }

    fn disconnect_port(&mut self, direction: super::port::Direction) -> Result<(), Error> {
        use super::port::Direction::*;
        match direction {
            In => self.midi_ports_in.disconnect(),
            Out => self.midi_ports_out.disconnect(),
        }
        self.refresh_ports()?;

        Ok(())
    }

    fn refresh_ports(&mut self) -> Result<(), Error> {
        self.midi_ports_in.refresh()?;
        self.midi_ports_out.refresh()?;
        self.ports_panel
            .lock()
            .unwrap()
            .update(&self.midi_ports_in, &self.midi_ports_out);

        Ok(())
    }

    fn use_ctrl_surf(&mut self, ctrl_surf_name: Arc<str>) -> Result<(), Error> {
        let ctrl_surf = crate::ctrl_surf::FACTORY
            .build(&ctrl_surf_name)
            .ok_or_else(|| {
                self.ctrl_surf = None;
                self.ctrl_surf_panel.lock().unwrap().update(None);

                Error::UnknownControlSurface(ctrl_surf_name.clone())
            })?;

        self.ctrl_surf = Some(ctrl_surf);
        self.ctrl_surf_panel.lock().unwrap().update(ctrl_surf_name);

        self.try_ctrl_surf_identification()?;

        Ok(())
    }

    fn try_ctrl_surf_identification(&mut self) -> Result<(), Error> {
        // FIXME scan for known ctrl surf on currently selected port
        // and other ports if identification fails.
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            if !self.midi_ports_out.is_connected() || !self.midi_ports_in.is_connected() {
                log::warn!("Need both MIDI ports connected for Control Surface identification");
            }

            let resp = ctrl_surf.lock().unwrap().start_identification();
            self.handle_ctrl_surf_resp(resp)?;
        }

        Ok(())
    }

    fn handle_midi_msg(&mut self, msg: midi::Msg) -> Result<(), Error> {
        match self.ctrl_surf {
            Some(ref ctrl_surf) => {
                let resp = ctrl_surf.lock().unwrap().msg_from_device(msg);
                self.handle_ctrl_surf_resp(resp)
            }
            None => Ok(()),
        }
    }

    fn handle_ctrl_surf_resp(&mut self, resp: Vec<ctrl_surf::Msg>) -> Result<(), Error> {
        use ctrl_surf::Msg::*;

        for msg in resp {
            match msg {
                ToApp(event) => {
                    use CtrlSurfEvent::*;

                    log::debug!("Ctrl surf: {event:?}");
                    match event {
                        Transport(_) => self.players.handle_event(event)?,
                        Mixer(ref mixer) => {
                            use ctrl_surf::event::Mixer::*;
                            match mixer {
                                Volume(_) => self.players.handle_event(event)?,
                                Mute => log::warn!("Attempt to mute (unimplemented)"),
                            }
                        }
                        DataRequest => self.players.handle_event(event)?,
                    }
                }
                ToDevice(msg) => {
                    if self.midi_ports_out.is_connected() {
                        let _ = self.midi_ports_out.send(&msg);
                    }
                }
                ConnectionStatus(res) => {
                    use ctrl_surf::msg::ConnectionStatus::*;
                    match res {
                        InProgress => (),
                        Result(Ok(())) => {
                            log::info!("Ctrl surf device handshake success");
                        }
                        Result(Err(err)) => {
                            log::debug!("Ctrl surf device handshake: {err}");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn feed_ctrl_surf_back(&mut self, event: impl Into<ctrl_surf::event::Feedback>) {
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            let resp = {
                let mut ctrl_surf = ctrl_surf.lock().unwrap();
                if !ctrl_surf.is_connected() {
                    return;
                }
                ctrl_surf.event_to_device(event.into())
            };

            let _ = self.handle_ctrl_surf_resp(resp);
        }
    }

    fn handle_player_event(&mut self, event: ctrl_surf::event::Feedback) -> Result<(), Error> {
        use ctrl_surf::event::{Data::*, Feedback::*, Transport::Stop};

        match event {
            Transport(Stop) => {
                log::info!("Player: Stop");
                self.feed_ctrl_surf_back(Stop);
                self.refresh_players()?;
                self.player_panel.lock().unwrap().reset_data();
                self.must_repaint = true;
            }
            Data(Track(ref track)) => {
                log::debug!("Player: Track {:?} - {:?}", track.artist, track.title);
                self.player_panel.lock().unwrap().update_track(track);
                self.feed_ctrl_surf_back(event);
            }
            Data(Timecode(tc)) => {
                log::trace!("Player: {event:?}");
                self.player_panel.lock().unwrap().update_position(tc);
                self.feed_ctrl_surf_back(event);
                self.must_repaint = true;
            }
            NewApp(_) => {
                log::trace!("Player: {event:?}");
                let is_connected = self
                    .ctrl_surf
                    .as_ref()
                    .map_or(false, |cs| cs.lock().unwrap().is_connected());
                if !is_connected {
                    self.players.send_all_data()?;
                } else {
                    self.feed_ctrl_surf_back(event);
                }
            }
            _ => {
                log::debug!("Player: {event:?}");
                self.feed_ctrl_surf_back(event);
            }
        }

        Ok(())
    }

    fn refresh_players(&mut self) -> Result<(), Error> {
        self.players.refresh()?;
        self.player_panel
            .lock()
            .unwrap()
            .update_players(&self.players);

        Ok(())
    }

    fn run_loop(
        mut self,
        req_rx: channel::Receiver<app::Request>,
        evt_rx: channel::Receiver<ctrl_surf::event::Feedback>,
        midi_rx: channel::Receiver<midi::Msg>,
    ) {
        loop {
            channel::select! {
                recv(req_rx) -> request => {
                    match request {
                        Ok(request) => match self.handle_request(request) {
                            Ok(ControlFlow::Continue(())) => (),
                            Ok(ControlFlow::Break(())) => break,
                            Err(err) => {
                                log::error!("{err}");
                                let _ = self.err_tx.send(err);
                            }
                        },
                        Err(err) => {
                            log::error!("Error UI request channel: {err}");
                            break;
                        }
                    }
                }
                recv(evt_rx) -> pevent => {
                    match pevent {
                        Ok(pevent) => match self.handle_player_event(pevent) {
                            Ok(()) => (),
                            Err(err) => {
                                log::error!("{err}");
                                let _ = self.err_tx.send(err);
                            }
                        },
                        Err(err) => {
                            log::error!("Error player event channel: {err}");
                            break;
                        }
                    }
                }
                recv(midi_rx) -> midi_msg => {
                    match midi_msg {
                        Ok(midi_msg) => match self.handle_midi_msg(midi_msg) {
                            Ok(()) => (),
                            Err(err) => {
                                log::error!("{err}");
                                let _ = self.err_tx.send(err);
                            }
                        },
                        Err(err) => {
                            log::error!("Error MIDI msg channel: {err}");
                            break;
                        }
                    }
                }
            }

            if self.must_repaint {
                if let Some(ref frame) = self.frame {
                    frame.request_repaint();
                }
                self.must_repaint = false;
            }
        }

        log::debug!("Shutting down App Controller loop");
    }
}
