//! Live preview frames (report 7). The keyboard's own firmware draws the
//! effect while the host sends frames, and nothing is stored, so leaving
//! without saving returns the profile untouched. Frames carry a running time,
//! so the host has to keep sending them; the keyboard does not animate a
//! preview by itself.

use serde_json::Value;

use crate::error::{Result, invalid};
use crate::json::{Obj, byte, int, obj, rgb, word};
use crate::protocol::{RID_PREVIEW, mode_index, size};

/// Milliseconds between frames, the interval Unleash used.
pub const PREVIEW_PERIOD: u64 = 40;

fn get_byte(e: &Obj, k: &str, default: u8) -> Result<u8> {
    e.get(k).map_or(Ok(default), |v| byte(v, k))
}

fn start_stop(e: &Obj, restart: bool) -> Result<u8> {
    let start = e.get("start").map_or(Ok(0), |v| int(v, "start"))? & 0xF;
    let stop = e.get("stop").map_or(Ok(0), |v| int(v, "stop"))? & 0xF;
    let v = ((start << 4) | stop) as u8;
    Ok(if restart { v | 0x80 } else { v })
}

/// A sub-mode the keyboard accepts, or the default. A profile can hold 255 for
/// "not set"; the keyboard only accepts an in-range value in a preview frame
/// (Unleash's Led.cs checks the same) and goes dark on anything else.
fn in_range(e: &Obj, k: &str, max: i64) -> Result<u8> {
    let v = e.get(k).map_or(Ok(0), |v| int(v, k))?;
    Ok(if (0..=max).contains(&v) { v as u8 } else { 0 })
}

/// One preview frame. `e` is the effect's settings from `Profile::lighting()`,
/// `t` the milliseconds since the preview began.
pub fn preview_report(mode: &str, e: &Value, t: u64, restart: bool) -> Result<Vec<u8>> {
    let empty = Obj::new();
    let e = if e.is_null() { &empty } else { obj(e, mode)? };
    let index =
        mode_index(mode).ok_or_else(|| invalid(format!("{mode} is not a lighting mode")))?;
    let mut b = vec![0u8; size(RID_PREVIEW)];
    let sub = match mode {
        "spiral" => in_range(e, "direction", 1)?,
        "wave" => in_range(e, "direction", 3)?,
        "trigger" => in_range(e, "submode", 2)?,
        _ => 0,
    };
    b[0..7].copy_from_slice(&[
        RID_PREVIEW,
        0xEA,
        0x02,
        u8::from(mode != "off"),
        index,
        sub,
        0,
    ]);
    if mode == "off" {
        return Ok(b);
    }
    if mode == "static" {
        b[11] = get_byte(e, "zone", 0x80)?;
        b[12] = start_stop(e, restart)?;
        b[13] = get_byte(e, "brightness", 255)?;
        let color = match e.get("color") {
            Some(c) if !c.is_null() && c.as_array().is_none_or(|a| !a.is_empty()) => {
                rgb(c, "static")?
            }
            _ => [255, 0, 0],
        };
        b[14..17].copy_from_slice(&color);
        return Ok(b);
    }

    // the period, called shift for the trigger effect; unset or 0 means a second
    let cycle = match if e.contains_key("cycle") {
        e.get("cycle")
    } else {
        e.get("shift")
    } {
        None | Some(Value::Null) => 1000,
        Some(v) => match word(v, "cycle")? {
            0 => 1000,
            c => c,
        },
    };
    b[7..9].copy_from_slice(&((t % cycle as u64) as u16).to_le_bytes());
    b[9..11].copy_from_slice(&cycle.to_le_bytes());
    b[11] = get_byte(e, "zone", 0x80)?;
    b[12] = start_stop(e, restart)?;
    let cols: &[Value] = match e.get("colors") {
        Some(c) => c
            .as_array()
            .ok_or_else(|| invalid("colors must be a list"))?,
        None => &[],
    };
    let color_of = |c: &Value| -> Result<[u8; 3]> {
        rgb(
            obj(c, "color")?
                .get("color")
                .ok_or_else(|| invalid("a color needs its red, green and blue"))?,
            "color",
        )
    };
    b[13] = match e.get("brightness") {
        Some(v) => byte(v, "brightness")?,
        None => match cols.first() {
            Some(c) => byte(
                obj(c, "color")?
                    .get("brightness")
                    .ok_or_else(|| invalid("a color needs its brightness"))?,
                "brightness",
            )?,
            None => 255,
        },
    };
    match mode {
        "breathing" => {
            for (i, c) in cols.iter().take(2).enumerate() {
                b[14 + 3 * i..17 + 3 * i].copy_from_slice(&color_of(c)?);
            }
        }
        "pulse" => {
            let bg = match e.get("background") {
                Some(v) if !v.is_null() && v.as_object().is_none_or(|o| !o.is_empty()) => {
                    color_of(v)?
                }
                _ => [0, 0, 0],
            };
            b[14..17].copy_from_slice(&bg);
            for (i, c) in cols.iter().take(7).enumerate() {
                b[17 + 3 * i..20 + 3 * i].copy_from_slice(&color_of(c)?);
            }
        }
        "wave" | "trigger" => {
            for (i, c) in cols.iter().take(7).enumerate() {
                b[14 + 3 * i..17 + 3 * i].copy_from_slice(&color_of(c)?);
            }
            if mode == "trigger" {
                b[35] = get_byte(e, "key", 0)?;
            }
        }
        // spiral and stars carry no color list in the preview report
        _ => {}
    }
    Ok(b)
}
