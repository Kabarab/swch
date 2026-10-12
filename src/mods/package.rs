use super::types::*;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read},
    path::Path,
};

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn read_manifest(dir: &Path, platform: Option<&str>) -> Result<Manifest> {
    let bytes = fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?;
    if bytes.len() > 16384 {
        return Err("Manifest too large".into());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    manifest.validate(platform)?;
    for entry in [&manifest.backend, &manifest.ui, &manifest.theme]
        .into_iter()
        .flatten()
    {
        let p = dir.join(safe_relative(entry)?);
        if !p.is_file()
            || !p
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(dir.canonicalize().map_err(|e| e.to_string())?)
        {
            return Err("Entry point missing or outside package".into());
        }
    }
    if let Some(p) = &manifest.theme {
        let theme: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(p)).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        theme_tokens(&theme)?;
    }
    Ok(manifest)
}

/// Never execute package code while inspecting/extracting it.
pub fn extract(
    bytes: &[u8],
    expected_hash: &str,
    destination: &Path,
    platform: Option<&str>,
) -> Result<Manifest> {
    if bytes.len() as u64 > MAX_PACKAGE || digest(bytes) != expected_hash.to_ascii_lowercase() {
        return Err("Package size or SHA-256 mismatch".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > 10000 {
        return Err("Too many package entries".into());
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    // Validate every entry before writing anything.
    for i in 0..zip.len() {
        let file = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().trim_end_matches('/');
        safe_relative(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err("Duplicate package path".into());
        }
        if name.to_ascii_lowercase().ends_with(".node")
            || file.unix_mode().is_some_and(|m| {
                m & 0o170000 != 0 && m & 0o170000 != 0o100000 && m & 0o170000 != 0o040000
            })
        {
            return Err("Links and native addons are not supported".into());
        }
        total = total
            .checked_add(file.size())
            .ok_or("Package size overflow")?;
        if total > MAX_EXPANDED {
            return Err("Expanded package too large".into());
        }
    }
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = destination.join(safe_relative(file.name().trim_end_matches('/'))?);
        if file.is_dir() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        (&mut file)
            .take(MAX_EXPANDED + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() as u64 != file.size() {
            return Err("Invalid expanded size".into());
        }
        fs::write(&path, data).map_err(|e| e.to_string())?;
    }
    read_manifest(destination, platform)
}
