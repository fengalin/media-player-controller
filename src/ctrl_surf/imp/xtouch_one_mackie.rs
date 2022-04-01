use std::sync::{Arc, Mutex};

use crate::{
    ctrl_surf::{
        self,
        event::{self, *},
        Error, Msg,
    },
    midi,
};

const MACKIE_ID: [u8; 3] = [0x00, 0x00, 0x66];
const MCU_ID: u8 = 0x14;
const QUERY_STATUS: u8 = 0x00;

const IDENTIFICATION_DATA: [u8; MACKIE_ID.len() + 2] = [
    MACKIE_ID[0],
    MACKIE_ID[1],
    MACKIE_ID[2],
    MCU_ID,
    QUERY_STATUS,
];

mod button {
    use crate::midi::Tag;
    pub const TAG: Tag = Tag::from(0x90);

    pub const PRESSED: u8 = 127;
    pub const RELEASED: u8 = 0;
    pub const ON: u8 = PRESSED;
    pub const OFF: u8 = RELEASED;

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

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    AwaitingDeviceId,
    Connected,
    Disconnected,
    Playing,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
enum FaderState {
    Released,
    Touched { last_volume: Option<f64> },
}

#[derive(Debug)]
pub struct XTouchOneMackie {
    last_tc: TimecodeBreakDown,
    chan: midi::Channel,
    state: State,
    fader_state: FaderState,
}

impl Default for XTouchOneMackie {
    fn default() -> Self {
        Self {
            last_tc: TimecodeBreakDown::default(),
            chan: midi::Channel::default(),
            state: State::Disconnected,
            fader_state: FaderState::Released,
        }
    }
}

impl crate::ctrl_surf::ControlSurface for XTouchOneMackie {
    fn start_identification(&mut self) -> Vec<Msg> {
        *self = XTouchOneMackie {
            state: State::AwaitingDeviceId,
            ..Default::default()
        };

        log::debug!("Starting device identification");

        midi::Msg::new_sysex(&IDENTIFICATION_DATA)
            .to_device()
            .into()
    }

    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Vec<Msg> {
        let buf = msg.inner();

        if let Some(&tag_chan) = buf.first() {
            self.chan = midi::Channel::from(tag_chan);

            match midi::Tag::from_tag_chan(tag_chan) {
                button::TAG => {
                    if let Some(id_value) = buf.get(1..=2) {
                        use button::*;
                        use Transport::*;

                        match id_value {
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

    fn event_to_device(&mut self, event: Feedback) -> Vec<Msg> {
        if self.state == State::AwaitingDeviceId {
            log::debug!("Ignoring event while awaiting device Id");
            return Msg::none();
        }

        use Feedback::*;
        match event {
            Transport(event) => {
                use event::Transport::*;
                match event {
                    Play => return self.app_play(),
                    Pause => return self.app_pause(),
                    Stop => return self.reset(),
                    _ => (),
                }
            }
            Mixer(mixer) => {
                use event::Mixer::*;
                match mixer {
                    Volume(vol) => return self.app_volume(vol),
                    Mute => (),
                }
            }
            NewApp(app) => {
                log::debug!("New application {app}. Reseting and requesting data");
                let mut msg_list = self.reset();
                msg_list.push(CtrlSurfEvent::DataRequest.into());

                return msg_list;
            }
            Data(data) => {
                use event::Data::*;
                match data {
                    Timecode(tc) => return self.app_timecode(tc),
                    AppName(player) => {
                        log::debug!("got {}", player);
                        // FIXME send to player name to device
                    }
                    Track(_) => (),
                }
            }
        }

        Msg::none()
    }

    fn is_connected(&self) -> bool {
        !matches!(self.state, State::AwaitingDeviceId | State::Disconnected)
    }

    fn reset(&mut self) -> Vec<Msg> {
        use button::*;
        use display_7_seg::*;
        use State::*;

        let mut list = Vec::new();

        let tag_chan = button::TAG | self.chan;
        list.push([tag_chan, PREVIOUS, OFF].into());
        list.push([tag_chan, NEXT, OFF].into());
        list.push([tag_chan, STOP, OFF].into());
        list.push([tag_chan, PLAY, OFF].into());

        for idx in 0..10 {
            list.push([display_7_seg::TAG.into(), TIME_LEFT_DIGIT - idx as u8, b' '].into());
        }

        let state = match self.state {
            Connected | Playing | Stopped => Connected,
            other => other,
        };

        *self = XTouchOneMackie {
            state,
            ..Default::default()
        };

        list
    }
}

/// Device events.
impl XTouchOneMackie {
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
                // FIXME is this a problem or even possible?
                Volume(vol).to_app().into()
            }
        }
    }
}

/// App events.
impl XTouchOneMackie {
    fn app_play(&mut self) -> Vec<Msg> {
        use button::*;
        use State::*;

        let mut list = Vec::new();
        let tag_chan = button::TAG | self.chan;

        match self.state {
            Connected | Stopped => {
                self.state = Playing;
                list.push([tag_chan, STOP, OFF].into());
            }
            Playing => (),
            AwaitingDeviceId | Disconnected => unreachable!(),
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
            Connected | Playing => {
                self.state = Stopped;
                list.push([tag_chan, PLAY, OFF].into());
            }
            Stopped => (),
            AwaitingDeviceId | Disconnected => unreachable!(),
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

    fn app_timecode(&mut self, tc: ctrl_surf::Timecode) -> Vec<Msg> {
        use display_7_seg::*;

        let mut list = Vec::new();
        let tc = TimecodeBreakDown::from(tc);

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
impl XTouchOneMackie {
    fn device_sysex(&mut self, msg: midi::Msg) -> Vec<Msg> {
        if self.state != State::AwaitingDeviceId {
            log::debug!("Ignoring sysex message {}", msg.display());
            return Msg::none();
        }

        let res = self.device_handshake_resp(msg);
        if res.is_err() {
            return Msg::DeviceHandshake(res).into();
        }

        vec![
            Msg::DeviceHandshake(Ok(())),
            CtrlSurfEvent::DataRequest.to_app(),
        ]
    }

    fn device_handshake_resp(&mut self, msg: midi::Msg) -> Result<(), Error> {
        use crate::bytes::Displayable;
        use Error::*;

        self.state = State::Stopped;

        let data = msg.parse_sysex()?;
        if data.len() < 5 {
            return Err(UnexpectedDeviceResponse(msg.display().to_owned()));
        }

        if data[0..3] != MACKIE_ID {
            return Err(ManufacturerMismatch {
                expected: Displayable::from(MACKIE_ID.as_slice()).to_owned(),
                found: Displayable::from(&data[0..3]).to_owned(),
            });
        }

        if data[3] != MCU_ID {
            return Err(DeviceMismatch {
                expected: MCU_ID,
                found: data[0],
            });
        }

        if data[4] != 0x01 {
            return Err(UnexpectedDeviceStatus {
                expected: 0x01,
                found: data[1],
            });
        }

        log::debug!(
            "Device handshake success. Found: {}",
            Displayable::from(&data[5..]),
        );

        self.state = State::Connected;

        Ok(())
    }
}

impl crate::ctrl_surf::Buildable for XTouchOneMackie {
    const NAME: &'static str = "X-Touch One (Makie mode)";

    fn build() -> crate::ctrl_surf::ControlSurfaceArc {
        Arc::new(Mutex::new(Self::default()))
    }
}

#[derive(Debug)]
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
