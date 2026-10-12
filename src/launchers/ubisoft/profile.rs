//! Read the public profile name from Ubisoft's own cached session JSON.
//! Only a record for the selected UUID can supply a name; credentials stay local.
use super::session::account_uuid;
use serde_json::Value;
use std::path::Path;

fn record_name(record: &Value, uuid: &str) -> Option<String> {
    let record = record.as_object()?;
    let id = record
        .get("userId")
        .or_else(|| record.get("profileId"))?
        .as_str()?;
    if account_uuid(id).ok().as_deref() != Some(uuid) {
        return None;
    }
    if record
        .get("platformType")
        .and_then(Value::as_str)
        .is_some_and(|p| p != "uplay" && p != "pc")
    {
        return None;
    }
    let name = record.get("nameOnPlatform")?.as_str()?.trim();
    if name.is_empty()
        || name.len() > 200
        || name.contains('@')
        || name.contains('\u{fffd}')
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(name.to_string())
}

/// LevelDB files include binary headers around UTF-8 JSON records. Parse complete
/// objects instead of borrowing a nearby name from a different user's record.
pub fn name_from_cache(bytes: &[u8], account_id: &str) -> Option<String> {
    if let Some(values) = super::cache::table_values(bytes) {
        return values.iter().rev().find_map(|value| {
            if value.first() == Some(&0) && value.len() % 2 == 1 {
                let chars: Vec<_> = value[1..]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                if let Ok(text) = String::from_utf16(&chars) {
                    return name_from_json(text.as_bytes(), account_id);
                }
            }
            name_from_json(value, account_id)
        });
    }
    name_from_json(bytes, account_id)
}

fn name_from_json(bytes: &[u8], account_id: &str) -> Option<String> {
    let uuid = account_uuid(account_id).ok()?;
    // String searches skip large unrelated binary/HTTP payloads efficiently.
    // Replaced binary header bytes cannot enter a name (record_name rejects them).
    let text = String::from_utf8_lossy(bytes);
    let mut found = None;
    for (at, _) in text.match_indices("\"nameOnPlatform\"") {
        // Start at enclosing object boundaries, never scan arbitrary nearby strings.
        let mut begin = at.saturating_sub(64 * 1024);
        while !text.is_char_boundary(begin) {
            begin += 1;
        }
        for (offset, _) in text[begin..at].rmatch_indices('{').take(256) {
            let start = begin + offset;
            let chunk = &text.as_bytes()[start..text.len().min(start + 64 * 1024)];
            if let Some(Ok(record)) = serde_json::Deserializer::from_slice(chunk)
                .into_iter::<Value>()
                .next()
            {
                if let Some(name) = record_name(&record, &uuid) {
                    found = Some(name);
                    break;
                }
            }
        }
    }
    found
}

pub fn cached_name(data_dir: &Path, account_id: &str) -> Option<String> {
    let mut files = Vec::new();
    for relative in [
        "cache/http2/Default/Local Storage/leveldb",
        "cache/http2/Local Storage/leveldb",
        "cache/http2/Default/Cache/Cache_Data",
        "cache/http2/Cache/Cache_Data",
    ] {
        if let Ok(entries) = std::fs::read_dir(data_dir.join(relative)) {
            for entry in entries.flatten().take(1024) {
                let path = entry.path();
                let http_block = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| matches!(s, "data_0" | "data_1" | "data_2" | "data_3"));
                if !http_block
                    && !matches!(
                        path.extension().and_then(|e| e.to_str()),
                        Some("log" | "ldb")
                    )
                {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() && meta.len() <= 16 * 1024 * 1024 {
                        files.push((meta.modified().ok(), meta.len(), path));
                    }
                }
            }
        }
    }
    // New Connect versions cache /users/me separately from the login session.
    // Inspect only the client's HTTP response files, within the same byte budget.
    let worker = data_dir.join("cache/http2/Default/Service Worker/CacheStorage");
    if let Ok(origins) = std::fs::read_dir(worker) {
        for origin in origins.flatten().take(32) {
            let Ok(caches) = std::fs::read_dir(origin.path()) else {
                continue;
            };
            for cache in caches.flatten().take(256) {
                let Ok(entries) = std::fs::read_dir(cache.path()) else {
                    continue;
                };
                for entry in entries.flatten().take(256) {
                    let path = entry.path();
                    if !path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with("_0"))
                    {
                        continue;
                    }
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() && meta.len() <= 2 * 1024 * 1024 {
                            files.push((meta.modified().ok(), meta.len(), path));
                        }
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.2.cmp(&a.2)));
    let mut total = 0;
    for (_, size, path) in files {
        total += size;
        if total > 64 * 1024 * 1024 {
            break;
        }
        if let Ok(bytes) = std::fs::read(path) {
            if let Some(name) = name_from_cache(&bytes, account_id) {
                return Some(name);
            }
        }
    }
    None
}
