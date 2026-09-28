//! Names from the local Steam appinfo cache (v39–41). Unknown versions are ignored.
//! Layout: https://github.com/ValveResourceFormat/SteamAppInfo/blob/master/README.md
use std::collections::BTreeMap;
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let b = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(b)
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn text(&mut self) -> Option<String> {
        let n = self.bytes.get(self.pos..)?.iter().position(|b| *b == 0)?;
        let b = self.take(n)?;
        self.take(1)?;
        Some(String::from_utf8_lossy(b).into_owned())
    }
}
#[derive(Default, Debug, Clone)]
pub struct AppMetadata {
    pub name: Option<String>,
    pub is_free: bool,
    pub has_inventory: bool,
}
fn read_metadata(
    r: &mut Reader<'_>,
    strings: Option<&[String]>,
    path: &mut Vec<String>,
    meta: &mut AppMetadata,
) -> Option<()> {
    if path.len() > 32 {
        return None;
    }
    loop {
        let kind = r.take(1)?[0];
        if kind == 8 {
            return Some(());
        }
        let key = if let Some(strings) = strings {
            strings.get(r.u32()? as usize)?.clone()
        } else {
            r.text()?
        };
        if kind == 0 {
            path.push(key);
            read_metadata(r, strings, path, meta)?;
            path.pop();
            continue;
        }
        let value = match kind {
            1 => r.text()?,
            2 => r.u32()?.to_string(),
            3 | 4 | 6 => {
                r.take(4)?;
                String::new()
            }
            7 | 10 => {
                r.take(8)?;
                String::new()
            }
            _ => return None,
        };
        let parent = path.last().map(String::as_str);
        if parent == Some("common") && key == "name" {
            meta.name = Some(value.clone());
        }
        if (parent == Some("extended") && key == "isfreeapp")
            || (parent == Some("common") && key == "freetodownload")
        {
            meta.is_free |= value == "1";
        }
        if parent == Some("common") && key == "market_presence" {
            meta.has_inventory = value == "1";
        }
    }
}
pub fn metadata(bytes: &[u8]) -> BTreeMap<String, AppMetadata> {
    fn parse(bytes: &[u8]) -> Option<BTreeMap<String, AppMetadata>> {
        let mut r = Reader { bytes, pos: 0 };
        let magic = r.u32()?;
        r.u32()?;
        if ![0x07564427, 0x07564428, 0x07564429].contains(&magic) {
            return None;
        }
        let strings = if magic == 0x07564429 {
            let offset = usize::try_from(u64::from_le_bytes(r.take(8)?.try_into().ok()?)).ok()?;
            let mut table = Reader {
                bytes: bytes.get(offset..)?,
                pos: 0,
            };
            let count = table.u32()? as usize;
            if count > table.bytes.len() {
                return None;
            }
            let mut strings = Vec::new();
            for _ in 0..count {
                strings.push(table.text()?);
            }
            Some(strings)
        } else {
            None
        };
        let mut out = BTreeMap::new();
        loop {
            let id = r.u32()?;
            if id == 0 {
                break;
            }
            let size = r.u32()? as usize;
            let data = r.take(size)?;
            let header = if magic >= 0x07564428 { 60 } else { 40 };
            let Some(bytes) = data.get(header..) else {
                continue;
            };
            let mut meta = AppMetadata::default();
            if read_metadata(
                &mut Reader { bytes, pos: 0 },
                strings.as_deref(),
                &mut Vec::new(),
                &mut meta,
            )
            .is_some()
            {
                out.insert(id.to_string(), meta);
            }
        }
        Some(out)
    }
    parse(bytes).unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_v40_and_v41_names_and_handles_truncation() {
        for magic in [0x07564428u32, 0x07564429] {
            let mut kv = vec![];
            for (kind, key, index) in [(0u8, "appinfo", 0u32), (0, "common", 1), (1, "name", 2)] {
                kv.push(kind);
                if magic == 0x07564429 {
                    kv.extend(index.to_le_bytes());
                } else {
                    kv.extend(key.as_bytes());
                    kv.push(0);
                }
            }
            kv.extend(b"Portal 2\0\x08\x08\x08");
            let mut bytes = vec![];
            bytes.extend(magic.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            let offset = 16 + 8 + 60 + kv.len() + 4;
            if magic == 0x07564429 {
                bytes.extend((offset as u64).to_le_bytes());
            }
            bytes.extend(620u32.to_le_bytes());
            bytes.extend(((60 + kv.len()) as u32).to_le_bytes());
            bytes.extend([0u8; 60]);
            bytes.extend(kv);
            bytes.extend(0u32.to_le_bytes());
            if magic == 0x07564429 {
                bytes.extend(3u32.to_le_bytes());
                bytes.extend(b"appinfo\0common\0name\0");
            }
            assert_eq!(
                metadata(&bytes).get("620").and_then(|m| m.name.as_deref()),
                Some("Portal 2")
            );
            for end in 0..bytes.len() {
                let _ = metadata(&bytes[..end]);
            }
        }
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[test]
    fn reads_integer_free_and_market_flags_without_confusing_publisher_names() {
        let mut kv = b"\x00appinfo\0\x00common\0\x01name\0Game\0\x02market_presence\0".to_vec();
        kv.extend(1u32.to_le_bytes());
        kv.extend(b"\x00associations\0\x00publisher\0\x01name\0Publisher\0\x08\x08\x08\x00extended\0\x02isfreeapp\0");
        kv.extend(1u32.to_le_bytes());
        kv.extend(b"\x08\x08\x08");
        let mut data = Vec::new();
        data.extend(0x07564428u32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(730u32.to_le_bytes());
        data.extend((60u32 + kv.len() as u32).to_le_bytes());
        data.extend([0u8; 60]);
        data.extend(kv);
        data.extend(0u32.to_le_bytes());
        let all = metadata(&data);
        let app = all.get("730").unwrap();
        assert_eq!(app.name.as_deref(), Some("Game"));
        assert!(app.is_free);
        assert!(app.has_inventory);
    }
}
