//! Parity with the original Python editor: every case in the parity vectors
//! (made by parity/gen.py from that editor's code) must come out of this
//! crate identically, byte for byte and field for field.
//!
//! The committed vectors are synthetic. Vectors built from a real keyboard's
//! backups, in parity/private/, are checked too when they are present.

use std::collections::VecDeque;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use evga_z10::conn::{Conn, Device};
use evga_z10::lcd::lcd_frame;
use evga_z10::macros::{macro_body, macro_data, macro_meta, macro_record_bytes, macro_steps};
use evga_z10::preview::preview_report;
use evga_z10::{Keyboard, Profile, Result, Transport, Z10};
use serde_json::{Value, json};

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vector_files() -> Vec<(String, Value)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity");
    let mut files = vec![root.join("vectors.json.gz")];
    if let Ok(dir) = fs::read_dir(root.join("private")) {
        let mut private: Vec<_> = dir
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with(".json.gz"))
            .collect();
        private.sort();
        files.extend(private);
    }
    files
        .into_iter()
        .map(|f| {
            let mut s = String::new();
            flate2::read::GzDecoder::new(
                fs::File::open(&f).expect("parity vectors; run parity/gen.py"),
            )
            .read_to_string(&mut s)
            .unwrap();
            (
                f.file_name().unwrap().to_string_lossy().into_owned(),
                serde_json::from_str(&s).unwrap(),
            )
        })
        .collect()
}

#[test]
fn profiles_decode_edit_and_encode() {
    let mut n = 0;
    for (file, v) in vector_files() {
        for case in v["profiles"].as_array().unwrap() {
            let raw = unhex(case["raw"].as_str().unwrap());
            let p = Profile::decode(&raw).unwrap();
            assert_eq!(p.name(), case["name"], "{file}: name");
            assert_eq!(p.lighting(), case["lighting"], "{file}: lighting view");
            assert_eq!(
                serde_json::to_string(&p.lighting()).unwrap(),
                serde_json::to_string(&case["lighting"]).unwrap(),
                "{file}: lighting key order"
            );
            assert_eq!(json!(p.keys(false)), case["keys"], "{file}: keys");
            assert_eq!(json!(p.keys(true)), case["fnkeys"], "{file}: Fn keys");
            assert_eq!(p.checksum(), case["checksum"].as_u64().unwrap() as u8);
            for (slot, hex) in case["encode"].as_object().unwrap() {
                assert_eq!(
                    p.encode(slot.parse().unwrap()),
                    unhex(hex.as_str().unwrap()),
                    "{file}: encode into {slot}"
                );
            }
            for (i, e) in case["edits"].as_array().unwrap().iter().enumerate() {
                let mut q = p.clone();
                let r = q.set_lighting(&e["patch"]);
                if e["error"] == true {
                    assert!(
                        r.is_err(),
                        "{file}: edit {i} should be refused: {}",
                        e["patch"]
                    );
                    continue;
                }
                r.unwrap_or_else(|err| panic!("{file}: edit {i} failed: {err}: {}", e["patch"]));
                assert_eq!(
                    q.raw,
                    unhex(e["raw"].as_str().unwrap()),
                    "{file}: edit {i} bytes: {}",
                    e["patch"]
                );
                if !e["lighting"].is_null() {
                    assert_eq!(q.lighting(), e["lighting"], "{file}: edit {i} view");
                }
                n += 1;
            }
            for c in case["names"].as_array().unwrap() {
                let mut q = p.clone();
                q.set_name(c["name"].as_str().unwrap());
                assert_eq!(
                    q.raw,
                    unhex(c["raw"].as_str().unwrap()),
                    "{file}: name {}",
                    c["name"]
                );
                assert_eq!(q.name(), c["read"], "{file}: name read back");
            }
            for c in case["set_keys"].as_array().unwrap() {
                let mut q = p.clone();
                let d: Vec<u8> = c["define"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u8)
                    .collect();
                q.set_key(
                    c["pos"].as_u64().unwrap() as usize,
                    d.try_into().unwrap(),
                    c["secondary"] == true,
                )
                .unwrap();
                assert_eq!(q.raw, unhex(c["raw"].as_str().unwrap()), "{file}: set key");
            }
        }
    }
    assert!(n > 100, "only {n} edits were checked");
}

#[test]
fn preview_frames() {
    for (file, v) in vector_files() {
        for (i, c) in v["preview"].as_array().unwrap().iter().enumerate() {
            let got = preview_report(
                c["mode"].as_str().unwrap(),
                &c["e"],
                c["t"].as_u64().unwrap(),
                c["restart"] == true,
            )
            .unwrap_or_else(|e| panic!("{file}: frame {i}: {e}"));
            assert_eq!(
                got,
                unhex(c["frame"].as_str().unwrap()),
                "{file}: frame {i}: {} {}",
                c["mode"],
                c["e"]
            );
        }
    }
}

#[test]
fn macros_both_ways() {
    for (file, v) in vector_files() {
        let m = &v["macros"];
        for (i, r) in m["reports"].as_array().unwrap().iter().enumerate() {
            let raw = unhex(r["raw"].as_str().unwrap());
            let steps = macro_steps(&raw);
            assert_eq!(
                Value::Array(steps.clone()),
                r["steps"],
                "{file}: report {i} steps"
            );
            assert_eq!(macro_meta(&raw), r["meta"], "{file}: report {i} settings");
            let editable = macro_data(&steps).unwrap() == macro_record_bytes(&raw);
            assert_eq!(
                json!(editable),
                r["editable"],
                "{file}: report {i} editable"
            );
        }
        for (i, e) in m["encode"].as_array().unwrap().iter().enumerate() {
            let got = macro_data(e["steps"].as_array().unwrap()).unwrap();
            assert_eq!(
                got,
                unhex(e["data"].as_str().unwrap()),
                "{file}: encode {i}"
            );
        }
        for (i, b) in m["bodies"].as_array().unwrap().iter().enumerate() {
            let u = |k: &str| b[k].as_u64().unwrap() as u8;
            let got = macro_body(
                b["name"].as_str().unwrap(),
                b["steps"].as_array().unwrap(),
                u("run"),
                u("repeat"),
                u("unit"),
            )
            .unwrap();
            assert_eq!(
                got,
                unhex(b["body"].as_str().unwrap()),
                "{file}: body {i} ({})",
                b["name"]
            );
        }
    }
}

#[test]
fn display_frames() {
    for (file, v) in vector_files() {
        for c in v["lcd"].as_array().unwrap() {
            let rows: Vec<&str> = c["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap())
                .collect();
            // The Python editor sent any Windows-1252 code, and the keyboard
            // draws noise or a lit cell for most above 0x7E. Ectomatter spells
            // those in characters the keyboard draws instead (lcd::lcd_text),
            // so only text the keyboard draws as itself is held to parity.
            if !rows.iter().all(|r| r.chars().all(evga_z10::lcd::lcd_draws)) {
                continue;
            }
            assert_eq!(
                lcd_frame(&rows),
                unhex(c["frame"].as_str().unwrap()),
                "{file}: {rows:?}"
            );
        }
    }
}

/// A recorded exchange: each report sent ("set") or answer read ("get").
type Log = Arc<Mutex<VecDeque<(String, Vec<u8>)>>>;

/// A keyboard that replays a recorded exchange: every report sent must be
/// the next one in the log, and every read returns the next recorded answer.
struct Replay {
    log: Log,
    name: String,
}

impl Transport for Replay {
    fn set_feature(&mut self, buf: &[u8]) -> Result<()> {
        let (kind, want) = self
            .log
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("{}: sent more than the original", self.name));
        assert_eq!(kind, "set", "{}: sent where the original read", self.name);
        assert_eq!(buf, &want[..], "{}: a sent report differs", self.name);
        Ok(())
    }
    fn get_feature(&mut self, rid: u8, len: usize) -> Result<Vec<u8>> {
        let (kind, answer) = self
            .log
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("{}: read more than the original", self.name));
        assert_eq!(kind, "get", "{}: read where the original sent", self.name);
        assert_eq!(
            (answer[0], answer.len()),
            (rid, len),
            "{}: read a different report",
            self.name
        );
        Ok(answer)
    }
    fn path(&self) -> String {
        "fake".into()
    }
    fn pause(&mut self, _: std::time::Duration) {}
}

fn replay(case: &Value, file: &str) -> (Log, String) {
    let log: VecDeque<_> = case["log"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e[0].as_str().unwrap().to_string(),
                unhex(e[1].as_str().unwrap()),
            )
        })
        .collect();
    (
        Arc::new(Mutex::new(log)),
        format!("{file}: {}", case["name"].as_str().unwrap()),
    )
}

/// The original editor's exchange less the reads it repeated: it read each
/// stored macro twice in a row (once for its settings, again for its steps),
/// and this code reads each once. Every report written stays.
fn reads_once(log: &Log) {
    let mut l = log.lock().unwrap();
    let mut out = VecDeque::new();
    let mut seen = std::collections::HashSet::new();
    while let Some((kind, b)) = l.pop_front() {
        if kind == "set" && b[0] == 9 {
            if b[3] != 1 || b[4] == 0 {
                seen.clear();
            } else if !seen.insert(b[4]) {
                l.pop_front(); // and its answer
                continue;
            }
        }
        out.push_back((kind, b));
    }
    *l = out;
}

fn device(log: &Log, name: &str) -> Device {
    Z10::new(Box::new(Replay {
        log: log.clone(),
        name: name.into(),
    }) as Box<dyn Transport>)
}

#[test]
fn every_command_sends_what_the_original_sent() {
    for (file, v) in vector_files() {
        for c in v["device"]["commands"].as_array().unwrap() {
            let (log, name) = replay(c, &file);
            let mut k = device(&log, &name);
            let a = &c["args"];
            let cmd = c["name"].as_str().unwrap();
            let arg = |i: usize| cmd.split(' ').nth(i).unwrap().parse::<u8>().unwrap();
            let got: Result<Value> = match cmd.split(' ').next().unwrap() {
                "firmware_version" => k.firmware_version().map(|(s, f)| json!([s, f])),
                "profile_number" => k.profile_number().map(|n| json!(n)),
                "select_profile" => k.select_profile(arg(1)).map(|b| json!(b)),
                "reload_profile" => k.reload_profile().map(|b| json!(b)),
                "read_profile" => k
                    .read_profile(arg(1))
                    .map(|p| json!(p.raw.iter().map(|b| format!("{b:02x}")).collect::<String>())),
                "write_profile" => {
                    let p = Profile::decode(&unhex(a["raw"].as_str().unwrap())).unwrap();
                    k.write_profile(arg(1), &p).map(|b| json!(b))
                }
                "read_macros" => k.read_macros().map(|m| {
                    Value::Object(m.into_iter().map(|(i, v)| (i.to_string(), v)).collect())
                }),
                "write_macro" => k
                    .write_macro(arg(1), &unhex(a["body"].as_str().unwrap()))
                    .map(|b| json!(b)),
                "delete_macro" => k.delete_macro(arg(1)).map(|b| json!(b)),
                "stream_colors" => {
                    let colors: Vec<[u8; 3]> = a["colors"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| {
                            let c = c.as_array().unwrap();
                            [
                                c[0].as_u64().unwrap() as u8,
                                c[1].as_u64().unwrap() as u8,
                                c[2].as_u64().unwrap() as u8,
                            ]
                        })
                        .collect();
                    k.stream_colors(&colors, a["brightness"].as_u64().unwrap() as u8)
                        .map(|()| Value::Null)
                }
                "stop_stream" => k.stop_stream().map(|()| Value::Null),
                "exit_sync" => k.exit_sync().map(|()| Value::Null),
                other => panic!("no replay for {other}"),
            };
            if c["error"] == true {
                assert!(got.is_err(), "{name}: should fail");
            } else {
                assert_eq!(got.unwrap(), c["result"], "{name}: result");
            }
            assert!(
                log.lock().unwrap().is_empty(),
                "{name}: stopped short of the original"
            );
        }
    }
}

#[test]
fn every_editor_action_matches_the_original() {
    for (file, v) in vector_files() {
        for c in v["device"]["editor"].as_array().unwrap() {
            let (log, name) = replay(c, &file);
            reads_once(&log);
            let (l, nm) = (log.clone(), name.clone());
            let kb = Keyboard::new(Conn::new(Arc::new(move || Ok(device(&l, &nm)))), None);
            let got = match c["name"].as_str().unwrap().split(' ').next().unwrap() {
                "state" => kb.state(),
                "save" => kb.save(c["n"].as_u64().unwrap() as u8, &c["body"]),
                "write_macro" => kb.write_macro(c["index"].as_u64().unwrap() as u8, &c["body"]),
                "activate" => kb
                    .activate(c["n"].as_u64().unwrap() as u8)
                    .map(|n| json!(n)),
                other => panic!("no replay for {other}"),
            };
            if c["error"] == true {
                assert!(got.is_err(), "{name}: should fail");
            } else {
                let got = got.unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(got, c["result"], "{name}: result");
            }
            assert!(
                log.lock().unwrap().is_empty(),
                "{name}: stopped short of the original"
            );
        }
    }
}

// (recording is Ectomatter's own now, its parity checked there: apps/ectomatter/src/recording.rs)
