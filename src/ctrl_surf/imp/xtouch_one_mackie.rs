use std::sync::{Arc, Mutex};

use crate::{
    ctrl_surf::{
        self,
        event::{self, *},
        Error, Response,
    },
    midi::{self, MsgList},
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

#[derive(PartialEq)]
enum State {
    Playing,
    Stopped,
    AwaitingDeviceId,
}

#[derive(Clone, Copy)]
enum FaderState {
    Released,
    Touched { last_volume: Option<f64> },
}

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
            state: State::Stopped,
            fader_state: FaderState::Released,
        }
    }
}

impl crate::ctrl_surf::ControlSurface for XTouchOneMackie {
    fn start_identification(&mut self) -> Response {
        self.reset();
        self.state = State::AwaitingDeviceId;

        log::debug!("Starting device identification");

        Response::from_msg(midi::Msg::new_sysex(&IDENTIFICATION_DATA))
    }

    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Response {
        let buf = msg.inner();

        if let Some(&tag_chan) = buf.first() {
            self.chan = midi::Channel::from(tag_chan);

            match midi::Tag::from_tag_chan(tag_chan) {
                button::TAG => {
                    if let Some(id_value) = buf.get(1..=2) {
                        use button::*;
                        use Transport::*;

                        match id_value {
                            [PREVIOUS, PRESSED] => return Response::from_event(Previous),
                            [NEXT, PRESSED] => return Response::from_event(Next),
                            [STOP, PRESSED] => return Response::from_event(Stop),
                            [PLAY, PRESSED] => return Response::from_event(PlayPause),
                            [FADER_TOUCHED, value] => return self.fader_touch(*value),
                            _ => (),
                        }
                    }
                }
                fader::TAG => {
                    if let Some(value) = buf.get(1..=2) {
                        return self.fader_moved(value);
                    }
                }
                midi::sysex::TAG => return self.handle_sysex_msg(msg),
                _ => (),
            }
        }

        Response::none()
    }

    fn event_to_device(&mut self, event: Feedback) -> Response {
        if self.state == State::AwaitingDeviceId {
            log::warn!("Ignoring event while awaiting device Id");
            return Response::none();
        }

        use Feedback::*;
        match event {
            Transport(event) => {
                use event::Transport::*;
                match event {
                    Play => return self.play(),
                    Pause => return self.pause(),
                    Stop => return Response::from_msg_list(self.reset()),
                    _ => (),
                }
            }
            Mixer(mixer) => {
                use event::Mixer::*;
                match mixer {
                    Volume(vol) => return self.volume(vol),
                    Mute => (),
                }
            }
            Data(data) => {
                use event::Data::*;
                match data {
                    Timecode(tc) => return self.timecode(tc),
                    Player(player) => {
                        log::debug!("got {}", player);
                        return Response::from_msg_list(self.reset());
                        // FIXME send to device
                    }
                    Track(_) => (),
                }
            }
        }

        Response::none()
    }

    fn reset(&mut self) -> MsgList {
        use button::*;
        use display_7_seg::*;

        let mut list = MsgList::new();

        let tag_chan = button::TAG | self.chan;
        list.push([tag_chan, PREVIOUS, OFF]);
        list.push([tag_chan, NEXT, OFF]);
        list.push([tag_chan, STOP, OFF]);
        list.push([tag_chan, PLAY, OFF]);

        for idx in 0..10 {
            list.push([display_7_seg::TAG.into(), TIME_LEFT_DIGIT - idx as u8, b' ']);
        }

        *self = XTouchOneMackie::default();

        list
    }
}

impl XTouchOneMackie {
    fn build_fader_msg(&self, vol: f64) -> [u8; 3] {
        let two_bytes = midi::normalized_f64::to_be(vol).unwrap();
        [fader::TAG | self.chan, two_bytes[0], two_bytes[1]]
    }

    fn fader_touch(&mut self, value: u8) -> Response {
        use FaderState::*;

        let is_touched = value > fader::TOUCH_THRSD;
        match self.fader_state {
            Released if is_touched => {
                self.fader_state = Touched { last_volume: None };
            }
            Touched { last_volume } if !is_touched => {
                self.fader_state = Released;
                if let Some(vol) = last_volume {
                    return Response::from(Mixer::Volume(vol), self.build_fader_msg(vol));
                }
            }
            _ => (),
        }

        Response::none()
    }

    fn fader_moved(&mut self, buf: &[u8]) -> Response {
        use FaderState::*;

        let vol = match midi::normalized_f64::from_be(buf) {
            Ok(value) => value,
            Err(err) => {
                log::error!("Fader moved value: {err}");
                return Response::none();
            }
        };

        match &mut self.fader_state {
            Touched { last_volume } => {
                *last_volume = Some(vol);
                Response::from_event(Mixer::Volume(vol))
            }
            Released => {
                // FIXME is this a problem?
                Response::from_event(Mixer::Volume(vol))
            }
        }
    }
}

impl XTouchOneMackie {
    fn play(&mut self) -> Response {
        use button::*;
        use State::*;

        let mut list = MsgList::new();
        let tag_chan = button::TAG | self.chan;

        match self.state {
            Stopped => {
                self.state = Playing;
                list.push([tag_chan, STOP, OFF]);
            }
            Playing => (),
            AwaitingDeviceId => unreachable!(),
        }

        list.push([tag_chan, PLAY, ON]);

        Response::from_msg_list(list)
    }

    fn pause(&mut self) -> Response {
        use button::*;
        use State::*;

        let mut list = MsgList::new();
        let tag_chan = button::TAG | self.chan;

        match self.state {
            Playing => {
                self.state = Stopped;
                list.push([tag_chan, PLAY, OFF]);
            }
            Stopped => (),
            AwaitingDeviceId => unreachable!(),
        }

        list.push([tag_chan, STOP, ON]);

        Response::from_msg_list(list)
    }

    fn volume(&mut self, vol: f64) -> Response {
        use FaderState::*;

        match &mut self.fader_state {
            Released => Response::from_msg(self.build_fader_msg(vol)),
            Touched { last_volume } => {
                // user touches fader => don't move it before it's released.
                *last_volume = Some(vol);

                Response::none()
            }
        }
    }

    fn timecode(&mut self, tc: ctrl_surf::Timecode) -> Response {
        use display_7_seg::*;

        let mut list = MsgList::new();
        let tc = TimecodeBreakDown::from(tc);

        for (idx, (&last_digit, digit)) in self.last_tc.0.iter().zip(tc.0).enumerate() {
            if last_digit != digit {
                list.push([TAG.into(), TIME_LEFT_DIGIT - idx as u8, digit]);
            }
        }

        self.last_tc = tc;

        Response::from_msg_list(list)
    }
}

impl XTouchOneMackie {
    fn handle_sysex_msg(&mut self, msg: midi::Msg) -> Response {
        if self.state != State::AwaitingDeviceId {
            log::debug!("Ignoring sysex message {}", msg.display());
            return Response::none();
        }

        let res = self.handle_identification_resp(msg);
        Response::from_event(CtrlSurfEvent::Identification(res))
    }

    fn handle_identification_resp(&mut self, msg: midi::Msg) -> Result<(), Error> {
        use crate::bytes::Displayable;
        use Error::*;

        self.state = State::Stopped;

        let data = msg.try_get_sysex_data()?;
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

        if let Ok(data_str) = std::str::from_utf8(&data[2..]) {
            log::debug!("Device identification success. Found: {data_str}");
        } else {
            let displayable = Displayable::from(&data[2..]);
            log::debug!("Device identification success. Found: {displayable}");
        }

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
