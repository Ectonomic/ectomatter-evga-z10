//! What the editor asks of the keyboard, in the shapes its screens use.
//!
//! The keyboard is the source of truth: every read comes from it, and every
//! save writes a whole profile or macro, then reads it back to confirm. Before
//! the first write of a session, every profile and macro is saved as the raw
//! bytes the keyboard sent, so any change can be undone by hand.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Map, Value, json};

use crate::conn::Conn;
use crate::error::{Error, Result, invalid};
use crate::json::{byte, obj};
use crate::live::Live;
use crate::macros::{MACRO_SLOTS, macro_body, macro_data, macro_record_bytes, macro_steps};
use crate::profile::{KeyDefine, Profile};
use crate::protocol::NKEYS;

/// The profile slots, and 254 for the factory default the editor resets keys to.
pub const PROFILES: [u8; 6] = [1, 2, 3, 4, 5, 254];

pub struct Keyboard {
    conn: Conn,
    pub live: Live,
    backups: Option<PathBuf>,
    backed_up: Mutex<bool>,
    /// run between each of a long job's exchanges with the keyboard (see
    /// `between`)
    between: Option<Box<dyn Fn() + Send + Sync>>,
}

fn view(p: &Profile) -> Value {
    json!({ "name": p.name(), "lighting": p.lighting(), "keys": p.keys(false), "fnkeys": p.keys(true) })
}

/// A profile in the core protocol's shared shapes: its name, what each key
/// does on either layer by the keys' ids, and its lighting (shared.rs).
fn shared_view(p: &Profile) -> Value {
    json!({ "name": p.name(), "keys": crate::shared::layers(p), "lighting": crate::shared::lighting_of(&p.lighting()) })
}

/// A stored macro, with its steps, marked editable only where this code
/// rebuilds its records byte for byte, so a save can never change a step it
/// did not understand.
fn macro_view(meta: Value, raw: &[u8]) -> Result<Value> {
    let steps = macro_steps(raw);
    let editable = macro_data(&steps)? == macro_record_bytes(raw);
    let mut m = meta.as_object().cloned().unwrap_or_default();
    m.insert("steps".into(), Value::Array(steps));
    m.insert("editable".into(), json!(editable));
    Ok(Value::Object(m))
}

impl Keyboard {
    /// `backups` is where the first write of a session saves what it
    /// replaces; `None` turns backups off, for tests.
    pub fn new(conn: Conn, backups: Option<PathBuf>) -> Keyboard {
        let live = Live::start(conn.clone());
        Keyboard {
            conn,
            live,
            backups,
            backed_up: Mutex::new(false),
            between: None,
        }
    }

    /// As `new`, with the preview's loop kept by the caller: `tick` when
    /// `wants_tick` says so, and again as each `tick` asks. For where there
    /// are no threads, as a core module in Ectomatter.
    pub fn driven(conn: Conn, backups: Option<PathBuf>) -> Keyboard {
        Keyboard {
            conn,
            live: Live::driven(),
            backups,
            backed_up: Mutex::new(false),
            between: None,
        }
    }

    /// Run `f` between each of a long job's exchanges with the keyboard (a
    /// whole read, a save, a backup), when the keyboard is free. Where the
    /// preview's loop has no thread, `f` keeps it going and takes the
    /// previews asked for meanwhile, so they don't wait for the job.
    pub fn between(mut self, f: impl Fn() + Send + Sync + 'static) -> Keyboard {
        self.between = Some(Box::new(f));
        self
    }

    fn pass(&self) {
        if let Some(f) = &self.between {
            f();
        }
    }

    /// Whether what the preview asks for has changed since its last frame.
    pub fn fresh(&self) -> bool {
        self.live.fresh()
    }

    /// One beat of the preview's loop (see `driven`).
    pub fn tick(&self) -> crate::live::Next {
        self.live.tick(&self.conn)
    }

    /// Whether the preview's loop has a beat due (see `driven`).
    pub fn wants_tick(&self) -> bool {
        self.live.wants_tick()
    }

    /// Whether a preview is asked for now.
    pub fn previewing(&self) -> bool {
        self.live.showing()
    }

    /// Everything the editor shows: firmware, the active profile, every
    /// profile and every macro.
    pub fn state(&self) -> Result<Value> {
        let (path, fw, active) = self.conn.with(|k| Ok((k.path(), k.firmware_version()?.1, k.profile_number()?)))?;
        let mut profiles = Map::new();
        for n in PROFILES {
            self.pass();
            profiles.insert(n.to_string(), view(&self.conn.with(|k| k.read_profile(n))?));
        }
        let macros = self.macros()?;
        Ok(json!({ "path": path, "firmware": fw, "active": active, "profiles": profiles, "macros": macros, "live": self.live.status() }))
    }

    /// Everything a device's page shows, in the core protocol's shared shapes
    /// (`device/read`): firmware, the active profile, each profile, the keys
    /// as the keyboard was made (profile 254), every macro and the preview's
    /// state, the firmware described (section 4). The keyboard is read just
    /// as `state` reads it.
    pub fn read(&self) -> Result<Value> {
        let (path, fw, active) = self.conn.with(|k| Ok((k.path(), k.firmware_version()?.1, k.profile_number()?)))?;
        let mut profiles = Map::new();
        let mut factory = Value::Null;
        for n in PROFILES {
            self.pass();
            let p = self.conn.with(|k| k.read_profile(n))?;
            if n == 254 {
                factory = json!({ "keys": crate::shared::layers(&p), "lighting": crate::shared::lighting_of(&p.lighting()) });
            } else {
                profiles.insert(n.to_string(), shared_view(&p));
            }
        }
        let macros = self.macros_shared()?;
        // (its firmware as the protocol describes firmware: whose, and what a custom build adds)
        let firmware = crate::firmware::describe(fw);
        Ok(json!({ "path": path, "firmware": firmware, "active": active, "profiles": profiles, "factory": factory, "macros": macros, "live": self.live.status() }))
    }

    /// One profile in the shared shapes (`profiles/get`).
    pub fn profile(&self, n: u8) -> Result<Value> {
        if !PROFILES.contains(&n) {
            return Err(invalid("profiles are numbered 1 to 5"));
        }
        self.conn.with(|k| k.read_profile(n)).map(|p| shared_view(&p))
    }

    /// Every stored macro in the shared shapes (`macros/list`), each read once.
    pub fn macros_shared(&self) -> Result<Value> {
        let all = self.macros()?;
        Ok(Value::Object(all.as_object().map(|m| m.iter().map(|(i, v)| (i.clone(), crate::shared::macro_of(v))).collect()).unwrap_or_default()))
    }

    /// Save macro `index` in the shared shapes (`macros/set`): its `name`, its
    /// `steps` and how it plays as the keyboard keeps it (`run`, `repeat`,
    /// `unit`); or delete it (`macros/delete`, `delete: true`). Answers with
    /// every macro.
    pub fn write_macro_shared(&self, index: u8, body: &Value) -> Result<Value> {
        let mut legacy = obj(body, "macro")?.clone();
        if let Some(steps) = legacy.get("steps").and_then(Value::as_array) {
            let steps = crate::shared::legacy_steps(steps)?;
            legacy.insert("steps".into(), Value::Array(steps));
        }
        // (every macro as the write read them back, not read again)
        let all = self.write_macro(index, &Value::Object(legacy))?;
        Ok(Value::Object(all.as_object().map(|m| m.iter().map(|(i, v)| (i.clone(), crate::shared::macro_of(v))).collect()).unwrap_or_default()))
    }

    /// Every stored macro (see `macro_view`), each read once.
    fn macros(&self) -> Result<Value> {
        self.pass();
        let mut out = Map::new();
        for i in self.conn.with(|k| k.macro_slots())? {
            self.pass();
            let raw = self.conn.with(|k| k.macro_report(i))?;
            if let Some(meta) = crate::device::stored_macro(&raw) {
                out.insert(i.to_string(), macro_view(meta, &raw)?);
            }
        }
        Ok(Value::Object(out))
    }

    /// Let go of the keyboard entirely, so nothing of Ectomatter's has it open.
    pub fn release(&self) {
        self.conn.close();
    }

    /// Where both sliders are, or nothing on firmware that cannot say.
    pub fn sliders(&self) -> Result<Value> {
        self.conn.with(|k| Ok(match k.sliders()? {
            Some(s) => serde_json::to_value(s).unwrap_or(Value::Null),
            None => Value::Null,
        }))
    }

    /// Hold the dimmer at a level, or give the slider it back.
    pub fn set_dimmer(&self, level: Option<u8>) -> Result<bool> {
        self.conn.with(|k| k.set_dimmer(level))
    }

    /// Light the screen, for now: until the profile is applied again.
    pub fn set_lcd_light(&self, brightness: u8, rgb: [u8; 3]) -> Result<bool> {
        self.conn.with(|k| k.set_lcd_light(brightness, rgb))
    }

    /// Show these built-in screens, for now: until the profile is applied again.
    pub fn set_lcd_apps(&self, apps: u8) -> Result<bool> {
        self.conn.with(|k| k.set_lcd_apps(apps))
    }

    /// Set the clock on the keyboard's status screen: the year, then the month, day,
    /// hour, minute and second. It stands still until the next one.
    pub fn set_time(&self, year: u16, time: [u8; 5]) -> Result<bool> {
        self.conn.with(|k| k.set_time(year, time))
    }

    /// Take the clock off the status screen until a time is set again.
    pub fn clear_time(&self) -> Result<bool> {
        self.conn.with(|k| k.clear_time())
    }

    /// Ask the keyboard to stop doing a slider's job by itself.
    pub fn set_modes(&self, quiet_volume: bool, keep_dimmer: bool) -> Result<bool> {
        self.conn.with(|k| k.set_modes(quiet_volume, keep_dimmer))
    }

    fn backup(&self) -> Result<()> {
        let mut done = self.backed_up.lock().unwrap_or_else(|p| p.into_inner());
        let Some(root) = &self.backups else {
            return Ok(());
        };
        if *done {
            return Ok(());
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let dir = root.join(secs.to_string());
        fs::create_dir_all(&dir)?;
        for n in PROFILES {
            self.pass();
            fs::write(dir.join(format!("profile-{n}.bin")), &self.conn.with(|k| k.read_profile(n))?.raw)?;
        }
        self.pass();
        for i in self.conn.with(|k| k.macro_slots())? {
            self.pass();
            let raw = self.conn.with(|k| k.macro_report(i))?;
            if crate::device::stored_macro(&raw).is_some() {
                fs::write(dir.join(format!("macro-{i:03}.bin")), raw)?;
            }
        }
        *done = true;
        Ok(())
    }

    /// Save profile `n`. `body` holds any of `name`, `lighting`, `keys` and
    /// `fnkeys` (116 assignments of four bytes each).
    pub fn save(&self, n: u8, body: &Value) -> Result<Value> {
        let body = obj(body, "profile")?;
        self.save_with(n, |p| {
            if let Some(name) = body
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                p.set_name(&name.chars().take(60).collect::<String>());
            }
            if let Some(l) = body.get("lighting") {
                p.set_lighting(l)?;
            }
            for (field, secondary) in [("keys", false), ("fnkeys", true)] {
                let Some(keys) = body.get(field).filter(|v| !v.is_null()) else {
                    continue;
                };
                let keys = keys
                    .as_array()
                    .ok_or_else(|| invalid(format!("{field} must be a list")))?;
                for (pos, d) in keys.iter().enumerate().take(NKEYS) {
                    let d = d
                        .as_array()
                        .filter(|d| d.len() == 4)
                        .ok_or_else(|| invalid("a key is four numbers"))?;
                    let define: KeyDefine = [
                        byte(&d[0], "key")?,
                        byte(&d[1], "key")?,
                        byte(&d[2], "key")?,
                        byte(&d[3], "key")?,
                    ];
                    p.set_key(pos, define, secondary)?;
                }
            }
            Ok(())
        })
        .map(|p| view(&p))
    }

    /// Save profile `n` in the shared shapes (`profiles/set`): `body` holds any
    /// of its `name`, its `lighting` (shared.rs) and its `keys`, any keys of
    /// either layer by their ids. Answers with the profile as read back.
    pub fn save_shared(&self, n: u8, body: &Value) -> Result<Value> {
        let body = obj(body, "profile")?;
        self.save_with(n, |p| {
            if let Some(name) = body
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                p.set_name(&name.chars().take(60).collect::<String>());
            }
            if let Some(l) = body.get("lighting") {
                p.set_lighting(&crate::shared::legacy_lighting(l)?)?;
            }
            if let Some(keys) = body.get("keys").filter(|v| !v.is_null()) {
                crate::shared::set_layers(p, keys)?;
            }
            Ok(())
        })
        .map(|p| shared_view(&p))
    }

    /// Save profile `n` changed by `apply`: read it, change it, write it,
    /// load it again where the keyboard runs it, and read it back to confirm.
    /// Every way of saving a profile goes through here, so each sends the
    /// keyboard the same exchange.
    fn save_with(&self, n: u8, apply: impl FnOnce(&mut Profile) -> Result<()>) -> Result<Profile> {
        if !(1..=5).contains(&n) {
            return Err(invalid("profiles are numbered 1 to 5"));
        }
        self.backup()?;
        self.pass();
        let (before, p) = self.conn.with(|k| {
            let mut p = k.read_profile(n)?;
            let before = p.clone();
            apply(&mut p)?;
            if !k.write_profile(n, &p)? {
                return Err(Error::Refused {
                    what: format!("the write to profile {n}"),
                    status: 0,
                });
            }
            Ok((before, p))
        })?;
        self.pass();
        self.conn.with(|k| {
            // The profile in use is loaded again so the keyboard runs what was
            // saved. Not while a preview shows a change to the lighting alone
            // that the preview itself can show: it already shows it, and
            // loading would drop the preview for a moment, a flash on the keys
            // (ending the preview loads it). Spiral's colors it can't show (the
            // keyboard takes them from the profile loaded), so a change to
            // them is loaded at once.
            let lighting_only = before.same_but_lighting(&p) && before.preview_shows(&p);
            if k.profile_number()? == Some(n) && !(lighting_only && self.live.showing()) {
                k.reload_profile()?;
            }
            let back = k.read_profile(n)?;
            if back.raw[8..] != p.raw[8..] {
                return Err(invalid(format!(
                    "profile {n} read back different from what was written"
                )));
            }
            Ok(back)
        })
    }

    /// Save or delete macro `index`. `body` holds `name`, `steps` and the
    /// play settings `run`, `repeat` and `unit`, or `delete: true`.
    pub fn write_macro(&self, index: u8, body: &Value) -> Result<Value> {
        let body = obj(body, "macro")?;
        if !(1..=MACRO_SLOTS).contains(&index) {
            return Err(invalid("macros are numbered 1 to 100"));
        }
        let setting = |k: &str, d: u8| body.get(k).map_or(Ok(d), |v| byte(v, k));
        self.backup()?;
        self.pass();
        self.conn.with(|k| {
            let ok = if body
                .get("delete")
                .is_some_and(|v| crate::json::truthy(Some(v)))
            {
                k.delete_macro(index)?
            } else {
                let name = body.get("name").and_then(Value::as_str).unwrap_or("Macro");
                let steps: &[Value] = match body.get("steps") {
                    Some(Value::Array(a)) => a,
                    _ => &[],
                };
                k.write_macro(
                    index,
                    &macro_body(
                        name,
                        steps,
                        setting("run", 9)?,
                        setting("repeat", 0)?,
                        setting("unit", 1)?,
                    )?,
                )?
            };
            if !ok {
                return Err(Error::Refused {
                    what: format!("the write to macro {index}"),
                    status: 0,
                });
            }
            Ok(())
        })?;
        self.macros()
    }

    /// Hand the lighting back to the stored profile: end any preview stream
    /// (one a program left running when it was stopped short) and apply the
    /// profile again, as a preview's own end does.
    pub fn hand_back(&self) -> Result<()> {
        self.conn.with(|k| {
            k.stop_stream()?;
            k.exit_sync()?;
            k.reload_profile()?;
            Ok(())
        })
    }

    /// The profile the keyboard is using now, as it reports it (it can be
    /// switched on the keyboard itself).
    pub fn active(&self) -> Result<Option<u8>> {
        self.conn.with(|k| k.profile_number())
    }

    /// Switch the keyboard to profile `n`; returns the profile it reports.
    pub fn activate(&self, n: u8) -> Result<Option<u8>> {
        if !(1..=5).contains(&n) {
            return Err(invalid("profiles are numbered 1 to 5"));
        }
        self.conn.with(|k| {
            if !k.select_profile(n)? {
                return Err(Error::Refused {
                    what: format!("switching to profile {n}"),
                    status: 0,
                });
            }
            k.profile_number()
        })
    }

    /// Show something on the keyboard without storing it. With the editor
    /// open in more than one window, only the one used last (which says so
    /// with `claim`) owns the preview; the others are ignored.
    pub fn preview(&self, body: &Value) -> Result<Value> {
        let body = obj(body, "preview")?;
        let tab = body.get("tab").and_then(Value::as_str).map(str::to_string);
        if crate::json::truthy(body.get("claim")) || self.live.owner().is_none() {
            self.live.set_owner(tab.clone());
        }
        if tab != self.live.owner() {
            let mut s = self.live.status();
            s["mode"] = Value::Null;
            s["elsewhere"] = json!(true);
            return Ok(s);
        }
        if crate::json::truthy(body.get("stop")) {
            self.live.stop();
        } else if let Some(c) = body.get("colors") {
            let c = c
                .as_array()
                .ok_or_else(|| invalid("colors must be a list"))?;
            let mut out = Vec::with_capacity(c.len());
            for rgb in c {
                let a = rgb
                    .as_array()
                    .filter(|a| a.len() == 3)
                    .ok_or_else(|| invalid("a color is three numbers"))?;
                out.push([
                    byte(&a[0], "red")?,
                    byte(&a[1], "green")?,
                    byte(&a[2], "blue")?,
                ]);
            }
            self.live.keys(out);
        } else if body.get("lighting").is_some_and(|l| l["mode"] == "off") {
            // The original editor's Off frame is the one that leaves the preview,
            // so the keyboard showed its stored lighting instead: every key black
            // shows Off.
            self.live.keys(vec![[0; 3]; crate::protocol::NKEYS]);
        } else if let Some(l) = body.get("lighting") {
            // `t`: where the editor's own drawing of the effect is (ms)
            self.live.preset_at(l.clone(), body.get("t").and_then(Value::as_u64));
        }
        Ok(self.live.status())
    }

    /// Show four rows of text on the display.
    pub fn show_lcd(&self, rows: &[&str]) -> Result<()> {
        self.conn.with(|k| k.show_lcd(rows))
    }

    /// Hand the display back to the keyboard's own screens.
    pub fn hand_back_lcd(&self) -> Result<()> {
        self.conn.with(|k| k.hand_back_lcd())
    }
}
