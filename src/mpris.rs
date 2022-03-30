use crossbeam_channel as channel;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::ctrl_surf::{self, event::CtrlSurfEvent};

const PROGRESS_INTERVAL_MS: u32 = 250;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("DBus error")]
    Dbus(#[from] mpris::DBusError),

    #[error("MPRIS event error")]
    Event(#[from] mpris::EventError),

    #[error("MPRIS sending: {}", 0)]
    EventSend(#[from] channel::SendError<ctrl_surf::event::Feedback>),

    #[error("MPRIS event recv: {}", 0)]
    EventRecv(#[from] channel::TryRecvError),

    #[error("Error finding MPRIS player: {}", .0)]
    Finding(#[from] mpris::FindingError),

    #[error("No Players")]
    NoPlayers,

    #[error("Unknown MPRIS player {}", .0)]
    Unknwon(Arc<str>),
}

#[derive(Debug)]
pub struct Players<'a> {
    map: BTreeMap<Arc<str>, mpris::Player<'a>>,
    cur: Option<(Arc<str>, Arc<AtomicBool>)>,
    evt_tx: channel::Sender<ctrl_surf::event::Feedback>,
}

impl<'a> Players<'a> {
    pub fn new() -> (Self, channel::Receiver<ctrl_surf::event::Feedback>) {
        let (evt_tx, evt_rx) = channel::unbounded();

        (
            Self {
                map: BTreeMap::new(),
                cur: None,
                evt_tx,
            },
            evt_rx,
        )
    }

    pub fn refresh(&mut self) -> Result<(), Error> {
        let finder = mpris::PlayerFinder::new()?;
        self.map.clear();

        for player in finder.find_all()? {
            self.map.insert(player.identity().into(), player);
        }

        if let Some(ref cur) = self.cur {
            if self.map.contains_key(&cur.0) {
                return Ok(());
            }

            log::debug!("Player {} no longer available", cur.0);
            cur.1.store(true, Ordering::Release);
            self.cur = None;

            if self.map.is_empty() {
                return Ok(());
            }
        }

        let player_name = if let Ok(player) = finder.find_active() {
            player.identity().into()
        } else {
            // Couldn't find any active player, take the first in the list.
            match self.map.keys().next().cloned() {
                Some(player_name) => player_name,
                None => return Ok(()),
            }
        };

        self.set_cur(player_name)
    }

    pub fn has_cur(&self) -> bool {
        self.cur.is_some()
    }

    pub fn cur(&self) -> Option<Arc<str>> {
        self.cur.as_ref().map(|cur| cur.0.clone())
    }

    pub fn list(&self) -> impl Iterator<Item = Arc<str>> + '_ {
        self.map.keys().cloned()
    }

    pub fn set_cur(&mut self, player_name: Arc<str>) -> Result<(), Error> {
        if !self.map.contains_key(&player_name) {
            return Err(Error::Unknwon(player_name));
        }

        if let Some(ref mut cur) = self.cur {
            cur.1.store(true, Ordering::Release);
        }

        let stopper = self.spawn_loops(player_name.clone());
        self.cur = Some((player_name, stopper));

        Ok(())
    }
}

impl<'a> Players<'a> {
    pub fn handle_event(&self, event: impl Into<CtrlSurfEvent>) -> Result<(), Error> {
        match self.cur.as_ref() {
            Some(cur) => match self.map.get(&cur.0) {
                Some(player) => self.handle_event_(player, event),
                None => Err(Error::Unknwon(cur.0.clone())),
            },
            None => Ok(()),
        }
    }

    fn handle_event_(
        &self,
        player: &mpris::Player<'a>,
        event: impl Into<CtrlSurfEvent>,
    ) -> Result<(), Error> {
        use CtrlSurfEvent::*;

        match event.into() {
            Transport(event) => {
                use ctrl_surf::event::Transport::*;
                match event {
                    Play => player.play()?,
                    Pause | Stop => {
                        // Don't Stop as that leads to no more track being selected.
                        player.pause()?;
                    }
                    PlayPause => player.play_pause()?,
                    Previous => player.previous()?,
                    Next => player.next()?,
                    StepForward => todo!(),
                    StepBackward => todo!(),
                }
            }
            Mixer(event) => {
                use ctrl_surf::event::Mixer::*;
                match event {
                    Volume(value) => player.set_volume(value)?,
                    Mute => unimplemented!("Not available on mpris::Player"),
                }
            }
            Identification(_) => (),
        }

        Ok(())
    }
}

impl<'a> Players<'a> {
    fn spawn_loops(&mut self, player_name: Arc<str>) -> Arc<AtomicBool> {
        let stopper = Arc::new(AtomicBool::new(false));

        let evt_tx = self.evt_tx.clone();
        let stopper_cl = stopper.clone();
        let player_name_cl = player_name.clone();
        log::debug!("Spawning event loop for MPRIS player {player_name}");
        std::thread::spawn(move || {
            if let Err(err) = Self::event_loop(player_name_cl, evt_tx, stopper_cl) {
                log::error!("MPRIS Player event loop: {err}");
            }
        });

        let evt_tx = self.evt_tx.clone();
        let stopper_cl = stopper.clone();
        log::debug!("Spawning progress loop for MPRIS player {player_name}");
        std::thread::spawn(move || {
            if let Err(err) = Self::progress_loop(player_name, evt_tx, stopper_cl) {
                log::error!("MPRIS Player progress loop: {err}");
            }
        });

        stopper
    }

    fn event_loop(
        player_name: Arc<str>,
        evt_tx: channel::Sender<ctrl_surf::event::Feedback>,
        stopper: Arc<AtomicBool>,
    ) -> Result<(), Error> {
        let finder = mpris::PlayerFinder::new()?;
        let player = finder.find_by_name(&player_name)?;

        evt_tx.send(ctrl_surf::event::Data::Player(player_name).into())?;
        evt_tx.send(player.get_playback_status()?.into())?;

        if let Ok(vol) = player.get_volume() {
            evt_tx.send(ctrl_surf::event::Mixer::Volume(vol).into())?;
        }

        if let Ok(meta) = player.get_metadata() {
            evt_tx.send(ctrl_surf::Track::from(meta).into())?;
        }

        if let Ok(pos) = player.get_position() {
            evt_tx.send(ctrl_surf::Timecode::from(pos).into())?;
        }

        // events.next() is blocking...
        for event in player.events()? {
            if stopper.load(Ordering::Acquire) {
                break;
            }

            evt_tx.send(event?.into())?;
        }

        Ok(())
    }

    fn progress_loop(
        player_name: Arc<str>,
        evt_tx: channel::Sender<ctrl_surf::event::Feedback>,
        stopper: Arc<AtomicBool>,
    ) -> Result<(), Error> {
        let finder = mpris::PlayerFinder::new()?;
        let player = finder.find_by_name(&player_name)?;

        let mut progress = player.track_progress(PROGRESS_INTERVAL_MS)?;
        let mut last_pos = std::time::Duration::MAX;
        loop {
            if stopper.load(Ordering::Acquire) {
                break;
            }

            let tick = progress.tick();
            if tick.player_quit {
                break;
            }

            let pos = tick.progress.position();
            if last_pos != pos {
                evt_tx.send(ctrl_surf::Timecode::from(pos).into())?;
                last_pos = pos;
            }
        }

        Ok(())
    }
}

impl From<mpris::Event> for ctrl_surf::event::Feedback {
    fn from(event: mpris::Event) -> Self {
        use ctrl_surf::{
            event::{Mixer, Transport},
            Track,
        };
        use mpris::Event::*;

        match event {
            Playing => Transport::Play.into(),
            Paused => Transport::Pause.into(),
            Stopped | PlayerShutDown => Transport::Stop.into(),
            VolumeChanged(value) => Mixer::Volume(value).into(),
            TrackChanged(meta) => Track::from(meta).into(),
            _ => {
                log::warn!("Player event {:?}", event);
                // FIXME
                Mixer::Mute.into()
            }
        }
    }
}

impl From<mpris::PlaybackStatus> for ctrl_surf::event::Feedback {
    fn from(status: mpris::PlaybackStatus) -> Self {
        use ctrl_surf::event::Transport;
        use mpris::PlaybackStatus::*;

        match status {
            Playing => Transport::Play.into(),
            Paused => Transport::Pause.into(),
            Stopped => Transport::Stop.into(),
        }
    }
}

impl From<mpris::Metadata> for ctrl_surf::Track {
    fn from(meta: mpris::Metadata) -> Self {
        let artist = meta
            .artists()
            .and_then(|artists| artists.first().map(|artist| Arc::from(*artist)));

        let image = meta.art_url().as_ref().and_then(|image_url| {
            let image_path = image_url.trim_start_matches("file://");
            image::io::Reader::open(image_path)
                .map_err(|_| ())
                .and_then(|reader| reader.decode().map_err(|_| ()).map(Arc::from))
                .ok()
        });

        ctrl_surf::Track {
            artist,
            title: meta.title().map(Arc::from),
            image,
        }
    }
}
