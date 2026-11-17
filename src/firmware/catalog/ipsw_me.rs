// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{CachePolicy, CatalogDevice, FirmwareCatalog, FirmwareInfo};
use anyhow::{Context, Result, anyhow, bail};
use const_format::concatcp;
use serde_json::Value;
use std::collections::HashSet;

const API_BASE: &str = "https://api.ipsw.me/v4";
const DEVICES_URL: &str = concatcp!(API_BASE, "/devices");

#[derive(Debug, Default)]
pub(super) struct IpswMe;

impl FirmwareCatalog for IpswMe {
    async fn fetch_devices(&self) -> Result<Vec<CatalogDevice>> {
        let response = super::client()?
            .get(DEVICES_URL)
            .send()
            .await
            .context("failed to request the IPSW device catalog")?
            .error_for_status()
            .context("the IPSW device catalog request was rejected")?;
        let entries: Vec<Value> = response
            .json()
            .await
            .context("invalid IPSW device catalog response")?;
        let mut identifiers = HashSet::new();
        let mut devices: Vec<CatalogDevice> = entries
            .into_iter()
            .filter_map(|entry| {
                let identifier = entry.get("identifier")?.as_str()?.trim().to_string();
                if identifier.is_empty() || !identifiers.insert(identifier.clone()) {
                    return None;
                }
                let name = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(&identifier)
                    .trim()
                    .to_string();
                Some(CatalogDevice { name, identifier })
            })
            .collect();
        devices.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.identifier.cmp(&b.identifier))
        });
        if devices.is_empty() {
            bail!("the IPSW device catalog is empty");
        }
        Ok(devices)
    }

    async fn fetch_catalog(
        &self,
        identifier: &str,
        _policy: CachePolicy,
    ) -> Result<Vec<FirmwareInfo>> {
        // No internal feed cache: every call requests fresh device metadata.
        let url = format!("{API_BASE}/ipsw/device/{}", urlencoding::encode(identifier));
        let response = super::client()?
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to request firmware catalog for {identifier}"))?
            .error_for_status()
            .with_context(|| format!("firmware catalog rejected the request for {identifier}"))?;
        let body: Value = response
            .json()
            .await
            .context("invalid firmware catalog response")?;
        let returned_identifier = body
            .get("identifier")
            .and_then(Value::as_str)
            .unwrap_or(identifier);
        let entries = body
            .get("firmwares")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("firmware catalog response has no firmwares array"))?;

        let mut firmwares: Vec<FirmwareInfo> = entries
            .iter()
            .filter_map(|entry| {
                let version = entry.get("version")?.as_str()?.to_string();
                let build_id = entry.get("buildid")?.as_str()?.to_string();
                let url = entry.get("url")?.as_str()?.to_string();
                Some(FirmwareInfo {
                    identifier: entry
                        .get("identifier")
                        .and_then(Value::as_str)
                        .unwrap_or(returned_identifier)
                        .to_string(),
                    version,
                    build_id,
                    release_date: entry
                        .get("releasedate")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    file_size: entry.get("filesize").and_then(Value::as_u64).unwrap_or(0),
                    url,
                    signed: entry
                        .get("signed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    sha1: entry
                        .get("sha1sum")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    sha256: entry
                        .get("sha256sum")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect();
        firmwares.sort_by(|a, b| {
            b.signed
                .cmp(&a.signed)
                .then_with(|| b.release_date.cmp(&a.release_date))
                .then_with(|| b.build_id.cmp(&a.build_id))
        });
        Ok(firmwares)
    }
}
