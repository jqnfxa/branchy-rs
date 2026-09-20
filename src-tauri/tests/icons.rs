//! Every icon the configuration names has to exist.
//!
//! This is here because it did not. `tauri-build` needs `icons/icon.ico` to
//! produce a Windows resource file and fails the build without it — but only on
//! Windows, so a Linux machine and a Linux CI job both stayed green while the
//! Windows job broke. A missing file is not worth discovering one platform at a
//! time.

use std::path::Path;

fn config() -> serde_json::Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
        .expect("tauri.conf.json is readable");
    serde_json::from_str(&text).expect("tauri.conf.json is valid JSON")
}

#[test]
fn every_configured_icon_exists() {
    let config = config();
    let icons = config["bundle"]["icon"]
        .as_array()
        .expect("bundle.icon is a list");
    assert!(!icons.is_empty(), "no icons configured at all");

    for entry in icons {
        let name = entry.as_str().expect("an icon path is a string");
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
        assert!(path.is_file(), "{name} is configured but missing");
        assert!(
            path.metadata().expect("readable").len() > 0,
            "{name} is empty"
        );
    }
}

#[test]
fn a_windows_resource_icon_is_present() {
    // tauri-build looks for this by name on Windows, whatever the config says
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.ico");
    assert!(
        path.is_file(),
        "icons/icon.ico is required for the Windows resource file"
    );

    let raw = std::fs::read(&path).expect("readable");
    assert!(raw.len() > 6, "too short to be an icon");
    assert_eq!(&raw[0..4], &[0, 0, 1, 0], "not an ICO header");
    let count = u16::from_le_bytes([raw[4], raw[5]]);
    assert!(count >= 4, "only {count} sizes; Windows wants several");
}
