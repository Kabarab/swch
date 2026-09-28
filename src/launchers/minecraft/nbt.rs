use crate::storage::models::MinecraftServer;

/// Parses a Minecraft servers.dat NBT file and extracts the server entries.
/// Returns an empty Vec if parsing fails or no servers are found.
pub fn parse_servers_dat(data: &[u8]) -> Vec<MinecraftServer> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut cursor = 0;

    // Check root compound tag (0x0A)
    if cursor >= data.len() || data[cursor] != 10 {
        return Vec::new();
    }
    cursor += 1;

    // Read root compound name (usually empty)
    if !skip_string(data, &mut cursor) {
        return Vec::new();
    }

    let mut servers = Vec::new();

    // Iterate tags in root compound until TAG_End (0) or EOF
    while cursor < data.len() {
        let tag_type = data[cursor];
        cursor += 1;
        if tag_type == 0 {
            break; // TAG_End
        }

        let name = match read_string(data, &mut cursor) {
            Some(n) => n,
            None => break,
        };

        if tag_type == 9 && name == "servers" {
            // Found TAG_List("servers")
            if cursor >= data.len() {
                break;
            }
            let item_type = data[cursor];
            cursor += 1;
            let count = match read_i32(data, &mut cursor) {
                Some(c) if c > 0 => c as usize,
                _ => 0,
            };

            if item_type == 10 {
                // List of TAG_Compound
                for _ in 0..count {
                    if let Some(srv) = read_server_compound(data, &mut cursor) {
                        servers.push(srv);
                    } else {
                        break;
                    }
                }
            } else {
                // Unknown list item type, skip
                for _ in 0..count {
                    if !skip_tag_payload(item_type, data, &mut cursor) {
                        break;
                    }
                }
            }
        } else {
            // Other tag in root, skip payload
            if !skip_tag_payload(tag_type, data, &mut cursor) {
                break;
            }
        }
    }

    servers
}

/// Reads a server entry compound from NBT data.
fn read_server_compound(data: &[u8], cursor: &mut usize) -> Option<MinecraftServer> {
    let mut name = None;
    let mut ip = None;
    let mut icon = None;

    while *cursor < data.len() {
        let tag_type = data[*cursor];
        *cursor += 1;
        if tag_type == 0 {
            // TAG_End of this server compound
            break;
        }

        let tag_name = read_string(data, cursor)?;

        match (tag_type, tag_name.as_str()) {
            (8, "name") => {
                name = read_string(data, cursor);
            }
            (8, "ip") => {
                ip = read_string(data, cursor);
            }
            (8, "icon") => {
                let mut raw_icon = read_string(data, cursor)?;
                if !raw_icon.starts_with("data:image/") {
                    raw_icon = format!("data:image/png;base64,{}", raw_icon);
                }
                icon = Some(raw_icon);
            }
            _ => {
                if !skip_tag_payload(tag_type, data, cursor) {
                    return None;
                }
            }
        }
    }

    let ip_str = ip?;
    let name_str = name.unwrap_or_else(|| ip_str.clone());
    let id = sanitize_server_id(&ip_str);

    Some(MinecraftServer {
        id,
        name: name_str,
        ip: ip_str,
        icon,
        is_favorite: false,
        is_imported: true,
        last_played: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Generates a clean identifier for a server based on its IP.
pub fn sanitize_server_id(ip: &str) -> String {
    let clean = ip
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' })
        .collect::<String>()
        .to_lowercase();
    format!("srv_{}", clean)
}

/// Writes a list of MinecraftServer instances into official NBT servers.dat binary format.
pub fn write_servers_dat(servers: &[MinecraftServer]) -> Vec<u8> {
    let mut out = Vec::new();

    // Root Compound
    out.push(10); // TAG_Compound
    write_nbt_string(&mut out, ""); // root compound name

    // TAG_List "servers"
    out.push(9); // TAG_List
    write_nbt_string(&mut out, "servers");
    out.push(10); // Item type: TAG_Compound
    out.extend_from_slice(&(servers.len() as i32).to_be_bytes());

    for s in servers {
        // Tag String "name"
        out.push(8);
        write_nbt_string(&mut out, "name");
        write_nbt_string(&mut out, &s.name);

        // Tag String "ip"
        out.push(8);
        write_nbt_string(&mut out, "ip");
        write_nbt_string(&mut out, &s.ip);

        // Tag String "icon" if present
        if let Some(ref ic) = s.icon {
            let clean_icon = if let Some(stripped) = ic.strip_prefix("data:image/png;base64,") {
                stripped
            } else if let Some(stripped) = ic.strip_prefix("data:image/jpeg;base64,") {
                stripped
            } else {
                ic.as_str()
            };
            out.push(8);
            write_nbt_string(&mut out, "icon");
            write_nbt_string(&mut out, clean_icon);
        }

        // Tag Byte "acceptTextures" = 1
        out.push(1);
        write_nbt_string(&mut out, "acceptTextures");
        out.push(1);

        // TAG_End for server compound
        out.push(0);
    }

    // Root TAG_End
    out.push(0);

    out
}

fn write_nbt_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len() as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

fn read_string(data: &[u8], cursor: &mut usize) -> Option<String> {
    if *cursor + 2 > data.len() {
        return None;
    }
    let len = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]) as usize;
    *cursor += 2;
    if *cursor + len > data.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&data[*cursor..*cursor + len]).to_string();
    *cursor += len;
    Some(s)
}

fn skip_string(data: &[u8], cursor: &mut usize) -> bool {
    if *cursor + 2 > data.len() {
        return false;
    }
    let len = u16::from_be_bytes([data[*cursor], data[*cursor + 1]]) as usize;
    *cursor += 2 + len;
    *cursor <= data.len()
}

fn read_i32(data: &[u8], cursor: &mut usize) -> Option<i32> {
    if *cursor + 4 > data.len() {
        return None;
    }
    let val = i32::from_be_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]);
    *cursor += 4;
    Some(val)
}

fn skip_tag_payload(tag_type: u8, data: &[u8], cursor: &mut usize) -> bool {
    match tag_type {
        0 => true, // TAG_End
        1 => { // Byte
            *cursor += 1;
            *cursor <= data.len()
        }
        2 => { // Short
            *cursor += 2;
            *cursor <= data.len()
        }
        3 => { // Int
            *cursor += 4;
            *cursor <= data.len()
        }
        4 => { // Long
            *cursor += 8;
            *cursor <= data.len()
        }
        5 => { // Float
            *cursor += 4;
            *cursor <= data.len()
        }
        6 => { // Double
            *cursor += 8;
            *cursor <= data.len()
        }
        7 => { // Byte Array
            let len = match read_i32(data, cursor) {
                Some(l) if l >= 0 => l as usize,
                _ => return false,
            };
            *cursor += len;
            *cursor <= data.len()
        }
        8 => skip_string(data, cursor),
        9 => { // List
            if *cursor >= data.len() {
                return false;
            }
            let elem_type = data[*cursor];
            *cursor += 1;
            let count = match read_i32(data, cursor) {
                Some(c) if c >= 0 => c as usize,
                _ => return false,
            };
            for _ in 0..count {
                if !skip_tag_payload(elem_type, data, cursor) {
                    return false;
                }
            }
            true
        }
        10 => { // Compound
            while *cursor < data.len() {
                let inner_tag = data[*cursor];
                *cursor += 1;
                if inner_tag == 0 {
                    return true;
                }
                if !skip_string(data, cursor) {
                    return false;
                }
                if !skip_tag_payload(inner_tag, data, cursor) {
                    return false;
                }
            }
            false
        }
        11 => { // Int Array
            let len = match read_i32(data, cursor) {
                Some(l) if l >= 0 => l as usize,
                _ => return false,
            };
            *cursor += len * 4;
            *cursor <= data.len()
        }
        12 => { // Long Array
            let len = match read_i32(data, cursor) {
                Some(l) if l >= 0 => l as usize,
                _ => return false,
            };
            *cursor += len * 8;
            *cursor <= data.len()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nbt_servers_roundtrip() {
        let servers = vec![
            MinecraftServer {
                id: "srv_mc.hypixel.net".to_string(),
                name: "Hypixel Network".to_string(),
                ip: "mc.hypixel.net".to_string(),
                icon: Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==".to_string()),
                is_favorite: true,
                is_imported: true,
                last_played: None,
                updated_at: "2026-09-25T00:00:00Z".to_string(),
            },
            MinecraftServer {
                id: "srv_2b2t.org".to_string(),
                name: "2b2t".to_string(),
                ip: "2b2t.org".to_string(),
                icon: None,
                is_favorite: false,
                is_imported: true,
                last_played: None,
                updated_at: "2026-09-25T00:00:00Z".to_string(),
            },
        ];

        let bytes = write_servers_dat(&servers);
        assert!(!bytes.is_empty());
        assert_eq!(bytes[0], 10); // TAG_Compound

        let parsed = parse_servers_dat(&bytes);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Hypixel Network");
        assert_eq!(parsed[0].ip, "mc.hypixel.net");
        assert!(parsed[0].icon.as_ref().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(parsed[1].name, "2b2t");
        assert_eq!(parsed[1].ip, "2b2t.org");
        assert!(parsed[1].icon.is_none());
    }

    #[test]
    fn test_nbt_corrupt_data() {
        assert!(parse_servers_dat(&[]).is_empty());
        assert!(parse_servers_dat(&[10, 0, 0, 0]).is_empty());
        assert!(parse_servers_dat(&[1, 2, 3]).is_empty());
    }
}
