//! Small helpers for reading the editor's JSON the way the keyboard needs it:
//! bytes must be 0 to 255, words 0 to 65535, and anything else is refused
//! rather than wrapped, so a bad value never reaches the keyboard as a
//! different one.

use serde_json::{Map, Value};

use crate::error::{Result, invalid};

pub type Obj = Map<String, Value>;

pub fn int(v: &Value, what: &str) -> Result<i64> {
    match v {
        Value::Bool(b) => Ok(*b as i64),
        Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .ok_or_else(|| invalid(format!("{what} must be a whole number"))),
        _ => Err(invalid(format!("{what} must be a number"))),
    }
}

pub fn byte(v: &Value, what: &str) -> Result<u8> {
    let n = int(v, what)?;
    u8::try_from(n).map_err(|_| invalid(format!("{what} must be 0 to 255, not {n}")))
}

pub fn word(v: &Value, what: &str) -> Result<u16> {
    let n = int(v, what)?;
    u16::try_from(n).map_err(|_| invalid(format!("{what} must be 0 to 65535, not {n}")))
}

pub fn obj<'a>(v: &'a Value, what: &str) -> Result<&'a Obj> {
    v.as_object()
        .ok_or_else(|| invalid(format!("{what} must be an object")))
}

/// A color as the editor holds it: `{brightness, color: [r, g, b]}`, as the
/// four bytes the keyboard stores (brightness first).
pub fn argb(c: &Value, what: &str) -> Result<[u8; 4]> {
    let c = obj(c, what)?;
    let b = byte(
        c.get("brightness")
            .ok_or_else(|| invalid(format!("{what} needs a brightness")))?,
        what,
    )?;
    let rgb = rgb(
        c.get("color")
            .ok_or_else(|| invalid(format!("{what} needs a color")))?,
        what,
    )?;
    Ok([b, rgb[0], rgb[1], rgb[2]])
}

pub fn rgb(v: &Value, what: &str) -> Result<[u8; 3]> {
    let a = v
        .as_array()
        .ok_or_else(|| invalid(format!("{what} color must be [r, g, b]")))?;
    if a.len() != 3 {
        return Err(invalid(format!("{what} color must have three parts")));
    }
    Ok([byte(&a[0], what)?, byte(&a[1], what)?, byte(&a[2], what)?])
}

/// Truthiness the way the editor's step records use it (`down: true`).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}
