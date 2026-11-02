//! The keyboard itself: finding it, and every command this crate sends.

use std::thread::sleep;
use std::time::Duration;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::lcd::lcd_frame;
use crate::macros::{MACRO_SLOTS, macro_meta};
use crate::profile::Profile;
use crate::protocol::*;

/// Where both sliders are, as Ectomatter's firmware reports them.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Sliders {
    /// the dimmer, 0 to 255, which is what the lighting is multiplied by
    pub dimmer: u8,
    /// and its reading straight from the slider, about 0 to 4095
    pub dimmer_raw: u16,
    /// the volume slider's last worked-out step, and its own reading
    pub volume: u8,
    pub volume_raw: u16,
    /// whether the dimmer is being held at a level the host asked for
    pub dimmer_held: bool,
    /// what it has been asked to stop doing by itself, from build 3
    pub quiet_volume: bool,
    pub keep_dimmer: bool,
    /// whether Fn is held, from build 4 (earlier builds leave this byte
    /// unwritten, so it means something only where the firmware has "fn.read")
    pub fn_held: bool,
}

/// How reports reach the keyboard. The real one is HID; tests use a fake that
/// records what would have been sent.
pub trait Transport: Send {
    /// Send a feature report. `buf[0]` is its id.
    fn set_feature(&mut self, buf: &[u8]) -> Result<()>;
    /// Read feature report `rid`, `len` bytes including the id.
    fn get_feature(&mut self, rid: u8, len: usize) -> Result<Vec<u8>>;
    /// Where the keyboard is, for showing to the user.
    fn path(&self) -> String;
    /// Give the keyboard time to act on a request before its answer is read.
    fn pause(&mut self, d: Duration) {
        sleep(d);
    }
}

/// The keyboard over HID, on any system hidapi supports.
#[cfg(feature = "hid")]
pub struct HidTransport {
    dev: hidapi::HidDevice,
    path: String,
}

#[cfg(feature = "hid")]
impl HidTransport {
    /// Open the Z10's own collection. On Windows and macOS each collection is
    /// its own device; on Linux they share the interface's hidraw node.
    pub fn open(api: &hidapi::HidApi) -> Result<HidTransport> {
        let mine = |d: &&hidapi::DeviceInfo| d.vendor_id() == VID && d.product_id() == PID;
        let infos: Vec<_> = api.device_list().filter(mine).collect();
        let info = infos
            .iter()
            .find(|d| d.usage_page() == USAGE_PAGE && d.usage() == USAGE)
            .or_else(|| infos.iter().find(|d| d.interface_number() == INTERFACE))
            .ok_or(Error::NotFound)?;
        let dev = info.open_device(api)?;
        Ok(HidTransport {
            dev,
            path: info.path().to_string_lossy().into_owned(),
        })
    }
}

#[cfg(feature = "hid")]
impl Transport for HidTransport {
    fn set_feature(&mut self, buf: &[u8]) -> Result<()> {
        self.dev.send_feature_report(buf)?;
        Ok(())
    }
    fn get_feature(&mut self, rid: u8, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        buf[0] = rid;
        self.dev.get_feature_report(&mut buf)?;
        Ok(buf)
    }
    fn path(&self) -> String {
        self.path.clone()
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A slot's settings, from the keyboard's answer for it, or nothing where the
/// answer says the slot holds no macro.
pub fn stored_macro(r: &[u8]) -> Option<Value> {
    (r[0] == RID_MACRO && r[6] == SUCCESS && r[8] != 0xFF).then(|| macro_meta(r))
}

pub struct Z10<T: Transport> {
    pub t: T,
}

#[cfg(feature = "hid")]
impl Z10<HidTransport> {
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new()?;
        Ok(Z10 {
            t: HidTransport::open(&api)?,
        })
    }
}

impl<T: Transport> Z10<T> {
    pub fn new(t: T) -> Self {
        Z10 { t }
    }

    pub fn path(&self) -> String {
        self.t.path()
    }

    // raw transfers -----------------------------------------------------------
    pub fn set(&mut self, buf: &[u8]) -> Result<()> {
        assert_eq!(
            buf.len(),
            size(buf[0]),
            "report {} is the wrong size",
            buf[0]
        );
        self.t.set_feature(buf)
    }

    pub fn get(&mut self, rid: u8) -> Result<Vec<u8>> {
        self.t.get_feature(rid, size(rid))
    }

    /// Send a request, give the keyboard `wait` to act on it, then read its answer.
    pub fn xfer(&mut self, buf: &[u8], wait: Duration) -> Result<Vec<u8>> {
        self.set(buf)?;
        self.t.pause(wait);
        self.get(buf[0])
    }

    // report 4: general commands ----------------------------------------------
    /// Returns the status byte and the data that follows it.
    pub fn general(
        &mut self,
        main: u8,
        sub1: u8,
        sub2: u8,
        data: &[u8],
        wait: Duration,
    ) -> Result<(u8, Vec<u8>)> {
        let mut buf = vec![0u8; size(RID_GENERAL)];
        buf[0..7].copy_from_slice(&[RID_GENERAL, 0xEA, 0x02, main, sub1, sub2, 0]);
        buf[7..7 + data.len()].copy_from_slice(data);
        let r = self.xfer(&buf, wait)?;
        Ok((r[6], r[7..].to_vec()))
    }

    /// The firmware version as the keyboard reports it: 0x0111 is v1.17, as
    /// EVGA prints both bytes in decimal.
    pub fn firmware_version(&mut self) -> Result<(u8, u16)> {
        let (status, d) = self.general(READ_FW, 1, 0, &[], ms(10))?;
        Ok((status, u16::from(d[1]) << 8 | u16::from(d[0])))
    }

    /// The profile the keyboard is using, 1 to 5.
    pub fn profile_number(&mut self) -> Result<Option<u8>> {
        let (status, d) = self.profile_command(1, &[], ms(10))?;
        Ok((status == SUCCESS).then_some(d[0]))
    }

    /// A profile command, and its own answer. Switching profiles writes the
    /// keyboard's memory, which can take it longer than the wait, and until
    /// it is done the last answer (another command's) is still there to be
    /// read, which once taken for this one's left every answer after it a step
    /// behind. The answer echoes the command and whether it was a read (1) or
    /// a switch (0), so it is read again every 10 ms, for up to half a second,
    /// until it is this one's.
    fn profile_command(&mut self, sub1: u8, data: &[u8], wait: Duration) -> Result<(u8, Vec<u8>)> {
        let mut buf = vec![0u8; size(RID_GENERAL)];
        buf[0..7].copy_from_slice(&[RID_GENERAL, 0xEA, 0x02, PROFILE_NUM, sub1, 0, 0]);
        buf[7..7 + data.len()].copy_from_slice(data);
        self.set(&buf)?;
        self.t.pause(wait);
        for _ in 0..50 {
            let r = self.get(RID_GENERAL)?;
            if r[1] == 0xEA && r[2] == 0x02 && r[3] == PROFILE_NUM && r[4] == sub1 {
                return Ok((r[6], r[7..].to_vec()));
            }
            self.t.pause(ms(10));
        }
        Err(Error::Invalid("the keyboard did not answer about its profiles".into()))
    }

    /// Where both sliders are, on Ectomatter's firmware from build 2.
    ///
    /// Returns the dimmer's level 0-255 and its raw reading, the volume
    /// slider's level and raw reading, and whether a level of the host's is
    /// being held on the dimmer. EVGA's firmware does not answer this.
    pub fn sliders(&mut self) -> Result<Option<Sliders>> {
        let (status, d) = self.general(READ_SLIDERS, 0, 0, &[], ms(10))?;
        if status != SUCCESS {
            return Ok(None);
        }
        Ok(Some(Sliders {
            dimmer: d[0],
            dimmer_raw: u16::from(d[2]) << 8 | u16::from(d[1]),
            volume: d[3],
            volume_raw: u16::from(d[5]) << 8 | u16::from(d[4]),
            dimmer_held: d[6] != 0,
            quiet_volume: d[7] & QUIET_VOLUME != 0,
            keep_dimmer: d[7] & KEEP_DIMMER != 0,
            fn_held: d.get(8).is_some_and(|b| *b != 0),
        }))
    }

    /// Hold the dimmer at `level`, or with `None` give the slider it back.
    /// The hold ends by itself as soon as the slider is moved.
    pub fn set_dimmer(&mut self, level: Option<u8>) -> Result<bool> {
        let (hold, level) = match level {
            Some(l) => (1, l),
            None => (0, 0),
        };
        Ok(self.general(SET_DIMMER, level, hold, &[], ms(10))?.0 == SUCCESS)
    }

    /// Ask the keyboard to stop doing a slider's job by itself, so the host
    /// can do it instead. Both off is how it behaves with nothing running,
    /// and they are forgotten when it is unplugged. Firmware build 3 on.
    pub fn set_modes(&mut self, quiet_volume: bool, keep_dimmer: bool) -> Result<bool> {
        let flags = u8::from(quiet_volume) * QUIET_VOLUME + u8::from(keep_dimmer) * KEEP_DIMMER;
        Ok(self.general(SET_MODES, flags, 0, &[], ms(10))?.0 == SUCCESS)
    }

    pub fn select_profile(&mut self, n: u8) -> Result<bool> {
        assert!((1..=5).contains(&n));
        Ok(self.profile_command(0, &[n], ms(100))?.0 == SUCCESS)
    }

    /// Light the screen at `brightness` in `rgb` until the profile is applied
    /// again (switched to, saved, or the keyboard restarted).
    pub fn set_lcd_light(&mut self, brightness: u8, rgb: [u8; 3]) -> Result<bool> {
        Ok(self.general(LCD_LIGHT_RAM, 0, 0, &[brightness, rgb[0], rgb[1], rgb[2]], ms(5))?.0 == SUCCESS)
    }

    /// Show these built-in screens (the profile's bitmask) until the profile is applied again.
    pub fn set_lcd_apps(&mut self, apps: u8) -> Result<bool> {
        Ok(self.general(LCD_APPS_RAM, 0, 0, &[apps], ms(5))?.0 == SUCCESS)
    }

    /// Set the clock on the keyboard's status screen, as EVGA's Unleash RGB sends it once
    /// a second: the year, then the month (1 to 12), the day, the hour (0 to 23), the
    /// minute and the second. It stands still until the next one.
    pub fn set_time(&mut self, year: u16, time: [u8; 5]) -> Result<bool> {
        let [lo, hi] = year.to_le_bytes();
        let [mo, d, h, mi, s] = time;
        Ok(self.general(LCD_TIME, 0, 0, &[lo, hi, mo, d, h, mi, s], ms(5))?.0 == SUCCESS)
    }

    /// Take the clock off the status screen until a time is set again, as EVGA's Unleash
    /// RGB does on closing. (The keyboard blanks the screen's bottom row as it does,
    /// whatever is in it.)
    pub fn clear_time(&mut self) -> Result<bool> {
        Ok(self.general(LCD_TIME, CLEAR_TIME, 0, &[], ms(5))?.0 == SUCCESS)
    }

    /// Make the keyboard apply its stored profile again, ending any preview.
    pub fn reload_profile(&mut self) -> Result<bool> {
        Ok(self.general(RELOAD_PROFILE, 0, 0, &[], ms(100))?.0 == SUCCESS)
    }

    // report 8: whole profiles ------------------------------------------------
    /// `n` is 1 to 5, or 254 for the factory default profile.
    /// Another program writing to the keyboard at the same moment, as the LCD
    /// service does every second or two, can garble an answer; a profile that
    /// fails its checksum is read again, up to three times.
    pub fn read_profile(&mut self, n: u8) -> Result<Profile> {
        let mut buf = vec![0u8; size(RID_PROFILE)];
        buf[0..7].copy_from_slice(&[RID_PROFILE, 0xEA, 0x02, 1, n, 0, 0]);
        let mut tries = 0;
        loop {
            let r = self.xfer(&buf, ms(20))?;
            if r[6] != SUCCESS {
                return Err(Error::Refused {
                    what: format!("reading profile {n}"),
                    status: r[6],
                });
            }
            match Profile::decode(&r) {
                Err(Error::Checksum) if tries < 3 => tries += 1,
                done => return done,
            }
        }
    }

    pub fn write_profile(&mut self, n: u8, profile: &Profile) -> Result<bool> {
        assert!((1..=5).contains(&n));
        Ok(self.xfer(&profile.encode(n), ms(100))?[6] == SUCCESS)
    }

    // report 9: macros --------------------------------------------------------
    /// Macro slot `index` as the keyboard sends it. Slot 0 is the table of
    /// which slots are in use.
    ///
    /// The table carries no checksum, so it is read after the whole wait the
    /// original editor gave every macro (150 ms). A slot's answer names the
    /// slot and carries a checksum, so it is read every 10 ms and taken as
    /// soon as it is whole and this slot's; until then the last answer
    /// (another slot's, or the table) is still there to be read. At most the
    /// same 150 ms.
    pub fn macro_report(&mut self, index: u8) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; size(RID_MACRO)];
        buf[0..7].copy_from_slice(&[RID_MACRO, 0xEA, 0x02, 1, index, 0, 0]);
        if index == 0 {
            return self.xfer(&buf, ms(150));
        }
        self.set(&buf)?;
        let mut waited = 0;
        loop {
            self.t.pause(ms(10));
            waited += 10;
            let r = self.get(RID_MACRO)?;
            let whole = r[3] == 1 && r[4] == index && r[7] == csum(&r[8..]);
            if whole || waited >= 150 {
                return Ok(r);
            }
        }
    }

    /// The slots the table says are in use.
    pub fn macro_slots(&mut self) -> Result<Vec<u8>> {
        let used = self.macro_report(0)?;
        Ok((1..=MACRO_SLOTS).filter(|i| used[7 + *i as usize] == 1).collect())
    }

    /// Every stored macro's settings, by slot.
    pub fn read_macros(&mut self) -> Result<Vec<(u8, Value)>> {
        let mut out = Vec::new();
        for i in self.macro_slots()? {
            let r = self.macro_report(i)?;
            if let Some(m) = stored_macro(&r) {
                out.push((i, m));
            }
        }
        Ok(out)
    }

    /// `body` is the 1025 bytes after the command header, from `macro_body()`.
    pub fn write_macro(&mut self, index: u8, body: &[u8]) -> Result<bool> {
        let mut buf = vec![0u8; size(RID_MACRO)];
        buf[0..7].copy_from_slice(&[RID_MACRO, 0xEA, 0x02, 0, index, 0, 0]);
        buf[7..7 + body.len()].copy_from_slice(body);
        Ok(self.xfer(&buf, ms(150))?[6] == SUCCESS)
    }

    pub fn delete_macro(&mut self, index: u8) -> Result<bool> {
        let mut buf = vec![0u8; size(RID_MACRO)];
        buf[0..7].copy_from_slice(&[RID_MACRO, 0xEA, 0x02, 0, index, 0, 0]);
        buf[8] = 0xFF; // name length, the marker for unused
        buf[9..59].fill(0xFF);
        buf[1031] = 0xFF; // slot free
        buf[7] = csum(&buf[8..]);
        Ok(self.xfer(&buf, ms(150))?[6] == SUCCESS)
    }

    // report 6: live per-key colors, drawn by the host and never stored ----
    /// `colors` is 116 (r, g, b) by key position.
    pub fn stream_colors(&mut self, colors: &[[u8; 3]], brightness: u8) -> Result<()> {
        let mut body = Vec::with_capacity(4 * colors.len());
        for [r, g, b] in colors {
            body.extend_from_slice(&[brightness, *r, *g, *b]);
        }
        let mut buf = vec![RID_KEYS, 0xEA, 0x02, 1, 0, 0, 0, csum(&body)];
        buf.extend_from_slice(&body);
        self.set(&buf)
    }

    pub fn stop_stream(&mut self) -> Result<()> {
        let mut buf = vec![0u8; size(RID_KEYS)];
        buf[0..4].copy_from_slice(&[RID_KEYS, 0xEA, 0x02, 0]);
        self.set(&buf)
    }

    // report 7: leave the live preset preview ---------------------------------
    pub fn exit_sync(&mut self) -> Result<()> {
        let mut buf = vec![0u8; size(RID_PREVIEW)];
        buf[0..3].copy_from_slice(&[RID_PREVIEW, 0xEA, 0x02]);
        self.set(&buf)
    }

    // report 10: the display --------------------------------------------------
    /// Show four rows of up to twenty characters.
    pub fn show_lcd(&mut self, rows: &[&str]) -> Result<()> {
        self.set(&lcd_frame(rows))
    }

    /// Hand the display back to the keyboard's own screens.
    pub fn hand_back_lcd(&mut self) -> Result<()> {
        self.set(&crate::lcd::lcd_frame_cmd(&[], 0x00))
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use std::collections::VecDeque;

    /// A keyboard that is slow to answer: each read gives the next of its
    /// answers, the last again once they run out.
    struct Slow(VecDeque<Vec<u8>>, Vec<u8>);

    impl Transport for Slow {
        fn set_feature(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn get_feature(&mut self, _: u8, len: usize) -> Result<Vec<u8>> {
            if let Some(r) = self.0.pop_front() {
                self.1 = r;
            }
            let mut r = self.1.clone();
            r.resize(len, 0);
            Ok(r)
        }
        fn path(&self) -> String {
            "slow".into()
        }
        fn pause(&mut self, _: Duration) {}
    }

    fn answer(main: u8, sub1: u8, status: u8, data: u8) -> Vec<u8> {
        vec![RID_GENERAL, 0xEA, 0x02, main, sub1, 0, status, data]
    }

    /// A keyboard that keeps what it is sent, and answers that it took it.
    struct Kept(Vec<Vec<u8>>);

    impl Transport for Kept {
        fn set_feature(&mut self, b: &[u8]) -> Result<()> {
            self.0.push(b.to_vec());
            Ok(())
        }
        fn get_feature(&mut self, _: u8, len: usize) -> Result<Vec<u8>> {
            let last = self.0.last().cloned().unwrap_or_default();
            let mut r = vec![RID_GENERAL, 0xEA, 0x02, last[3], last[4], 0, SUCCESS];
            r.resize(len, 0);
            Ok(r)
        }
        fn path(&self) -> String {
            "kept".into()
        }
        fn pause(&mut self, _: Duration) {}
    }

    /// The clock goes as EVGA's WriteDateTimeFeatureReport lays it out: command 10, the
    /// year low byte first, then the month, day, hour, minute and second; cleared, the
    /// same command with 0x10 after it.
    #[test]
    fn the_clock_is_sent_as_evga_sends_it() {
        let mut k = Z10::new(Kept(Vec::new()));
        assert!(k.set_time(2026, [10, 12, 0, 5, 9]).unwrap());
        assert!(k.clear_time().unwrap());
        let sent = &k.t.0;
        assert_eq!(sent[0], vec![4, 0xEA, 0x02, 10, 0, 0, 0, 0xEA, 0x07, 10, 12, 0, 5, 9, 0, 0, 0]);
        assert_eq!(sent[1], vec![4, 0xEA, 0x02, 10, 0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    /// A switch of profiles the keyboard is still carrying out when its
    /// answer is first read leaves the last answer there (a read of the
    /// sliders, then of the profile): neither is taken for the switch's, nor
    /// the switch's for the next read's.
    #[test]
    fn a_slow_switch_is_not_answered_by_the_last_command() {
        let mut k = Z10::new(Slow(
            VecDeque::from([answer(READ_SLIDERS, 0, 0xC1, 0), answer(PROFILE_NUM, 1, SUCCESS, 1), answer(PROFILE_NUM, 0, SUCCESS, 0)]),
            Vec::new(),
        ));
        assert!(k.select_profile(2).unwrap(), "the switch's own answer, once it came");
        let mut k = Z10::new(Slow(VecDeque::from([answer(PROFILE_NUM, 0, SUCCESS, 0), answer(PROFILE_NUM, 1, SUCCESS, 2)]), Vec::new()));
        assert_eq!(k.profile_number().unwrap(), Some(2), "the read's own answer, not the switch's");
        // a keyboard that never answers is said so, not taken at its last answer
        let mut k = Z10::new(Slow(VecDeque::from([answer(READ_SLIDERS, 0, SUCCESS, 0)]), Vec::new()));
        assert!(k.profile_number().is_err());
    }
}
