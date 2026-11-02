//! The Z10's wire format: report ids and sizes, command codes and the checksum.
//!
//! Every exchange is a HID feature report on the keyboard's LED-indicator
//! collection (usage page 0x08, usage 0x4B), which carries reports 3 to 10.
//! Each report starts `<id> ea 02`, and its size is fixed by the keyboard's
//! report descriptor.
//!
//! The firmware updater (report 5, EnterIapMode, EraseApFirmware) is
//! deliberately absent from this crate.

pub const VID: u16 = 0x3842;
pub const PID: u16 = 0x2611;

/// The collection that carries the keyboard's own reports.
pub const USAGE_PAGE: u16 = 0x0008;
pub const USAGE: u16 = 0x004B;
/// Its USB interface, for systems that report interfaces rather than collections.
pub const INTERFACE: i32 = 1;

/// The two bytes after the report id in every report.
pub const HDR: [u8; 2] = [0xEA, 0x02];

pub const RID_GENERAL: u8 = 4;
/// Ectomatter's own firmware adds these two to report 4 (see the firmware gate).
pub const READ_SLIDERS: u8 = 0x40;
pub const SET_DIMMER: u8 = 0x41;
pub const SET_MODES: u8 = 0x42;
/// what the keyboard can be asked to stop doing by itself, from build 3
pub const QUIET_VOLUME: u8 = 1;
pub const KEEP_DIMMER: u8 = 2;
pub const RID_KEYS: u8 = 6;
pub const RID_PREVIEW: u8 = 7;
pub const RID_PROFILE: u8 = 8;
pub const RID_MACRO: u8 = 9;
pub const RID_LCD: u8 = 10;

/// The size of a report, including its id byte.
pub fn size(rid: u8) -> usize {
    match rid {
        RID_GENERAL => 17,
        RID_KEYS => 472,
        RID_PREVIEW => 40,
        RID_PROFILE => 1501,
        RID_MACRO => 1032,
        RID_LCD => 88,
        _ => panic!("report {rid} is not one this crate sends"),
    }
}

// General command (report 4) main codes.
pub const READ_FW: u8 = 1;
pub const PROFILE_NUM: u8 = 6;
pub const RELOAD_PROFILE: u8 = 8;
/// The screen's light (brightness, red, green, blue) and the built-in screens
/// it shows (a bitmask), in the keyboard's working memory, not its profiles:
/// as EVGA's Unleash RGB sets them while its window is open.
pub const LCD_LIGHT_RAM: u8 = 13;
pub const LCD_APPS_RAM: u8 = 14;
/// The clock on the keyboard's own status screen (EVGA's DateTime): a date and time
/// for it to show, or, with CLEAR_TIME, none. The keyboard keeps no time itself: it
/// shows the last time sent until another comes.
pub const LCD_TIME: u8 = 10;
pub const CLEAR_TIME: u8 = 0x10;

// Status bytes the keyboard answers with.
pub const SUCCESS: u8 = 0xC0;
pub const FAIL: u8 = 0xC1;
pub const BUSY: u8 = 0xC2;

pub const NKEYS: usize = 116;

/// Lighting modes in the order the keyboard numbers them.
pub const MODES: [&str; 8] = [
    "off",
    "static",
    "breathing",
    "pulse",
    "spiral",
    "wave",
    "stars",
    "trigger",
];

pub fn mode_index(mode: &str) -> Option<u8> {
    MODES.iter().position(|m| *m == mode).map(|i| i as u8)
}

/// The keyboard's checksum: the byte that makes the sum come to zero.
pub fn csum(data: &[u8]) -> u8 {
    data.iter()
        .fold(0u8, |a, b| a.wrapping_add(*b))
        .wrapping_neg()
}
