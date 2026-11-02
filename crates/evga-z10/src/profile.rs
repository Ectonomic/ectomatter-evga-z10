//! A profile: one whole report 8, 1501 bytes, decoded field by field from
//! Unleash RGB's ProfileUsbFeatureReport.
//!
//! The raw bytes are kept, so fields this crate does not understand survive a
//! read, edit and write untouched. The lighting view is JSON in the shape the
//! editor uses.

use serde_json::{Value, json};

use crate::error::{Error, Result, invalid};
use crate::json::{argb, byte, obj, word};
use crate::protocol::{MODES, NKEYS, RID_PROFILE, csum, mode_index, size};

// Offsets, with the 7-byte command header at 0.
const O_CK: usize = 7;
const O_NAMELEN: usize = 8;
const O_NAME: usize = 10;
const NAME_BYTES: usize = 126;
const O_GAMEKEYS: usize = 136;
const O_LCDCOLOR: usize = 137;
const O_LCDAPPS: usize = 141;
const O_PRIMARY: usize = 264;
const O_SECONDARY: usize = 728;
const O_MODE: usize = 1192;

impl Profile {
    /// Whether `other` differs from this profile only in its lighting (the
    /// effect and its settings, from the mode on): its name, keys and every
    /// other setting the same.
    pub fn same_but_lighting(&self, other: &Profile) -> bool {
        self.raw.len() == other.raw.len() && self.raw[8..O_MODE] == other.raw[8..O_MODE]
    }

    /// Whether the keyboard's preview of an effect shows all of how `other`
    /// differs from this profile: the preview carries every effect's
    /// settings and colors but Spiral's colors (their count and the colors
    /// themselves), which the keyboard takes from the profile it has loaded.
    pub fn preview_shows(&self, other: &Profile) -> bool {
        let spiral_colors = O_SPIRAL + 18..O_WAVE;
        self.raw.len() == other.raw.len() && self.raw[spiral_colors.clone()] == other.raw[spiral_colors]
    }
}
const O_SUB: usize = 1193;
const O_STATIC: usize = 1224;
const O_BREATH: usize = 1244;
const O_PULSE: usize = 1272;
const O_SPIRAL: usize = 1329;
const O_WAVE: usize = 1380;
const O_TRIGGER: usize = 1431;
const O_STARS: usize = 1482;

/// One key's assignment: four bytes, the function and its three arguments.
pub type KeyDefine = [u8; 4];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub raw: Vec<u8>,
}

fn u16_at(r: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([r[o], r[o + 1]])
}

fn put_u16(r: &mut [u8], o: usize, v: u16) {
    r[o..o + 2].copy_from_slice(&v.to_le_bytes());
}

fn argb_at(r: &[u8], o: usize) -> Value {
    json!({ "brightness": r[o], "color": [r[o + 1], r[o + 2], r[o + 3]] })
}

impl Profile {
    /// A profile as the keyboard sent it, checked against its checksum.
    pub fn decode(report: &[u8]) -> Result<Profile> {
        if report.len() != size(RID_PROFILE) {
            return Err(invalid(format!(
                "a profile is {} bytes, not {}",
                size(RID_PROFILE),
                report.len()
            )));
        }
        if csum(&report[8..]) != report[O_CK] {
            return Err(Error::Checksum);
        }
        Ok(Profile {
            raw: report.to_vec(),
        })
    }

    pub fn checksum(&self) -> u8 {
        csum(&self.raw[8..])
    }

    /// The report that writes this profile into slot `n`.
    pub fn encode(&self, n: u8) -> Vec<u8> {
        let mut b = self.raw.clone();
        b[0..7].copy_from_slice(&[RID_PROFILE, 0xEA, 0x02, 0, n, 0, 0]);
        b[O_CK] = csum(&b[8..]);
        b
    }

    pub fn name(&self) -> String {
        let n = u16_at(&self.raw, O_NAMELEN) as usize;
        let bytes = &self.raw[O_NAME..O_NAME + (2 * n).min(NAME_BYTES)];
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    }

    pub fn set_name(&mut self, s: &str) {
        let units: Vec<u16> = s.encode_utf16().take(NAME_BYTES / 2).collect();
        put_u16(&mut self.raw, O_NAMELEN, units.len() as u16);
        let field = &mut self.raw[O_NAME..O_NAME + NAME_BYTES];
        field.fill(0);
        for (i, u) in units.iter().enumerate() {
            field[2 * i..2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
    }

    /// Every key's assignment, by key position. `secondary` is the Fn layer.
    pub fn keys(&self, secondary: bool) -> Vec<KeyDefine> {
        let o = if secondary { O_SECONDARY } else { O_PRIMARY };
        (0..NKEYS)
            .map(|i| self.raw[o + 4 * i..o + 4 * i + 4].try_into().unwrap())
            .collect()
    }

    pub fn set_key(&mut self, pos: usize, define: KeyDefine, secondary: bool) -> Result<()> {
        if pos >= NKEYS {
            return Err(invalid(format!("there is no key at position {pos}")));
        }
        let o = if secondary { O_SECONDARY } else { O_PRIMARY } + 4 * pos;
        self.raw[o..o + 4].copy_from_slice(&define);
        Ok(())
    }

    /// The lighting settings, in the editor's shape.
    pub fn lighting(&self) -> Value {
        let r = &self.raw;
        let colors = |o: usize, count: usize, stride: usize| -> Vec<Value> {
            (0..count).map(|i| argb_at(r, o + stride * i)).collect()
        };
        let mode = r[O_MODE];
        let mode_v = if (mode as usize) < MODES.len() {
            json!(MODES[mode as usize])
        } else {
            json!(mode)
        };

        let o = O_STATIC;
        let stat = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2],
            "brightness": r[o + 16], "color": [r[o + 17], r[o + 18], r[o + 19]],
        });
        let o = O_BREATH;
        let breath = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2], "random": r[o + 3],
            "cycle": u16_at(r, o + 16),
            "colors": (0..2).map(|i| {
                let c = o + 19 + 5 * i;
                json!({ "hold": r[o + 18 + 5 * i], "brightness": r[c], "color": [r[c + 1], r[c + 2], r[c + 3]] })
            }).collect::<Vec<_>>(),
        });
        let o = O_PULSE;
        let pulse = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2], "random": r[o + 3],
            "cycle": u16_at(r, o + 16), "count": r[o + 18],
            "on_hold": r[o + 19], "off_hold": r[o + 20], "background": argb_at(r, o + 21),
            "colors": colors(o + 25, 8, 4),
        });
        let o = O_SPIRAL;
        let spiral = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2], "direction": r[o + 3],
            "cycle": u16_at(r, o + 16), "count": r[o + 18], "colors": colors(o + 19, 8, 4),
        });
        let o = O_WAVE;
        let wave = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2], "random": r[o + 3], "direction": r[o + 4],
            "cycle": u16_at(r, o + 16), "count": r[o + 18], "colors": colors(o + 19, 8, 4),
        });
        let o = O_TRIGGER;
        let trigger = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2], "random": r[o + 3], "submode": r[o + 4],
            "shift": u16_at(r, o + 16), "count": r[o + 18], "colors": colors(o + 19, 8, 4),
        });
        let o = O_STARS;
        let stars = json!({
            "zone": r[o], "start": r[o + 1], "stop": r[o + 2],
            "cycle": u16_at(r, o + 16), "brightness": r[o + 18],
        });
        json!({
            "mode": mode_v, "sub": r[O_SUB],
            "static": stat, "breathing": breath, "pulse": pulse, "spiral": spiral,
            "wave": wave, "trigger": trigger, "stars": stars,
            "lcd": { "color": argb_at(r, O_LCDCOLOR), "apps": r[O_LCDAPPS] },
            "game_keys": r[O_GAMEKEYS],
        })
    }

    /// Write back any subset of what `lighting()` returns. Fields left out stay
    /// as they are.
    pub fn set_lighting(&mut self, d: &Value) -> Result<()> {
        let d = obj(d, "lighting")?;
        let r = &mut self.raw;
        let has = |e: &crate::json::Obj, k: &str| e.contains_key(k);

        if let Some(m) = d.get("mode") {
            let name = m
                .as_str()
                .ok_or_else(|| invalid("mode must be a mode's name"))?;
            r[O_MODE] = mode_index(name)
                .ok_or_else(|| invalid(format!("{name} is not a lighting mode")))?;
        }
        if let Some(s) = d.get("sub") {
            r[O_SUB] = byte(s, "sub")?;
        }

        // the first bytes every effect shares, plus the ones particular to it
        fn head(
            r: &mut [u8],
            o: usize,
            e: &crate::json::Obj,
            extra: &[(&str, usize)],
        ) -> Result<()> {
            for (k, off) in [("zone", 0), ("start", 1), ("stop", 2)].iter().chain(extra) {
                if let Some(v) = e.get(*k) {
                    r[o + off] = byte(v, k)?;
                }
            }
            Ok(())
        }
        fn put4(r: &mut [u8], at: usize, v: [u8; 4]) -> Result<()> {
            r.get_mut(at..at + 4)
                .ok_or_else(|| invalid("too many colors"))?
                .copy_from_slice(&v);
            Ok(())
        }

        if let Some(e) = d.get("static") {
            let e = obj(e, "static")?;
            head(r, O_STATIC, e, &[])?;
            if has(e, "color") {
                put4(r, O_STATIC + 16, argb(&Value::Object(e.clone()), "static")?)?;
            }
        }
        if let Some(e) = d.get("breathing") {
            let e = obj(e, "breathing")?;
            let o = O_BREATH;
            head(r, o, e, &[("random", 3)])?;
            if let Some(c) = e.get("cycle") {
                put_u16(r, o + 16, word(c, "cycle")?);
            }
            if let Some(cs) = e.get("colors") {
                for (i, c) in cs
                    .as_array()
                    .ok_or_else(|| invalid("colors must be a list"))?
                    .iter()
                    .enumerate()
                {
                    let hold = match obj(c, "color")?.get("hold") {
                        Some(h) => byte(h, "hold")?,
                        None => 10,
                    };
                    *r.get_mut(o + 18 + 5 * i)
                        .ok_or_else(|| invalid("too many colors"))? = hold;
                    put4(r, o + 19 + 5 * i, argb(c, "color")?)?;
                }
            }
        }
        for (key, o, extra, base) in [
            ("pulse", O_PULSE, &[("random", 3)][..], 25),
            ("spiral", O_SPIRAL, &[("direction", 3)][..], 19),
            ("wave", O_WAVE, &[("random", 3), ("direction", 4)][..], 19),
            (
                "trigger",
                O_TRIGGER,
                &[("random", 3), ("submode", 4)][..],
                19,
            ),
        ] {
            let Some(e) = d.get(key) else { continue };
            let e = obj(e, key)?;
            head(r, o, e, extra)?;
            // the period is called cycle, or shift for the trigger effect
            let t = if e.contains_key("cycle") {
                e.get("cycle")
            } else {
                e.get("shift")
            };
            if let Some(t) = t.filter(|t| !t.is_null()) {
                put_u16(r, o + 16, word(t, "cycle")?);
            }
            if let Some(c) = e.get("count") {
                r[o + 18] = byte(c, "count")?;
            }
            if key == "pulse" {
                for (k, off) in [("on_hold", 19), ("off_hold", 20)] {
                    if let Some(v) = e.get(k) {
                        r[o + off] = byte(v, k)?;
                    }
                }
                if let Some(bg) = e.get("background") {
                    put4(r, o + 21, argb(bg, "background")?)?;
                }
            }
            if let Some(cs) = e.get("colors") {
                let cs = cs
                    .as_array()
                    .ok_or_else(|| invalid("colors must be a list"))?;
                for (i, c) in cs.iter().take(8).enumerate() {
                    put4(r, o + base + 4 * i, argb(c, "color")?)?;
                }
            }
        }
        if let Some(e) = d.get("stars") {
            let e = obj(e, "stars")?;
            head(r, O_STARS, e, &[])?;
            if let Some(c) = e.get("cycle") {
                put_u16(r, O_STARS + 16, word(c, "cycle")?);
            }
            if let Some(b) = e.get("brightness") {
                r[O_STARS + 18] = byte(b, "brightness")?;
            }
        }
        // Unleash writes the effect's direction (or trigger style) into the
        // header's sub-mode byte as well; an unset 255 becomes the default of 0.
        let slot = match r[O_MODE] {
            4 => Some((O_SPIRAL, 3)),
            5 => Some((O_WAVE, 4)),
            7 => Some((O_TRIGGER, 4)),
            _ => None,
        };
        if let Some((o, off)) = slot {
            if r[o + off] > 3 {
                r[o + off] = 0;
            }
            r[O_SUB] = r[o + off];
        } else if d.contains_key("mode") {
            r[O_SUB] = 0;
        }
        if let Some(l) = d.get("lcd") {
            let l = obj(l, "lcd")?;
            if let Some(c) = l.get("color") {
                put4(r, O_LCDCOLOR, argb(c, "lcd color")?)?;
            }
            if let Some(a) = l.get("apps") {
                r[O_LCDAPPS] = byte(a, "apps")?;
            }
        }
        if let Some(g) = d.get("game_keys") {
            r[O_GAMEKEYS] = byte(g, "game_keys")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_to_the_lighting_alone_is_told_apart_from_any_other() {
        let a = Profile { raw: vec![0; 1501] };
        let mut b = a.clone();
        // the effect, and a setting of it
        b.raw[O_MODE] = 3;
        b.raw[O_PULSE + 16] = 0x10;
        assert!(a.same_but_lighting(&b));
        // a key, the name, game mode's keys, the screen's light: not the lighting alone
        for o in [O_PRIMARY + 4 * 20, O_SECONDARY, O_NAME, O_GAMEKEYS, O_LCDCOLOR] {
            let mut c = b.clone();
            c.raw[o] ^= 1;
            assert!(!a.same_but_lighting(&c), "a change at {o}");
        }
    }

    #[test]
    fn spirals_colors_are_what_a_preview_cannot_show() {
        let a = Profile { raw: vec![0; 1501] };
        // Spiral's speed, and another effect's colors: the preview shows them
        let mut b = a.clone();
        b.raw[O_SPIRAL + 16] = 0x20;
        b.raw[O_WAVE + 20] = 0xff;
        assert!(a.preview_shows(&b));
        // Spiral's count of colors, its first color's red, its last color's blue: it doesn't
        for o in [O_SPIRAL + 18, O_SPIRAL + 20, O_SPIRAL + 19 + 4 * 7 + 3] {
            let mut c = a.clone();
            c.raw[o] ^= 1;
            assert!(!a.preview_shows(&c), "a change at {o}");
        }
    }
}
