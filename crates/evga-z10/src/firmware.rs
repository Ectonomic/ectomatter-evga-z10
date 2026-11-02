//! Which firmware the keyboard runs, and what it can therefore do.
//!
//! EVGA's own builds report a version such as 0x0111, which EVGA prints as
//! v1.17 (both bytes in decimal). Ectomatter's custom builds of 1.17 report a
//! second byte of 171 and up: 0x01AB is v1.171, custom build 1. A feature
//! only custom firmware has is offered only when the keyboard reports a build
//! that has it, so a keyboard on EVGA's firmware is never sent a command it
//! does not know.

use serde_json::{json, Value};

/// A feature some custom build adds, and the first build that has it.
pub struct Feature {
    pub id: &'static str,
    pub name: &'static str,
    pub since: u8,
}

/// What each custom build adds. Build 1 changed only how the volume slider
/// steps, which needs nothing from the host.
pub const FEATURES: &[Feature] = &[
    Feature { id: "sliders.read", name: "Reading both sliders", since: 2 },
    Feature { id: "dimmer.set", name: "Setting the dimmer", since: 2 },
    Feature { id: "volume.quiet", name: "Freeing the volume slider", since: 3 },
    Feature { id: "dimmer.keep", name: "Keeping the dimmer where you set it", since: 3 },
    Feature { id: "fn.read", name: "Saying when Fn is held", since: 4 },
    Feature { id: "onboard.layout", name: "Keeping an onboard layout", since: 5 },
    Feature { id: "status.read", name: "Saying its mode and key rollover", since: 5 },
];

/// The first second byte a custom build reports: 171 is build 1.
const FIRST_CUSTOM: u8 = 171;

/// Retail or custom, the version as EVGA would print it, and the custom
/// features this firmware has; with the words Ectomatter's keyboard tab says
/// it in (docs/core-protocol.md, section 4, the firmware a device runs):
/// `words` for its Firmware row, a `tag` beside them, a `note` under them,
/// a `chip` beside the keyboard's name, `none` for a build that adds nothing,
/// and each later build's feature as `words`.
pub fn describe(version: u16) -> Value {
    let (major, minor) = ((version >> 8) as u8, (version & 0xff) as u8);
    let printed = format!("{major}.{minor:02}");
    if minor < 100 {
        return json!({ "kind": "retail", "version": printed, "build": null, "features": [], "words": format!("EVGA v{printed}"),
            "note": "Features that need Ectomatter’s custom firmware show here when the keyboard runs it." });
    }
    // 100 to 170 is no build of EVGA's or of ours: modified by someone else
    if minor < FIRST_CUSTOM {
        return json!({ "kind": "unknown", "version": printed, "build": null, "features": [], "words": format!("v{printed}"), "chip": "Unofficial build",
            "note": "This version is neither EVGA’s nor Ectomatter’s, so no custom features are offered." });
    }
    let build = minor - FIRST_CUSTOM + 1;
    let features: Vec<Value> = FEATURES
        .iter()
        .filter(|f| build >= f.since)
        .map(|f| json!({ "id": f.id, "name": f.name }))
        .collect();
    let next = FEATURES.iter().filter(|f| build < f.since).map(|f| json!({ "id": f.id, "name": f.name, "since": f.since, "words": format!("{} (build {})", f.name, f.since) })).collect::<Vec<_>>();
    json!({ "kind": "custom", "version": printed, "build": build, "features": features, "later": next,
        "words": format!("Ectomatter v{printed}, build {build}"), "tag": "Unofficial", "chip": format!("Unofficial build {build}"),
        "none": "Nothing Ectomatter controls yet. This build fixes how the volume slider steps." })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evga_builds_are_retail_and_offer_nothing_extra() {
        let d = describe(0x0111);
        assert_eq!(d["kind"], "retail");
        assert_eq!(d["words"], "EVGA v1.17");
        assert!(d.get("chip").is_none(), "its maker's own build has no chip");
        assert_eq!(d["version"], "1.17");
        assert_eq!(d["features"], json!([]));
    }

    #[test]
    fn custom_builds_offer_what_they_have() {
        let one = describe(0x01AB);
        assert_eq!((one["kind"].as_str(), one["version"].as_str(), one["build"].as_u64()), (Some("custom"), Some("1.171"), Some(1)));
        assert_eq!((one["words"].as_str(), one["tag"].as_str(), one["chip"].as_str()), (Some("Ectomatter v1.171, build 1"), Some("Unofficial"), Some("Unofficial build 1")));
        assert_eq!(one["later"][0]["words"], "Reading both sliders (build 2)");
        assert_eq!(one["features"], json!([]));
        assert_eq!(one["later"].as_array().unwrap().len(), FEATURES.len());
        let two = describe(0x01AC);
        assert_eq!(two["build"], 2);
        let ids: Vec<&str> = two["features"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["sliders.read", "dimmer.set"]);
        assert_eq!(two["later"].as_array().unwrap().len(), 5);

        let three = describe(0x01AD);
        assert_eq!(three["build"], 3);
        assert_eq!(three["features"].as_array().unwrap().len(), 4);
        assert_eq!(three["later"][0]["id"], "fn.read");

        let four = describe(0x01AE);
        assert_eq!(four["build"], 4);
        assert_eq!(four["features"].as_array().unwrap().len(), 5);
        let later: Vec<&str> = four["later"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
        assert_eq!(later, ["onboard.layout", "status.read"]);

        let five = describe(0x01AF);
        assert_eq!(five["build"], 5);
        assert_eq!(five["features"].as_array().unwrap().len(), FEATURES.len());
        assert_eq!(five["later"], json!([]));
    }

    #[test]
    fn a_number_neither_evga_nor_ectomatter_uses_is_unknown() {
        assert_eq!(describe(0x0165)["kind"], "unknown");
    }
}
