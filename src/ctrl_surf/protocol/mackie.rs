use once_cell::sync::Lazy;
use std::{sync::Arc, time::Duration};

use crate::{
    ctrl_surf::{
        self,
        event::{self, *},
        Error, Msg, Timecode,
    },
    midi,
};

mod connection {
    pub const MACKIE_ID: [u8; 3] = [0x00, 0x00, 0x66];

    pub const LOGIC_CONTROL_ID: u8 = 0x10;
    pub const LOGIC_CONTROL_EXT_ID: u8 = 0x11;

    pub const QUERY_DEVICE: u8 = 0x00;
    pub const QUERY_HOST: u8 = 0x01;
    pub const HOST_REPLY: u8 = 0x02;
    pub const DEVICE_OK: u8 = 0x03;
    pub const DEVICE_ERR: u8 = 0x04;

    // For some reasons, sending these two doesn't work with XTouch-One
    //pub const RESET_FADERS: u8 = 0x61;
    //pub const RESET_LEDS: u8 = 0x62;

    // TODO?
    //pub const GO_OFFLINE: u8 = 0x0f;
}

mod button {
    use crate::midi::Tag;
    pub const TAG: Tag = Tag::from(0x90);

    pub const PRESSED: u8 = 127;
    pub const RELEASED: u8 = 0;
    pub const ON: u8 = PRESSED;
    pub const OFF: u8 = RELEASED;

    pub const MUTE: u8 = 16;
    pub const PREVIOUS: u8 = 91;
    pub const NEXT: u8 = 92;
    pub const STOP: u8 = 93;
    pub const PLAY: u8 = 94;
    pub const FADER_TOUCHED: u8 = 104;
}

mod display_7_seg {
    use crate::midi::Tag;
    pub const TAG: Tag = Tag::from(0xb0);

    pub const TIME_LEFT_DIGIT: u8 = 0x49;
}

mod fader {
    use crate::midi::Tag;
    pub const TAG: Tag = Tag::from(0xe0);

    pub const TOUCH_THRSD: u8 = 64;
}

static NO_APP: Lazy<Arc<str>> = Lazy::new(|| "_NOAPP_".into());

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Connecting(ConnectionStatus),
    PendingAppData,
    Connected,
    Disconnected,
    Playing,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ConnectionStatus {
    DeviceQueried,
    ChallengeReplied,
}

#[derive(Clone, Copy, Debug)]
enum FaderState {
    Released,
    Touched { last_volume: Option<f64> },
}

#[derive(Debug)]
pub struct Mackie {
    device_id: u8,
    last_tc: TimecodeBreakDown,
    chan: midi::Channel,
    state: State,
    is_muted: bool,
    fader_state: FaderState,
    app: Arc<str>,
}

impl Mackie {
    pub fn new(device_id: u8) -> Self {
        Self {
            device_id,
            last_tc: TimecodeBreakDown::default(),
            chan: midi::Channel::default(),
            state: State::Disconnected,
            is_muted: false,
            fader_state: FaderState::Released,
            app: NO_APP.clone(),
        }
    }
}

impl crate::ctrl_surf::ControlSurface for Mackie {
    fn start_connection(&mut self) -> Vec<Msg> {
        use connection::*;

        log::debug!("Attempt to connect to device {:#02x}", self.device_id);

        self.state = State::Connecting(ConnectionStatus::DeviceQueried);

        vec![
            midi::Msg::new_sysex(&self.payload_for(QUERY_DEVICE)).to_device(),
            Msg::connetion_in_progress(),
        ]
    }

    fn abort_connection(&mut self) -> Vec<Msg> {
        if let State::Connecting(_) = self.state {
            log::debug!("Aborting connection to device {:#02x}", self.device_id);

            self.state = State::Disconnected;
        }

        Msg::none()
    }

    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Vec<Msg> {
        let buf = msg.inner();

        if let Some(&tag_chan) = buf.first() {
            self.chan = midi::Channel::from(tag_chan);

            match midi::Tag::from_tag_chan(tag_chan) {
                button::TAG => {
                    if let Some(id_value) = buf.get(1..=2) {
                        use button::*;
                        use Mixer::*;
                        use Transport::*;

                        match id_value {
                            [MUTE, PRESSED] => {
                                if self.is_muted {
                                    return Unmute.to_app().into();
                                } else {
                                    return Mute.to_app().into();
                                }
                            }
                            [PREVIOUS, PRESSED] => return Previous.to_app().into(),
                            [NEXT, PRESSED] => return Next.to_app().into(),
                            [STOP, PRESSED] => return Stop.to_app().into(),
                            [PLAY, PRESSED] => return PlayPause.to_app().into(),
                            [FADER_TOUCHED, value] => return self.device_fader_touch(*value),
                            _ => (),
                        }
                    }
                }
                fader::TAG => {
                    if let Some(value) = buf.get(1..=2) {
                        return self.device_fader_moved(value);
                    }
                }
                midi::sysex::TAG => return self.device_sysex(msg),
                _ => (),
            }
        }

        Msg::none()
    }

    fn event_from_app(&mut self, event: AppEvent) -> Vec<Msg> {
        if !self.is_connected() {
            log::debug!("Ignoring App event: Control surface not connected.");
            return Msg::none();
        }

        use AppEvent::*;
        match event {
            Transport(event) => {
                use event::Transport::*;
                match event {
                    Play => return self.app_play(),
                    Pause => return self.app_pause(),
                    Stop => {
                        // FIXME go offline
                        return self.reset();
                    }
                    _ => (),
                }
            }
            Mixer(mixer) => {
                use event::Mixer::*;
                match mixer {
                    Volume(vol) => return self.app_volume(vol),
                    Mute => return self.app_mute(),
                    Unmute => return self.app_unmute(),
                }
            }
            NewApp(app) => {
                let msg_list = if app != self.app {
                    log::debug!("New application {app}");

                    self.app = app;
                    self.state = State::PendingAppData;
                    // FIXME send player name to device

                    CtrlSurfEvent::DataRequest.to_app().into()
                } else {
                    Msg::none()
                };

                return msg_list;
            }
            Data(data) => {
                use event::Data::*;

                if self.state == State::PendingAppData {
                    self.state = State::Connected;
                }

                match data {
                    Position(pos) => return self.app_position(pos),
                    Track(_) => (),
                    PlaybackStatus(status) => {
                        use crate::ctrl_surf::data::PlaybackStatus::*;

                        match status {
                            Playing => return self.app_play(),
                            Paused => return self.app_pause(),
                            Stopped => return self.reset(),
                        }
                    }
                }
            }
        }

        Msg::none()
    }

    fn is_connected(&self) -> bool {
        !matches!(self.state, State::Connecting(_) | State::Disconnected)
    }

    fn reset(&mut self) -> Vec<Msg> {
        use button::*;
        use display_7_seg::*;
        use State::*;

        let mut list = Vec::new();

        let tag_chan = button::TAG | self.chan;
        list.push([tag_chan, MUTE, OFF].into());
        list.push([tag_chan, PREVIOUS, OFF].into());
        list.push([tag_chan, NEXT, OFF].into());
        list.push([tag_chan, STOP, OFF].into());
        list.push([tag_chan, PLAY, OFF].into());

        for idx in 0..10 {
            list.push([display_7_seg::TAG.into(), TIME_LEFT_DIGIT - idx as u8, b' '].into());
        }

        self.state = match self.state {
            Connected | Playing | Stopped => Connected,
            other => other,
        };
        self.is_muted = false;
        self.last_tc = TimecodeBreakDown::default();
        self.app = NO_APP.clone();

        list
    }
}

/// Device events.
impl Mackie {
    fn build_fader_msg(&self, vol: f64) -> Msg {
        let two_bytes = midi::normalized_f64::to_be(vol).unwrap();
        [fader::TAG | self.chan, two_bytes[0], two_bytes[1]].into()
    }

    fn device_fader_touch(&mut self, value: u8) -> Vec<Msg> {
        use FaderState::*;
        use Mixer::*;

        let is_touched = value > fader::TOUCH_THRSD;
        match self.fader_state {
            Released if is_touched => {
                self.fader_state = Touched { last_volume: None };
            }
            Touched { last_volume } if !is_touched => {
                self.fader_state = Released;
                if let Some(vol) = last_volume {
                    return vec![Volume(vol).to_app(), self.build_fader_msg(vol)];
                }
            }
            _ => (),
        }

        Msg::none()
    }

    fn device_fader_moved(&mut self, buf: &[u8]) -> Vec<Msg> {
        use FaderState::*;
        use Mixer::*;

        let vol = match midi::normalized_f64::from_be(buf) {
            Ok(value) => value,
            Err(err) => {
                log::error!("Fader moved value: {err}");
                return Msg::none();
            }
        };

        match &mut self.fader_state {
            Touched { last_volume } => {
                *last_volume = Some(vol);
                Volume(vol).to_app().into()
            }
            Released => {
                // This could indicate that the fader is moved with
                // an object or finger nail, which would cause the servo motor
                // to struggle when the app replies with the new volume.
                log::warn!("Fader moved but no touch detected => ignoring");

                Msg::none()
            }
        }
    }
}

/// App events.
impl Mackie {
    fn app_mute(&mut self) -> Vec<Msg> {
        use button::*;

        self.is_muted = true;
        Msg::from([TAG | self.chan, MUTE, ON]).into()
    }

    fn app_unmute(&mut self) -> Vec<Msg> {
        use button::*;

        self.is_muted = false;
        Msg::from([TAG | self.chan, MUTE, OFF]).into()
    }

    fn app_play(&mut self) -> Vec<Msg> {
        use button::*;
        use State::*;

        let mut list = Vec::new();
        let tag_chan = button::TAG | self.chan;

        match self.state {
            Connected | PendingAppData | Stopped => {
                self.state = Playing;
                list.push([tag_chan, STOP, OFF].into());
            }
            Playing => (),
            Connecting(_) | Disconnected => unreachable!(),
        }

        list.push([tag_chan, PLAY, ON].into());

        list
    }

    fn app_pause(&mut self) -> Vec<Msg> {
        use button::*;
        use State::*;

        let mut list = Vec::new();
        let tag_chan = button::TAG | self.chan;

        match self.state {
            Connected | PendingAppData | Playing => {
                self.state = Stopped;
                list.push([tag_chan, PLAY, OFF].into());
            }
            Stopped => (),
            Connecting(_) | Disconnected => unreachable!(),
        }

        list.push([tag_chan, STOP, ON].into());

        list
    }

    fn app_volume(&mut self, vol: f64) -> Vec<Msg> {
        use FaderState::*;

        match &mut self.fader_state {
            Released => self.build_fader_msg(vol).into(),
            Touched { last_volume } => {
                // user touches fader => don't move it before it's released.
                *last_volume = Some(vol);

                Msg::none()
            }
        }
    }

    fn app_position(&mut self, pos: Duration) -> Vec<Msg> {
        use display_7_seg::*;

        let mut list = Vec::new();
        let tc = TimecodeBreakDown::from(Timecode::from(pos));

        for (idx, (&last_digit, digit)) in self.last_tc.0.iter().zip(tc.0).enumerate() {
            if last_digit != digit {
                list.push([TAG.into(), TIME_LEFT_DIGIT - idx as u8, digit].into());
            }
        }

        self.last_tc = tc;

        list
    }
}

/// Device handshake.
impl Mackie {
    fn device_sysex(&mut self, msg: midi::Msg) -> Vec<Msg> {
        self.device_connection(msg)
            .unwrap_or_else(|err| Msg::from_connection_result(Err(err)).into())
    }

    fn device_connection(&mut self, msg: midi::Msg) -> Result<Vec<Msg>, Error> {
        use crate::bytes::Displayable;
        use connection::*;
        use Error::*;

        let payload = msg.parse_sysex()?;

        // Check header
        if payload.len() < 5 {
            return Err(UnexpectedDeviceMsg(msg.display().to_owned()));
        }

        if payload[0..3] != MACKIE_ID {
            return Err(ManufacturerMismatch {
                expected: Displayable::from(MACKIE_ID.as_slice()).to_owned(),
                found: Displayable::from(&payload[0..3]).to_owned(),
            });
        }

        if self.device_id != payload[3] {
            let err = DeviceIdMismatch {
                expected: self.device_id,
                found: payload[3],
            };
            log::debug!("{err}");
            return Err(err);
        }

        let msg_list = match (payload[4], payload.get(5..)) {
            (QUERY_HOST, Some(serial_challenge)) => self
                .device_query_host(serial_challenge)
                .map_err(|_| UnexpectedDeviceMsg(msg.display().to_owned()))?,
            (DEVICE_OK, Some(_serial)) => self.device_connected(),
            (DEVICE_ERR, Some(_serial)) => {
                self.state = State::Disconnected;
                let err = ConnectionError;
                log::debug!("{err}");
                return Err(err);
            }
            _ => {
                self.state = State::Disconnected;
                let err = UnexpectedDeviceMsg(msg.display().to_owned());
                log::debug!("{err}");
                return Err(err);
            }
        };

        Ok(msg_list)
    }

    fn device_query_host(&mut self, serial_challenge: &[u8]) -> Result<Vec<Msg>, ()> {
        use connection::*;

        let (ser, chlg) = serial_challenge
            .get(..7)
            .zip(serial_challenge.get(7..11))
            .ok_or_else(|| {
                self.state = State::Disconnected;
                log::error!("Device QUERY HOST: invalid serial / challenge");
            })?;

        let msg_list =
            if self.device_id == LOGIC_CONTROL_ID || self.device_id == LOGIC_CONTROL_EXT_ID {
                let mut resp = [0u8; 5 + 7 + 4];

                self.prepare_payload(&mut resp, HOST_REPLY);
                resp[5..12].copy_from_slice(ser);
                resp[12] = 0x7F & (chlg[0] + (chlg[1] ^ 0x0a) - chlg[3]);
                resp[13] = 0x7F & ((chlg[2] >> 4) ^ (chlg[0] + chlg[3]));
                resp[14] = 0x7F & ((chlg[3] - (chlg[2] << 2)) ^ (chlg[0] | chlg[1]));
                resp[15] = 0x7F & (chlg[1] - chlg[2] + (0xf0 ^ (chlg[3] << 4)));

                self.state = State::Connecting(ConnectionStatus::ChallengeReplied);
                log::debug!("Device QUERY HOST challenge replied");

                vec![
                    midi::Msg::new_sysex(&resp).to_device(),
                    Msg::connetion_in_progress(),
                ]
            } else {
                // No need for a challenge reply
                self.device_connected()
            };

        Ok(msg_list)
    }

    fn device_connected(&mut self) -> Vec<Msg> {
        log::debug!("Connected to device {:#02x}", self.device_id);
        self.state = State::PendingAppData;

        vec![
            Msg::from_connection_result(Ok(())),
            CtrlSurfEvent::DataRequest.to_app(),
        ]
    }

    fn payload_for(&self, req_id: u8) -> [u8; 5] {
        let mut payload = [0u8; 5];
        self.prepare_payload(&mut payload, req_id);

        payload
    }

    fn prepare_payload(&self, payload: &mut [u8], req_id: u8) {
        payload[..=2].copy_from_slice(&connection::MACKIE_ID);
        payload[3] = self.device_id;
        payload[4] = req_id;
    }
}

#[derive(Clone, Copy, Debug)]
struct TimecodeBreakDown([u8; 10]);

impl Default for TimecodeBreakDown {
    fn default() -> Self {
        Self([b' '; 10])
    }
}

impl From<ctrl_surf::Timecode> for TimecodeBreakDown {
    fn from(tc: ctrl_surf::Timecode) -> Self {
        use std::io::Write;

        let printable = format!("{:>13.3}", tc);
        let bytes = printable.as_bytes();

        let mut this = Self::default();

        let mut cur = std::io::Cursor::new(this.0.as_mut_slice());
        cur.write_all(&bytes[..=2]).unwrap();
        cur.write_all(&bytes[4..=5]).unwrap();
        cur.write_all(&bytes[7..=8]).unwrap();
        cur.write_all(&bytes[10..=12]).unwrap();

        this
    }
}
