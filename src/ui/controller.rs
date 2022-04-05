use crossbeam_channel as channel;
use eframe::epi;
use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::app::{self, Error};
use crate::{
    ctrl_surf::{self, AppEvent, CtrlSurfEvent},
    midi, mpris,
};

const CTRL_SURF_CONNECTION_TIMEOUT: Duration = Duration::from_millis(250);

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

#[derive(Clone, Copy, Debug)]
enum DelayedEvent {
    CtrlSurfConnectionTimeout,
}

struct Controller<'a> {
    err_tx: channel::Sender<Error>,

    timer: timer::Timer,
    delayed_evt_tx: channel::Sender<DelayedEvent>,

    ctrl_surf: Option<ctrl_surf::ControlSurfaceArc>,
    ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
    ctrl_surf_conn_timeout: Option<timer::Guard>,

    midi_ports: midi::port::InOutManager,
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
        let (delayed_evt_tx, delayed_evt_rx) = channel::unbounded();

        let (midi_tx, midi_rx) = channel::unbounded();
        let midi_ports =
            midi::port::InOutManager::try_new(client_name, midi_tx).map_err(|err| {
                log::error!("Error MIDI ports manager: {}", err);
                let _ = err_tx.send(err.into());
            })?;

        let (players, evt_rx) = mpris::Players::new();

        Self {
            err_tx,

            timer: timer::Timer::new(),
            delayed_evt_tx,

            ctrl_surf: None,
            ctrl_surf_panel,
            ctrl_surf_conn_timeout: None,

            midi_ports,

            ports_panel,
            players,
            player_panel,

            must_repaint: false,
            frame: None,
        }
        .run_loop(req_rx, evt_rx, midi_rx, delayed_evt_rx);

        Ok(())
    }

    #[must_use]
    fn delay_event(&mut self, event: DelayedEvent, timeout: Duration) -> timer::Guard {
        let sender = self.delayed_evt_tx.clone();
        self.timer
            .schedule_with_delay(chrono::Duration::from_std(timeout).unwrap(), move || {
                let _ = sender.send(event);
            })
    }

    fn handle_request(&mut self, request: app::Request) -> Result<ControlFlow<(), ()>, Error> {
        use app::Request::*;

        match request {
            ConnectPort((direction, port_name)) => {
                self.midi_ports.connect(direction, port_name)?;
                self.try_connect_ctrl_surf()?;
                self.refresh_ports()?;
            }
            DisconnectPort(direction) => {
                self.send_to_ctrl_surf(ctrl_surf::event::Transport::Stop);
                self.midi_ports.disconnect(direction)?;
                self.refresh_ports()?;
            }
            RefreshPorts => self.refresh_ports()?,
            UseControlSurface(ctrl_surf) => self.use_ctrl_surf(ctrl_surf)?,
            NoControlSurface => {
                self.send_to_ctrl_surf(ctrl_surf::event::Transport::Stop);
                self.ctrl_surf = None;
                log::info!("Control Surface not used");
            }
            ResetControlSurface => {
                self.send_to_ctrl_surf(ctrl_surf::event::Transport::Stop);
            }
            ScanControlSurface => self.start_scan(),
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
}

/// MIDI stuff.
impl<'a> Controller<'a> {
    fn refresh_ports(&mut self) -> Result<(), Error> {
        self.midi_ports.refresh()?;
        self.ports_panel.lock().unwrap().update(&self.midi_ports);

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
}

/// Control Surface stuff.
impl<'a> Controller<'a> {
    fn use_ctrl_surf(&mut self, ctrl_surf_name: Arc<str>) -> Result<(), Error> {
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            let mut ctrl_surf = ctrl_surf.lock().unwrap();
            if ctrl_surf.is_connected() {
                let resp = ctrl_surf.reset();
                drop(ctrl_surf);

                let _ = self.handle_ctrl_surf_resp(resp);
            }
        }

        let ctrl_surf = crate::ctrl_surf::FACTORY
            .build(&ctrl_surf_name)
            .ok_or_else(|| {
                self.ctrl_surf = None;
                self.ctrl_surf_panel.lock().unwrap().update(None);

                Error::UnknownControlSurface(ctrl_surf_name.clone())
            })?;

        self.ctrl_surf = Some(ctrl_surf);
        self.ctrl_surf_panel.lock().unwrap().update(ctrl_surf_name);

        self.try_connect_ctrl_surf()?;

        Ok(())
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
                                Mute => log::warn!("Attempt to mute (not implemented)"),
                            }
                        }
                        DataRequest => self.players.handle_event(event)?,
                    }
                }
                ToDevice(msg) => {
                    if self.midi_ports.are_connected() {
                        let _ = self.midi_ports.send(msg);
                    }
                }
                ConnectionStatus(res) => {
                    use ctrl_surf::msg::ConnectionStatus::*;
                    match res {
                        InProgress => {
                            log::debug!(
                                "Waiting for connection to Control Surface {}",
                                self.ctrl_surf_panel.lock().unwrap().cur
                            );
                            self.ctrl_surf_conn_timeout = Some(self.delay_event(
                                DelayedEvent::CtrlSurfConnectionTimeout,
                                CTRL_SURF_CONNECTION_TIMEOUT,
                            ));
                        }
                        Result(Ok(())) => {
                            self.ctrl_surf_conn_timeout = None;
                            log::info!(
                                "Connected to Control Surface {}",
                                self.ctrl_surf_panel.lock().unwrap().cur
                            );

                            self.midi_ports.abort_scanner();
                            // FIXME re-enable UI in case we were scanning
                        }
                        Result(Err(err)) => {
                            log::debug!("Control Surface connection: {err}");

                            if self.midi_ports.is_scanning() {
                                let _ = self.scan_next();
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn send_to_ctrl_surf(&mut self, event: impl Into<AppEvent>) {
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            let resp = {
                let mut ctrl_surf = ctrl_surf.lock().unwrap();
                if !ctrl_surf.is_connected() {
                    return;
                }
                ctrl_surf.event_from_app(event.into())
            };

            let _ = self.handle_ctrl_surf_resp(resp);
        }
    }

    fn try_connect_ctrl_surf(&mut self) -> Result<(), Error> {
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            if !self.midi_ports.are_connected() {
                self.start_scan();
                return Ok(());
            }

            log::info!(
                "Trying to connect to Control Surface {}",
                self.ctrl_surf_panel.lock().unwrap().cur
            );
            self.ctrl_surf_conn_timeout = None;
            let resp = ctrl_surf.lock().unwrap().start_connection();
            self.handle_ctrl_surf_resp(resp)?;
        }

        Ok(())
    }

    fn ctrl_surf_connection_timeout(&mut self) {
        self.ctrl_surf_conn_timeout = None;
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            let mut ctrl_surf = ctrl_surf.lock().unwrap();
            if !ctrl_surf.is_connected() {
                let resp = ctrl_surf.abort_connection();
                drop(ctrl_surf);
                let _ = self.handle_ctrl_surf_resp(resp);

                let err = Error::ControlSurfaceConnection(
                    self.ctrl_surf_panel.lock().unwrap().cur.clone(),
                );

                if self.midi_ports.is_scanning() {
                    log::info!("{err}");
                    let _ = self.scan_next();
                } else {
                    log::error!("{err}");
                    let _ = self.err_tx.send(err);
                }
            }
        }
    }

    fn start_scan(&mut self) {
        if self
            .ctrl_surf
            .as_ref()
            .map_or(false, |cs| !cs.lock().unwrap().is_connected())
        {
            let res = self.scan_next();
            if res.is_some() {
                // FIXME disable the UI so that user doesn't interfer during scanning
            }
        }
    }

    fn scan_next(&mut self) -> Option<Arc<str>> {
        match self.midi_ports.scanner_next() {
            Some(port_name) => {
                log::info!("Scanning {port_name}");
                self.ports_panel.lock().unwrap().update(&self.midi_ports);
                let _ = self.try_connect_ctrl_surf();

                Some(port_name)
            }
            None => {
                let _ = self.midi_ports.disconnect(midi::port::Direction::In);
                let _ = self.midi_ports.disconnect(midi::port::Direction::Out);
                self.ports_panel.lock().unwrap().update(&self.midi_ports);
                self.must_repaint = true;

                let ctrl_surf = self.ctrl_surf_panel.lock().unwrap().cur.clone();
                let err = Error::ControlSurfaceNotFound(ctrl_surf);
                log::error!("{err}");
                let _ = self.err_tx.send(err);

                // FIXME re-enable UI

                None
            }
        }
    }
}

/// Mpris Player stuff.
impl<'a> Controller<'a> {
    fn handle_player_event(&mut self, event: AppEvent) -> Result<(), Error> {
        use ctrl_surf::event::{AppEvent::*, Data::*, Transport::Stop};

        match event {
            Transport(Stop) => {
                log::info!("Player: Stop");
                self.send_to_ctrl_surf(Stop);
                self.refresh_players()?;
                self.player_panel.lock().unwrap().reset_data();
                self.must_repaint = true;
            }
            Data(Track(ref track)) => {
                log::debug!("Player: Track {:?} - {:?}", track.artist, track.title);
                self.player_panel.lock().unwrap().update_track(track);
                self.send_to_ctrl_surf(event);
            }
            Data(Timecode(tc)) => {
                log::trace!("Player: {event:?}");
                self.player_panel.lock().unwrap().update_position(tc);
                self.send_to_ctrl_surf(event);
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
                    self.send_to_ctrl_surf(event);
                }
            }
            _ => {
                log::debug!("Player: {event:?}");
                self.send_to_ctrl_surf(event);
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
}

/// Controller loop.
impl<'a> Controller<'a> {
    fn run_loop(
        mut self,
        req_rx: channel::Receiver<app::Request>,
        evt_rx: channel::Receiver<AppEvent>,
        midi_rx: channel::Receiver<midi::Msg>,
        delayed_evt_rx: channel::Receiver<DelayedEvent>,
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
                recv(delayed_evt_rx) -> devt => {
                    use DelayedEvent::*;
                    match devt {
                        Ok(devt) => match devt {
                            CtrlSurfConnectionTimeout => {
                                self.ctrl_surf_connection_timeout();
                            }
                        },
                        Err(err) => {
                            log::error!("Error delayed event channel: {err}");
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
