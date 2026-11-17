// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{CachePolicy, CatalogDevice, FirmwareCatalog, FirmwareInfo};
use anyhow::{Context, Result, bail};
use const_format::concatcp;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const API_BASE: &str = "https://api.appledb.dev";
const DEVICES_URL: &str = concatcp!(API_BASE, "/device/main.json");
type FeedCache = HashMap<&'static str, (Instant, Arc<Vec<Value>>)>;
static FEEDS: LazyLock<Mutex<FeedCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Default)]
pub(super) struct AppleDb;

impl FirmwareCatalog for AppleDb {
    async fn fetch_devices(&self) -> Result<Vec<CatalogDevice>> {
        let entries: Vec<Value> = super::client()?
            .get(DEVICES_URL)
            .send()
            .await
            .context("Failed to fetch AppleDB device catalog")?
            .error_for_status()
            .context("AppleDB device catalog request failed")?
            .json()
            .await
            .context("Invalid AppleDB device catalog")?;
        let mut seen = HashSet::new();
        let mut devices = Vec::new();
        for entry in entries {
            for identifier in strings(&entry["identifier"]) {
                if os_for_identifier(identifier).is_none() || !seen.insert(identifier.to_string()) {
                    continue;
                }
                devices.push(CatalogDevice {
                    name: entry["name"].as_str().unwrap_or(identifier).to_string(),
                    identifier: identifier.to_string(),
                });
            }
        }
        devices.sort_by_key(|device| (device.name.to_lowercase(), device.identifier.clone()));
        if devices.is_empty() {
            bail!("AppleDB device catalog is empty");
        }
        Ok(devices)
    }

    async fn fetch_catalog(
        &self,
        identifier: &str,
        policy: CachePolicy,
    ) -> Result<Vec<FirmwareInfo>> {
        let Some(os) = os_for_identifier(identifier) else {
            bail!("Unsupported AppleDB device: {identifier}")
        };
        let entries = feed(os, policy).await?;
        Ok(parse_catalog(&entries, identifier))
    }
}

// Serialize feed requests to avoid fetching the same large dataset for each device.
async fn feed(os: &'static str, policy: CachePolicy) -> Result<Arc<Vec<Value>>> {
    let mut cache = FEEDS.lock().await;
    if policy == CachePolicy::UseCache {
        if let Some((time, entries)) = cache.get(os) {
            if time.elapsed() < Duration::from_secs(1800) {
                return Ok(entries.clone());
            }
        }
    }
    let entries: Vec<Value> = super::client()?
        .get(format!("{API_BASE}/ios/{os}/main.json"))
        .send()
        .await
        .context("Failed to fetch AppleDB firmware catalog")?
        .error_for_status()
        .context("AppleDB firmware catalog request failed")?
        .json()
        .await
        .context("Invalid AppleDB firmware catalog")?;
    let entries = Arc::new(entries);
    cache.insert(os, (Instant::now(), entries.clone()));
    Ok(entries)
}

fn os_for_identifier(identifier: &str) -> Option<&'static str> {
    if ["iPhone", "iPad", "iPod"]
        .iter()
        .any(|prefix| identifier.starts_with(prefix))
    {
        Some("iOS")
    } else if identifier.starts_with("AppleTV") {
        Some("tvOS")
    } else if identifier.starts_with("AudioAccessory") {
        Some("audioOS")
    } else if identifier.starts_with("RealityDevice") {
        Some("visionOS")
    } else if identifier.starts_with("iBridge") {
        Some("bridgeOS")
    } else if ["Mac", "iMac"]
        .iter()
        .any(|prefix| identifier.starts_with(prefix))
    {
        Some("macOS")
    } else {
        None
    }
}

fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(value) => vec![value],
        Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn supports(value: &Value, identifier: &str) -> bool {
    strings(value).contains(&identifier)
}

fn parse_catalog(entries: &[Value], identifier: &str) -> Vec<FirmwareInfo> {
    let mut firmwares = HashMap::<String, FirmwareInfo>::new();
    for entry in entries {
        // Preserve the release catalog behavior: prerelease builds aren't offered.
        if entry["beta"].as_bool() == Some(true) || entry["rc"].as_bool() == Some(true) {
            continue;
        }
        let (Some(version), Some(build), Some(sources)) = (
            entry["version"].as_str(),
            entry["build"].as_str(),
            entry["sources"].as_array(),
        ) else {
            continue;
        };
        for source in sources {
            if source["type"].as_str() != Some("ipsw") {
                continue;
            }
            // Source compatibility is narrower than a release's device list.
            let devices = source.get("deviceMap").unwrap_or(&entry["deviceMap"]);
            if !supports(devices, identifier) {
                continue;
            }
            let Some(links) = source["links"].as_array() else {
                continue;
            };
            let link = links
                .iter()
                .filter(|link| {
                    link["active"].as_bool() != Some(false)
                        && link["url"].as_str().is_some_and(|url| {
                            url.starts_with("https://") || url.starts_with("http://")
                        })
                })
                .max_by_key(|link| {
                    (
                        link["preferred"].as_bool().unwrap_or(false),
                        link["url"]
                            .as_str()
                            .unwrap_or_default()
                            .starts_with("https://"),
                    )
                });
            let Some(url) = link.and_then(|link| link["url"].as_str()) else {
                continue;
            };
            let firmware = FirmwareInfo {
                identifier: identifier.to_string(),
                version: version.to_string(),
                build_id: build.to_string(),
                release_date: entry["released"].as_str().unwrap_or_default().to_string(),
                file_size: source["size"].as_u64().unwrap_or(0),
                url: url.to_string(),
                signed: entry["signed"].as_bool() == Some(true)
                    || supports(&entry["signed"], identifier),
                sha1: source["hashes"]["sha1"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                sha256: source["hashes"]["sha2-256"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            };
            // Keep one restore artifact per device/build, preferring verifiable metadata.
            match firmwares.entry(build.to_string()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(firmware);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if (firmware.expected_digest().is_some(), firmware.file_size > 0)
                        > (
                            entry.get().expected_digest().is_some(),
                            entry.get().file_size > 0,
                        )
                    {
                        entry.insert(firmware);
                    }
                }
            }
        }
    }
    let mut firmwares: Vec<_> = firmwares.into_values().collect();
    firmwares.sort_by(|a, b| {
        b.signed
            .cmp(&a.signed)
            .then_with(|| b.release_date.cmp(&a.release_date))
            .then_with(|| b.build_id.cmp(&a.build_id))
    });
    firmwares
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Snapshot of https://api.appledb.dev/ios/iOS;14G61.json (IPSW metadata only).
    #[test]
    fn real_appledb_1034_fixture_covers_issue_164() {
        let release: Value = serde_json::from_str(include_str!("fixtures/ios-14G61.json")).unwrap();
        for id in ["iPhone5,3", "iPhone5,4", "iPad3,4"] {
            let catalog = parse_catalog(std::slice::from_ref(&release), id);
            assert_eq!(catalog.len(), 1);
            assert_eq!(catalog[0].version, "10.3.4");
            assert_eq!(catalog[0].build_id, "14G61");
            assert!(catalog[0].signed);
            assert!(catalog[0].file_size > 0);
            assert_eq!(catalog[0].sha256.len(), 64);
            assert!(catalog[0].url.ends_with("_Restore.ipsw"));
        }
    }

    fn release() -> Value {
        json!({"version":"10.3.4", "build":"14G61", "released":"2019-07-22",
        "deviceMap":["iPhone5,3","iPhone5,4","iPad3,4"],
        "signed":["iPhone5,3","iPad3,4"],
        "sources":[
            {"type":"ipsw","deviceMap":["iPhone5,3","iPhone5,4"],"size":123,
             "hashes":{"sha1":"phone-sha1","sha2-256":"phone-sha256"},
             "links":[{"url":"https://example.com/phone.ipsw","active":true,"preferred":true}]},
            {"type":"ipsw","deviceMap":["iPad3,4"],"size":456,
             "links":[{"url":"https://example.com/tablet.ipsw","active":true}]},
            {"type":"ota","deviceMap":["iPhone5,3"],"links":[{"url":"https://example.com/ota.zip"}]}
        ]})
    }

    #[test]
    fn reported_models_receive_1034_and_their_own_restore_source() {
        for (id, file, signed) in [
            ("iPhone5,3", "phone", true),
            ("iPhone5,4", "phone", false),
            ("iPad3,4", "tablet", true),
        ] {
            let entries = parse_catalog(&[release()], id);
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].version, "10.3.4");
            assert!(entries[0].url.contains(file));
            assert_eq!(entries[0].signed, signed);
        }
        let phone = parse_catalog(&[release()], "iPhone5,3");
        assert_eq!(phone[0].file_size, 123);
        assert_eq!(phone[0].expected_digest(), Some(("sha256", "phone-sha256")));
    }

    #[test]
    fn handles_signing_variants_duplicates_and_missing_metadata() {
        let mut entry = release();
        entry["signed"] = json!(true);
        let entries = parse_catalog(&[entry.clone(), entry.clone()], "iPhone5,4");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].signed);
        entry.as_object_mut().unwrap().remove("signed");
        let entries = parse_catalog(&[entry], "iPad3,4");
        assert!(!entries[0].signed);
        assert!(entries[0].expected_digest().is_none());
    }

    #[test]
    fn rejects_ota_only_incompatible_and_inactive_sources() {
        assert!(parse_catalog(&[release()], "iPhone99,1").is_empty());
        let mut entry = release();
        entry["sources"][0]["links"][0]["active"] = json!(false);
        assert!(parse_catalog(&[entry.clone()], "iPhone5,3").is_empty());
        entry["sources"] = json!([{"type":"ota","deviceMap":["iPhone5,3"],"links":[{"url":"https://example.com/ota.zip"}]}]);
        assert!(parse_catalog(&[entry], "iPhone5,3").is_empty());
    }
}
