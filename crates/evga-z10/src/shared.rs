//! The Z10 in the core protocol's shared shapes (docs/core-protocol.md): its
//! keys by their ids, what each does as a shared action, its lighting and its
//! macros' steps, and what it can do. Each turns the keyboard's own form into
//! the shared one and back without losing anything: what no shared shape can
//! hold travels as the keyboard's own bytes (`{"raw": [...]}`).

use ectomatter_protocol::{Action, Jump, Plays, keys, media};
use serde_json::{Map, Value, json};

use crate::error::{Result, invalid};
use crate::profile::{KeyDefine, Profile};
use crate::protocol::NKEYS;

/// Each key's id, by its place in the keyboard's own reports (its `pos`), as
/// the core's description (cores/evga-z10/device.json) names it.
pub const CONTROLS: [&str; NKEYS] = [
    "gpc", "esc", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "print-screen",
    "scroll-lock", "pause", "stop", "previous", "play", "next", "e1", "grave", "1", "2", "3", "4", "5", "6",
    "7", "8", "9", "0", "minus", "equals", "backspace", "insert", "home", "page-up", "num-lock",
    "keypad-slash", "keypad-star", "keypad-minus", "e2", "tab", "q", "w", "e", "r", "t", "y", "u", "i", "o",
    "p", "left-bracket", "right-bracket", "backslash", "delete", "end", "page-down", "keypad-7", "keypad-8",
    "keypad-9", "keypad-plus", "e3", "caps-lock", "a", "s", "d", "f", "g", "h", "j", "k", "l", "semicolon",
    "quote", "enter", "keypad-4", "keypad-5", "keypad-6", "e4", "left-shift", "z", "x", "c", "v", "b", "n",
    "m", "comma", "period", "slash", "right-shift", "e6", "up", "e7", "keypad-1", "keypad-2", "keypad-3",
    "keypad-enter", "e5", "left-ctrl", "fn", "left-alt", "space", "right-alt", "win", "menu", "right-ctrl",
    "left", "down", "right", "keypad-0", "keypad-dot",
];

/// The key at a place in the keyboard's reports.
pub fn control(pos: usize) -> Option<&'static str> {
    CONTROLS.get(pos).copied()
}

/// A key's place in the keyboard's reports, by its id.
pub fn pos_of(id: &str) -> Option<usize> {
    CONTROLS.iter().position(|c| *c == id)
}

/// The codes the USB standard reserves that a key given a host slot sends,
/// one a slot: no key is meant to send one and the system turns none into a
/// key press, so Ectomatter hears them in the keyboard's own reports.
pub const HOST_FIRST: u8 = 0xA5;
pub const HOST_SLOTS: u8 = 11;

/// The keyboard's own actions, which have no shared word: their ids and labels.
pub const OWN: [(&str, &str); 1] = [("game-mode", "Game mode toggle")];

/// What a key does, as a shared action: the keyboard's four bytes as the
/// action they stand for, or as themselves where none stands for them exactly.
pub fn action_of(d: KeyDefine) -> Action {
    let a = structured(d);
    if define_of(&a) == Some(d) { a } else { Action::Raw(d.to_vec()) }
}

/// The shared action the bytes stand for, if one does (checked by
/// `action_of`, which keeps the bytes where it doesn't stand for them exactly).
fn structured([f, a, b, c]: KeyDefine) -> Action {
    match f {
        0 if a == 0 && b == 0 && c == 0 => Action::None,
        0 if a == 0 && c == 0 && (HOST_FIRST..HOST_FIRST + HOST_SLOTS).contains(&b) => Action::Host((b - HOST_FIRST) as u32),
        0 => {
            let mut codes: Vec<u8> = (0..8).filter(|i| a & (1 << i) != 0).map(|i| keys::FIRST_MODIFIER + i).collect();
            codes.extend([b, c].into_iter().filter(|u| *u != 0));
            if codes.len() == 1 { Action::Key(codes[0]) } else { Action::Shortcut(codes) }
        }
        2 => Action::Media(a as u16 | (b as u16) << 8),
        3 => Action::Macro {
            n: a as u32,
            plays: match b {
                11 => Plays::Times(c as u32),
                12 => Plays::WhileHeld,
                8 => Plays::UntilPressedAgain,
                _ => Plays::Once,
            },
        },
        4 => Action::Fn,
        5 => Action::Own(OWN[0].0.into()),
        6 => Action::Scroll { up: a == 0 },
        7 => Action::Mouse(1),
        8 => Action::Mouse(2),
        9 => Action::Mouse(3),
        17..=21 => Action::Profile(Jump::To((f - 16) as u32)),
        22 => Action::Profile(Jump::Next),
        23 => Action::Profile(Jump::Previous),
        255 => Action::Off,
        _ => Action::Raw(vec![f, a, b, c]),
    }
}

/// The keyboard's four bytes for a shared action, or none where the keyboard
/// can't do it.
pub fn define_of(a: &Action) -> Option<KeyDefine> {
    Some(match a {
        Action::None => [0, 0, 0, 0],
        Action::Off => [255, 0, 0, 0],
        Action::Key(u) if keys::is_modifier(*u) => [0, 1 << (u - keys::FIRST_MODIFIER), 0, 0],
        Action::Key(u) => [0, 0, *u, 0],
        Action::Shortcut(codes) => {
            let mods = codes.iter().filter(|u| keys::is_modifier(**u)).fold(0u8, |m, u| m | 1 << (u - keys::FIRST_MODIFIER));
            let rest: Vec<u8> = codes.iter().copied().filter(|u| !keys::is_modifier(*u)).collect();
            if rest.len() > 2 || codes.is_empty() {
                return None;
            }
            [0, mods, rest.first().copied().unwrap_or(0), rest.get(1).copied().unwrap_or(0)]
        }
        Action::Media(c) => [2, (c & 0xFF) as u8, (c >> 8) as u8, 0],
        Action::Macro { n, plays } => {
            let n = u8::try_from(*n).ok()?;
            match plays {
                Plays::Once => [3, n, 0, 0],
                Plays::Times(t) => [3, n, 11, u8::try_from(*t).ok()?],
                Plays::WhileHeld => [3, n, 12, 0],
                Plays::UntilPressedAgain => [3, n, 8, 0],
            }
        }
        Action::Fn => [4, 0, 0, 0],
        Action::Own(id) if id == OWN[0].0 => [5, 0, 0, 0],
        Action::Scroll { up } => [6, if *up { 0 } else { 1 }, 0, 0],
        Action::Mouse(1) => [7, 0, 0, 0],
        Action::Mouse(2) => [8, 0, 0, 0],
        Action::Mouse(3) => [9, 0, 0, 0],
        Action::Profile(Jump::To(n)) if (1..=5).contains(n) => [16 + *n as u8, 0, 0, 0],
        Action::Profile(Jump::Next) => [22, 0, 0, 0],
        Action::Profile(Jump::Previous) => [23, 0, 0, 0],
        Action::Host(n) if *n < HOST_SLOTS as u32 => [0, 0, HOST_FIRST + *n as u8, 0],
        Action::Raw(b) if b.len() == 4 => [b[0], b[1], b[2], b[3]],
        _ => return None,
    })
}

/// What every key does on a profile, on each layer, by the keys' ids: the
/// keyboard's own keys as "base" and its Fn layer as "fn".
pub fn layers(p: &Profile) -> Value {
    let layer = |secondary: bool| -> Map<String, Value> {
        p.keys(secondary).into_iter().enumerate().filter_map(|(pos, d)| Some((control(pos)?.to_string(), action_of(d).write()))).collect()
    };
    json!({ "base": layer(false), "fn": layer(true) })
}

/// Set any keys of either layer on a profile, by their ids.
pub fn set_layers(p: &mut Profile, layers: &Value) -> Result<()> {
    let o = layers.as_object().ok_or_else(|| invalid("keys must be a set of layers"))?;
    for (id, actions) in o {
        let secondary = match id.as_str() {
            "base" => false,
            "fn" => true,
            other => return Err(invalid(format!("the keyboard has no layer {other}"))),
        };
        let actions = actions.as_object().ok_or_else(|| invalid(format!("layer {id} must be a set of keys")))?;
        for (control, v) in actions {
            let pos = pos_of(control).ok_or_else(|| invalid(format!("the keyboard has no key {control}")))?;
            let a = Action::read(v).ok_or_else(|| invalid(format!("{v} is not an action")))?;
            let d = define_of(&a).ok_or_else(|| invalid(format!("the keyboard can't give a key {v}")))?;
            p.set_key(pos, d, secondary)?;
        }
    }
    Ok(())
}

/// The effects whose settings the keyboard keeps, each its own, whichever is in use.
const EFFECTS: [&str; 7] = ["static", "breathing", "pulse", "spiral", "wave", "trigger", "stars"];

/// The keyboard's lighting, or any part of it, in the shared shape: the keys'
/// effect, the sub-mode it is stored with and every effect's settings; the
/// display's backlight and its built-in screens; and the keys game mode leaves lit.
pub fn lighting_of(l: &Value) -> Value {
    // (what isn't lighting at all is passed on as it is, for the keyboard
    // code to refuse where it always has)
    if !l.is_object() {
        return l.clone();
    }
    let mut out = Map::new();
    let mut keys = Map::new();
    if let Some(m) = l.get("mode") {
        keys.insert("effect".into(), m.clone());
    }
    if let Some(s) = l.get("sub") {
        keys.insert("sub".into(), s.clone());
    }
    let effects: Map<String, Value> = EFFECTS.iter().filter_map(|e| Some((e.to_string(), l.get(*e)?.clone()))).collect();
    if !effects.is_empty() {
        keys.insert("effects".into(), Value::Object(effects));
    }
    if !keys.is_empty() {
        out.insert("keys".into(), Value::Object(keys));
    }
    if let Some(lcd) = l.get("lcd") {
        out.insert("screen".into(), lcd.clone());
    }
    if let Some(g) = l.get("game_keys") {
        out.insert("game_keys".into(), g.clone());
    }
    Value::Object(out)
}

/// Lighting in the shared shape, or any part of it, in the keyboard's own.
pub fn legacy_lighting(s: &Value) -> Result<Value> {
    let s = s.as_object().ok_or_else(|| invalid("lighting must be a set of lights"))?;
    let mut out = Map::new();
    if let Some(k) = s.get("keys") {
        let k = k.as_object().ok_or_else(|| invalid("the keys' light must be an object"))?;
        if let Some(e) = k.get("effect") {
            out.insert("mode".into(), e.clone());
        }
        if let Some(sub) = k.get("sub") {
            out.insert("sub".into(), sub.clone());
        }
        if let Some(effects) = k.get("effects") {
            // (settings for an effect the keyboard keeps none for are left out, as
            // the keyboard code leaves out what it doesn't know)
            for (id, v) in effects.as_object().ok_or_else(|| invalid("effects must be a set of effects"))? {
                if EFFECTS.contains(&id.as_str()) {
                    out.insert(id.clone(), v.clone());
                }
            }
        }
    }
    if let Some(screen) = s.get("screen") {
        out.insert("lcd".into(), screen.clone());
    }
    if let Some(g) = s.get("game_keys") {
        out.insert("game_keys".into(), g.clone());
    }
    Ok(Value::Object(out))
}

/// The keyboard's own number for a modifier in a macro, which differs from
/// the usages it uses for every other key there: Left Ctrl is 112.
const MACRO_MODIFIER_FIRST: u64 = 112;

/// A macro's steps in the shared shape: each key or media key by its name
/// (or its number, where it has none), pressed or let go, and the wait
/// after it (`after`, in milliseconds); a wait before the first press as
/// `wait`.
pub fn steps_of(steps: &[Value]) -> Vec<Value> {
    steps
        .iter()
        .map(|s| {
            let Some(o) = s.as_object() else { return s.clone() };
            let mut out = Map::new();
            for (k, v) in o {
                match k.as_str() {
                    "code" => {
                        // (a key by its name where the keyboard's code is one key's: its
                        // modifiers' own numbers, or a usage it keeps whole; any other
                        // number as it is)
                        let named = match v.as_u64() {
                            Some(c @ MACRO_MODIFIER_FIRST..=119) => keys::name(keys::FIRST_MODIFIER + (c - MACRO_MODIFIER_FIRST) as u8),
                            Some(c @ (0..=111 | 120..=127)) => keys::name(c as u8),
                            _ => None,
                        };
                        out.insert("key".into(), named.map_or_else(|| v.clone(), Value::String));
                    }
                    "media" => {
                        let named = v.as_u64().and_then(|u| u16::try_from(u).ok()).and_then(media::name);
                        out.insert("media".into(), named.map_or_else(|| v.clone(), |n| json!(n)));
                    }
                    "delay" => {
                        out.insert("after".into(), v.clone());
                    }
                    _ => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Object(out)
        })
        .collect()
}

/// Steps in the shared shape as the keyboard's own.
pub fn legacy_steps(steps: &[Value]) -> Result<Vec<Value>> {
    steps
        .iter()
        .map(|s| {
            let o = s.as_object().ok_or_else(|| invalid("a step must be an object"))?;
            let mut out = Map::new();
            for (k, v) in o {
                match k.as_str() {
                    "key" => {
                        // (a name as the keyboard's code for it; a number as it is)
                        let code = match v {
                            Value::String(n) => {
                                let u = keys::usage(n).ok_or_else(|| invalid(format!("{n} is not a key")))? as u64;
                                if keys::is_modifier(u as u8) {
                                    json!(MACRO_MODIFIER_FIRST + u - keys::FIRST_MODIFIER as u64)
                                } else if u < MACRO_MODIFIER_FIRST || (120..128).contains(&u) {
                                    json!(u)
                                } else {
                                    return Err(invalid(format!("a macro on the keyboard can't press {n}")));
                                }
                            }
                            v => v.clone(),
                        };
                        out.insert("code".into(), code);
                    }
                    "media" => {
                        let c = match v {
                            Value::String(n) => json!(media::code(n).ok_or_else(|| invalid(format!("{n} is not a media key")))?),
                            v => v.clone(),
                        };
                        out.insert("media".into(), c);
                    }
                    "after" => {
                        out.insert("delay".into(), v.clone());
                    }
                    _ => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(Value::Object(out))
        })
        .collect()
}

/// A stored macro in the shared shape: its name, its steps, how it plays as
/// the keyboard keeps it (`run`, `repeat`, `unit`), how long its steps are,
/// and whether Ectomatter can rewrite them exactly.
pub fn macro_of(m: &Value) -> Value {
    let mut out = m.as_object().cloned().unwrap_or_default();
    if let Some(run) = out.remove("method") {
        out.insert("run".into(), run);
    }
    if let Some(steps) = out.get("steps").and_then(Value::as_array) {
        let shared = steps_of(steps);
        out.insert("steps".into(), Value::Array(shared));
    }
    Value::Object(out)
}

/// A profile as the keyboard code's own editor calls gave it (its name,
/// lighting, and both layers as lists of four bytes), in the shared shapes.
pub fn profile_of_view(p: &Value) -> Value {
    let layer = |keys: &Value| -> Value {
        let m: Map<String, Value> = keys
            .as_array()
            .map(|a| {
                a.iter()
                    .enumerate()
                    .take(NKEYS)
                    .filter_map(|(pos, d)| {
                        let b: Vec<u8> = d.as_array()?.iter().map(|v| v.as_u64().and_then(|v| u8::try_from(v).ok())).collect::<Option<_>>()?;
                        let b: [u8; 4] = b.try_into().ok()?;
                        Some((control(pos)?.to_string(), action_of(b).write()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Value::Object(m)
    };
    json!({ "name": p["name"], "keys": { "base": layer(&p["keys"]), "fn": layer(&p["fnkeys"]) }, "lighting": lighting_of(&p["lighting"]) })
}

/// A whole read of the keyboard as the keyboard code's own editor call
/// (`state`) gave it, in the shared shapes `device/read` gives: for states an
/// older Ectomatter saved.
pub fn read_of_state(s: &Value) -> Value {
    let profiles: Map<String, Value> = (1..=5).map(|n: u8| (n.to_string(), profile_of_view(&s["profiles"][n.to_string()]))).collect();
    let factory = profile_of_view(&s["profiles"]["254"]);
    let macros: Map<String, Value> = s["macros"].as_object().map(|m| m.iter().map(|(i, v)| (i.clone(), macro_of(v))).collect()).unwrap_or_default();
    json!({
        "path": s["path"], "firmware": crate::firmware::describe(s["firmware"].as_u64().unwrap_or(0) as u16), "active": s["active"], "profiles": profiles,
        "factory": { "keys": factory["keys"], "lighting": factory["lighting"] },
        "macros": macros, "live": s["live"],
    })
}

/// Whether a saved state is in the keyboard code's own shape rather than the shared one.
pub fn is_own_state(s: &Value) -> bool {
    s["profiles"].as_object().is_some_and(|p| p.values().any(|v| v.get("fnkeys").is_some()))
}

/// The keys' zones, each by its bit in an effect's `zone`, in the order the
/// frontend lists them; the whole keyboard is the bit that lights every key.
const WHOLE: u64 = 0x80;
const ZONES: [(u64, &str); 6] = [(WHOLE, "Whole keyboard"), (0x02, "F-keys"), (0x04, "Number row"), (0x10, "WASD"), (0x08, "Arrows"), (0x01, "Numpad")];

/// The keys each zone lights, by their places (data/zones.json, as EVGA's
/// editor has them), but the whole keyboard's: every key.
fn zone_keys(bit: u64) -> Option<Value> {
    let zones: Value = serde_json::from_str(include_str!("../data/zones.json")).unwrap_or(Value::Null);
    let name = match bit {
        0x10 => "WASD",
        0x08 => "Arrows",
        0x04 => "Numbers",
        0x02 => "FKeys",
        0x01 => "Numpad",
        _ => return None,
    };
    Some(zones[name].clone())
}

/// The keys' effects, in the order the frontend shows them: each one's
/// settings, kept where `at` says in the effect's own settings (`/colors/*/`
/// every color's, `fields` a value of several, `also` the same value written
/// there too, where it's kept). Where each goes on the page is in
/// `lighting_layout`.
fn effects() -> Value {
    let color = json!({ "type": "color", "label": "Color", "at": "/color", "form": "rgb" });
    let colors = |n: usize| json!({ "type": "colors", "label": "Colors", "at": "/colors", "item": "/color", "count": n, "form": "rgb" });
    let random = json!({ "type": "bool", "label": "In a random order", "at": "/random", "form": "number" });
    let level = |at: &str| json!({ "type": "int", "label": "Brightness", "min": 0, "max": 255, "default": 255, "percent": true, "tint": true, "at": at, "also": if at == "/brightness" { json!([]) } else { json!(["/background/brightness"]) } });
    let speed = |min: u64, max: u64, at: &str| json!({ "type": "duration", "label": "Speed", "min": min, "max": max, "step": 1, "speed": true, "at": at });
    let option = |value: Value, label: &str, icon: &str| if icon.is_empty() { json!({ "value": value, "label": label }) } else { json!({ "value": value, "label": label, "icon": icon }) };
    let turning = json!({ "type": "choice", "label": "Direction", "at": "/direction", "default": 0, "options": [option(json!(0), "Clockwise", "clockwise"), option(json!(1), "Counterclockwise", "counterclockwise")] });
    // (kept 0 up, 1 down, 2 right, 3 left; shown up, down, left, right)
    let rolling = json!({ "type": "choice", "label": "Direction", "at": "/direction", "default": 0, "options": [option(json!(0), "Up", "up"), option(json!(1), "Down", "down"), option(json!(3), "Left", "left"), option(json!(2), "Right", "right")] });
    let lights_up = json!({ "type": "choice", "label": "Lights up", "menu": true, "at": "/submode", "default": 0, "options": [option(json!(0), "Each key you type", ""), option(json!(1), "One key", ""), option(json!(2), "The key and the keys around it", "")] });
    let zone = json!({ "type": "zones", "label": "Zones", "light": "keys", "at": "/zone", "default": WHOLE });
    // when it plays: the keyboard's start (0 with the profile, 1 on a key press)
    // and stop (0 never, 1 on a key press, 2 on key release) together, as EVGA's
    // LedStartMode and LedStopMode; a rule between those that start with the
    // profile and those that start on a key press
    let plays = json!({
        "type": "choice",
        "label": "Plays",
        "menu": true,
        "fields": ["/start", "/stop"],
        "default": [0, 0],
        "options": [
            option(json!([0, 0]), "All the time", ""),
            option(json!([0, 1]), "Until you press a key", ""),
            option(json!([0, 2]), "Until you let go of a key", ""),
            { "value": [1, 0], "label": "From your first key press", "rule": true },
            option(json!([1, 2]), "While you hold a key", ""),
            option(json!([1, 1]), "From one key press to the next", ""),
        ],
        "other": "Something else (start {0}, stop {1})",
    });
    let effect = |id: &str, label: &str, note: &str, params: Value| json!({ "id": id, "label": label, "note": note, "reset": true, "params": params });
    json!([
        { "id": "off", "label": "Off", "note": "The keys stay dark on {profile}. Choose an effect to light them." },
        effect("static", "Static", "One color on every key.", json!({ "color": color, "brightness": level("/brightness"), "zone": zone, "plays": plays })),
        effect(
            "breathing",
            "Breathing",
            "The keys fade in and out, a color each breath.",
            json!({ "colors": colors(2), "random": random, "brightness": level("/colors/*/brightness"), "speed": speed(1000, 5000, "/cycle"), "zone": zone, "plays": plays }),
        ),
        effect(
            "pulse",
            "Pulse",
            "The keys jump from one color to the next, in turn.",
            json!({ "colors": colors(7), "random": random, "brightness": level("/colors/*/brightness"), "speed": speed(6000, 10000, "/cycle"), "zone": zone, "plays": plays }),
        ),
        effect(
            "spiral",
            "Spiral",
            "Colors turn around the keyboard like a pinwheel.",
            json!({ "colors": colors(7), "brightness": level("/colors/*/brightness"), "speed": speed(1000, 5000, "/cycle"), "direction": turning, "zone": zone, "plays": plays }),
        ),
        effect(
            "wave",
            "Wave",
            "Colors roll across the keyboard.",
            json!({ "colors": colors(7), "random": random, "brightness": level("/colors/*/brightness"), "speed": speed(1000, 5000, "/cycle"), "direction": rolling, "zone": zone, "plays": plays }),
        ),
        effect(
            "stars",
            "Stars",
            "Keys twinkle on at random.",
            json!({ "brightness": level("/brightness"), "speed": speed(500, 4000, "/cycle"), "zone": zone, "plays": plays }),
        ),
        {
            "id": "trigger",
            "label": "Reactive",
            "note": "A key lights when you press it, then fades.",
            "reset": true,
            "reactive": true,
            "params": { "colors": colors(7), "brightness": level("/colors/*/brightness"), "speed": speed(50, 1000, "/shift"), "submode": lights_up, "zone": zone, "plays": plays },
        },
        {
            "id": "custom",
            "label": "Per key",
            "paint": true,
            "light": "keys",
            "note": "Select keys on the keyboard to paint them. Black turns a key off. The keyboard can’t store a color for each key, so they show only while Ectomatter is open.",
        },
    ])
}

/// Where the keys' effects' settings go on the Lighting page (the
/// description's `layout`, by effect): in sections, as the keyboard's page has
/// them, and where and when each plays in a box of its own beside them.
pub fn lighting_layout() -> Value {
    let place = json!({ "section": "Zones and timing", "box": "place", "show": ["zone", "plays"] });
    let color = |show: &[&str]| json!({ "section": "Color", "show": show });
    let motion = |show: &[&str]| json!({ "section": "Motion", "show": show });
    json!({
        "static": [color(&["color", "brightness"]), place],
        "breathing": [color(&["colors", "random", "brightness"]), motion(&["speed"]), place],
        "pulse": [color(&["colors", "random", "brightness"]), motion(&["speed"]), place],
        "spiral": [color(&["colors", "brightness"]), motion(&["speed", "direction"]), place],
        "wave": [color(&["colors", "random", "brightness"]), motion(&["speed", "direction"]), place],
        "stars": [color(&["brightness"]), motion(&["speed"]), place],
        "trigger": [color(&["colors", "brightness"]), motion(&["speed", "submode"]), place],
    })
}

/// What the keyboard can do, by the protocol's capabilities, for its
/// description (`device/describe`); its art, controls, print and light are in
/// its description file, device.json.
pub fn capabilities() -> Value {
    json!({
        "profiles": { "stored": "device", "count": 5, "factory": true },
        "keys": {
            "stored": "device",
            "layers": [{ "id": "base", "label": "When pressed" }, { "id": "fn", "label": "When Fn is held" }],
            "actions": ["key", "shortcut", "media", "mouse", "macro", "profile", "fn", "host", "own", "off", "none"],
            "own": OWN.iter().map(|(id, label)| json!({ "id": id, "label": label })).collect::<Vec<_>>(),
            "host": { "slots": HOST_SLOTS },
            "factory": true,
            // (the keys game mode can turn off, each by its bit in a profile's `game_keys`)
            "game_keys": [{ "bit": 0x40, "label": "Windows key" }, { "bit": 0x01, "label": "Alt+Tab" }, { "bit": 0x08, "label": "Alt+F4" }, { "bit": 0x04, "label": "Ctrl+Esc" }, { "bit": 0x02, "label": "Tab" }, { "bit": 0x10, "label": "Left Shift" }, { "bit": 0x20, "label": "Menu key" }],
        },
        "macros": {
            "stored": "device",
            "slots": crate::macros::MACRO_SLOTS,
            // (each step its event and the wait after it: a key's is 1 byte and 3, a media key's 3 and 3)
            "budget": { "bytes": crate::macros::MACRO_DATA_MAX, "cost": { "key": 4, "media": 6 } },
            "steps": ["key", "media", "wait"],
            "plays": ["once", "times", "while-held", "until-pressed-again"],
        },
        "lighting": {
            "stored": "device",
            "lights": [{ "id": "keys", "label": "Keys", "zones": ZONES.iter().map(|(bit, label)| { let mut z = json!({ "bit": bit, "label": label, "whole": *bit == WHOLE }); if let Some(k) = zone_keys(*bit) { z["controls"] = k; } z }).collect::<Vec<_>>() }, { "id": "screen", "label": "Display" }],
            // (where a profile keeps the keys' effect and each effect's own settings)
            "kept": { "keys": { "effect": "/lighting/keys/effect", "settings": "/lighting/keys/effects/{effect}" } },
            "effects": effects(),
            "preview": true,
            "direct": { "controls": NKEYS, "rate": 25 },
        },
        // (the screens it shows by itself when Ectomatter doesn't, each by its bit in a profile's `screen.apps`)
        "displays": [{ "control": "lcd", "kind": "text", "cols": crate::lcd::LCD_COLS, "rows": crate::lcd::LCD_ROWS, "draws": crate::lcd::DRAWS,
            "apps": [{ "bit": 0x20, "label": "Time" }, { "bit": 0x04, "label": "Profile number" }, { "bit": 0x08, "label": "Game mode" }, { "bit": 0x01, "label": "Key rollover" }, { "bit": 0x02, "label": "EVGA Precision X1 stats" }],
            // (a layout it keeps and draws by itself, from build 5: the pieces it can draw, and what
            // it shows for its mode and rollover until it can say them)
            "onboard": { "feature": "onboard.layout", "pieces": ["text", "clock", "date", "profile", "profile-number", "game-mode", "key-rollover"],
                "until": { "game-mode": "PC MODE", "key-rollover": "6 KEY" } },
            // (what it says of itself, from build 5)
            "status": { "feature": "status.read", "reads": ["game-mode", "key-rollover"], "from": "The keyboard, with Ectomatter’s firmware build 5 or later" } }],
        "settings": {
            "stored": "device",
            "items": [
                // (the backlight's level, held while the dimmer does something else: from build 2)
                { "id": "dimmer", "type": "int", "min": 0, "max": 255, "label": "Backlight", "held": true, "role": "backlight", "feature": "dimmer.set" },
                { "id": "modes", "type": "modes", "label": "What the sliders do by themselves" },
            ],
        },
        // the sliders: each one's job when the keyboard does it by itself (`own`), and the
        // firmware features that let it be read (`reads`) and freed of that job (`frees`)
        "input": {
            "watch": true,
            "sliders": [
                { "id": "dimmer", "label": "Dimmer", "called": "dimmer", "glyph": "◐", "own": "backlight", "reads": "sliders.read", "frees": "dimmer.keep", "range": [400, 3700],
                  "notes": {
                      "busy": "The backlight stays where it was, though it may flicker as this slider moves. Its firmware can hold it steady, from build 3.",
                      "freed": "The backlight stays where it was, and follows the volume slider if that is on keyboard brightness.",
                  } },
                { "id": "volume", "label": "Volume", "called": "volume slider", "glyph": "♪", "own": "output", "reads": "sliders.read", "frees": "volume.quiet", "range": [400, 3700],
                  "notes": { "busy": "The keyboard still sends volume keys from this slider as well. Its firmware can be told not to, from build 3." } },
            ],
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind of key the keyboard stores comes back byte for byte through
    /// its shared action, and every code Ectomatter has no word for comes back
    /// as itself.
    #[test]
    fn every_key_code_comes_back_through_its_action() {
        let mut seen = 0;
        for f in 0..=255u8 {
            for a in [0u8, 1, 2, 3, 0x11, 0x80, 0xFF] {
                for b in [0u8, 4, 0x28, 0x68, 0xA5, 0xAF, 0xB0, 0xE0, 0xFF] {
                    for c in [0u8, 1, 2, 5, 0x1D, 0xA6, 0xFF] {
                        let d = [f, a, b, c];
                        let act = action_of(d);
                        assert_eq!(define_of(&act), Some(d), "{d:?} as {act:?}");
                        assert_eq!(Action::read(&act.write()), Some(act.clone()), "{d:?} written as {}", act.write());
                        seen += 1;
                    }
                }
            }
        }
        assert!(seen > 100_000);
        // the shapes Ectomatter shows by name, and none of them raw
        let named = [
            ([0, 0, 0, 0], json!({ "none": true })),
            ([255, 0, 0, 0], json!({ "off": true })),
            ([0, 0, 0x68, 0], json!({ "key": "F13" })),
            ([0, 1, 0, 0], json!({ "key": "LeftCtrl" })),
            ([0, 1, 6, 0], json!({ "shortcut": ["LeftCtrl", "C"] })),
            ([0, 0, 0xA5, 0], json!({ "host": 0 })),
            ([2, 0xE2, 0, 0], json!({ "media": "Mute" })),
            ([3, 4, 11, 3], json!({ "macro": 4, "plays": "times", "times": 3 })),
            ([3, 4, 8, 0], json!({ "macro": 4, "plays": "until-pressed-again" })),
            ([4, 0, 0, 0], json!({ "fn": true })),
            ([5, 0, 0, 0], json!({ "own": "game-mode" })),
            ([6, 1, 0, 0], json!({ "scroll": "down" })),
            ([9, 0, 0, 0], json!({ "mouse": 3 })),
            ([18, 0, 0, 0], json!({ "profile": 2 })),
            ([23, 0, 0, 0], json!({ "profile": "previous" })),
        ];
        for (d, want) in named {
            assert_eq!(action_of(d).write(), want, "{d:?}");
        }
        // what the keyboard couldn't store is refused, not stored as something else
        assert_eq!(define_of(&Action::Mouse(4)), None);
        assert_eq!(define_of(&Action::Profile(Jump::To(6))), None);
        assert_eq!(define_of(&Action::Shortcut(vec![4, 5, 6])), None);
    }

    /// Lighting, whole or in part, comes back exactly through the shared shape.
    #[test]
    fn lighting_comes_back_through_the_shared_shape() {
        let whole = json!({
            "mode": "wave", "sub": 255,
            "static": { "zone": 1, "color": [1, 2, 3] }, "breathing": { "cycle": 3000 }, "pulse": {}, "spiral": {},
            "wave": { "direction": 2 }, "trigger": {}, "stars": { "brightness": 9 },
            "lcd": { "color": [255, 0, 0, 255], "apps": 3 }, "game_keys": 7,
        });
        let shared = lighting_of(&whole);
        assert_eq!(shared["keys"]["effect"], "wave");
        assert_eq!(shared["keys"]["effects"]["stars"]["brightness"], 9);
        assert_eq!(shared["screen"]["apps"], 3);
        assert_eq!(legacy_lighting(&shared).unwrap(), whole);
        for part in [json!({ "mode": "static" }), json!({ "spiral": { "count": 3 } }), json!({ "lcd": { "apps": 1 } }), json!({})] {
            assert_eq!(legacy_lighting(&lighting_of(&part)).unwrap(), part, "{part}");
        }
    }

    /// A macro's steps come back exactly through the shared shape, the
    /// modifiers by their shared names.
    #[test]
    fn macro_steps_come_back_through_the_shared_shape() {
        let steps = vec![
            json!({ "wait": 40, "delay": 0 }),
            json!({ "code": 112, "down": true, "delay": 10 }),
            json!({ "code": 6, "down": true, "delay": 20 }),
            json!({ "code": 6, "down": false, "delay": 0 }),
            json!({ "code": 112, "down": false, "delay": 5 }),
            json!({ "media": 0xE2, "down": true, "delay": 0 }),
            json!({ "media": 0xE2, "down": false, "delay": 0 }),
            json!({ "code": 120, "down": true, "delay": 0 }),
            json!({ "code": 176, "down": true, "delay": 0 }),
            json!({ "code": 224, "down": false, "delay": 0 }),
        ];
        let shared = steps_of(&steps);
        assert_eq!(shared[1], json!({ "key": "LeftCtrl", "down": true, "after": 10 }));
        assert_eq!(shared[2]["key"], "C");
        assert_eq!(shared[5]["media"], "Mute");
        assert_eq!(shared[7]["key"], 120);
        assert_eq!(shared[8]["key"], 176, "a number the keyboard trims to seven bits stays a number");
        assert_eq!(shared[9]["key"], 224);
        assert_eq!(legacy_steps(&shared).unwrap(), steps);
        assert!(legacy_steps(&[json!({ "key": "F21", "down": true })]).is_err(), "F21's code is Left Ctrl's in a macro");
    }
}
