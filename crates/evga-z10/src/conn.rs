//! One shared connection to the keyboard. Everything that talks to it (the
//! editor's reads and saves, the live preview, the display) takes turns
//! through this, and a connection that fails is dropped and reopened on the
//! next use, so unplugging and replugging the keyboard recovers by itself.

use std::sync::{Arc, Mutex};

use crate::device::{Transport, Z10};
use crate::error::Result;

pub type Device = Z10<Box<dyn Transport>>;
pub type Opener = Arc<dyn Fn() -> Result<Device> + Send + Sync>;

impl Transport for Box<dyn Transport> {
    fn set_feature(&mut self, buf: &[u8]) -> Result<()> {
        (**self).set_feature(buf)
    }
    fn get_feature(&mut self, rid: u8, len: usize) -> Result<Vec<u8>> {
        (**self).get_feature(rid, len)
    }
    fn path(&self) -> String {
        (**self).path()
    }
    fn pause(&mut self, d: std::time::Duration) {
        (**self).pause(d)
    }
}

#[derive(Clone)]
pub struct Conn {
    inner: Arc<Mutex<Option<Device>>>,
    opener: Opener,
}

impl Conn {
    pub fn new(opener: Opener) -> Conn {
        Conn {
            inner: Arc::new(Mutex::new(None)),
            opener,
        }
    }

    /// The real keyboard, over HID.
    #[cfg(feature = "hid")]
    pub fn hid() -> Conn {
        Conn::new(Arc::new(|| {
            let d = Z10::open()?;
            Ok(Z10::new(Box::new(d.t) as Box<dyn Transport>))
        }))
    }

    /// Let go of the keyboard, closing it. The next call opens it afresh.
    pub fn close(&self) {
        *self.inner.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// Run `f` with the keyboard, opening it if need be. If `f` fails, the
    /// connection is dropped so the next call opens it afresh.
    pub fn with<R>(&self, f: impl FnOnce(&mut Device) -> Result<R>) -> Result<R> {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            *g = Some((self.opener)()?);
        }
        let r = f(g.as_mut().unwrap());
        if r.is_err() {
            *g = None;
        }
        r
    }
}
