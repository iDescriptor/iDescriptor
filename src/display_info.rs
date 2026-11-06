// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reads the display panel serial number over the diagnostics relay and maps
//! its prefix to the panel manufacturer (Samsung, LG, ...), similar to the
//! "Screen Serial Number" row of 3uTools' verification report.

use crate::qvariantmap_insert;
use anyhow::{Result, anyhow};
use idevice::diagnostics_relay::DiagnosticsRelayClient;
use log::{debug, warn};
use plist::{Dictionary, Value};
use qmetaobject::QString;
use qttypes::QVariantMap;

/// Screen serial prefix -> manufacturer, using the short names 3uTools shows
/// next to "Screen Serial Number" (e.g. "LG", "Samsung"). Apple does not
/// document these; the mapping comes from community reports (3uTools
/// verification reports, MacRumors threads, r/PWM_Sensitive, iParts4u's LCD
/// guide). Longer prefixes must come before shorter ones that share a start.
// FIXME: none of these prefixes are confirmed by Apple; a 3uTools label from a
// real device is the best evidence available for each entry.
const PANEL_MANUFACTURER_PREFIXES: &[(&str, &str)] = &[
    // OLED panels
    ("G9N", "Samsung"),
    ("G9P", "Samsung"),
    ("G9Q", "Samsung"),
    // iPad Pro (M4/M5) tandem OLED.
    ("G9L", "Samsung"),
    ("GVC", "LG"),
    ("GVH", "LG"),
    ("GH", "LG"),
    // Community-assumed (MacRumors iPhone 17 poll and screen-lottery thread,
    // Reddit); no confirmed 3uTools sample yet. JLN is seen on base iPhone 17
    // (`JLNHTT…`, `JLNHSS…`) and is not one of the Samsung/LG prefixes.
    ("J5V", "BOE"),
    ("JLN", "BOE"),
    // LCD panels (iPhone 6s - 11, SE)
    ("DTP", "LG"),
    ("C3F", "LG"),
    ("DKH", "Sharp"),
    ("COV", "Sharp"),
    // JDI (Japan Display) took over Toshiba's LCD business; older guides label
    // these "Toshiba". iParts4u also listed C11 under Sharp, Reddit's iPhone 11
    // list only under JDI.
    ("FVQ", "JDI"),
    ("F7C", "JDI"),
    ("C11", "JDI"),
    // FIXME: `G0P` was seen on an iPhone 11 and guessed to be BOE only from the
    // panel's look; add it once confirmed against 3uTools' label.
];

/// Device-tree nodes holding the display authentication IC. The name is a
/// per-generation codename (`mogul-display` on recent models); the tree is also
/// scanned for any node ending in `-display`.
const KNOWN_AUTH_NODES: &[&str] = &["mogul-display"];

/// Module serial (18 chars) + `-` + auth IC serial (12 hex chars), as found in
/// the common name of the Apple-signed display certificate.
const MODULE_SERIAL_LEN: usize = 18;
const AUTH_IC_SERIAL_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSerialSource {
    /// Apple-signed certificate of the display authentication IC (iOS 26+ safe).
    AuthCertificate,
    /// `raw-panel-serial-number` property in IODeviceTree (older iOS versions).
    DeviceTree,
    /// `AppleCLCD2`'s `Panel_ID` string.
    PanelId,
}

impl PanelSerialSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::AuthCertificate => "auth_certificate",
            Self::DeviceTree => "device_tree",
            Self::PanelId => "panel_id",
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct DisplayInfo {
    /// Screen serial number as read from the device, like 3uTools' "Screen
    /// Serial Number" (may contain several `+`-separated fields).
    pub screen_serial: String,
    /// Module serial (first field of `screen_serial`); its prefix identifies
    /// the manufacturer.
    pub panel_serial: String,
    pub source: Option<PanelSerialSource>,
    pub auth_node: Option<String>,
    pub auth_ic_serial: Option<String>,
    /// `auth-passed` flag set by iOS after the display challenge-response.
    pub auth_passed: Option<bool>,
    pub panel_id: Option<String>,
}

impl DisplayInfo {
    pub fn prefix(&self) -> &str {
        self.panel_serial
            .get(..3)
            .unwrap_or(self.panel_serial.as_str())
    }

    pub fn manufacturer(&self) -> Option<&'static str> {
        panel_manufacturer(&self.panel_serial)
    }

    pub fn to_qvariantmap(&self) -> QVariantMap {
        let mut map = QVariantMap::default();
        qvariantmap_insert!(
            map,
            "screen_serial",
            QString::from(self.screen_serial.as_str())
        );
        qvariantmap_insert!(map, "prefix", QString::from(self.prefix()));
        qvariantmap_insert!(
            map,
            "manufacturer",
            QString::from(self.manufacturer().unwrap_or_default())
        );
        qvariantmap_insert!(
            map,
            "source",
            QString::from(
                self.source
                    .map(PanelSerialSource::as_str)
                    .unwrap_or_default()
            )
        );
        qvariantmap_insert!(
            map,
            "auth_node",
            QString::from(self.auth_node.as_deref().unwrap_or_default())
        );
        qvariantmap_insert!(
            map,
            "auth_ic_serial",
            QString::from(self.auth_ic_serial.as_deref().unwrap_or_default())
        );
        let auth_status = match self.auth_passed {
            Some(true) => "passed",
            Some(false) => "failed",
            None => "unknown",
        };
        qvariantmap_insert!(map, "auth_status", QString::from(auth_status));
        qvariantmap_insert!(
            map,
            "panel_id",
            QString::from(self.panel_id.as_deref().unwrap_or_default())
        );
        map
    }
}

pub fn panel_manufacturer(serial: &str) -> Option<&'static str> {
    PANEL_MANUFACTURER_PREFIXES
        .iter()
        .find(|(prefix, _)| serial.starts_with(prefix))
        .map(|(_, manufacturer)| *manufacturer)
}

pub async fn read_display_info(diag: &mut DiagnosticsRelayClient) -> Result<DisplayInfo> {
    let mut info = DisplayInfo::default();

    let tree = match diag.ioregistry(Some("IODeviceTree"), None, None).await {
        Ok(tree) => tree.map(Value::Dictionary),
        Err(e) => {
            warn!("read_display_info: failed to read IODeviceTree: {e}");
            None
        }
    };

    let mut auth_nodes = Vec::new();
    if let Some(tree) = &tree {
        collect_display_auth_nodes(tree, &mut auth_nodes);
    }
    let mut names: Vec<String> = Vec::new();
    let candidates = auth_nodes
        .iter()
        .map(|(name, _)| name.as_str())
        .chain(KNOWN_AUTH_NODES.iter().copied());
    for candidate in candidates {
        if !names.iter().any(|n| n == candidate) {
            names.push(candidate.to_string());
        }
    }

    for name in names {
        let from_tree = auth_nodes
            .iter()
            .find(|(n, node)| *n == name && node.contains_key("certificate"))
            .map(|(_, node)| *node);
        let queried;
        let entry = match from_tree {
            Some(node) => node,
            None => match diag.ioregistry(None, Some(&name), None).await {
                Ok(Some(entry)) => {
                    queried = entry;
                    &queried
                }
                Ok(None) => continue,
                Err(e) => {
                    debug!("read_display_info: no ioregistry entry named {name}: {e}");
                    continue;
                }
            },
        };
        if apply_auth_entry(&mut info, &name, entry) {
            break;
        }
    }

    if info.panel_serial.is_empty() {
        if let Some(text) = tree.as_ref().and_then(find_raw_panel_serial) {
            set_screen_serial(&mut info, &text, PanelSerialSource::DeviceTree);
        }
    }

    let clcd = match diag.ioregistry(None, None, Some("AppleCLCD2")).await {
        Ok(Some(entry)) => Some(entry),
        _ => diag
            .ioregistry(None, Some("AppleCLCD2"), None)
            .await
            .ok()
            .flatten(),
    };
    if let Some(panel_id) = clcd
        .as_ref()
        .and_then(|entry| entry.get("Panel_ID"))
        .and_then(value_text)
        .filter(|s| !s.is_empty())
    {
        if info.panel_serial.is_empty() {
            set_screen_serial(&mut info, &panel_id, PanelSerialSource::PanelId);
        }
        info.panel_id = Some(panel_id);
    }

    if info.panel_serial.is_empty() {
        return Err(anyhow!(
            "display panel serial number not found in the device I/O registry"
        ));
    }
    Ok(info)
}

/// Fills `info` from a display auth node; returns false if the node carries no
/// recognisable module serial.
fn apply_auth_entry(info: &mut DisplayInfo, name: &str, entry: &Dictionary) -> bool {
    let Some(Value::Data(der)) = entry.get("certificate") else {
        return false;
    };
    let Some((module, ic)) = find_auth_serial(der) else {
        return false;
    };
    info.screen_serial = module.clone();
    info.panel_serial = module;
    info.auth_ic_serial = Some(ic);
    info.auth_node = Some(name.to_string());
    info.source = Some(PanelSerialSource::AuthCertificate);
    info.auth_passed = entry.get("auth-passed").and_then(value_flag);
    if let Some(Value::Data(idsn)) = entry.get("idsn") {
        info.auth_ic_serial = Some(hex::encode_upper(idsn));
    }
    true
}

/// Finds `<module serial>-<auth IC serial>` inside a DER certificate blob.
fn find_auth_serial(der: &[u8]) -> Option<(String, String)> {
    let total = MODULE_SERIAL_LEN + 1 + AUTH_IC_SERIAL_LEN;
    der.windows(total).find_map(|w| {
        let (module, rest) = w.split_at(MODULE_SERIAL_LEN);
        let valid = module[0].is_ascii_uppercase()
            && module[1..]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            && rest[0] == b'-'
            && rest[1..]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(b));
        valid.then(|| {
            (
                String::from_utf8_lossy(module).into_owned(),
                String::from_utf8_lossy(&rest[1..]).into_owned(),
            )
        })
    })
}

/// Collects every dictionary whose `name` ends in `-display`.
fn collect_display_auth_nodes<'a>(value: &'a Value, out: &mut Vec<(String, &'a Dictionary)>) {
    match value {
        Value::Dictionary(dict) => {
            if let Some(name) = dict.get("name").and_then(value_text) {
                if name.ends_with("-display") {
                    out.push((name, dict));
                }
            }
            for child in dict.values() {
                collect_display_auth_nodes(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_display_auth_nodes(child, out);
            }
        }
        _ => {}
    }
}

/// Returns the first `raw-panel-serial-number` value that holds a serial.
fn find_raw_panel_serial(value: &Value) -> Option<String> {
    match value {
        Value::Dictionary(dict) => dict
            .get("raw-panel-serial-number")
            .and_then(value_text)
            .filter(|text| module_serial(text).is_some())
            .or_else(|| dict.values().find_map(find_raw_panel_serial)),
        Value::Array(items) => items.iter().find_map(find_raw_panel_serial),
        _ => None,
    }
}

/// Stores the full serial string; does nothing if it holds no module serial.
fn set_screen_serial(info: &mut DisplayInfo, text: &str, source: PanelSerialSource) {
    if let Some(module) = module_serial(text) {
        info.screen_serial = text.trim().to_string();
        info.panel_serial = module;
        info.source = Some(source);
    }
}

/// Screen serial strings are `+`-separated; the module serial is the first field.
fn module_serial(text: &str) -> Option<String> {
    let field = text.split('+').next()?.trim();
    let valid = field.len() >= 10 && field.bytes().all(|b| b.is_ascii_alphanumeric());
    valid.then(|| field.to_ascii_uppercase())
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim_end_matches('\0').to_string()),
        Value::Data(bytes) => Some(
            String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string(),
        ),
        _ => None,
    }
}

fn value_flag(value: &Value) -> Option<bool> {
    match value {
        Value::Boolean(b) => Some(*b),
        Value::Integer(i) => i.as_unsigned().map(|v| v != 0),
        Value::Data(bytes) => Some(bytes.iter().any(|b| *b != 0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_prefixes_and_leaves_unknown_unresolved() {
        assert_eq!(panel_manufacturer("G9N4340000000000AA"), Some("Samsung"));
        assert_eq!(panel_manufacturer("G9Q0000000000000AA"), Some("Samsung"));
        assert_eq!(panel_manufacturer("GH33340000000000AA"), Some("LG"));
        assert_eq!(panel_manufacturer("GVC2460000000000AA"), Some("LG"));
        assert_eq!(panel_manufacturer("DTP5390000000000AA"), Some("LG"));
        assert_eq!(panel_manufacturer("C110000000000000AA"), Some("JDI"));
        assert_eq!(panel_manufacturer("J5VH000000000000AA"), Some("BOE"));
        assert_eq!(panel_manufacturer("JLNHTT006GD0000U6M"), Some("BOE"));
        assert_eq!(panel_manufacturer("G9LHP4U7WKS0000L4B"), Some("Samsung"));
        assert_eq!(panel_manufacturer("G0P0000000000000AA"), None);
        assert_eq!(panel_manufacturer(""), None);
    }

    #[test]
    fn extracts_module_and_ic_serial_from_certificate_bytes() {
        let mut der = vec![
            0x30, 0x82, 0x01, 0x0c, 0x06, 0x03, 0x55, 0x04, 0x03, 0x0c, 0x1f,
        ];
        der.extend_from_slice(b"G9N1234567890ABCDE-0011AABBCCDD");
        der.extend_from_slice(&[0x31, 0x00]);
        assert_eq!(
            find_auth_serial(&der),
            Some(("G9N1234567890ABCDE".into(), "0011AABBCCDD".into()))
        );
    }

    #[test]
    fn rejects_certificate_without_serial_pattern() {
        // Lower-case hex in the IC part and a too-short module serial.
        assert_eq!(find_auth_serial(b"G9N1234567890ABCDE-0011aabbccdd"), None);
        assert_eq!(
            find_auth_serial(b"Display CA M1 G9N12345-0011AABBCCDD"),
            None
        );
    }

    #[test]
    fn keeps_full_serial_and_takes_module_from_first_field() {
        let mut info = DisplayInfo::default();
        set_screen_serial(
            &mut info,
            " DTP5390000000000AA+0x12+FOO ",
            PanelSerialSource::PanelId,
        );
        assert_eq!(info.screen_serial, "DTP5390000000000AA+0x12+FOO");
        assert_eq!(info.panel_serial, "DTP5390000000000AA");
        assert_eq!(info.prefix(), "DTP");
        assert_eq!(info.manufacturer(), Some("LG"));
        assert_eq!(info.source, Some(PanelSerialSource::PanelId));
    }

    #[test]
    fn ignores_serial_strings_without_module_serial() {
        let mut info = DisplayInfo::default();
        set_screen_serial(
            &mut info,
            "\u{1}\u{2}+G9P0123456789ABCDE",
            PanelSerialSource::PanelId,
        );
        set_screen_serial(&mut info, "", PanelSerialSource::PanelId);
        assert_eq!(info, DisplayInfo::default());
    }

    #[test]
    fn finds_raw_panel_serial_in_nested_tree_with_data_value() {
        let mut empty = Dictionary::new();
        empty.insert("raw-panel-serial-number".into(), Value::Data(vec![0; 4]));
        let mut panel = Dictionary::new();
        panel.insert(
            "raw-panel-serial-number".into(),
            // Same layout as values posted from iPhone 17 Pro / 18 Pro devices.
            Value::Data(b"G9PHP0000000000L4B+A+A14B0000000000000200+FX9HN0000000000000+9000000000000000000000000\0".to_vec()),
        );
        let mut root = Dictionary::new();
        root.insert(
            "children".into(),
            Value::Array(vec![Value::Dictionary(empty), Value::Dictionary(panel)]),
        );
        assert_eq!(
            find_raw_panel_serial(&Value::Dictionary(root)),
            Some("G9PHP0000000000L4B+A+A14B0000000000000200+FX9HN0000000000000+9000000000000000000000000".into())
        );
    }

    #[test]
    fn auth_entry_reads_flag_and_prefers_idsn_for_ic_serial() {
        let mut entry = Dictionary::new();
        entry.insert(
            "certificate".into(),
            Value::Data(b"..GH31234567890ABCDE-0011AABBCCDD..".to_vec()),
        );
        entry.insert("auth-passed".into(), Value::Data(vec![1, 0, 0, 0]));
        entry.insert("idsn".into(), Value::Data(vec![0xde, 0xad, 0xbe, 0xef]));

        let mut info = DisplayInfo::default();
        assert!(apply_auth_entry(&mut info, "mogul-display", &entry));
        assert_eq!(info.screen_serial, "GH31234567890ABCDE");
        assert_eq!(info.panel_serial, "GH31234567890ABCDE");
        assert_eq!(info.manufacturer(), Some("LG"));
        assert_eq!(info.auth_passed, Some(true));
        assert_eq!(info.auth_ic_serial.as_deref(), Some("DEADBEEF"));
        assert_eq!(info.source, Some(PanelSerialSource::AuthCertificate));
    }

    #[test]
    fn auth_entry_without_certificate_is_skipped() {
        let mut entry = Dictionary::new();
        entry.insert("auth-passed".into(), Value::Boolean(true));
        let mut info = DisplayInfo::default();
        assert!(!apply_auth_entry(&mut info, "mogul-display", &entry));
        assert_eq!(info, DisplayInfo::default());
    }
}
