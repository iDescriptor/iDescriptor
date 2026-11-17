// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

mod appledb;
mod ipsw_me;

use anyhow::Result;
use reqwest::Client;
use std::time::Duration;

/// Controls whether a backend may reuse its internal catalog cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CachePolicy {
    UseCache,
    Refresh,
}

/// Shared contract for catalog backends. Selection remains enum-based, so these
/// async methods need no boxed futures or dynamic dispatch.
pub(super) trait FirmwareCatalog {
    async fn fetch_devices(&self) -> Result<Vec<CatalogDevice>>;
    async fn fetch_catalog(
        &self,
        identifier: &str,
        policy: CachePolicy,
    ) -> Result<Vec<FirmwareInfo>>;
}

#[derive(Debug, Default, Clone)]
pub(super) struct FirmwareInfo {
    pub(super) identifier: String,
    pub(super) version: String,
    pub(super) build_id: String,
    pub(super) release_date: String,
    pub(super) file_size: u64,
    pub(super) url: String,
    pub(super) signed: bool,
    pub(super) sha1: String,
    pub(super) sha256: String,
}

impl FirmwareInfo {
    pub(super) fn key(&self) -> String {
        firmware_key(&self.identifier, &self.build_id)
    }

    pub(super) fn file_name(&self) -> String {
        format!(
            "{}_{}_{}_Restore.ipsw",
            safe_file_segment(&self.identifier),
            safe_file_segment(&self.version),
            safe_file_segment(&self.build_id)
        )
    }

    pub(super) fn expected_digest(&self) -> Option<(&'static str, &str)> {
        if !self.sha256.is_empty() {
            Some(("sha256", &self.sha256))
        } else if !self.sha1.is_empty() {
            Some(("sha1", &self.sha1))
        } else {
            None
        }
    }
}

pub(super) fn firmware_key(identifier: &str, build_id: &str) -> String {
    format!("{identifier}:{build_id}")
}

fn safe_file_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum Provider {
    #[default]
    AppleDb,
    IpswMe,
}

impl Provider {
    pub(super) fn parse(id: &str) -> Option<Self> {
        match id {
            "appledb" => Some(Self::AppleDb),
            "ipsw_me" => Some(Self::IpswMe),
            _ => None,
        }
    }

    pub(super) fn id(self) -> &'static str {
        match self {
            Self::AppleDb => "appledb",
            Self::IpswMe => "ipsw_me",
        }
    }
}

#[derive(Debug)]
pub(super) struct CatalogDevice {
    pub(super) name: String,
    pub(super) identifier: String,
}

fn client() -> Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .user_agent(concat!("iDescriptor/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

impl FirmwareCatalog for Provider {
    async fn fetch_catalog(
        &self,
        identifier: &str,
        policy: CachePolicy,
    ) -> Result<Vec<FirmwareInfo>> {
        match self {
            Self::AppleDb => appledb::AppleDb.fetch_catalog(identifier, policy).await,
            Self::IpswMe => ipsw_me::IpswMe.fetch_catalog(identifier, policy).await,
        }
    }

    async fn fetch_devices(&self) -> Result<Vec<CatalogDevice>> {
        match self {
            Self::AppleDb => appledb::AppleDb.fetch_devices().await,
            Self::IpswMe => ipsw_me::IpswMe.fetch_devices().await,
        }
    }
}
