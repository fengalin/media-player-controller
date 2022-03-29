mod error;
pub use error::Error;

mod io;

pub mod msg;
pub use msg::Msg;

pub mod port;
pub use port::{DirectionalPorts, PortsIn, PortsOut};

pub fn build_cc(channel: midi_msg::Channel, control: u8, value: u8) -> midi_msg::MidiMsg {
    midi_msg::MidiMsg::ChannelVoice {
        channel,
        msg: midi_msg::ChannelVoiceMsg::ControlChange {
            control: midi_msg::ControlChange::Undefined { control, value },
        },
    }
}

pub fn build_note_on(channel: midi_msg::Channel, note: u8, velocity: u8) -> midi_msg::MidiMsg {
    midi_msg::MidiMsg::ChannelVoice {
        channel,
        msg: midi_msg::ChannelVoiceMsg::NoteOn { note, velocity },
    }
}

pub fn build_pitch_bend(channel: midi_msg::Channel, bend: u16) -> midi_msg::MidiMsg {
    midi_msg::MidiMsg::ChannelVoice {
        channel,
        msg: midi_msg::ChannelVoiceMsg::PitchBend { bend },
    }
}

pub fn build_channel_pressure(channel: midi_msg::Channel, pressure: u8) -> midi_msg::MidiMsg {
    midi_msg::MidiMsg::ChannelVoice {
        channel,
        msg: midi_msg::ChannelVoiceMsg::ChannelPressure { pressure },
    }
}
