use std::sync::{Arc, Mutex};

use crate::{
    ctrl_surf::{
        self,
        event::{self, *},
        Response,
    },
    midi::{self, MsgList},
};

/*
const MACKIE_ID: [u8; 2] = [0x00, 0x66];
const MCU_ID: u8 = 0x14;
*/
// const MCU_EXT_ID: u8 = 0x15;

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
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Response {
        let msg = msg.into_inner();

        if let Some(&tag_chan) = msg.first() {
            self.chan = midi::Channel::from(tag_chan);

            match midi::Tag::from(tag_chan) {
                button::TAG => {
                    if let Some(id_value) = msg.get(1..=2) {
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
                    if let Some(value) = msg.get(1..=2) {
                        return self.fader_moved(value);
                    }
                }
                _ => (),
            }
        }

        Response::none()
    }

    fn event_to_device(&mut self, event: Feedback) -> Response {
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
                        log::debug!("ctrl_surf got {}", player);
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
