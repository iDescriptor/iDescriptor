use super::PlistFields;
// SPDX-FileCopyrightText: 2025-2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use sha2::{Digest, Sha384};
use std::path::{Component, Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};

// Default revision remains pinned; each preparation uses one immutable revision.
pub const DEFAULT_REVISION: &str = "6eae353ae694bda1c421d4a3eee5459ae59c99a1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Personalized,
    Cryptex,
}
impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Personalized" => Some(Self::Personalized),
            "Cryptex" => Some(Self::Cryptex),
            _ => None,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Personalized => "Personalized",
            Self::Cryptex => "Cryptex",
        }
    }
    fn upstream(self) -> &'static str {
        match self {
            Self::Personalized => "Xcode_iOS_DDI_Personalized",
            Self::Cryptex => "Xcode_iOS_DDI_Cryptex",
        }
    }
    pub fn directory(self, root: &Path, revision: &str) -> PathBuf {
        root.join(self.id()).join(revision)
    }
    fn payloads(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Personalized => &[
                ("PersonalizedDMG", "Image.dmg"),
                ("LoadableTrustCache", "Image.dmg.trustcache"),
            ],
            Self::Cryptex => &[
                ("Cryptex1,GenericDmg", "Image.dmg"),
                ("Cryptex1,GenericTrustCache", "Image.dmg.trustcache"),
                ("Cryptex1,CryptexInfoPlist", "Image.dmg.cryptex_info"),
                ("Cryptex1,GenericVolume", "Image.dmg.root_hash"),
            ],
        }
    }
}
pub struct Bundle {
    pub directory: PathBuf,
    pub manifest: plist::Value,
}
impl Bundle {
    pub async fn open(kind: Kind, root: &Path, revision: &str) -> Result<Self> {
        validate_revision(revision)?;
        let directory = kind.directory(root, revision);
        let manifest: plist::Value =
            plist::from_bytes(&fs::read(directory.join("BuildManifest.plist")).await?)?;
        for (component, name) in kind.payloads() {
            let path = directory.join(name);
            let size = fs::metadata(&path).await?.len();
            ensure!(size > 0, "Developer image file {name} is empty");
            let expected = component_digest(&manifest, component)?;
            let mut file = fs::File::open(&path).await?;
            use tokio::io::AsyncReadExt;
            let mut hash = Sha384::new();
            let mut buffer = vec![0; 1024 * 1024];
            loop {
                let count = file.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            ensure!(
                hash.finalize().as_slice() == expected,
                "Developer image digest mismatch for {name}"
            );
        }
        Ok(Self {
            directory,
            manifest,
        })
    }
    pub async fn read(&self, name: &str) -> Result<Vec<u8>> {
        ensure!(
            Path::new(name)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "Invalid image bundle path"
        );
        Ok(fs::read(self.directory.join(name)).await?)
    }
}
fn component_digest<'a>(manifest: &'a plist::Value, component: &str) -> Result<&'a [u8]> {
    let identities = manifest
        .get("BuildIdentities")
        .and_then(plist::Value::as_array)
        .context("Missing build identities")?;
    let mut digest: Option<&[u8]> = None;
    for identity in identities {
        if let Some(value) = identity.get("Manifest").and_then(|m| m.get(component)) {
            let bytes = value
                .get("Digest")
                .and_then(plist::Value::as_data)
                .context("Missing image digest")?;
            ensure!(bytes.len() == 48, "Unsupported image digest");
            if let Some(previous) = digest {
                ensure!(previous == bytes, "Ambiguous image component digest");
            }
            digest = Some(bytes);
        }
    }
    digest.context("Image component missing from manifest")
}

pub async fn fetch(
    kind: Kind,
    root: &Path,
    revision: &str,
    progress: &(dyn Fn(f64) + Send + Sync),
) -> Result<Bundle> {
    validate_revision(revision)?;
    static DOWNLOAD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _download = DOWNLOAD.lock().await;
    if let Ok(bundle) = Bundle::open(kind, root, revision).await {
        return Ok(bundle);
    }
    fs::create_dir_all(root).await?;
    let temporary = root.join(format!(".ddi-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&temporary).await?;
    let guard = crate::utils::TempDirGuard::new(temporary.clone());
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(std::time::Duration::from_secs(900))
        .build()?;
    let names = std::iter::once("BuildManifest.plist")
        .chain(kind.payloads().iter().map(|(_, name)| *name))
        .collect::<Vec<_>>();
    for (index, name) in names.iter().enumerate() {
        let url = format!(
            "https://raw.githubusercontent.com/doronz88/DeveloperDiskImage/{revision}/PersonalizedImages/{}/{name}",
            kind.upstream()
        );
        let response = client.get(url).send().await?.error_for_status()?;
        let total = response.content_length();
        let mut written = 0u64;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary.join(name))
            .await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            written += chunk.len() as u64;
            ensure!(
                written <= 2 * 1024 * 1024 * 1024,
                "Developer image exceeds size limit"
            );
            file.write_all(&chunk).await?;
            let fraction = total
                .filter(|total| *total > 0)
                .map(|total| written as f64 / total as f64)
                .unwrap_or(0.0);
            progress((index as f64 + fraction.min(1.0)) / names.len() as f64);
        }
        if let Some(total) = total {
            ensure!(total == written, "Incomplete developer image download");
        }
        file.flush().await?;
    }
    // Validate the entire temporary bundle before publishing it in the cache.
    let stage_root = temporary.join("validated");
    let stage_directory = kind.directory(&stage_root, revision);
    fs::create_dir_all(&stage_directory).await?;
    for name in &names {
        fs::rename(temporary.join(name), stage_directory.join(name)).await?;
    }
    Bundle::open(kind, &stage_root, revision).await?;
    let destination = kind.directory(root, revision);
    fs::create_dir_all(destination.parent().unwrap()).await?;
    if fs::try_exists(&destination).await? {
        if let Ok(bundle) = Bundle::open(kind, root, revision).await {
            return Ok(bundle);
        }
        // A corrupt/incomplete cached bundle is not a usable fallback.
        fs::remove_dir_all(&destination).await?;
    }
    fs::rename(&stage_directory, &destination).await?;
    drop(guard);
    progress(1.0);
    Bundle::open(kind, root, revision).await
}
pub fn validate_revision(revision: &str) -> Result<()> {
    ensure!(
        revision.len() == 40
            && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !revision.bytes().any(|byte| byte.is_ascii_uppercase()),
        "Developer image revision must be a full lowercase 40-character commit hash"
    );
    Ok(())
}

pub fn identity(manifest: &plist::Value, board: u64, chip: u64) -> Result<&plist::Value> {
    manifest.get("BuildIdentities").and_then(plist::Value::as_array).context("Missing build identities")?.iter()
        .find(|entry| number(entry.get("ApBoardID")) == Some(board) && number(entry.get("ApChipID")) == Some(chip))
        .context("Developer image has no build identity for this device; a Cryptex image may be required")
}
pub fn number(value: Option<&plist::Value>) -> Option<u64> {
    let value = value?;
    value.as_unsigned_integer().or_else(|| {
        value.as_string().and_then(|text| {
            if let Some(hex) = text.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()
            } else {
                text.parse().ok()
            }
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_are_validated_and_have_separate_caches() {
        let root = Path::new("/cache");
        let revision = "1111111111111111111111111111111111111111";
        assert!(validate_revision(DEFAULT_REVISION).is_ok());
        assert!(validate_revision(revision).is_ok());
        for invalid in [
            "main",
            "../outside",
            "",
            "ABCDEF0123456789ABCDEF0123456789ABCDEF0123",
        ] {
            assert!(validate_revision(invalid).is_err());
        }
        assert_eq!(
            Kind::Personalized.directory(root, DEFAULT_REVISION),
            root.join("Personalized").join(DEFAULT_REVISION)
        );
        assert_eq!(
            Kind::Personalized.directory(root, revision),
            root.join("Personalized").join(revision)
        );
        assert_ne!(
            Kind::Personalized.directory(root, DEFAULT_REVISION),
            Kind::Personalized.directory(root, revision)
        );
        assert_ne!(
            Kind::Personalized.directory(root, revision),
            Kind::Cryptex.directory(root, revision)
        );
    }

    #[tokio::test]
    async fn custom_revision_accepts_new_build_and_still_rejects_corrupt_payloads() {
        let root = std::env::temp_dir().join(format!("ddi-bundle-test-{}", uuid::Uuid::new_v4()));
        let _guard = crate::utils::TempDirGuard::new(root.clone());
        let revision = "1111111111111111111111111111111111111111";
        let directory = Kind::Personalized.directory(&root, revision);
        fs::create_dir_all(&directory).await.unwrap();
        let mut components = plist::Dictionary::new();
        for (component, name) in Kind::Personalized.payloads() {
            let bytes = name.as_bytes();
            fs::write(directory.join(name), bytes).await.unwrap();
            let mut digest = plist::Dictionary::new();
            digest.insert(
                "Digest".into(),
                plist::Value::Data(Sha384::digest(bytes).to_vec()),
            );
            components.insert((*component).into(), plist::Value::Dictionary(digest));
        }
        let mut identity = plist::Dictionary::new();
        identity.insert("Manifest".into(), plist::Value::Dictionary(components));
        let mut manifest = plist::Dictionary::new();
        manifest.insert(
            "ProductBuildVersion".into(),
            plist::Value::String("FutureBuild".into()),
        );
        manifest.insert(
            "BuildIdentities".into(),
            plist::Value::Array(vec![plist::Value::Dictionary(identity)]),
        );
        plist::to_file_xml(directory.join("BuildManifest.plist"), &manifest).unwrap();
        assert!(
            Bundle::open(Kind::Personalized, &root, revision)
                .await
                .is_ok()
        );
        assert!(
            Bundle::open(Kind::Personalized, &root, DEFAULT_REVISION)
                .await
                .is_err()
        );
        fs::write(directory.join("Image.dmg"), b"corrupted")
            .await
            .unwrap();
        let error = Bundle::open(Kind::Personalized, &root, revision)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("digest mismatch"));
    }

    #[test]
    fn identity_matches_both_chip_and_board() {
        let manifest = plist::Value::from_reader_xml(br#"<plist version="1.0"><dict><key>BuildIdentities</key><array><dict><key>ApBoardID</key><string>0x02</string><key>ApChipID</key><string>0x8103</string></dict></array></dict></plist>"#.as_slice()).unwrap();
        assert!(identity(&manifest, 2, 0x8103).is_ok());
        assert!(identity(&manifest, 3, 0x8103).is_err());
        assert!(identity(&manifest, 2, 0x8104).is_err());
    }
}
