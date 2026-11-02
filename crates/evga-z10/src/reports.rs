//! The Z10's input reports, as it sends them to the computer on its three
//! interfaces, and what each says is down. Read beside the computer, which
//! still gets every report as before; nothing is ever sent to the keyboard.
//!
//! - Interface 0, the boot keyboard: the modifiers, a byte kept empty, then
//!   six keys (no report number).
//! - Interface 2, report 0x10: the modifiers, a byte kept empty, then ten
//!   keys (after the report's number).
//! - Interface 1, report 2: the media key down, a consumer usage (two bytes,
//!   low first), or 0 for none. (Its report 1 is the keyboard's mouse, and
//!   its feature reports are the editor's: neither is read here.)
//!
//! Each report says everything down on its own interface; what is down on
//! the keyboard is all of them together.

/// What one report says is down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Down {
    /// the keyboard usages down, the modifiers among them as their own
    /// usages (0xE0 Left Ctrl to 0xE7 Right GUI)
    Keys(Vec<u8>),
    /// the media key down (a consumer usage), or none (0)
    Media(u16),
}

/// What a report from the keyboard's interface `iface` says is down; none for
/// a report that says nothing of keys (the keyboard's mouse, a short one).
pub fn parse(iface: i32, r: &[u8]) -> Option<Down> {
    match iface {
        0 if r.len() >= 8 => Some(Down::Keys(keys(r[0], &r[2..8]))),
        2 if r.len() >= 13 && r[0] == 0x10 => Some(Down::Keys(keys(r[1], &r[3..13]))),
        1 if r.len() >= 3 && r[0] == 0x02 => Some(Down::Media(u16::from_le_bytes([r[1], r[2]]))),
        _ => None,
    }
}

/// The modifiers' bits and the key bytes as usages: no key (0) and the
/// keyboard's "too many keys at once" codes (1 to 3) left out.
fn keys(mods: u8, ks: &[u8]) -> Vec<u8> {
    (0..8u8).filter(|b| mods & (1 << b) != 0).map(|b| 0xE0 + b).chain(ks.iter().copied().filter(|k| *k > 3)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each interface's report read as the keyboard sends it: the boot
    /// report with Left Shift and A, report 0x10 with a host slot's code
    /// among letters, a media key and its letting go; the keyboard's mouse,
    /// a short report and the rollover codes say nothing of keys.
    #[test]
    fn each_report_says_what_is_down() {
        assert_eq!(parse(0, &[0x02, 0, 0x04, 0, 0, 0, 0, 0]), Some(Down::Keys(vec![0xE1, 0x04])));
        let mut r = vec![0x10, 0x01, 0, 0x04, 0x05, 0xA5];
        r.extend([0; 7]);
        assert_eq!(parse(2, &r), Some(Down::Keys(vec![0xE0, 0x04, 0x05, 0xA5])));
        assert_eq!(parse(1, &[0x02, 0xE9, 0x00]), Some(Down::Media(0xE9)));
        assert_eq!(parse(1, &[0x02, 0x00, 0x00]), Some(Down::Media(0)));
        assert_eq!(parse(1, &[0x01, 0, 5, 5, 0]), None, "the keyboard's mouse");
        assert_eq!(parse(2, &[0x11, 0, 0, 0, 0, 0, 0]), None, "report 0x11");
        assert_eq!(parse(0, &[0, 0, 0x04]), None, "too short");
        assert_eq!(parse(0, &[0, 0, 1, 1, 1, 1, 1, 1]), Some(Down::Keys(vec![])), "too many keys at once");
    }
}
