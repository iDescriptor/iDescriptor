// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{bundle::Bundle, transport::Connection};
use anyhow::{Context, Result, ensure};
use idevice::{
    RsdService,
    services::cryptexd::{
        self, Cryptex1Assets, CryptexInstallProgress, CryptexInstallRequest, CryptexdClient,
    },
};
use std::{future::Future, time::Duration};

async fn step<T>(name: &str, seconds: u64, work: impl Future<Output = Result<T>>) -> Result<T> {
    log::info!("Developer Cryptex: {name}");
    let result = tokio::time::timeout(Duration::from_secs(seconds), work)
        .await
        .with_context(|| format!("Developer Cryptex: {name} timed out after {seconds} seconds"))?
        .with_context(|| format!("Developer Cryptex: {name}"))?;
    log::info!("Developer Cryptex: {name} completed");
    Ok(result)
}
pub async fn installed(connection: &mut Connection) -> Result<bool> {
    step("checking installed images", 30, async {
        Ok(
            cryptexd::installed_ddi(&mut connection.transport, &mut connection.handshake)
                .await?
                .is_some(),
        )
    })
    .await
}
pub async fn install(
    connection: &mut Connection,
    bundle: &Bundle,
    progress: super::Progress<'_>,
) -> Result<()> {
    let service = connection
        .handshake
        .services
        .get("com.apple.security.cryptexd.remote")
        .context("Device does not offer Cryptex installation")?;
    if let Some(features) = &service.features {
        ensure!(
            features.iter().any(|feature| feature == "CryptexInstall"),
            "Device does not support Cryptex installation"
        );
    }
    if installed(connection).await? {
        return Ok(());
    }
    let assets = Cryptex1Assets::load(&bundle.directory).await?;
    // Same sequence as upstream install_ddi, with app-level progress and deadlines.
    // Each routine gets a fresh client because cryptexd closes it after replying.
    progress("identifiers", 0.0);
    let chip = step("reading device personalization identifiers", 30, async {
        Ok(
            CryptexdClient::connect_rsd(&mut connection.transport, &mut connection.handshake)
                .await?
                .read_personalization_identifiers()
                .await?,
        )
    })
    .await?;
    let domain = assets.nonce_domain()?;
    progress("nonce", 0.0);
    let nonce = step("reading device Cryptex nonce", 30, async {
        Ok(
            CryptexdClient::connect_rsd(&mut connection.transport, &mut connection.handshake)
                .await?
                .cryptex_nonce(domain)
                .await?,
        )
    })
    .await?;
    let mut request = idevice::tss::TSSRequest::new();
    request.add_cryptex1_tags(&assets.build_identity, &chip, &nonce)?;
    progress("signing", 0.0);
    let response = step("requesting Apple's personalization ticket", 60, async {
        Ok(request.send().await?)
    })
    .await?;
    let response = response
        .as_dictionary()
        .context("Apple returned an invalid Cryptex ticket response")?;
    let im4m = response
        .get("Cryptex1,Ticket")
        .or_else(|| response.get("ApImg4Ticket"))
        .and_then(plist::Value::as_data)
        .context("Apple returned no Cryptex personalization ticket")?
        .to_vec();
    let properties = assets.cryptex1_properties()?;
    let install = CryptexInstallRequest::new(
        assets.image,
        assets.trustcache,
        im4m,
        assets.info,
        assets.volumehash,
        properties,
    );
    progress("connecting", 0.0);
    let client = step("connecting to Cryptex installer", 30, async {
        Ok(
            CryptexdClient::connect_rsd(&mut connection.transport, &mut connection.handshake)
                .await?,
        )
    })
    .await?;
    progress("uploading", 0.0);
    let mut last_update = std::time::Instant::now() - Duration::from_secs(1);
    let mut last_payload = "";
    step("uploading and installing developer Cryptex", 900, async {
        client.install_with_progress(install, |event| match event {
            CryptexInstallProgress::Uploading { payload, sent, total } => {
                let changed = last_payload != payload;
                if changed {
                    log::info!("Developer Cryptex: uploading {payload} ({sent}/{total} total bytes)");
                    last_payload = payload;
                }
                if changed || sent == total || last_update.elapsed() >= Duration::from_millis(100) {
                    progress("uploading", sent as f64 / total.max(1) as f64);
                    last_update = std::time::Instant::now();
                }
            }
            CryptexInstallProgress::WaitingForReply => {
                log::info!("Developer Cryptex: all payloads uploaded; waiting for the device installation reply (60-second deadline)");
                progress("install_reply", 1.0);
            }
        }).await?;
        Ok(())
    }).await?;
    progress("verifying", 1.0);
    ensure!(
        installed(connection).await?,
        "Developer Cryptex is not installed after installation"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stalled_step_reports_which_request_timed_out() {
        let error = step::<()>("reading device nonce", 0, std::future::pending())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("reading device nonce timed out"));
    }
    #[tokio::test]
    async fn failed_step_preserves_the_underlying_error() {
        let error = step::<()>("reading identifiers", 1, async {
            anyhow::bail!("connection closed")
        })
        .await
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("reading identifiers"));
        assert!(message.contains("connection closed"));
    }
}
