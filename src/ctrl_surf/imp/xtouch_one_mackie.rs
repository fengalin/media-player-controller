use std::sync::{Arc, Mutex};

use crate::ctrl_surf::{
    self,
    event::{self, *},
    MidiMsgList, Response,
};
use crate::midi;

/*
const MACKIE_ID: [u8; 2] = [0x00, 0x66];
const MCU_ID: u8 = 0x14;
*/
// const MCU_EXT_ID: u8 = 0x15;

// Only one channel for now (X-Touch One)
const CHANNEL: midi_msg::Channel = midi_msg::Channel::Ch1;

pub const BUTTON_PRESSED: u8 = 127;
pub const BUTTON_RELEASED: u8 = 0;
pub const BUTTON_ON: u8 = BUTTON_PRESSED;
pub const BUTTON_OFF: u8 = BUTTON_RELEASED;

const FADER_MAX: u16 = 0x3fff;
const FADER_STEP: f64 = 1f64 / FADER_MAX as f64;
const FADER_TOUCH_THRSD: u8 = 64;

mod note {
    pub const PREVIOUS: u8 = 91;
    pub const NEXT: u8 = 92;
    pub const STOP: u8 = 93;
    pub const PLAY: u8 = 94;
    pub const FADER_TOUCH: u8 = 104;
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
    state: State,
    fader_state: FaderState,
}

impl Default for XTouchOneMackie {
    fn default() -> Self {
        Self {
            last_tc: TimecodeBreakDown::default(),
            state: State::Stopped,
            fader_state: FaderState::Released,
        }
    }
}

impl crate::ctrl_surf::ControlSurface for XTouchOneMackie {
    fn msg_from_device(&mut self, msg: crate::midi::Msg) -> Response {
        use midi_msg::MidiMsg::*;

        if let ChannelVoice { channel, msg } = msg.inner {
            if channel != CHANNEL {
                return Response::none();
            }

            use midi_msg::ChannelVoiceMsg::*;
            match msg {
                NoteOn { note, velocity } => {
                    use Transport::*;

                    let is_pressed = velocity == BUTTON_PRESSED;
                    match note {
                        note::PREVIOUS if is_pressed => return Response::from_event(Previous),
                        note::NEXT if is_pressed => return Response::from_event(Next),
                        note::STOP if is_pressed => return Response::from_event(Stop),
                        note::PLAY if is_pressed => return Response::from_event(PlayPause),
                        note::FADER_TOUCH => return self.fader_touch(velocity),
                        _ => (),
                    }
                }
                // FIXME use raw msg
                PitchBend { bend } => {
                    return self.fader_moved(bend);
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

    fn reset(&mut self) -> MidiMsgList {
        let mut list = MidiMsgList::new();
        list.push(midi::build_note_on(CHANNEL, note::PREVIOUS, BUTTON_OFF).to_midi());
        list.push(midi::build_note_on(CHANNEL, note::NEXT, BUTTON_OFF).to_midi());
        list.push(midi::build_note_on(CHANNEL, note::STOP, BUTTON_OFF).to_midi());
        list.push(midi::build_note_on(CHANNEL, note::PLAY, BUTTON_OFF).to_midi());

        // Reset 7 segments display
        for idx in 0..10 {
            list.push([0xb0, 0x49 - idx as u8, b' ']);
        }

        *self = XTouchOneMackie::default();

        list
    }
}

impl XTouchOneMackie {
    fn fader_touch(&mut self, value: u8) -> Response {
        use FaderState::*;

        let is_touched = value > FADER_TOUCH_THRSD;
        match self.fader_state {
            Released if is_touched => {
                self.fader_state = Touched { last_volume: None };
            }
            Touched { last_volume } if !is_touched => {
                self.fader_state = Released;
                if let Some(vol) = last_volume {
                    return Response::from(
                        Mixer::Volume(vol),
                        // FIXME pass a buffer to ease adaptation for multi faders ctrl surfs
                        midi::build_pitch_bend(CHANNEL, (FADER_MAX as f64 * vol) as u16).to_midi(),
                    );
                }
            }
            _ => (),
        }

        Response::none()
    }

    fn fader_moved(&mut self, value: u16) -> Response {
        use FaderState::*;

        let vol = value.min(FADER_MAX) as f64 * FADER_STEP;

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
        use State::*;

        let mut list = MidiMsgList::new();
        match self.state {
            Stopped => {
                self.state = Playing;
                list.push(midi::build_note_on(CHANNEL, note::STOP, BUTTON_OFF).to_midi());
            }
            Playing => (),
        }

        list.push(midi::build_note_on(CHANNEL, note::PLAY, BUTTON_ON).to_midi());

        Response::from_msg_list(list)
    }

    fn pause(&mut self) -> Response {
        use State::*;

        let mut list = MidiMsgList::new();
        match self.state {
            Playing => {
                self.state = Stopped;
                list.push(midi::build_note_on(CHANNEL, note::PLAY, BUTTON_OFF).to_midi());
            }
            Stopped => (),
        }

        list.push(midi::build_note_on(CHANNEL, note::STOP, BUTTON_ON).to_midi());

        Response::from_msg_list(list)
    }

    fn volume(&mut self, vol: f64) -> Response {
        use FaderState::*;

        match &mut self.fader_state {
            Released => {
                // FIXME pass a buffer to ease adaptation for multi faders ctrl surfs
                Response::from_msg_list(
                    midi::build_pitch_bend(CHANNEL, (FADER_MAX as f64 * vol) as u16).to_midi(),
                )
            }
            Touched { last_volume } => {
                // user touches fader => don't move it before it's released.
                *last_volume = Some(vol);

                Response::none()
            }
        }
    }

    fn timecode(&mut self, tc: ctrl_surf::Timecode) -> Response {
        let mut list = MidiMsgList::new();
        let tc = TimecodeBreakDown::from(tc);

        for (idx, (&last_digit, digit)) in self.last_tc.0.iter().zip(tc.0).enumerate() {
            if last_digit != digit {
                list.push(vec![0xb0, 0x49 - idx as u8, digit]);
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
