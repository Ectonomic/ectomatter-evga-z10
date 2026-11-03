//! The EVGA Z10 RGB's core: the keyboard's library (crates/evga-z10),
//! reaching the keyboard through the USB connection Ectomatter lends it, and
//! answering Ectomatter's calls (wit/core.wit).
//!
//! The calls are the core protocol's (docs/core-protocol.md), in its shared
//! shapes (evga_z10::shared): profiles, keys, macros, lighting, the
//! display, settings and the sliders. The preview's loop has no thread here:
//! it runs a beat at a time on the timer it asks Ectomatter for, and between
//! each exchange of a long call (a whole read, a save), where the previews
//! and the display's frames asked for meanwhile are taken too, so the keys
//! and the screen keep up whatever else the keyboard is doing.
//!
//! Asked to (`input/watch`), the core listens to the keyboard's own key
//! reports, read beside the computer, which still gets every key: each key
//! given a host slot is told as it goes down and up, and while Ectomatter
//! watches every press, every key and media key too, by what it sent.

use std::cell::{Cell, RefCell};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use ectomatter_protocol::Action;
use evga_z10::protocol::{INTERFACE, PID, USAGE, USAGE_PAGE, VID};
use evga_z10::reports::{self, Down};
use evga_z10::shared::{HOST_FIRST, HOST_SLOTS};
use evga_z10::{Conn, Error, Keyboard, Result, Transport, Z10};
use serde_json::{Value, json};

wit_bindgen::generate!({ path: "../../../wit", world: "core" });

use ectomatter::core::{host, usb};

/// The timer the preview's loop runs on.
const BEAT: u32 = 1;
/// The timer the key reports are listened to on: every 8 ms, a press heard
/// within a frame, and every 32 once nothing has come for two seconds; and
/// every two seconds, a keyboard not there is looked for again.
const HEAR: u32 = 2;
const HEAR_EVERY: u32 = 8;
const HEAR_QUIET: u32 = 32;
const QUIET_AFTER: Duration = Duration::from_secs(2);
const LOOK_AGAIN: u32 = 2000;

/// The calls a long call takes between its exchanges with the keyboard: what
/// shows on the keys and the display now, which mustn't wait for a whole
/// read or a save.
const QUICK: [&str; 5] = ["lighting/preview", "lighting/direct", "lighting/preview/stop", "displays/show", "displays/hand-back"];

/// The keyboard's collection, as Ectomatter opened it.
struct Lent {
    dev: usb::HidDevice,
    path: String,
}

// A core module runs on one thread; nothing here is shared across threads.
unsafe impl Send for Lent {}

impl Transport for Lent {
    fn set_feature(&mut self, buf: &[u8]) -> Result<()> {
        self.dev.send_feature(buf).map_err(Error::Link)
    }
    fn get_feature(&mut self, rid: u8, len: usize) -> Result<Vec<u8>> {
        self.dev.get_feature(rid, len as u32).map_err(Error::Link)
    }
    fn path(&self) -> String {
        self.path.clone()
    }
}

/// The Z10's own collection, as the keyboard code finds it over hidapi: by
/// its interface and its usage page and usage, or else by either (its
/// ten-key interface has a collection of that usage too, which takes no
/// commands).
fn open() -> Result<evga_z10::conn::Device> {
    let all: Vec<_> = usb::devices().into_iter().filter(|d| d.vendor == VID && d.product == PID).collect();
    let info = all
        .iter()
        .find(|d| d.interface == INTERFACE && d.usage_page == USAGE_PAGE && d.usage == USAGE)
        .or_else(|| all.iter().find(|d| d.interface == INTERFACE))
        .or_else(|| all.iter().find(|d| d.interface < 0 && d.usage_page == USAGE_PAGE && d.usage == USAGE))
        .ok_or(Error::NotFound)?;
    let dev = usb::open(info).map_err(Error::Link)?;
    Ok(Z10::new(Box::new(Lent { dev, path: info.path.clone() }) as Box<dyn Transport>))
}

fn keyboard() -> &'static Keyboard {
    static KB: OnceLock<Keyboard> = OnceLock::new();
    // a session's first write saves what it replaces in the core's own folder,
    // unless `initialize` said not to (a test replaying a recorded exchange)
    KB.get_or_init(|| Keyboard::driven(Conn::new(Arc::new(open)), BACKUPS.get().then(|| "/data/backups".into())).between(between))
}

/// One of the keyboard's interfaces whose reports are listened to, opened
/// apart from the editor's: its number, and what its last report said was down.
struct Ear {
    iface: i32,
    dev: usb::HidDevice,
    keys: Vec<u8>,
}

/// Listening to the keyboard's key reports: its interfaces (none while it
/// isn't there, looked for again now and then), whether every press is told
/// or the host slots' alone, what is down (the keys over every interface, and
/// the media key), when a report last came, and whether the keyboard was last
/// told as there (`device/added`) or gone (`device/removed`).
struct Listen {
    ears: Vec<Ear>,
    all: bool,
    down: Vec<u8>,
    media: u16,
    heard: Instant,
    due: Instant,
    told_there: Option<bool>,
}

thread_local! {
    /// listening to the keyboard's key reports, once Ectomatter asks
    static LISTEN: RefCell<Option<Listen>> = const { RefCell::new(None) };
    /// whether a press has been heard yet, for the log's one line saying so
    static FIRST_HEARD: Cell<bool> = const { Cell::new(false) };
    /// whether the first write of a session saves what it replaces
    static BACKUPS: Cell<bool> = const { Cell::new(true) };
    /// whether the beat's timer is set
    static TIMED: Cell<bool> = const { Cell::new(false) };
    /// when the preview was changed, until the frame showing it goes out
    static ASKED: Cell<Option<Instant>> = const { Cell::new(None) };
    /// whether a change's time has been logged yet
    static LOGGED: Cell<bool> = const { Cell::new(false) };
    /// the sliders freed of their own jobs (input/free): the volume slider's volume keys,
    /// and the dimmer's hold on the backlight
    static FREED: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// Between two of a long call's exchanges: the previews asked for meanwhile,
/// and the preview's frame if one is due.
fn between() {
    for w in host::take_waiting(&QUICK.map(String::from)) {
        let p: Value = serde_json::from_str(&w.params).unwrap_or(Value::Null);
        let r = answer(&w.method, &p).map(|v| v.to_string());
        asked();
        host::answer(w.id, r.as_deref().map_err(String::as_str));
    }
    let kb = keyboard();
    if kb.wants_tick() {
        sent(kb.tick());
    }
    // (and the key reports, when they're due, so a press isn't held up either)
    if LISTEN.with(|l| l.borrow().as_ref().is_some_and(|l| Instant::now() >= l.due)) {
        hear();
    }
}

/// The keyboard's interfaces whose reports say what is down (0, 1 and 2),
/// each opened once, apart from the editor's; how many said in the log.
fn ears() -> Vec<Ear> {
    let out = open_ears();
    if !out.is_empty() {
        host::log(&format!("listening to the keyboard's keys on {} of its 3 interfaces", out.len()));
    }
    out
}

fn open_ears() -> Vec<Ear> {
    let all: Vec<_> = usb::devices().into_iter().filter(|d| d.vendor == VID && d.product == PID).collect();
    let mut out: Vec<Ear> = Vec::new();
    for iface in [0, 2, 1] {
        if let Some(d) = all.iter().find(|d| d.interface == iface) {
            if let Ok(dev) = usb::open(d) {
                // (what was waiting from before is no press now)
                while dev.read(0).is_ok_and(|r| !r.is_empty()) {}
                out.push(Ear { iface, dev, keys: Vec::new() });
            }
        }
    }
    out
}

/// Listen to the keyboard's key reports from now on, telling every press
/// (`all`) or the host slots' alone.
fn watch(all: bool) -> bool {
    let fresh = LISTEN.with(|l| {
        let mut l = l.borrow_mut();
        match l.as_mut() {
            Some(l) => {
                l.all = all;
                false
            }
            None => {
                *l = Some(Listen { ears: ears(), all, down: Vec::new(), media: 0, heard: Instant::now(), due: Instant::now(), told_there: None });
                true
            }
        }
    });
    if fresh {
        host::after(0, HEAR);
    }
    LISTEN.with(|l| l.borrow().as_ref().is_some_and(|l| !l.ears.is_empty()))
}

/// A press told to Ectomatter: what it sent, and for a host slot's key the slot.
fn tell(down: bool, sends: &Action) {
    let mut p = json!({ "device": "z10", "sends": sends.write() });
    if let Action::Host(n) = sends {
        p["host"] = json!(n);
    }
    host::notify(if down { "input/down" } else { "input/up" }, &p.to_string());
}

/// A key's usage as what it sent: a host slot's code as its slot.
fn sent_by(u: u8) -> Action {
    if (HOST_FIRST..HOST_FIRST + HOST_SLOTS).contains(&u) { Action::Host((u - HOST_FIRST) as u32) } else { Action::Key(u) }
}

/// What is down over every interface now, against what was: each key come
/// down or let go put in `told`.
fn changed(l: &mut Listen, told: &mut Vec<(bool, Action)>) {
    let mut down: Vec<u8> = Vec::new();
    for ear in &l.ears {
        for k in &ear.keys {
            if !down.contains(k) {
                down.push(*k);
            }
        }
    }
    told.extend(down.iter().filter(|k| !l.down.contains(k)).map(|k| (true, sent_by(*k))));
    told.extend(l.down.iter().filter(|k| !down.contains(k)).map(|k| (false, sent_by(*k))));
    l.down = down;
}

/// The key reports since last time: each change told, and the next listen
/// set; a keyboard gone lets go of everything down, and is looked for again.
fn hear() {
    let told = LISTEN.with(|l| {
        let mut l = l.borrow_mut();
        let Some(l) = l.as_mut() else { return None };
        let now = Instant::now();
        let mut there = !l.ears.is_empty();
        if l.ears.is_empty() {
            // (whether it's listed at all, opened or not, says it's there)
            there = usb::devices().iter().any(|d| d.vendor == VID && d.product == PID);
            if there {
                l.ears = ears();
            }
        }
        let mut told: Vec<(bool, Action)> = Vec::new();
        // (each report as it came: a key pressed and let go between two
        // listens is told both ways)
        let mut gone = false;
        for e in 0..l.ears.len() {
            // (a few at most: each report says all that is down on its interface)
            for _ in 0..64 {
                match l.ears[e].dev.read(0) {
                    Ok(r) if r.is_empty() => break,
                    Ok(r) => {
                        l.heard = now;
                        match reports::parse(l.ears[e].iface, &r) {
                            Some(Down::Keys(k)) => l.ears[e].keys = k,
                            Some(Down::Media(m)) => {
                                if m != l.media {
                                    if l.media != 0 {
                                        told.push((false, Action::Media(l.media)));
                                    }
                                    if m != 0 {
                                        told.push((true, Action::Media(m)));
                                    }
                                    l.media = m;
                                }
                                continue;
                            }
                            None => continue,
                        }
                        changed(l, &mut told);
                    }
                    Err(_) => {
                        gone = true;
                        break;
                    }
                }
            }
        }
        if gone {
            l.ears.clear();
            changed(l, &mut told);
            if l.media != 0 {
                told.push((false, Action::Media(l.media)));
                l.media = 0;
            }
        }
        let all = l.all;
        told.retain(|(_, a)| all || matches!(a, Action::Host(_)));
        // (gone, it's told so once a look no longer finds it)
        let there_now = there && !(gone && !usb::devices().iter().any(|d| d.vendor == VID && d.product == PID));
        let came = (l.told_there != Some(there_now)).then(|| {
            l.told_there = Some(there_now);
            there_now
        });
        let wait = if l.ears.is_empty() {
            LOOK_AGAIN
        } else if !l.down.is_empty() || now.duration_since(l.heard) < QUIET_AFTER {
            HEAR_EVERY
        } else {
            HEAR_QUIET
        };
        l.due = now + Duration::from_millis(wait as u64);
        Some((told, wait, came))
    });
    let Some((told, wait, came)) = told else { return };
    if !told.is_empty() && !FIRST_HEARD.get() {
        FIRST_HEARD.set(true);
        host::log("heard the keyboard's first press since listening began");
    }
    for (down, sends) in told {
        tell(down, &sends);
    }
    if let Some(there) = came {
        host::notify(if there { "device/added" } else { "device/removed" }, &json!({ "device": "z10" }).to_string());
    }
    host::after(wait, HEAR);
}

/// Note when a preview changed, for the log line its first frame gives. (A
/// preview stopped shows no frame: nothing is timed, or the next preview's
/// first frame, minutes on, would be timed from the stop.)
fn asked() {
    if !keyboard().previewing() {
        ASKED.set(None);
        return;
    }
    if keyboard().fresh() && ASKED.get().is_none() {
        ASKED.set(Some(Instant::now()));
    }
}

/// After a beat: if a frame went out for a changed preview, how long after
/// it was asked. Logged the first time, and whenever it took longer than
/// one of the screen's frames at 30 per second.
fn sent(n: evga_z10::live::Next) -> evga_z10::live::Next {
    if let evga_z10::live::Next::After(_) = n {
        if let Some(at) = ASKED.take() {
            let ms = at.elapsed().as_millis();
            if !LOGGED.get() || ms > 33 {
                LOGGED.set(true);
                host::log(&format!("the keys showed a change to the preview {ms} ms after it was asked"));
            }
        }
    }
    n
}

/// Set the beat's timer if the preview's loop has a beat due and none is set.
fn keep_time() {
    if keyboard().wants_tick() && !TIMED.get() {
        TIMED.set(true);
        host::after(0, BEAT);
    }
}

fn byte(v: &Value, what: &str) -> std::result::Result<u8, String> {
    v.as_u64().and_then(|n| u8::try_from(n).ok()).ok_or_else(|| format!("{what} must be 0 to 255"))
}

/// A preview's body in the keyboard code's own shape: its lighting, if it
/// shows one, from the shared shape.
fn preview_body(p: &Value) -> std::result::Result<Value, String> {
    let mut body = p.clone();
    if let Some(l) = p.get("lighting").filter(|l| !l.is_null()) {
        body["lighting"] = evga_z10::shared::legacy_lighting(l).map_err(|e| e.to_string())?;
    }
    Ok(body)
}

/// The keyboard as the protocol describes it (`device/describe`): who it is
/// and what it can do. Its drawing, its keys and its light are in its
/// device file, beside its module, which its manifest names (`deviceFile`).
fn describe() -> Value {
    json!({
        "device": "z10",
        "name": "EVGA Z10 RGB",
        "kind": "keyboard",
        "vendor": "EVGA",
        "model": "Z10 RGB",
        "capabilities": evga_z10::shared::capabilities(),
        "layout": { "lighting": evga_z10::shared::lighting_layout() },
    })
}

fn answer(method: &str, p: &Value) -> std::result::Result<Value, String> {
    // before the keyboard is made, which reads what this sets
    if method == "initialize" {
        let met = ectomatter_protocol::meet(p, "EVGA Z10 RGB", env!("CARGO_PKG_VERSION"))?;
        if let Some(b) = p["backups"].as_bool() {
            BACKUPS.set(b);
        }
        return Ok(met);
    }
    let kb = keyboard();
    let s = |r: Result<Value>| r.map_err(|e| e.to_string());
    let b = |r: Result<bool>| r.map(Value::Bool).map_err(|e| e.to_string());
    let profile = |p: &Value| byte(&p["profile"], "profile");
    match method {
        "device/describe" => Ok(describe()),
        "device/read" => s(kb.read()),
        "device/release" => {
            kb.release();
            Ok(Value::Null)
        }
        "profiles/active" => s(kb.active().map(|n| json!(n))),
        "profiles/activate" => s(kb.activate(profile(p)?).map(|n| json!(n))),
        "profiles/get" => s(kb.profile(profile(p)?)),
        "profiles/set" => s(kb.save_shared(profile(p)?, p)),
        "keys/get" => s(kb.profile(profile(p)?).and_then(|got| Ok(json!({ "layers": got["keys"], "factory": kb.profile(254)?["keys"] })))),
        "lighting/get" => s(kb.profile(profile(p)?).map(|got| got["lighting"].clone())),
        "lighting/set" => s(kb.save_shared(profile(p)?, &json!({ "lighting": p["lighting"] })).map(|got| got["lighting"].clone())),
        "lighting/preview" | "lighting/direct" => s(kb.preview(&preview_body(p)?)),
        "lighting/preview/stop" => {
            let mut body = p.clone();
            body["stop"] = json!(true);
            s(kb.preview(&body))
        }
        "lighting/hand-back" => kb.hand_back().map(|()| Value::Null).map_err(|e| e.to_string()),
        "macros/list" => s(kb.macros_shared()),
        "macros/set" => s(kb.write_macro_shared(byte(&p["macro"], "macro")?, p)),
        "macros/delete" => s(kb.write_macro_shared(byte(&p["macro"], "macro")?, &json!({ "delete": true }))),
        "settings/set" => match p["id"].as_str() {
            Some("dimmer") => b(kb.set_dimmer(if p["value"].is_null() { None } else { Some(byte(&p["value"], "the backlight")?) })),
            Some("modes") => b(kb.set_modes(p["value"]["quiet_volume"].as_bool().unwrap_or(false), p["value"]["keep_dimmer"].as_bool().unwrap_or(false))),
            other => Err(format!("The EVGA Z10 RGB has no setting {}.", other.unwrap_or("without a name"))),
        },
        "input/state" => s(kb.sliders()),
        // a slider freed of its own job, or given it back: the firmware's two modes at once
        "input/free" => {
            let (mut quiet, mut keep) = FREED.get();
            let free = p["free"].as_bool().unwrap_or(false);
            match p["slider"].as_str() {
                Some("volume") => quiet = free,
                Some("dimmer") => keep = free,
                other => return Err(format!("The EVGA Z10 RGB has no slider {}.", other.unwrap_or("without a name"))),
            }
            FREED.set((quiet, keep));
            b(kb.set_modes(quiet, keep))
        }
        "input/watch" => Ok(json!({ "listening": watch(p["all"].as_bool().unwrap_or(false)) })),
        "displays/show" => {
            let rows: Vec<&str> = p["rows"].as_array().map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            kb.show_lcd(&rows).map(|()| Value::Null).map_err(|e| e.to_string())
        }
        "displays/hand-back" => kb.hand_back_lcd().map(|()| Value::Null).map_err(|e| e.to_string()),
        "displays/light" => {
            let rgb = &p["rgb"];
            b(kb.set_lcd_light(byte(&p["brightness"], "brightness")?, [byte(&rgb[0], "red")?, byte(&rgb[1], "green")?, byte(&rgb[2], "blue")?]))
        }
        "displays/apps" => b(kb.set_lcd_apps(byte(&p["apps"], "apps")?)),
        "displays/time" => {
            let year = p["year"].as_u64().and_then(|y| u16::try_from(y).ok()).ok_or("year must be a year")?;
            let t = &p["time"];
            b(kb.set_time(year, [byte(&t[0], "month")?, byte(&t[1], "day")?, byte(&t[2], "hour")?, byte(&t[3], "minute")?, byte(&t[4], "second")?]))
        }
        "displays/time/clear" => b(kb.clear_time()),
        other => Err(format!("The EVGA Z10 RGB's core has no call named {other}.")),
    }
}

struct Core;

impl Guest for Core {
    fn call(method: String, params: String) -> std::result::Result<String, String> {
        let p: Value = if params.is_empty() { Value::Null } else { serde_json::from_str(&params).map_err(|e| format!("{method}: {e}"))? };
        let r = answer(&method, &p).map(|v| v.to_string());
        if QUICK.contains(&method.as_str()) && keyboard().fresh() {
            asked();
            // the change goes now, not at the next beat
            TIMED.set(true);
            host::after(0, BEAT);
        }
        keep_time();
        r
    }

    fn timer(tag: u32) {
        if tag == HEAR {
            hear();
            return;
        }
        if tag != BEAT {
            return;
        }
        TIMED.set(false);
        match sent(keyboard().tick()) {
            evga_z10::live::Next::After(d) | evga_z10::live::Next::Wait(d) => {
                TIMED.set(true);
                host::after(d.as_millis() as u32, BEAT);
            }
            evga_z10::live::Next::Now => {
                TIMED.set(true);
                host::after(0, BEAT);
            }
            evga_z10::live::Next::Idle => keep_time(),
        }
    }
}

export!(Core);
