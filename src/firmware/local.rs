// SPDX-FileCopyrightText: 2026 Uncore <https://github.com/uncor3>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::manager::{FirmwareInfo, verification_sidecar};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, Default)]
pub(super) struct LocalFile {
    pub path: PathBuf,
    pub version: String,
    pub build: String,
    pub identifiers: Vec<String>,
    pub bytes: u64,
    pub partial: bool,
    pub error: String,
    pub verified: bool,
    pub modified_ns: u128,
}

#[derive(Serialize, Deserialize)]
struct VerificationRecord {
    bytes: u64,
    modified_ns: u128,
    digest: String,
    version: String,
    build: String,
}

fn record_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.json", verification_sidecar(path).display()))
}

fn modified_ns(path: &Path) -> Result<u128> {
    Ok(fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_nanos())
}

pub(super) fn write_record(path: &Path, firmware: &FirmwareInfo) -> Result<()> {
    let (_, digest) = firmware
        .expected_digest()
        .context("missing firmware checksum")?;
    let record = VerificationRecord {
        bytes: fs::metadata(path)?.len(),
        modified_ns: modified_ns(path)?,
        digest: digest.to_ascii_lowercase(),
        version: firmware.version.clone(),
        build: firmware.build_id.clone(),
    };
    fs::write(record_path(path), serde_json::to_vec(&record)?)?;
    Ok(())
}

// Use the same saved-checksum approval for catalog rows and discovered IPSWs.
// This checks proof of a previous verification; it does not hash the file again.
pub(super) fn catalog_approves(path: &Path, firmware: &FirmwareInfo) -> bool {
    let Some((_, expected)) = firmware.expected_digest() else {
        return false;
    };
    fs::metadata(path).ok().is_some_and(|metadata| {
        metadata.is_file() && (firmware.file_size == 0 || metadata.len() == firmware.file_size)
    }) && fs::read_to_string(verification_sidecar(path))
        .ok()
        .is_some_and(|actual| actual.trim().eq_ignore_ascii_case(expected))
}

pub(super) fn record_matches(file: &LocalFile) -> bool {
    let check = || -> Result<bool> {
        let record: VerificationRecord =
            serde_json::from_slice(&fs::read(record_path(&file.path))?)?;
        let digest = fs::read_to_string(verification_sidecar(&file.path))?;
        Ok(record.bytes == file.bytes
            && record.modified_ns == modified_ns(&file.path)?
            && record.version == file.version
            && record.build == file.build
            && record.digest.eq_ignore_ascii_case(digest.trim()))
    };
    !file.partial && file.error.is_empty() && check().unwrap_or(false)
}

pub(super) fn remove_verification(path: &Path) {
    for marker in [verification_sidecar(path), record_path(path)] {
        if let Err(error) = fs::remove_file(&marker) {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Could not remove verification marker {}: {error}",
                    marker.display()
                );
            }
        }
    }
}

// A partial ZIP normally has no central directory yet. Only our canonical
// download names can identify a resumable partial file; validate against the
// catalog again before using it to resume a network download.
fn partial_identity(path: &Path) -> Option<(String, String, String)> {
    let stem = path
        .file_name()?
        .to_str()?
        .strip_suffix("_Restore.ipsw.part")?;
    let mut parts = stem.rsplitn(3, '_');
    let build = parts.next()?.to_owned();
    let version = parts.next()?.to_owned();
    let (family, model) = parts.next()?.rsplit_once('_')?;
    Some((format!("{family},{model}"), version, build))
}

pub(super) fn read_manifest(path: &Path) -> Result<(String, String, Vec<String>)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path)?)?;
    let manifest = zip
        .by_name("BuildManifest.plist")
        .context("IPSW has no BuildManifest.plist")?;
    const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
    ensure!(
        manifest.size() <= MAX_MANIFEST_BYTES,
        "IPSW build manifest is too large"
    );
    let mut bytes = Vec::new();
    manifest
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "IPSW build manifest is too large"
    );
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes))?;
    let dict = value
        .as_dictionary()
        .context("invalid IPSW build manifest")?;
    let text = |key| {
        dict.get(key)
            .and_then(plist::Value::as_string)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .with_context(|| format!("IPSW manifest is missing {key}"))
    };
    let mut identifiers: Vec<String> = dict
        .get("SupportedProductTypes")
        .and_then(plist::Value::as_array)
        .context("IPSW has no supported product types")?
        .iter()
        .filter_map(plist::Value::as_string)
        .map(str::to_owned)
        .collect();
    identifiers.sort();
    identifiers.dedup();
    ensure!(
        !identifiers.is_empty(),
        "IPSW has no supported product types"
    );
    Ok((
        text("ProductVersion")?,
        text("ProductBuildVersion")?,
        identifiers,
    ))
}

pub(super) fn scan(
    directory: &Path,
    cached: &std::collections::HashMap<PathBuf, LocalFile>,
) -> Result<Vec<LocalFile>> {
    ensure!(
        directory.is_dir(),
        "download directory does not exist or is not a directory"
    );
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).context("could not read download directory")? {
        let entry = entry?;
        // Do not follow links to archives outside the chosen directory.
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let partial = name.ends_with(".ipsw.part");
        if !partial && !name.ends_with(".ipsw") {
            continue;
        }
        let bytes = entry.metadata()?.len();
        let stamp = modified_ns(&path)?;
        if let Some(previous) = cached
            .get(&path)
            .filter(|file| file.bytes == bytes && file.modified_ns == stamp)
        {
            let mut file = previous.clone();
            file.verified = record_matches(&file);
            files.push(file);
            continue;
        }
        let mut file = LocalFile {
            bytes,
            modified_ns: stamp,
            path,
            partial,
            ..Default::default()
        };
        if partial {
            if let Some((identifier, version, build)) = partial_identity(&file.path) {
                file.identifiers = vec![identifier];
                file.version = version;
                file.build = build;
            }
        } else {
            match read_manifest(&file.path) {
                Ok((version, build, identifiers)) => {
                    file.version = version;
                    file.build = build;
                    file.identifiers = identifiers;
                }
                Err(error) => file.error = format!("{error:#}"),
            }
        }
        file.verified = record_matches(&file);
        files.push(file);
    }
    files.sort_by(|a, b| {
        a.partial
            .cmp(&b.partial)
            .then_with(|| b.version.cmp(&a.version))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(files)
}

pub(super) fn matching_firmware<'a>(
    file: &LocalFile,
    catalog: &'a [FirmwareInfo],
) -> Result<&'a FirmwareInfo> {
    let firmware = catalog
        .iter()
        .find(|firmware| {
            firmware.build_id == file.build
                && firmware.version == file.version
                && file.identifiers.contains(&firmware.identifier)
        })
        .context("this firmware build was not found in the catalog")?;
    if file.partial {
        let expected = format!("{}.part", firmware.file_name());
        ensure!(
            file.path
                .file_name()
                .is_some_and(|name| name == expected.as_str()),
            "partial file name does not match the firmware catalog"
        );
    } else if firmware.file_size > 0 && firmware.file_size != file.bytes {
        bail!("file size does not match the firmware catalog");
    }
    Ok(firmware)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        io::Write,
        time::{Duration, SystemTime},
    };

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "idescriptor-local-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn archive(path: &Path) {
        let mut manifest = plist::Dictionary::new();
        manifest.insert("ProductVersion".into(), "15.8.3".into());
        manifest.insert("ProductBuildVersion".into(), "19H386".into());
        manifest.insert(
            "SupportedProductTypes".into(),
            plist::Value::Array(vec![
                "iPhone8,2".into(),
                "iPhone8,1".into(),
                "iPhone8,1".into(),
            ]),
        );
        let mut bytes = Vec::new();
        plist::Value::Dictionary(manifest)
            .to_writer_xml(&mut bytes)
            .unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        zip.start_file(
            "BuildManifest.plist",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&bytes).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn finds_renamed_multimodel_archives_and_keeps_invalid_files_visible() {
        let dir = Directory::new();
        archive(&dir.0.join("renamed firmware.ipsw"));
        fs::write(dir.0.join("broken.ipsw"), b"not a zip").unwrap();
        fs::write(dir.0.join("unrelated.txt"), b"keep").unwrap();
        let files = scan(&dir.0, &HashMap::new()).unwrap();
        assert_eq!(files.len(), 2);
        let valid = files.iter().find(|file| file.error.is_empty()).unwrap();
        assert_eq!(valid.version, "15.8.3");
        assert_eq!(valid.build, "19H386");
        assert_eq!(valid.identifiers, ["iPhone8,1", "iPhone8,2"]);
        assert!(!valid.verified);
        assert!(files.iter().any(|file| !file.error.is_empty()));
    }

    #[test]
    fn partials_require_catalog_matching_names_to_resume() {
        let dir = Directory::new();
        let firmware = FirmwareInfo {
            identifier: "iPhone8,1".into(),
            version: "15.8.3".into(),
            build_id: "19H386".into(),
            ..Default::default()
        };
        fs::write(
            dir.0.join(format!("{}.part", firmware.file_name())),
            b"partial",
        )
        .unwrap();
        fs::write(dir.0.join("unknown.ipsw.part"), b"partial").unwrap();
        let files = scan(&dir.0, &HashMap::new()).unwrap();
        let known = files
            .iter()
            .find(|file| !file.identifiers.is_empty())
            .unwrap();
        assert_eq!(known.identifiers, ["iPhone8,1"]);
        assert!(matching_firmware(known, &[firmware.clone()]).is_ok());
        let mut renamed = known.clone();
        renamed.path = dir.0.join("renamed.ipsw.part");
        assert!(matching_firmware(&renamed, &[firmware]).is_err());
        assert!(
            files
                .iter()
                .any(|file| file.identifiers.is_empty() && file.partial)
        );
    }

    #[test]
    fn catalog_approval_accepts_legacy_marker_without_json() {
        let dir = Directory::new();
        let path = dir.0.join("firmware.ipsw");
        archive(&path);
        let mut firmware = FirmwareInfo {
            file_size: fs::metadata(&path).unwrap().len(),
            sha256: "ab".repeat(32),
            ..Default::default()
        };
        assert!(!catalog_approves(&path, &firmware));
        fs::write(
            verification_sidecar(&path),
            format!("{}\n", firmware.sha256.to_uppercase()),
        )
        .unwrap();
        assert!(!record_path(&path).exists());
        assert!(catalog_approves(&path, &firmware));
        firmware.file_size += 1;
        assert!(!catalog_approves(&path, &firmware));
        firmware.file_size -= 1;
        firmware.sha256 = "cd".repeat(32);
        assert!(!catalog_approves(&path, &firmware));
    }

    #[test]
    fn verification_record_detects_changed_files_and_survives_a_new_scan() {
        let dir = Directory::new();
        let path = dir.0.join("firmware.ipsw");
        archive(&path);
        let firmware = FirmwareInfo {
            version: "15.8.3".into(),
            build_id: "19H386".into(),
            sha256: "ab".repeat(32),
            ..Default::default()
        };
        fs::write(verification_sidecar(&path), &firmware.sha256).unwrap();
        write_record(&path, &firmware).unwrap();
        let files = scan(&dir.0, &HashMap::new()).unwrap();
        assert!(files[0].verified);
        let cached = files
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        let old = fs::metadata(&path).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old + Duration::from_secs(2)))
            .unwrap();
        let rescanned = scan(&dir.0, &cached).unwrap();
        assert!(!rescanned[0].verified);
        assert_eq!(rescanned[0].version, "15.8.3");
    }

    #[test]
    fn size_mismatch_and_missing_directory_are_errors() {
        let dir = Directory::new();
        assert!(scan(&dir.0.join("missing"), &HashMap::new()).is_err());
        let file = LocalFile {
            version: "15.8.3".into(),
            build: "19H386".into(),
            identifiers: vec!["iPhone8,1".into()],
            bytes: 3,
            ..Default::default()
        };
        let firmware = FirmwareInfo {
            identifier: "iPhone8,1".into(),
            version: file.version.clone(),
            build_id: file.build.clone(),
            file_size: 30,
            ..Default::default()
        };
        assert!(matching_firmware(&file, &[firmware]).is_err());
    }
}
