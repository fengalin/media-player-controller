use crossbeam_channel as channel;

#[cfg(feature = "pulsectl")]
use pulsectl::controllers::{DeviceControl, SinkController};

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::ctrl_surf::{self, CtrlSurfEvent};

const PROGRESS_INTERVAL_MS: u32 = 250;
const LOW_VOLUME: f64 = 0.1f64;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("DBus error")]
    Dbus(#[from] mpris::DBusError),

    #[error("MPRIS event error")]
    Event(#[from] mpris::EventError),

    #[error("MPRIS sending: {}", 0)]
    EventSend(#[from] channel::SendError<Event>),

    #[error("MPRIS event recv: {}", 0)]
    EventRecv(#[from] channel::TryRecvError),

    #[error("Error finding MPRIS player: {}", .0)]
    Finding(#[from] mpris::FindingError),

    #[error("No Players")]
    NoPlayers,

    #[error("Unknown MPRIS player {}", .0)]
    Unknwon(Arc<str>),

    #[cfg(feature = "pulsectl")]
    #[error("Volume controller error")]
    Volume(#[from] pulsectl::ControllerError),
}

pub enum Event {
    PlayerSpawned(Arc<str>),
    Caps(Caps),
    Mixer(ctrl_surf::event::Mixer),
    Data(ctrl_surf::event::Data),
    Transport(ctrl_surf::event::Transport),
}

impl From<ctrl_surf::event::Mixer> for Event {
    fn from(evt: ctrl_surf::event::Mixer) -> Self {
        Self::Mixer(evt)
    }
}

impl From<ctrl_surf::event::Data> for Event {
    fn from(evt: ctrl_surf::event::Data) -> Self {
        Self::Data(evt)
    }
}

impl From<ctrl_surf::data::Track> for Event {
    fn from(evt: ctrl_surf::data::Track) -> Self {
        Self::Data(ctrl_surf::event::Data::Track(evt))
    }
}

impl From<ctrl_surf::event::Transport> for Event {
    fn from(evt: ctrl_surf::event::Transport) -> Self {
        Self::Transport(evt)
    }
}

impl From<mpris::Event> for Event {
    fn from(event: mpris::Event) -> Self {
        use ctrl_surf::{
            event::{Data, Mixer, Transport},
            Track,
        };
        use mpris::Event::*;

        match event {
            Playing => Transport::Play.into(),
            Paused => Transport::Pause.into(),
            Stopped | PlayerShutDown => Transport::Stop.into(),
            VolumeChanged(value) => Mixer::Volume(value).into(),
            TrackChanged(meta) => Data::from(Track::from(meta)).into(),
            _ => {
                log::warn!("Player event {:?}", event);
                Mixer::Mute.into()
            }
        }
    }
}

impl From<mpris::PlaybackStatus> for Event {
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

bitflags::bitflags! {
    pub struct Caps: u16 {
        const SEEK     = 0b00000001;
        const PREVIOUS = 0b00000010;
        const NEXT     = 0b00000100;
        const VOLUME   = 0b00001000;
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Volume {
    Unmuted,
    Muted { prev_vol: f64 },
    Unknown,
}

impl Volume {
    pub fn is_muted(self) -> bool {
        matches!(self, Self::Muted { .. })
    }
}

impl Default for Volume {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug)]
struct CurrentPlayer<'a> {
    name: Arc<str>,
    player: mpris::Player<'a>,
    must_stop: Arc<AtomicBool>,
    volume: Volume,
    caps: Caps,
}

pub struct Players<'a> {
    list: Vec<Arc<str>>,
    cur: Option<CurrentPlayer<'a>>,
    evt_tx: channel::Sender<Event>,
    #[cfg(feature = "pulsectl")]
    volume_controller: SinkController,
    #[cfg(feature = "pulsectl")]
    device_index: u32,
}

struct CurrentPlayerView<'a> {
    player: &'a mpris::Player<'a>,
    volume: &'a mut Volume,
    caps: &'a mut Caps,

    evt_tx: channel::Sender<Event>,
    #[cfg(feature = "pulsectl")]
    volume_controller: &'a mut SinkController,
    #[cfg(feature = "pulsectl")]
    device_index: u32,
}

impl<'a> Players<'a> {
    pub fn try_new() -> Result<(Self, channel::Receiver<Event>), Error> {
        #[cfg(feature = "pulsectl")]
        let (volume_controller, device_index) = {
            let mut volume_controller = SinkController::create()?;
            let default_device = volume_controller.get_default_device()?;

            (volume_controller, default_device.index)
        };

        let (evt_tx, evt_rx) = channel::unbounded();

        Ok((
            Self {
                list: Vec::new(),
                cur: None,
                evt_tx,
                #[cfg(feature = "pulsectl")]
                volume_controller,
                #[cfg(feature = "pulsectl")]
                device_index,
            },
            evt_rx,
        ))
    }

    pub fn refresh(&mut self) -> Result<(), Error> {
        let finder = mpris::PlayerFinder::new()?;
        self.list.clear();

        let mut cur_found = false;
        for player in finder.find_all()? {
            let name: Arc<str> = player.identity().into();
            if let Some(ref cur) = self.cur {
                cur_found |= cur.name == name;
            }

            self.list.push(name);
        }

        if cur_found {
            return Ok(());
        }

        if let Some(ref cur) = self.cur {
            log::debug!("Player {} no longer available", cur.name);
            cur.must_stop.store(true, Ordering::Release);
            self.cur = None;
        }

        if self.list.is_empty() {
            return Ok(());
        }

        let player_name = if let Ok(player) = finder.find_active() {
            player.identity().into()
        } else {
            // Couldn't find any active player, take the first in the list.
            match self.list.get(0).cloned() {
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
        self.cur.as_ref().map(|cur| cur.name.clone())
    }

    fn cur_view<'b>(&'b mut self) -> Option<CurrentPlayerView<'b>>
    where
        'a: 'b,
    {
        self.cur.as_mut().map(|cur| CurrentPlayerView {
            player: &cur.player,
            volume: &mut cur.volume,
            caps: &mut cur.caps,

            evt_tx: self.evt_tx.clone(),
            #[cfg(feature = "pulsectl")]
            volume_controller: &mut self.volume_controller,
            #[cfg(feature = "pulsectl")]
            device_index: self.device_index,
        })
    }

    pub fn list(&self) -> impl Iterator<Item = Arc<str>> + '_ {
        self.list.iter().cloned()
    }

    pub fn set_cur(&mut self, name: Arc<str>) -> Result<(), Error> {
        if !self.list.iter().any(|n| n == &name) {
            return Err(Error::Unknwon(name));
        }

        if let Some(ref mut cur) = self.cur {
            cur.must_stop.store(true, Ordering::Release);

            // Unmute in case cur player muted at the system level.
            #[cfg(feature = "pulsectl")]
            {
                log::info!("Unmuting using system mixer");
                self.volume_controller
                    .set_device_mute_by_index(self.device_index, false);
                self.evt_tx.send(ctrl_surf::event::Mixer::Unmute.into())?;
            }
        }

        let finder = mpris::PlayerFinder::new()?;
        let player = finder.find_by_name(&name)?;

        let must_stop = self.spawn_loops(name.clone());

        self.cur = Some(CurrentPlayer {
            name,
            must_stop,
            player,
            volume: Volume::default(),
            caps: Caps::empty(),
        });

        Ok(())
    }
}

impl<'a> Players<'a> {
    pub fn handle_event(&mut self, event: impl Into<CtrlSurfEvent>) -> Result<(), Error> {
        if let Some(mut cur_view) = self.cur_view() {
            cur_view.handle_event(event)?;
        }

        Ok(())
    }

    pub fn send_all_data(&mut self) -> Result<(), Error> {
        if let Some(mut cur_view) = self.cur_view() {
            cur_view.update_caps()?;
            cur_view.send_all_data()?;
        }

        Ok(())
    }

    pub fn send_track_meta(&mut self) -> Result<(), Error> {
        if let Some(cur_view) = self.cur_view() {
            cur_view.send_track_meta()?;
        }

        Ok(())
    }

    pub fn unmute_system(&mut self) {
        #[cfg(feature = "pulsectl")]
        {
            log::debug!("Unmuting using system mixer");
            self.volume_controller
                .set_device_mute_by_index(self.device_index, false);
        }
        #[cfg(not(feature = "pulsectl"))]
        log::debug!("Unmuting using system mixer not available");
    }
}

impl<'a> CurrentPlayerView<'a> {
    pub fn handle_event(&mut self, event: impl Into<CtrlSurfEvent>) -> Result<(), Error> {
        use CtrlSurfEvent::*;

        match event.into() {
            Transport(event) => {
                use ctrl_surf::event::Transport::*;
                match event {
                    Play => self.player.play()?,
                    Pause | Stop => {
                        // Don't Stop as that leads to no more track being selected.
                        self.player.pause()?;
                    }
                    PlayPause => self.player.play_pause()?,
                    Previous => self.player.previous()?,
                    Next => self.player.next()?,
                    StepForward => todo!(),
                    StepBackward => todo!(),
                    SetPosition(pos) => {
                        let cur_pos = self.player.get_position()?;
                        let target = pos.as_micros() as i64 - cur_pos.as_micros() as i64;
                        self.player.seek(target)?;
                    }
                }
            }
            Mixer(event) => {
                use ctrl_surf::event::Mixer::*;
                match event {
                    Volume(value) => self.player.set_volume(value)?,
                    Mute => self.mute()?,
                    Unmute => self.unmute()?,
                }
            }
            DataRequest => {
                self.update_caps()?;
                self.send_all_data()?;
            }
        }

        Ok(())
    }

    fn send_all_data(&self) -> Result<(), Error> {
        self.evt_tx.send(Event::Caps(*self.caps))?;

        self.evt_tx
            .send(self.player.get_playback_status()?.into())?;

        if let Some(vol) = self.player.checked_get_volume()? {
            self.evt_tx
                .send(ctrl_surf::event::Mixer::Volume(vol).into())?;
        }

        self.send_track_meta()?;

        if let Ok(pos) = self.player.get_position() {
            self.evt_tx
                .send(ctrl_surf::event::Data::Position(pos).into())?;
        }

        Ok(())
    }

    fn send_track_meta(&self) -> Result<(), Error> {
        if let Ok(meta) = self.player.get_metadata() {
            self.evt_tx.send(ctrl_surf::Track::from(meta).into())?;
        }

        Ok(())
    }

    fn update_caps(&mut self) -> Result<(), Error> {
        *self.caps = get_caps(self.player)?;

        Ok(())
    }

    fn mute(&mut self) -> Result<(), Error> {
        use ctrl_surf::event::Mixer;
        use Volume::*;

        match self.volume {
            Unmuted | Unknown => {
                if let Some(vol) = self.player.checked_get_volume()? {
                    if vol < f64::EPSILON {
                        // Already muted, but don't know previous volume so keep it low
                        *self.volume = Muted {
                            prev_vol: LOW_VOLUME,
                        };
                        return Ok(());
                    }

                    *self.volume = Muted { prev_vol: vol };
                    self.player.set_volume(0f64)?;
                    return Ok(());
                }
            }
            Muted { .. } => {
                // Make sure everyone is up to date.
                self.evt_tx.send(Mixer::Mute.into())?;
                return Ok(());
            }
        };

        // Volume couldn't be muted using mpris player.

        #[cfg(feature = "pulsectl")]
        {
            log::debug!("Muting using system mixer");
            self.volume_controller
                .set_device_mute_by_index(self.device_index, true);
            *self.volume = Muted {
                prev_vol: LOW_VOLUME,
            };
            self.evt_tx.send(Mixer::Mute.into())?;
        }

        #[cfg(not(feature = "pulsectl"))]
        log::debug!("Muting using system mixer not available");

        Ok(())
    }

    fn unmute(&mut self) -> Result<(), Error> {
        use ctrl_surf::event::Mixer;
        use Volume::*;

        let can_volume = self.caps.contains(Caps::VOLUME);

        match self.volume {
            Muted { prev_vol } => {
                if can_volume {
                    self.player.set_volume(*prev_vol)?;
                    *self.volume = Unmuted;
                    return Ok(());
                }
            }
            Unknown => {
                if let Some(vol) = self.player.checked_get_volume()? {
                    if vol < f64::EPSILON {
                        // Don't know previous volume so unmute to a low volume
                        self.player.set_volume(LOW_VOLUME)?;
                    }
                    // else already unmuted
                    *self.volume = Unmuted;
                    return Ok(());
                }
            }
            Unmuted => {
                // Make sure everyone is up to date.
                self.evt_tx.send(Mixer::Unmute.into())?;
                return Ok(());
            }
        };

        // Volume couldn't be unmuted using mpris player.

        #[cfg(feature = "pulsectl")]
        {
            log::debug!("Unmuting using system mixer");
            self.volume_controller
                .set_device_mute_by_index(self.device_index, false);
            *self.volume = Unmuted;
            self.evt_tx.send(Mixer::Unmute.into())?;
        }

        #[cfg(not(feature = "pulsectl"))]
        log::debug!("Unmuting using system mixer not available");

        Ok(())
    }
}

impl<'a> Players<'a> {
    fn spawn_loops(&mut self, name: Arc<str>) -> Arc<AtomicBool> {
        let must_stop = Arc::new(AtomicBool::new(false));

        let evt_tx = self.evt_tx.clone();
        let must_stop_cl = must_stop.clone();
        let name_cl = name.clone();
        log::debug!("Spawning event loop for MPRIS player {name}");
        std::thread::spawn(move || {
            if let Err(err) = Self::event_loop(name_cl, evt_tx, must_stop_cl) {
                log::error!("MPRIS Player event loop: {err}");
            }
        });

        let evt_tx = self.evt_tx.clone();
        let must_stop_cl = must_stop.clone();
        log::debug!("Spawning progress loop for MPRIS player {name}");
        std::thread::spawn(move || {
            if let Err(err) = Self::progress_loop(name, evt_tx, must_stop_cl) {
                log::error!("MPRIS Player progress loop: {err}");
            }
        });

        must_stop
    }

    fn event_loop(
        name: Arc<str>,
        evt_tx: channel::Sender<Event>,
        stopper: Arc<AtomicBool>,
    ) -> Result<(), Error> {
        let finder = mpris::PlayerFinder::new()?;
        let player = finder.find_by_name(&name)?;

        evt_tx.send(Event::PlayerSpawned(name))?;

        // events.next() is blocking...
        for event in player.events()? {
            if stopper.load(Ordering::Acquire) {
                break;
            }

            match event? {
                mpris::Event::Playing => {
                    let caps = get_caps(&player)?;
                    evt_tx.send(Event::Caps(caps))?;
                    evt_tx.send(ctrl_surf::event::Transport::Play.into())?;
                }
                mpris::Event::VolumeChanged(vol) if vol > f64::EPSILON => {
                    evt_tx.send(ctrl_surf::event::Mixer::Volume(vol).into())?;
                }
                mpris::Event::VolumeChanged(_) => {
                    evt_tx.send(ctrl_surf::event::Mixer::Mute.into())?;
                }
                event => evt_tx.send(event.into())?,
            }
        }

        Ok(())
    }

    fn progress_loop(
        player_name: Arc<str>,
        evt_tx: channel::Sender<Event>,
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
                let _ = evt_tx.send(ctrl_surf::event::Transport::Stop.into());
                break;
            }

            let pos = tick.progress.position();
            if last_pos != pos {
                evt_tx.send(ctrl_surf::event::Data::Position(pos).into())?;
                last_pos = pos;
            }
        }

        Ok(())
    }
}

fn get_caps(player: &mpris::Player<'_>) -> Result<Caps, Error> {
    let mut caps = Caps::empty();
    if player.can_seek()? {
        caps.insert(Caps::SEEK);
    }
    if player.can_go_previous()? {
        caps.insert(Caps::PREVIOUS);
    }
    if player.can_go_next()? {
        caps.insert(Caps::NEXT);
    }

    if let Some(vol) = player.checked_get_volume()? {
        // Try to set volume to same value to check if players supports it.
        if player.checked_set_volume(vol)? {
            caps.insert(Caps::VOLUME);
        }
    }

    Ok(caps)
}

impl From<mpris::Metadata> for ctrl_surf::Track {
    fn from(meta: mpris::Metadata) -> Self {
        let artist = meta
            .artists()
            .and_then(|artists| artists.first().map(|artist| Arc::from(*artist)));

        let image_url = meta.art_url().map(Arc::from);

        ctrl_surf::Track {
            artist,
            album: meta.album_name().map(Arc::from),
            title: meta.title().map(Arc::from),
            duration: meta.length(),
            image_url,
        }
    }
}
