//! Macros (report 9). A macro's steps are a stream of records, as Unleash
//! writes them (MacroEditorViewModel.OnMacroRecording):
//!
//! ```text
//!   a key event     1 byte   the key's code, with bit 7 set for a release
//!   a media event   3 bytes  0x03, then the consumer usage, little endian;
//!                            0x03 00 00 releases the media key
//!   a wait          3 bytes  0x01, then milliseconds, little endian
//! ```
//!
//! Every event is followed by a wait, even a wait of 0, so a plain key step
//! comes out at four bytes. Steps are JSON in the editor's shape:
//! `{code, down, delay}` for a key, `{media, down, delay}` for a media key and
//! `{wait, delay}` for a pause before the first event, `delay` being the wait
//! after the step.

use serde_json::{Map, Value, json};

use crate::error::{Result, invalid};
use crate::json::{int, obj, truthy};
use crate::protocol::{RID_MACRO, csum, size};

pub const MACRO_NAME_MAX: usize = 50;
pub const MACRO_DATA_MAX: usize = 967;
pub const MACRO_SLOTS: u8 = 100;

const REC_WAIT: u8 = 0x01;
const REC_MEDIA: u8 = 0x03;

/// The keyboard's own numbering for the modifier keys, which differs from the
/// usage codes it uses for everything else.
pub const MACRO_MODIFIERS: [(u8, &str); 8] = [
    (112, "Left Ctrl"),
    (113, "Left Shift"),
    (114, "Left Alt"),
    (115, "Left Super"),
    (116, "Right Ctrl"),
    (117, "Right Shift"),
    (118, "Right Alt"),
    (119, "Right Super"),
];

/// The media keys a macro can send, by consumer usage, as Unleash offers them.
pub const MACRO_MEDIA: [(u16, &str); 16] = [
    (0xB5, "Next track"),
    (0xB6, "Previous track"),
    (0xB7, "Stop"),
    (0xCD, "Play/pause"),
    (0xE2, "Mute"),
    (0xE9, "Volume up"),
    (0xEA, "Volume down"),
    (0x183, "Media player"),
    (0x18A, "Mail"),
    (0x192, "Calculator"),
    (0x194, "My computer"),
    (0x221, "Browser search"),
    (0x223, "Browser home"),
    (0x224, "Browser back"),
    (0x225, "Browser forward"),
    (0x227, "Browser refresh"),
];

// Offsets in the whole report (header included).
const O_NAMELEN: usize = 8;
const O_NAME: usize = 9;
const O_METHOD: usize = 59;
const O_REPEAT: usize = 60;
const O_UNIT: usize = 61;
const O_LEN: usize = 62;
const O_DATA: usize = 64;

/// A stored macro's settings, without its steps.
pub fn macro_meta(raw: &[u8]) -> Value {
    let n = (raw[O_NAMELEN] as usize).min(MACRO_NAME_MAX);
    let name: String = raw[O_NAME..O_NAME + n]
        .iter()
        .map(|&b| if b < 0x80 { b as char } else { '\u{FFFD}' })
        .collect();
    json!({
        "name": name, "method": raw[O_METHOD], "repeat": raw[O_REPEAT], "unit": raw[O_UNIT],
        "length": u16::from_le_bytes([raw[O_LEN], raw[O_LEN + 1]]),
    })
}

/// The record stream a macro report holds.
pub fn macro_record_bytes(raw: &[u8]) -> &[u8] {
    let n = u16::from_le_bytes([raw[O_LEN], raw[O_LEN + 1]]) as usize;
    let end = (O_DATA + n).min(raw.len());
    &raw[O_DATA..end]
}

/// Decode a macro report into the editor's steps.
pub fn macro_steps(raw: &[u8]) -> Vec<Value> {
    let n = (u16::from_le_bytes([raw[O_LEN], raw[O_LEN + 1]]) as usize).min(MACRO_DATA_MAX);
    let data = &raw[O_DATA..(O_DATA + n).min(raw.len())];
    let mut out: Vec<Map<String, Value>> = Vec::new();
    let (mut i, mut held_media) = (0usize, 0u16);
    let step = |pairs: [(&str, Value); 3]| -> Map<String, Value> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    };
    while i < data.len() {
        let b = data[i];
        if b == REC_WAIT && i + 2 < data.len() {
            let ms = u16::from_le_bytes([data[i + 1], data[i + 2]]) as u64;
            match out.last_mut() {
                Some(last) => {
                    let d = last["delay"].as_u64().unwrap_or(0) + ms;
                    last.insert("delay".into(), json!(d));
                }
                // a wait before any event: keep it visible
                None => out.push(
                    [
                        ("wait".to_string(), json!(ms)),
                        ("delay".to_string(), json!(0)),
                    ]
                    .into_iter()
                    .collect(),
                ),
            }
            i += 3;
        } else if b == REC_MEDIA && i + 2 < data.len() {
            let usage = u16::from_le_bytes([data[i + 1], data[i + 2]]);
            if usage != 0 {
                held_media = usage;
                out.push(step([
                    ("media", json!(usage)),
                    ("down", json!(true)),
                    ("delay", json!(0)),
                ]));
            } else {
                // the release names no usage; it lets go of the last one pressed
                out.push(step([
                    ("media", json!(held_media)),
                    ("down", json!(false)),
                    ("delay", json!(0)),
                ]));
            }
            i += 3;
        } else {
            out.push(step([
                ("code", json!(b & 0x7F)),
                ("down", json!(b & 0x80 == 0)),
                ("delay", json!(0)),
            ]));
            i += 1;
        }
    }
    out.into_iter().map(Value::Object).collect()
}

fn ms_of(s: &Map<String, Value>) -> Result<u16> {
    let v = s.get("wait").or_else(|| s.get("delay"));
    let ms = match v {
        Some(v) => int(v, "wait")?,
        None => 0,
    }
    .min(0xFFFF);
    u16::try_from(ms).map_err(|_| invalid("a wait cannot be negative"))
}

/// Encode steps back into the record stream, the way Unleash writes it. Steps
/// past the keyboard's 967 bytes are left off.
pub fn macro_data(steps: &[Value]) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    for s in steps {
        let s = obj(s, "step")?;
        let mut rec = Vec::with_capacity(6);
        if s.contains_key("wait") {
        } else if let Some(m) = s.get("media") {
            let usage = if truthy(s.get("down")) {
                int(m, "media")?
            } else {
                0
            };
            rec.push(REC_MEDIA);
            rec.extend_from_slice(&((usage & 0xFFFF) as u16).to_le_bytes());
        } else {
            let code = int(
                s.get("code")
                    .ok_or_else(|| invalid("a key step needs a code"))?,
                "code",
            )?;
            rec.push(((code & 0x7F) as u8) | if truthy(s.get("down")) { 0 } else { 0x80 });
        }
        rec.push(REC_WAIT);
        rec.extend_from_slice(&ms_of(s)?.to_le_bytes());
        if data.len() + rec.len() > MACRO_DATA_MAX {
            break;
        }
        data.extend_from_slice(&rec);
    }
    Ok(data)
}

/// A macro report's body, everything after the 7-byte command header.
/// `run`, `repeat` and `unit` are the play settings Unleash stores with it.
pub fn macro_body(name: &str, steps: &[Value], run: u8, repeat: u8, unit: u8) -> Result<Vec<u8>> {
    // unused name and step bytes are 0xFF, as Unleash leaves them (Macro.cs,
    // InitializeMacroData), so a rewrite comes back byte for byte
    let mut b = vec![0xFFu8; size(RID_MACRO) - 7];
    let enc: Vec<u8> = name
        .chars()
        .map(|c| if c.is_ascii() { c as u8 } else { b'?' })
        .take(MACRO_NAME_MAX)
        .collect();
    b[1] = enc.len() as u8; // b[0] is the checksum
    b[2..2 + enc.len()].copy_from_slice(&enc);
    b[52] = run;
    b[53] = repeat;
    b[54] = unit;
    let data = macro_data(steps)?;
    b[55..57].copy_from_slice(&(data.len() as u16).to_le_bytes());
    b[57..57 + data.len()].copy_from_slice(&data);
    b[1024] = 1; // slot in use
    b[0] = csum(&b[1..]);
    Ok(b)
}
