//! Print everything the editor reads from the connected keyboard, as JSON.
//! Reads only; nothing is written.

use evga_z10::{Conn, Keyboard};

fn main() {
    let kb = Keyboard::new(Conn::hid(), None);
    match kb.state() {
        Ok(s) => println!("{}", serde_json::to_string_pretty(&s).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
