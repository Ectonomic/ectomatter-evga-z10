//! The display (report 10, 88 bytes), decoded from AIDA64:
//! `0a ea 02 01 00 00 00 <checksum>` and then 80 characters, four rows of
//! twenty, in Windows-1252. The checksum makes the characters sum to zero.
//!
//! The keyboard draws only some of those codes as characters. From its own
//! screen: printable ASCII draws as text, 0xB0 is a real degree sign, 0xDB
//! lights the whole cell, codes under 0x20 draw nothing, 0x80 to 0x9F draw
//! noise from memory, and most of 0xA0 to 0xDF light the whole cell. So only
//! the codes whose look is known are ever sent: every other character is
//! first spelled in them (see [lcd_text]), and the preview in the app runs
//! the same table (data/lcdfold.json), so it shows what the keyboard shows.

use std::sync::OnceLock;

use crate::protocol::{RID_LCD, csum, size};

pub const LCD_COLS: usize = 20;
pub const LCD_ROWS: usize = 4;

/// Windows-1252 for one character, or `?` where it has none (the full block, which the
/// keyboard draws for 0xDB, is 0xDB).
pub fn cp1252(c: char) -> u8 {
    const HIGH: [(char, u8); 27] = [
        ('€', 0x80),
        ('‚', 0x82),
        ('ƒ', 0x83),
        ('„', 0x84),
        ('…', 0x85),
        ('†', 0x86),
        ('‡', 0x87),
        ('ˆ', 0x88),
        ('‰', 0x89),
        ('Š', 0x8A),
        ('‹', 0x8B),
        ('Œ', 0x8C),
        ('Ž', 0x8E),
        ('‘', 0x91),
        ('’', 0x92),
        ('“', 0x93),
        ('”', 0x94),
        ('•', 0x95),
        ('–', 0x96),
        ('—', 0x97),
        ('˜', 0x98),
        ('™', 0x99),
        ('š', 0x9A),
        ('›', 0x9B),
        ('œ', 0x9C),
        ('ž', 0x9E),
        ('Ÿ', 0x9F),
    ];
    if c == '█' {
        return 0xDB; // the keyboard lights the whole cell for this code
    }
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return u as u8;
    }
    HIGH.iter().find(|(h, _)| *h == c).map_or(b'?', |(_, b)| *b)
}

/// The characters the keyboard's screen draws, as its description lists
/// them (`displays`, `draws`): the printable ASCII characters, the degree
/// sign, and the full block.
pub const DRAWS: [&str; 3] = [" ~", "°", "█"];

fn draws() -> &'static ectomatter_protocol::text::Draws {
    static D: OnceLock<ectomatter_protocol::text::Draws> = OnceLock::new();
    D.get_or_init(|| ectomatter_protocol::text::Draws::of(&serde_json::json!(DRAWS)))
}

/// Whether the keyboard draws a character as itself.
pub fn lcd_draws(c: char) -> bool {
    draws().draws(c)
}

/// Text in only the characters the keyboard draws (ectomatter_protocol::text).
pub fn lcd_text(s: &str) -> String {
    draws().spell(s)
}

/// The rows as the keyboard will show them: in the characters it draws (see
/// [lcd_text]), padded or cut to twenty, four of them.
pub fn lcd_rows(rows: &[&str]) -> Vec<String> {
    (0..LCD_ROWS)
        .map(|i| {
            let r: String = rows
                .get(i)
                .copied()
                .map(lcd_text)
                .unwrap_or_default()
                .chars()
                .take(LCD_COLS)
                .collect();
            format!("{r:<LCD_COLS$}")
        })
        .collect()
}

pub fn lcd_frame(rows: &[&str]) -> Vec<u8> {
    lcd_frame_cmd(rows, 0x01)
}

/// The display's report with `cmd` in its fourth byte: 1 draws the rows; 0,
/// with the rows blank, hands the screen back to the keyboard's own screens,
/// as EVGA's software does on closing.
pub fn lcd_frame_cmd(rows: &[&str], cmd: u8) -> Vec<u8> {
    let text: Vec<u8> = lcd_rows(rows).concat().chars().map(cp1252).collect();
    let mut buf = vec![0u8; size(RID_LCD)];
    buf[0..4].copy_from_slice(&[RID_LCD, 0xEA, 0x02, cmd]);
    buf[7] = csum(&text);
    buf[8..8 + text.len()].copy_from_slice(&text);
    buf
}

#[cfg(test)]
mod fold_tests {
    use super::*;

    #[test]
    fn sends_only_what_the_keyboard_draws() {
        assert_eq!(lcd_text("São Paulo 22°C"), "Sao Paulo 22°C");
        assert_eq!(lcd_text("Москва"), "Moskva");
        assert_eq!(lcd_text("Αθήνα"), "Athina");
        assert_eq!(lcd_text("Straße – “quoted”…"), "Strasse - \"quoted\"...");
        assert_eq!(lcd_text("東京"), "??");
        assert_eq!(lcd_rows(&["Zürich"])[0], "Zurich              ");
        let f = lcd_frame(&["█°A"]);
        assert_eq!(&f[8..11], &[0xDB, 0xB0, b'A']);
        assert!(f[8..88].iter().all(|&b| (0x20..=0x7E).contains(&b) || b == 0xB0 || b == 0xDB));
    }
}
