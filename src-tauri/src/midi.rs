//! The MIDI input: a port, a callback, and the four messages this instrument
//! understands.
//!
//! The connection lives on its own thread, which is the one thing midir's
//! callback cannot be trusted to outlive: a port that is opened from a command
//! handler would be closed the moment the handler returned. Here the thread owns
//! the connection and parks until the link is replaced, so playing survives every
//! panel redraw.
//!
//! Only notes, bend, the mod wheel and all-notes-off are read. Every other
//! message a controller sends is ignored rather than guessed at.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use midir::{Ignore, MidiInput};

use crate::synth::Event;

/// What the app holds while a port is open. Dropping the keep-alive channel is
/// what closes the port; nothing else has to be stopped by hand.
#[derive(Default)]
pub struct Link {
    pub device: String,
    /// Held only to keep the reader thread's park channel open.
    _keep: Option<Sender<()>>,
}

pub fn devices() -> Result<Vec<String>, String> {
    let input = MidiInput::new("AUDIORIUM").map_err(|error| error.to_string())?;
    let ports = input.ports();
    Ok(ports
        .iter()
        .map(|port| input.port_name(port).unwrap_or_else(|_| "MIDI input".into()))
        .collect())
}

/// What the reader keeps between messages: the note each channel is holding. A
/// controller that puts every finger on its own channel — which is what MPE is —
/// bends and presses that note rather than the whole instrument.
#[derive(Default)]
pub struct Channels {
    notes: [Option<u8>; 16],
}

/// The messages the engine accepts. Anything else is dropped, which is the
/// behaviour a controller expects of a synth that does not implement it.
fn interpret(message: &[u8], channels: &mut Channels, queue: &mut Vec<Event>) {
    if message.is_empty() {
        return;
    }
    let kind = message[0] & 0xf0;
    let channel = (message[0] & 0x0f) as usize;
    let data = &message[1..];
    match kind {
        0x90 if data.len() >= 2 => {
            let note = data[0] & 0x7f;
            let velocity = data[1] & 0x7f;
            if velocity == 0 {
                queue.push(Event::NoteOff { note });
                if channels.notes[channel] == Some(note) {
                    channels.notes[channel] = None;
                }
            } else {
                queue.push(Event::NoteOn {
                    note,
                    velocity: velocity as f32 / 127.0,
                });
                channels.notes[channel] = Some(note);
            }
        }
        0x80 if !data.is_empty() => {
            let note = data[0] & 0x7f;
            queue.push(Event::NoteOff { note });
            if channels.notes[channel] == Some(note) {
                channels.notes[channel] = None;
            }
        }
        // Polyphonic aftertouch names its own note, so it needs no channel state.
        0xa0 if data.len() >= 2 => queue.push(Event::Pressure {
            note: data[0] & 0x7f,
            amount: (data[1] & 0x7f) as f32 / 127.0,
        }),
        0xb0 if data.len() >= 2 => match data[0] & 0x7f {
            1 => queue.push(Event::ModWheel(data[1] as f32 / 127.0)),
            // 120 is all-sound-off and 123 is all-notes-off; both mean the same
            // thing to an instrument with no sustain pedal of its own.
            120 | 123 => queue.push(Event::AllNotesOff),
            _ => {}
        },
        // Channel pressure: on a per-note channel it is that note's aftertouch; on
        // the first channel it would have to be every note at once, which no voice
        // can hold, so it is left alone.
        0xd0 if !data.is_empty() => {
            if let Some(note) = channels.notes[channel] {
                queue.push(Event::Pressure {
                    note,
                    amount: (data[0] & 0x7f) as f32 / 127.0,
                });
            }
        }
        0xe0 if data.len() >= 2 => {
            let value = ((data[1] as i32 & 0x7f) << 7) | (data[0] as i32 & 0x7f);
            let semitones = (value as f32 - 8192.0) / 8192.0 * 2.0;
            // A channel holding one note is a finger of an MPE controller and bends
            // that note; the first channel is the keyboard, and bends the instrument.
            match channels.notes[channel] {
                Some(note) if channel != 0 => queue.push(Event::NoteBend { note, semitones }),
                _ => queue.push(Event::Bend(semitones)),
            }
        }
        _ => {}
    }
}

pub fn open(port: usize, events: Arc<Mutex<Vec<Event>>>) -> Result<Link, String> {
    let (ready, wait) = mpsc::sync_channel::<Result<String, String>>(1);
    let (keep, park) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("audiorium-midi".into())
        .spawn(move || {
            let mut input = match MidiInput::new("AUDIORIUM") {
                Ok(input) => input,
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                    return;
                }
            };
            input.ignore(Ignore::None);
            let ports = input.ports();
            let Some(target) = ports.get(port) else {
                let _ = ready.send(Err("That MIDI input is not connected.".into()));
                return;
            };
            let name = input
                .port_name(target)
                .unwrap_or_else(|_| "MIDI input".into());
            let queue = events.clone();
            let mut channels = Channels::default();
            let connection = input.connect(
                target,
                "audiorium-in",
                move |_stamp, message, _| {
                    if let Ok(mut queue) = queue.lock() {
                        interpret(message, &mut channels, &mut queue);
                    }
                },
                (),
            );
            match connection {
                Ok(_connection) => {
                    let _ = ready.send(Ok(name));
                    // Parked until the link is replaced: dropping the connection
                    // is the only way to close a midir port cleanly.
                    let _ = park.recv();
                }
                Err(error) => {
                    let _ = ready.send(Err(error.to_string()));
                }
            }
        })
        .map_err(|error| error.to_string())?;
    match wait.recv() {
        Ok(Ok(device)) => Ok(Link {
            device,
            _keep: Some(keep),
        }),
        Ok(Err(reason)) => Err(reason),
        Err(_) => Err("The MIDI input did not answer.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(message: &[u8]) -> Vec<Event> {
        let mut channels = Channels::default();
        events_on(message, &mut channels)
    }

    fn events_on(message: &[u8], channels: &mut Channels) -> Vec<Event> {
        let mut queue = Vec::new();
        interpret(message, channels, &mut queue);
        queue
    }

    #[test]
    fn a_note_on_and_off_become_the_engines_events() {
        assert_eq!(
            events(&[0x90, 60, 100]),
            vec![Event::NoteOn {
                note: 60,
                velocity: 100.0 / 127.0
            }]
        );
        assert_eq!(
            events(&[0x90, 60, 0]),
            vec![Event::NoteOff { note: 60 }]
        );
        assert_eq!(
            events(&[0x80, 60, 64]),
            vec![Event::NoteOff { note: 60 }]
        );
    }

    #[test]
    fn a_bend_is_measured_against_its_centre() {
        let centre = events(&[0xe0, 0x00, 0x40]);
        assert!(matches!(centre.as_slice(), [Event::Bend(value)] if value.abs() < 0.001));
        let top = events(&[0xe0, 0x7f, 0x7f]);
        assert!(matches!(top.as_slice(), [Event::Bend(value)] if (*value - 2.0).abs() < 0.01));
    }

    #[test]
    fn the_mod_wheel_and_panic_messages_are_read() {
        assert!(matches!(
            events(&[0xb0, 1, 127]).as_slice(),
            [Event::ModWheel(value)] if *value == 1.0
        ));
        assert_eq!(events(&[0xb0, 123, 0]), vec![Event::AllNotesOff]);
    }

    #[test]
    fn a_per_note_bend_belongs_to_its_own_finger() {
        let mut channels = Channels::default();
        // Channel 1 (index 1) is one finger of an MPE controller holding C4.
        assert_eq!(
            events_on(&[0x91, 60, 100], &mut channels),
            vec![Event::NoteOn { note: 60, velocity: 100.0 / 127.0 }]
        );
        let bend = events_on(&[0xe1, 0x00, 0x60], &mut channels);
        assert!(
            matches!(bend.as_slice(), [Event::NoteBend { note: 60, .. }]),
            "{bend:?}"
        );
        // The keyboard channel still bends the instrument.
        assert!(matches!(events(&[0xe0, 0x00, 0x60]).as_slice(), [Event::Bend(_)]));
        // Releasing the note stops it from being the channel's own.
        events_on(&[0x81, 60, 0], &mut channels);
        assert!(matches!(events_on(&[0xe1, 0x00, 0x60], &mut channels).as_slice(), [Event::Bend(_)]));
    }

    #[test]
    fn aftertouch_arrives_per_note_and_per_channel() {
        assert!(matches!(
            events(&[0xa0, 60, 64]).as_slice(),
            [Event::Pressure { note: 60, amount }] if (*amount - 64.0 / 127.0).abs() < 0.001
        ));
        let mut channels = Channels::default();
        events_on(&[0x92, 67, 100], &mut channels);
        assert!(matches!(
            events_on(&[0xd2, 127], &mut channels).as_slice(),
            [Event::Pressure { note: 67, amount }] if *amount == 1.0
        ));
    }

    #[test]
    fn a_message_the_engine_does_not_implement_is_ignored() {
        assert!(events(&[0xb0, 64, 127]).is_empty());
        assert!(events(&[]).is_empty());
        assert!(events(&[0x90]).is_empty());
    }
}
