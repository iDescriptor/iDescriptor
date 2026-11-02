// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
pub mod bundle;
mod cryptex;
#[cfg(target_os = "macos")]
mod native;
mod transport;
use crate::device_ctx::DeviceServices;
use anyhow::{Context, Result, ensure};
use bundle::Kind;
use idevice::{
    IdeviceService, RsdService, lockdown::LockdownClient, mobile_image_mounter::ImageMounter,
};
use std::path::Path;
pub type Progress<'a> = &'a (dyn Fn(&str, f64) + Send + Sync);

struct Mounter {
    client: ImageMounter,
    connection: Option<transport::Connection>,
}
impl std::ops::Deref for Mounter {
    type Target = ImageMounter;
    fn deref(&self) -> &ImageMounter {
        &self.client
    }
}
impl std::ops::DerefMut for Mounter {
    fn deref_mut(&mut self) -> &mut ImageMounter {
        &mut self.client
    }
}
async fn mounter(device: &DeviceServices, udid: &str) -> Result<Mounter> {
    let primary = {
        let provider = device.provider.lock().await;
        ImageMounter::connect(provider.as_ref()).await
    };
    match primary {
        Ok(client) => Ok(Mounter {
            client,
            connection: None,
        }),
        Err(error) => {
            let mut connection = transport::Connection::open(device, udid)
                .await
                .with_context(|| format!("Image mounter connection failed: {error}"))?;
            let client =
                ImageMounter::connect_rsd(&mut connection.transport, &mut connection.handshake)
                    .await?;
            Ok(Mounter {
                client,
                connection: Some(connection),
            })
        }
    }
}
async fn mounter_present(client: &mut ImageMounter, image_type: &str) -> Result<bool> {
    match client.lookup_image(image_type).await {
        Ok(signature) if !signature.is_empty() => return Ok(true),
        Err(idevice::IdeviceError::DeviceLocked) => {
            return Err(idevice::IdeviceError::DeviceLocked.into());
        }
        Err(idevice::IdeviceError::NotFound) | Ok(_) => {}
        Err(error) => return Err(error.into()),
    }
    // Older devices can hang up after CopyDevices, so only use it for modern DDIs.
    if image_type == "Personalized" {
        let images = client.copy_devices().await?;
        return Ok(images.iter().any(|entry| {
            entry.get("DiskImageType").and_then(plist::Value::as_string) == Some(image_type)
        }));
    }
    Ok(false)
}
pub async fn mounted(device: &DeviceServices, udid: &str, major: u32, minor: u32) -> Result<bool> {
    let mut client = mounter(device, udid).await?;
    if mounter_present(
        &mut client,
        if major >= 17 {
            "Personalized"
        } else {
            "Developer"
        },
    )
    .await?
    {
        return Ok(true);
    }
    if cryptex_supported(major, minor) {
        let mut connection = transport::Connection::open(device, udid).await?;
        if connection
            .handshake
            .services
            .contains_key("com.apple.security.cryptexd.remote")
        {
            return cryptex::installed(&mut connection).await;
        }
    }
    Ok(false)
}
fn cryptex_supported(major: u32, minor: u32) -> bool {
    major > 26 || major == 26 && minor >= 4
}
async fn ecid(device: &DeviceServices) -> Result<u64> {
    let provider = device.provider.lock().await;
    let mut client = LockdownClient::connect(provider.as_ref()).await?;
    let pairing = provider.get_pairing_file().await?;
    client.start_session(&pairing).await?;
    let value = client.get_value(Some("UniqueChipID"), None).await?;
    value
        .as_unsigned_integer()
        .context("Device did not return its ECID")
}
pub async fn prepare(
    udid: &str,
    selection: &str,
    root: &Path,
    revision: &str,
    progress: Progress<'_>,
) -> Result<()> {
    bundle::validate_revision(revision)?;
    let device = crate::device_ctx::get_device(udid.to_owned()).await?;
    let major = device.ios_version.major;
    let minor = device.ios_version.minor;
    ensure!(major >= 17, "Personalized images require iOS 17 or later");
    progress("checking", 0.0);
    let mut client = mounter(&device, udid).await?;
    ensure!(
        client.query_developer_mode_status().await?,
        "Enable Developer Mode in Settings → Privacy & Security on the device, then restart it"
    );
    if mounter_present(&mut client, "Personalized").await? {
        return Ok(());
    }
    let automatic = selection == "auto";
    let kind = Kind::parse(selection)
        .or_else(|| {
            automatic.then_some(if major >= 27 {
                Kind::Cryptex
            } else {
                Kind::Personalized
            })
        })
        .context("Unknown developer image type")?;
    if kind == Kind::Cryptex {
        ensure!(
            cryptex_supported(major, minor),
            "Developer Cryptex installation requires iOS 26.4 or later"
        );
        progress("connecting", 0.0);
        let mut connection = transport::Connection::open(&device, udid).await?;
        if connection
            .handshake
            .services
            .contains_key("com.apple.security.cryptexd.remote")
        {
            if cryptex::installed(&mut connection).await? {
                return Ok(());
            }
            progress("downloading", 0.0);
            let bundle =
                bundle::fetch(kind, root, revision, &|fraction| progress("downloading", fraction)).await?;
            return cryptex::install(&mut connection, &bundle, progress).await;
        }
        ensure!(automatic, "Device does not advertise the Cryptex service");
    }
    progress("downloading", 0.0);
    let bundle = bundle::fetch(Kind::Personalized, root, revision, &|fraction| {
        progress("downloading", fraction)
    })
    .await?;
    let identifiers = client.query_personalization_identifiers(None).await?;
    let identifiers = plist::Value::Dictionary(identifiers);
    let board = bundle::number(identifiers.get("BoardId")).context("Missing device board ID")?;
    let chip = bundle::number(identifiers.get("ChipID")).context("Missing device chip ID")?;
    if bundle::identity(&bundle.manifest, board, chip).is_err()
        && automatic
        && cryptex_supported(major, minor)
    {
        let mut connection = transport::Connection::open(&device, udid).await?;
        let bundle = bundle::fetch(Kind::Cryptex, root, revision, &|fraction| {
            progress("downloading", fraction)
        })
        .await?;
        return cryptex::install(&mut connection, &bundle, progress).await;
    }
    bundle::identity(&bundle.manifest, board, chip)?;
    let ecid = ecid(&device).await?;
    let manifest = bundle.read("BuildManifest.plist").await?;
    let image = bundle.read("Image.dmg").await?;
    let trust = bundle.read("Image.dmg.trustcache").await?;
    progress("personalizing", 0.0);
    // The library may reconnect after a missing manifest, so retain its provider.
    let Mounter { client, connection } = &mut client;
    if let Some(connection) = connection {
        client
            .mount_personalized_with_callback_rsd(
                &mut connection.transport,
                &mut connection.handshake,
                image,
                trust,
                &manifest,
                None,
                ecid,
                |((sent, total), ())| async move {
                    progress("uploading", sent as f64 / total.max(1) as f64);
                },
                (),
            )
            .await?;
    } else {
        let provider = device.provider.lock().await;
        client
            .mount_personalized_with_callback(
                provider.as_ref(),
                image,
                trust,
                &manifest,
                None,
                ecid,
                |((sent, total), ())| async move {
                    progress("uploading", sent as f64 / total.max(1) as f64);
                },
                (),
            )
            .await?;
    }
    progress("verifying", 1.0);
    let mut client = mounter(&device, udid).await?;
    ensure!(
        mounter_present(&mut client, "Personalized").await?,
        "Mount reported success but no personalized developer image is mounted"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cryptex_version_boundary_uses_minor_version() {
        assert!(!cryptex_supported(17, 4));
        assert!(!cryptex_supported(26, 3));
        assert!(cryptex_supported(26, 4));
        assert!(cryptex_supported(27, 0));
    }
}

trait PlistFields {
    fn get(&self, key: &str) -> Option<&plist::Value>;
}
impl PlistFields for plist::Value {
    fn get(&self, key: &str) -> Option<&plist::Value> {
        self.as_dictionary()?.get(key)
    }
}
