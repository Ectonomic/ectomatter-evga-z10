//! Keeps the keyboard showing a preview while the editor asks for one.
//!
//! The keyboard does not animate a preview by itself: the host sends a frame
//! every 40 ms, either an effect (report 7) or an explicit color for every
//! key (report 6). This runs that loop on its own thread, or, where there are
//! no threads (as a core module in Ectomatter), one beat at a time for whoever
//! keeps the time (`tick`). Nothing is stored, so stopping hands the lighting
//! back to the saved profile untouched.
//!
//! A change goes to the keyboard at once rather than at the next beat, and
//! an effect can follow the editor's own clock, so the keys show the moment
//! of the effect the editor's screen shows.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::conn::Conn;
use crate::preview::{PREVIEW_PERIOD, preview_report};

/// Seconds without a refresh before the preview gives up by itself.
pub const HOLD: Duration = Duration::from_secs(6);

/// The least time between two frames when a change comes sooner than the
/// next beat. (Untested how much faster than a beat the keyboard can take
/// frames; Unleash only ever sent one a beat.)
pub const SOONEST: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, PartialEq)]
enum Showing {
    Preset(Value),
    Keys(Vec<[u8; 3]>),
}

#[derive(Default)]
struct State {
    want: Option<Showing>,
    deadline: Option<Instant>,
    restart: bool,
    error: Option<String>,
    /// the editor window the preview belongs to
    owner: Option<String>,
    /// the editor's clock for the effect: its time (ms), and when it said so
    clock: Option<(u64, Instant)>,
    /// what is asked for has changed since the last frame
    fresh: bool,
}

/// Where the loop is: how far into the effect (ms), and what the keyboard is
/// set up for now (None, or whether it is keys).
#[derive(Default)]
struct Beat {
    t: u64,
    showing: Option<bool>,
    /// when the last frame went out
    last: Option<Instant>,
}

/// What the loop wants after a beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Next {
    /// a frame went out; the next is due after this long
    After(Duration),
    /// no frame was due yet: the next is due after this long
    Wait(Duration),
    /// the frame failed and the preview stopped; go round again at once
    Now,
    /// nothing showing: nothing to do until a preview is asked for
    Idle,
}

#[derive(Clone)]
pub struct Live {
    st: Arc<Mutex<State>>,
    wake: Arc<(Mutex<bool>, std::sync::Condvar)>,
    beat: Arc<Mutex<Beat>>,
}

impl Live {
    pub fn start(conn: Conn) -> Live {
        let live = Live::driven();
        let l = live.clone();
        thread::Builder::new()
            .name("preview".into())
            .spawn(move || l.run(conn))
            .expect("preview thread");
        live
    }

    /// The loop with no thread of its own: whoever keeps the time calls
    /// `tick` when `wants_tick` says so, and again as each `tick` asks.
    pub fn driven() -> Live {
        Live {
            st: Arc::default(),
            wake: Arc::default(),
            beat: Arc::default(),
        }
    }

    /// Whether a beat is due: a preview is asked for, or one still shows.
    pub fn wants_tick(&self) -> bool {
        self.lock().want.is_some() || self.beat.lock().unwrap_or_else(|p| p.into_inner()).showing.is_some()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn arm(&self, s: &mut State) {
        s.deadline = Some(Instant::now() + HOLD);
        s.error = None;
        let (m, cv) = &*self.wake;
        *m.lock().unwrap() = true;
        cv.notify_one();
    }

    pub fn preset(&self, lighting: Value) {
        self.preset_at(lighting, None);
    }

    /// An effect, at time `at` (ms) on the editor's clock, if it keeps one;
    /// otherwise the effect's time starts at 0 and goes up a beat a frame.
    pub fn preset_at(&self, lighting: Value, at: Option<u64>) {
        let mut s = self.lock();
        let want = Some(Showing::Preset(lighting));
        s.fresh |= s.want != want;
        s.restart = !matches!(s.want, Some(Showing::Preset(_)));
        s.want = want;
        s.clock = at.map(|t| (t, Instant::now()));
        self.arm(&mut s);
    }

    pub fn keys(&self, colors: Vec<[u8; 3]>) {
        let mut s = self.lock();
        let want = Some(Showing::Keys(colors));
        s.fresh |= s.want != want;
        s.want = want;
        self.arm(&mut s);
    }

    pub fn stop(&self) {
        let mut s = self.lock();
        s.fresh |= s.want.is_some();
        s.want = None;
        self.arm(&mut s);
    }

    /// Whether what is asked for has changed since the last frame.
    pub fn fresh(&self) -> bool {
        self.lock().fresh
    }

    /// Whether a preview is showing on the keyboard now.
    pub fn showing(&self) -> bool {
        self.lock().want.is_some()
    }

    pub fn owner(&self) -> Option<String> {
        self.lock().owner.clone()
    }

    pub fn set_owner(&self, o: Option<String>) {
        self.lock().owner = o;
    }

    /// `{mode, error}` as the editor reads it: mode is null, "preset" or "keys".
    pub fn status(&self) -> Value {
        let s = self.lock();
        let mode = match s.want {
            None => Value::Null,
            Some(Showing::Preset(_)) => json!("preset"),
            Some(Showing::Keys(_)) => json!("keys"),
        };
        json!({ "mode": mode, "error": s.error })
    }

    fn run(&self, conn: Conn) {
        loop {
            match self.tick(&conn) {
                Next::After(d) | Next::Wait(d) => thread::sleep(d),
                Next::Now => continue,
                Next::Idle => {
                    let (m, cv) = &*self.wake;
                    let g = m.lock().unwrap();
                    let (mut g, _) = cv.wait_timeout(g, Duration::from_secs(1)).unwrap();
                    *g = false;
                }
            }
        }
    }

    /// One beat: send the frame due now, or hand the lighting back once
    /// nothing is asked for. A frame is due a beat after the last, or as soon
    /// as what is asked for changes (but not sooner than `SOONEST`).
    pub fn tick(&self, conn: &Conn) -> Next {
        let mut b = self.beat.lock().unwrap_or_else(|p| p.into_inner());
        let (want, restart, clock) = {
            let mut s = self.lock();
            if s.want.is_some() && s.deadline.is_some_and(|d| Instant::now() > d) {
                s.want = None;
            }
            let gap = if s.fresh { SOONEST } else { Duration::from_millis(PREVIEW_PERIOD) };
            // (a millisecond's grace, for a timer that wakes a hair early)
            let since = b.last.map(|l| l.elapsed() + Duration::from_millis(1));
            if s.want.is_some() && b.showing.is_some() && since.is_some_and(|e| e < gap) {
                return Next::Wait(gap - since.unwrap_or_default());
            }
            s.fresh = false;
            (s.want.clone(), s.restart, s.clock)
        };
        let Some(want) = want else {
            if b.showing.take().is_some() {
                b.last = None;
                self.release(conn);
            }
            return Next::Idle;
        };
        let keys = matches!(want, Showing::Keys(_));
        let t = clock.map_or(b.t, |(at, when)| at + when.elapsed().as_millis() as u64);
        let showing = b.showing;
        let r = conn.with(|k| {
            let mut restart = restart;
            if showing != Some(keys) {
                // The keyboard ignores effect frames while it is in per-key
                // color mode, and the other way round, so leave both first.
                k.stop_stream()?;
                k.exit_sync()?;
                restart = true;
            }
            match &want {
                Showing::Preset(l) => {
                    let mode = l.get("mode").and_then(Value::as_str).unwrap_or("off");
                    let e = l.get(mode).cloned().unwrap_or(Value::Null);
                    k.set(&preview_report(mode, &e, t, restart)?)?;
                }
                Showing::Keys(c) => k.stream_colors(c, 255)?,
            }
            Ok(())
        });
        match r {
            Ok(()) => {
                b.showing = Some(keys);
                b.last = Some(Instant::now());
                self.lock().restart = false;
            }
            Err(e) => {
                let mut s = self.lock();
                s.error = Some(e.to_string());
                s.want = None;
                b.showing = None;
                b.last = None;
                return Next::Now;
            }
        }
        b.t += PREVIEW_PERIOD;
        Next::After(Duration::from_millis(PREVIEW_PERIOD))
    }

    /// Hand the lighting back to the stored profile.
    fn release(&self, conn: &Conn) {
        let r = conn.with(|k| {
            k.stop_stream()?;
            k.exit_sync()?;
            k.reload_profile()?;
            Ok(())
        });
        if let Err(e) = r {
            self.lock().error = Some(e.to_string());
        }
    }
}
