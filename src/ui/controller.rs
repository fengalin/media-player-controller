use crossbeam_channel as channel;
use eframe::egui;
use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
    time::Duration,
};

use super::app;
use crate::{
    ctrl_surf::{self, AppEvent},
    midi, mpris,
};

const CTRL_SURF_CONNECTION_TIMEOUT: Duration = Duration::from_millis(250);
const TRACK_META_RETRY_DELAY: Duration = Duration::from_millis(250);

pub struct Spawner {
    pub req_rx: channel::Receiver<app::Request>,
    pub err_tx: channel::Sender<anyhow::Error>,
    pub ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
    pub client_name: Arc<str>,
    pub ports_panel: Arc<Mutex<super::PortsPanel>>,
    pub player_panel: Arc<Mutex<super::PlayerPanel>>,
    pub egui_ctx: egui::Context,
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
                self.egui_ctx,
            );
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Couldn't connect to Control Surface: {}", .0)]
    ControlSurfaceConnection(Arc<str>),

    #[error("Control Surface not found: {}", .0)]
    ControlSurfaceNotFound(Arc<str>),

    #[error("Uknwown Control Surface: {}", .0)]
    UnknownControlSurface(Arc<str>),
}

#[derive(Clone, Copy, Debug)]
enum DelayedEvent {
    CtrlSurfConnectionTimeout,
    TrackMetaRetry,
}

struct Controller {
    err_tx: channel::Sender<anyhow::Error>,

    timer: timer::Timer,
    delayed_evt_tx: channel::Sender<DelayedEvent>,

    ctrl_surf: Option<ctrl_surf::ControlSurfaceArc>,
    ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
    ctrl_surf_conn_timeout: Option<timer::Guard>,

    midi_ports: midi::port::InOutManager,
    ports_panel: Arc<Mutex<super::PortsPanel>>,

    players: mpris::Players,
    player_panel: Arc<Mutex<super::PlayerPanel>>,
    player_meta_retry: Option<timer::Guard>,

    must_repaint: bool,
    egui_ctx: egui::Context,
}

// Important: panels Mutexes must be released as soon as possible.

impl Controller {
    fn run(
        req_rx: channel::Receiver<app::Request>,
        err_tx: channel::Sender<anyhow::Error>,
        ctrl_surf_panel: Arc<Mutex<super::ControlSurfacePanel>>,
        client_name: Arc<str>,
        ports_panel: Arc<Mutex<super::PortsPanel>>,
        player_panel: Arc<Mutex<super::PlayerPanel>>,
        egui_ctx: egui::Context,
    ) -> Result<(), ()> {
        use anyhow::Context;

        let (delayed_evt_tx, delayed_evt_rx) = channel::unbounded();

        let (midi_tx, midi_rx) = channel::unbounded();
        let midi_ports = midi::port::InOutManager::try_new(client_name, midi_tx)
            .context("Failed to create MIDI ports manager")
            .map_err(|err| {
                log::error!("{err}");
                let _ = err_tx.send(err);
            })?;

        let (players, evt_rx) = mpris::Players::try_new()
            .context("Failed to create MPRIS players manager")
            .map_err(|err| {
                log::error!("{err}");
                let _ = err_tx.send(err);
            })?;

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
            player_meta_retry: None,

            must_repaint: false,
            egui_ctx,
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

    fn display_err(&mut self, err: impl Into<anyhow::Error>) {
        fn inner(this: &mut Controller, err: anyhow::Error) {
            log::error!("{err}");
            let _ = this.err_tx.send(err);
            this.must_repaint = true;
        }

        inner(self, err.into());
    }

    fn handle_request(&mut self, request: app::Request) -> anyhow::Result<ControlFlow<(), ()>> {
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
            UsePlayer(player_name) => self.players.set_cur(player_name)?,
            RefreshPlayers => self.refresh_players()?,
            Shutdown => return Ok(ControlFlow::Break(())),
            Mixer(mevt) => {
                log::debug!("UI Player: {mevt:?}");
                let _ = self.players.handle_event(mevt);
            }
            Transport(tevt) => {
                log::debug!("UI Player: {tevt:?}");
                let _ = self.players.handle_event(tevt);
                if let ctrl_surf::event::Transport::SetPosition(_) = tevt {
                    self.player_panel.lock().unwrap().reset_pending_seek();
                    self.must_repaint = true;
                }
            }
        }

        Ok(ControlFlow::Continue(()))
    }
}

/// MIDI stuff.
impl Controller {
    fn refresh_ports(&mut self) -> anyhow::Result<()> {
        self.midi_ports.refresh()?;
        self.ports_panel.lock().unwrap().update(&self.midi_ports);

        Ok(())
    }

    fn handle_midi_msg(&mut self, msg: midi::Msg) -> anyhow::Result<()> {
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
impl Controller {
    fn use_ctrl_surf(&mut self, ctrl_surf_name: Arc<str>) -> anyhow::Result<()> {
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

    fn handle_ctrl_surf_resp(&mut self, resp: Vec<ctrl_surf::Msg>) -> anyhow::Result<()> {
        use ctrl_surf::Msg::*;

        for msg in resp {
            match msg {
                ToApp(event) => {
                    log::debug!("Ctrl surf: {event:?}");
                    self.players.handle_event(event)?;
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
                            log::debug!(
                                "Attempt to connect Control Surface {} failed: {err}",
                                self.ctrl_surf_panel.lock().unwrap().cur
                            );

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

    fn try_connect_ctrl_surf(&mut self) -> anyhow::Result<()> {
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
                    self.display_err(err);
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
                self.display_err(err);

                // FIXME re-enable UI

                None
            }
        }
    }
}

/// Mpris Player stuff.
impl Controller {
    fn handle_mpris_event(&mut self, event: crate::mpris::Event) -> anyhow::Result<()> {
        use crate::mpris::Event;
        use ctrl_surf::event::{AppEvent::*, Data::*, Mixer::*, Transport::*};

        match event {
            Event::Transport(PlayPause) => {
                log::info!("MPRIS Player: PlayPause");
                self.send_to_ctrl_surf(PlayPause);
                self.player_panel.lock().unwrap().play_pause();
                self.must_repaint = true;
            }
            Event::Transport(Play) => {
                log::info!("MPRIS Player: Play");
                self.send_to_ctrl_surf(Play);
                self.player_panel.lock().unwrap().set_playback_status(true);
                self.must_repaint = true;
            }
            Event::Transport(Pause) => {
                log::info!("MPRIS Player: Pause");
                self.send_to_ctrl_surf(Pause);
                self.player_panel.lock().unwrap().set_playback_status(false);
                self.must_repaint = true;
            }
            Event::Transport(Previous) => {
                log::info!("MPRIS Player: Previous");
                self.send_to_ctrl_surf(Previous);
                self.player_panel.lock().unwrap().set_playback_status(true);
                self.must_repaint = true;
            }
            Event::Transport(Next) => {
                log::info!("MPRIS Player: Next");
                self.send_to_ctrl_surf(Next);
                self.player_panel.lock().unwrap().set_playback_status(true);
                self.must_repaint = true;
            }
            Event::Transport(Stop) => {
                log::info!("MPRIS Player: Stop");
                self.send_to_ctrl_surf(Stop);
                self.refresh_players()?;
                {
                    let mut player_panel = self.player_panel.lock().unwrap();
                    player_panel.reset();
                    player_panel.set_playback_status(false);
                }
                self.must_repaint = true;
            }
            Event::Transport(SetPosition(pos)) => {
                log::info!("MPRIS Player: SetPosition {pos:?}");
                self.send_to_ctrl_surf(SetPosition(pos));
            }
            Event::Transport(tevt) => {
                log::debug!("MPRIS Player: Transport {tevt:?}");
                self.send_to_ctrl_surf(Transport(tevt));
            }
            Event::Data(Track(track)) => {
                let was_retrying = self.player_meta_retry.take().is_some();
                if !was_retrying && track.image_url.is_none() {
                    self.player_meta_retry = Some(
                        self.delay_event(DelayedEvent::TrackMetaRetry, TRACK_META_RETRY_DELAY),
                    );
                    log::debug!("MPRIS Player: Track without image, will try again");
                    return Ok(());
                }
                log::debug!("MPRIS Player: Track {:?} - {:?}", track.artist, track.title);
                self.player_panel.lock().unwrap().update_track(&track);
                self.send_to_ctrl_surf(Track(track));
            }
            Event::Data(Position(pos)) => {
                log::trace!("MPRIS Player: Position {pos:?}");
                self.player_panel.lock().unwrap().update_position(pos);
                self.send_to_ctrl_surf(Position(pos));
                self.must_repaint = true;
            }
            Event::Data(PlaybackStatus(status)) => {
                log::debug!("MPRIS Player: PlaybackStatus {status:?}");
                self.player_panel
                    .lock()
                    .unwrap()
                    .set_playback_status(status.is_playing());
                self.send_to_ctrl_surf(PlaybackStatus(status));
                self.must_repaint = true;
            }
            Event::PlayerSpawned(name) => {
                log::debug!("MPRIS Player: PlayerSpawned {name:?}");
                {
                    let mut player_panel = self.player_panel.lock().unwrap();
                    player_panel.reset();
                    player_panel.update_players(&self.players);
                }
                let is_connected = self
                    .ctrl_surf
                    .as_ref()
                    .map_or(false, |cs| cs.lock().unwrap().is_connected());
                if !is_connected {
                    self.players.send_all_data()?;
                } else {
                    self.send_to_ctrl_surf(NewApp(name));
                }
                self.players.unmute_system();
                self.must_repaint = true;
            }
            Event::Caps(caps) => {
                log::debug!("MPRIS Player: Caps");
                self.player_panel.lock().unwrap().set_caps(caps);
                self.must_repaint = true;
            }
            Event::Mixer(Volume(vol)) => {
                log::debug!("MPRIS Player: Volume {vol:?}");
                self.player_panel.lock().unwrap().set_volume(vol);
                self.send_to_ctrl_surf(Volume(vol));
                // From a control surface pov, setting the volume
                // wouldn't necessarily mean unmuting, but from
                // an Mpris player, it does. So make it clear.
                if vol < f64::EPSILON {
                    self.send_to_ctrl_surf(Mute);
                } else {
                    self.send_to_ctrl_surf(Unmute);
                }
                self.must_repaint = true;
            }
            Event::Mixer(Mute) => {
                log::debug!("MPRIS Player: Mute");
                self.player_panel.lock().unwrap().set_muted(true);
                self.send_to_ctrl_surf(Mute);
                self.must_repaint = true;
            }
            Event::Mixer(Unmute) => {
                log::debug!("MPRIS Player: Unmute");
                self.player_panel.lock().unwrap().set_muted(false);
                self.send_to_ctrl_surf(Unmute);
                self.must_repaint = true;
            }
        }

        Ok(())
    }

    fn refresh_players(&mut self) -> anyhow::Result<()> {
        self.players.refresh()?;
        self.player_panel
            .lock()
            .unwrap()
            .update_players(&self.players);

        Ok(())
    }
}

/// Controller loop.
impl Controller {
    fn run_loop(
        mut self,
        req_rx: channel::Receiver<app::Request>,
        player_rx: channel::Receiver<mpris::Event>,
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
                            Err(err) => self.display_err(err),
                        },
                        Err(err) => {
                            log::error!("Error UI request channel: {err}");
                            break;
                        }
                    }
                }
                recv(player_rx) -> pevent => {
                    match pevent {
                        Ok(pevent) => match self.handle_mpris_event(pevent) {
                            Ok(()) => (),
                            Err(err) => self.display_err(err),
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
                            Err(err) => self.display_err(err),
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
                            TrackMetaRetry => {
                                let _ = self.players.send_track_meta();
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
                self.egui_ctx.request_repaint();
                self.must_repaint = false;
            }
        }

        log::debug!("Shutting down App Controller loop");
    }
}
