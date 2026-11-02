//! The EVGA Z10 RGB's library: everything that talks to the keyboard.
//!
//! The protocol was worked out from EVGA's Unleash RGB 1.0.11.0 and from
//! AIDA64 (for the display). Ectomatter is not made or supported by EVGA.

pub mod conn;
pub mod device;
pub mod error;
pub mod firmware;
pub mod json;
pub mod keyboard;
pub mod lcd;
pub mod live;
pub mod macros;
pub mod preview;
pub mod profile;
pub mod protocol;
pub mod reports;
pub mod shared;

pub use conn::Conn;
#[cfg(feature = "hid")]
pub use device::HidTransport;
pub use device::{Sliders, Transport, Z10};
pub use error::{Error, Result};
pub use keyboard::Keyboard;
pub use profile::Profile;
