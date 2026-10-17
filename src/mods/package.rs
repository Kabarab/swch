use super::types::*;
use sha2::{Digest, Sha256};
use futures_util::{Stream, StreamExt};
use std::{
    collections::HashSet,
    fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::Path,
};
use tokio::io::AsyncWriteExt;

/// Spool a download/upload and hash each chunk without retaining the archive.
pub async fn spool<S, B, E>(stream: S, directory: &Path) -> Result<(tempfile::NamedTempFile, String)>
where
    S: Stream<Item = std::result::Result<B, E>>,
    B: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let limits = PackageLimits::from_env()?;
    tokio::fs::create_dir_all(directory).await.map_err(|e| e.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    let mut output = tokio::fs::File::from_std(temporary.reopen().map_err(|e| e.to_string())?);
    let mut hash = Sha256::new();
    let mut total = 0u64;
    futures_util::pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        total = total.checked_add(chunk.as_ref().len() as u64).ok_or("Package size overflow")?;
        if limits.zip_bytes.is_some_and(|max| total > max) { return Err("ZIP exceeds configured size limit".into()); }
        output.write_all(chunk.as_ref()).await.map_err(|e| e.to_string())?;
        hash.update(chunk.as_ref());
    }
    output.flush().await.map_err(|e| e.to_string())?;
    drop(output);
    Ok((temporary, format!("{:x}", hash.finalize())))
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn digest_reader(reader: &mut impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(format!("{:x}", hash.finalize()));
        }
        hash.update(&buffer[..count]);
    }
}

pub fn read_manifest(dir: &Path, platform: Option<&str>) -> Result<Manifest> {
    let bytes = fs::read(dir.join("manifest.json")).map_err(|e| e.to_string())?;
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
    let root = dir.canonicalize().map_err(|e| e.to_string())?;
    for tool in &manifest.native_tools {
        for file in &tool.files {
            let path = dir.join(safe_relative(&file.path)?).canonicalize().map_err(|e| e.to_string())?;
            if !path.starts_with(&root) || !path.is_file() { return Err("Native tool file missing or outside package".into()); }
            let mut input = std::io::BufReader::new(fs::File::open(path).map_err(|e| e.to_string())?);
            if digest_reader(&mut input)? != file.sha256.to_ascii_lowercase() { return Err("Native tool SHA-256 mismatch".into()); }
        }
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
    extract_reader(Cursor::new(bytes), expected_hash, destination, platform)
}

/// Inspect and extract ZIP/ZIP64 using bounded working memory, including individual
/// files larger than 4 GiB. The caller owns the staging directory on failure.
pub fn extract_reader<R: Read + Seek>(
    reader: R,
    expected_hash: &str,
    destination: &Path,
    platform: Option<&str>,
) -> Result<Manifest> {
    extract_reader_with_limits(reader, expected_hash, destination, platform, PackageLimits::from_env()?)
}

pub fn extract_reader_with_limits<R: Read + Seek>(
    mut reader: R,
    expected_hash: &str,
    destination: &Path,
    platform: Option<&str>,
    limits: PackageLimits,
) -> Result<Manifest> {
    let size = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    if limits.zip_bytes.is_some_and(|max| size > max) { return Err("ZIP exceeds configured size limit".into()); }
    reader.rewind().map_err(|e| e.to_string())?;
    if digest_reader(&mut reader)? != expected_hash.to_ascii_lowercase() {
        return Err("Package SHA-256 mismatch".into());
    }
    reader.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| e.to_string())?;
    if limits.files.is_some_and(|max| zip.len() as u64 > max) { return Err("Package exceeds configured entry limit".into()); }
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
        if name.to_ascii_lowercase().ends_with(".node") || file.unix_mode().is_some_and(|m| {
                m & 0o170000 != 0 && m & 0o170000 != 0o100000 && m & 0o170000 != 0o040000
            })
        {
            return Err("Links, special files and native Node addons are not supported".into());
        }
        total = total.checked_add(file.size()).ok_or("Package size overflow")?;
        if limits.expanded_bytes.is_some_and(|max| total > max) { return Err("Package exceeds configured expanded size limit".into()); }
    }
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = destination.join(safe_relative(file.name().trim_end_matches('/'))?);
        if file.is_dir() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut output = fs::File::create(&path).map_err(|e| e.to_string())?;
        let expected_size=file.size();
        let copied = io::copy(&mut (&mut file).take(expected_size.saturating_add(1)), &mut output).map_err(|e| e.to_string())?;
        if copied != file.size() {
            return Err("Invalid expanded size".into());
        }
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            output.set_permissions(fs::Permissions::from_mode(mode & 0o777))
                .map_err(|e| e.to_string())?;
        }
    }
    read_manifest(destination, platform)
}
