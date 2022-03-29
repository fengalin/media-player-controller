use crossbeam_channel as channel;
use eframe::epi;
use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

use super::app;
use crate::{
    ctrl_surf::{self, event::CtrlSurfEvent},
    midi, mpris,
};

pub struct Spawner {
    pub req_rx: channel::Receiver<app::Request>,
    pub err_tx: channel::Sender<app::Error>,
    pub ctrl_surf_widget: Arc<Mutex<super::ControlSurfaceWidget>>,
    pub client_name: Arc<str>,
    pub ports_widget: Arc<Mutex<super::PortsWidget>>,
    pub player_widget: Arc<Mutex<super::PlayerWidget>>,
}

impl Spawner {
    pub fn spawn(self) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let _ = Controller::run(
                self.req_rx,
                self.err_tx,
                self.ctrl_surf_widget,
                self.client_name,
                self.ports_widget,
                self.player_widget,
            );
        })
    }
}

struct Controller<'a> {
    err_tx: channel::Sender<app::Error>,

    ctrl_surf_tx: channel::Sender<ctrl_surf::Response>,
    ctrl_surf: Option<ctrl_surf::ControlSurfaceArc>,
    ctrl_surf_widget: Arc<Mutex<super::ControlSurfaceWidget>>,

    midi_ports_in: midi::PortsIn,
    midi_ports_out: midi::PortsOut,
    ports_widget: Arc<Mutex<super::PortsWidget>>,

    players: mpris::Players<'a>,
    player_widget: Arc<Mutex<super::PlayerWidget>>,

    must_repaint: bool,
    frame: Option<epi::Frame>,
}

// Important: widgets Mutexes must be released as soon as possible.

impl<'a> Controller<'a> {
    fn run(
        req_rx: channel::Receiver<app::Request>,
        err_tx: channel::Sender<app::Error>,
        ctrl_surf_widget: Arc<Mutex<super::ControlSurfaceWidget>>,
        client_name: Arc<str>,
        ports_widget: Arc<Mutex<super::PortsWidget>>,
        player_widget: Arc<Mutex<super::PlayerWidget>>,
    ) -> Result<(), ()> {
        let midi_ports_in = midi::PortsIn::try_new(client_name.clone()).map_err(|err| {
            log::error!("Error creating Controller: {}", err);
            let _ = err_tx.send(err.into());
        })?;

        let midi_ports_out = midi::PortsOut::try_new(client_name).map_err(|err| {
            log::error!("Error creating Controller: {}", err);
            let _ = err_tx.send(err.into());
        })?;

        let (ctrl_surf_tx, ctrl_surf_rx) = channel::unbounded();
        let (players, evt_rx) = mpris::Players::new();

        Self {
            err_tx,

            ctrl_surf: None,
            ctrl_surf_tx,
            ctrl_surf_widget,

            midi_ports_in,
            midi_ports_out,

            ports_widget,
            players,
            player_widget,

            must_repaint: false,
            frame: None,
        }
        .run_loop(req_rx, evt_rx, ctrl_surf_rx);

        Ok(())
    }

    fn handle_request(&mut self, request: app::Request) -> Result<ControlFlow<(), ()>, app::Error> {
        use app::Request::*;

        match request {
            Connect((direction, port_name)) => self.connect(direction, port_name)?,
            Disconnect(direction) => self.disconnect(direction)?,
            RefreshPorts => self.refresh_ports()?,
            UseControlSurface(ctrl_surf_name) => {
                self.ctrl_surf_widget.lock().unwrap().cur = ctrl_surf_name;
            }
            UsePlayer(player_name) => {
                self.players.set_cur(player_name).unwrap();
                {
                    let mut player_widget = self.player_widget.lock().unwrap();
                    player_widget.update_players(&self.players);
                    player_widget.reset_data();
                }
            }
            RefreshPlayers => self.refresh_players()?,
            ResetPlayer => {
                // FIXME actually reset
                self.feed_ctrl_surf_back(ctrl_surf::event::Transport::Stop);
            }
            Shutdown => {
                // FIXME could disconnect automatically in Drop impls
                let _ = self.disconnect(super::port::Direction::Out);
                let _ = self.disconnect(super::port::Direction::In);
                return Ok(ControlFlow::Break(()));
            }
            HaveFrame(egui_frame) => {
                self.frame = Some(egui_frame);
            }
            HaveContext(egui_ctx) => {
                self.player_widget.lock().unwrap().have_context(egui_ctx);
            }
        }

        Ok(ControlFlow::Continue(()))
    }

    fn connect(
        &mut self,
        direction: super::port::Direction,
        port_name: Arc<str>,
    ) -> Result<(), app::Error> {
        use super::port::Direction;
        match direction {
            Direction::In => {
                let ctrl_surf_name = self.ctrl_surf_widget.lock().unwrap().cur.clone();
                let ctrl_surf = crate::ctrl_surf::FACTORY
                    .build(&ctrl_surf_name)
                    .unwrap_or_else(|| panic!("Unknown Control Surface {}", ctrl_surf_name));

                self.ctrl_surf = Some(ctrl_surf.clone());

                let ctrl_surf_tx = self.ctrl_surf_tx.clone();
                let callback = move |msg_res| match msg_res {
                    Ok(msg) => {
                        let resp = ctrl_surf.lock().unwrap().msg_from_device(msg);
                        let _ = ctrl_surf_tx.send(resp);
                    }
                    Err(err) => {
                        log::error!("Error parsing Midi message: {err}");
                    }
                };
                self.midi_ports_in.connect(port_name, callback)?;
            }
            Direction::Out => {
                self.midi_ports_out.connect(port_name)?;
            }
        }

        self.refresh_ports()?;

        Ok(())
    }

    fn disconnect(&mut self, direction: super::port::Direction) -> Result<(), app::Error> {
        use super::port::Direction::*;
        match direction {
            In => self.midi_ports_in.disconnect(),
            Out => self.midi_ports_out.disconnect(),
        }
        self.refresh_ports()?;

        Ok(())
    }

    fn refresh_ports(&mut self) -> Result<(), app::Error> {
        self.midi_ports_in.refresh()?;
        self.midi_ports_out.refresh()?;
        self.ports_widget
            .lock()
            .unwrap()
            .update(&self.midi_ports_in, &self.midi_ports_out);

        Ok(())
    }

    fn handle_ctrl_surf_resp(&mut self, resp: ctrl_surf::Response) -> Result<(), app::Error> {
        use CtrlSurfEvent::*;

        let (event, msg_list) = resp.into_inner();

        if let Some(event) = event {
            log::info!("Ctrl surf: {event:?}");

            match event {
                Transport(_) => self.players.handle_event(event)?,
                Mixer(ref mixer) => {
                    use ctrl_surf::event::Mixer::*;
                    match mixer {
                        Volume(_) => self.players.handle_event(event)?,
                        Mute => log::warn!("Attempt to mute (unimplemented)"),
                    }
                }
            }
        }

        for msg in msg_list {
            let _ = self.midi_ports_out.send(&msg);
        }

        Ok(())
    }

    fn feed_ctrl_surf_back(&mut self, event: impl Into<ctrl_surf::event::Feedback>) {
        if let Some(ref ctrl_surf) = self.ctrl_surf {
            let resp = ctrl_surf.lock().unwrap().event_to_device(event.into());

            let _ = self.handle_ctrl_surf_resp(resp);
        }
    }

    fn handle_player_event(&mut self, event: ctrl_surf::event::Feedback) -> Result<(), app::Error> {
        use ctrl_surf::event::{Data::*, Feedback::*, Transport::Stop};

        match event {
            Transport(Stop) => {
                log::info!("Player: Stop");
                self.feed_ctrl_surf_back(Stop);
                self.refresh_players()?;
                self.player_widget.lock().unwrap().reset_data();
                self.must_repaint = true;
            }
            Data(Track(ref track)) => {
                log::debug!("Player: Track {:?} - {:?}", track.artist, track.title);
                self.player_widget.lock().unwrap().update_track(track);
                self.feed_ctrl_surf_back(event);
            }
            Data(Timecode(tc)) => {
                log::trace!("Player: {event:?}");
                self.player_widget.lock().unwrap().update_position(tc);
                self.feed_ctrl_surf_back(event);
                self.must_repaint = true;
            }
            _ => {
                log::debug!("Player: {event:?}");
                self.feed_ctrl_surf_back(event);
            }
        }

        Ok(())
    }

    fn refresh_players(&mut self) -> Result<(), app::Error> {
        self.players.refresh()?;
        self.player_widget
            .lock()
            .unwrap()
            .update_players(&self.players);

        Ok(())
    }

    fn run_loop(
        mut self,
        req_rx: channel::Receiver<app::Request>,
        evt_rx: channel::Receiver<ctrl_surf::event::Feedback>,
        ctrl_surf_rx: channel::Receiver<ctrl_surf::Response>,
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
                recv(ctrl_surf_rx) -> ctrl_resp => {
                    match ctrl_resp {
                        Ok(resp) => match self.handle_ctrl_surf_resp(resp) {
                            Ok(()) => (),
                            Err(err) => {
                                log::error!("{err}");
                                let _ = self.err_tx.send(err);
                            }
                        },
                        Err(err) => {
                            log::error!("Error control surface event channel: {err}");
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
