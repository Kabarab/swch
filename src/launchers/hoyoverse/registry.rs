//! Safe, cross-platform registry and configuration store for HoYoverse / miHoYo games.
use super::HoyoverseError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RegistryEntry {
    Binary(Vec<u8>),
    String(String),
    DWord(u32),
    QWord(u64),
}

impl RegistryEntry {
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Binary(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::DWord(d) => Some(*d as u64),
            Self::QWord(q) => Some(*q),
            _ => None,
        }
    }
}

pub(crate) fn hidden_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

/// Returns the path to the mock registry file/dir if configured via environment.
pub fn mock_registry_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SWCH_HOYOVERSE_REGISTRY_FILE") {
        return Some(PathBuf::from(path));
    }
    if let Some(dir) = std::env::var_os("SWCH_HOYOVERSE_REGISTRY_DIR") {
        return Some(PathBuf::from(dir).join("mock_registry.json"));
    }
    if let Some(dir) = std::env::var_os("SWCH_HOYOVERSE_DATA_DIR") {
        return Some(PathBuf::from(dir).join("registry.json"));
    }
    #[cfg(not(windows))]
    {
        let root = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".swch"));
        Some(root.join("swch/hoyoverse/mock_registry.json"))
    }
    #[cfg(windows)]
    None
}

/// Reads the whole mock registry from disk.
pub fn load_mock_registry(path: &Path) -> BTreeMap<String, BTreeMap<String, RegistryEntry>> {
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(data) = serde_json::from_slice(&bytes) {
            return data;
        }
    }
    BTreeMap::new()
}

/// Saves the mock registry to disk atomically.
pub fn save_mock_registry(
    path: &Path,
    data: &BTreeMap<String, BTreeMap<String, RegistryEntry>>,
) -> Result<(), HoyoverseError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(data)?;
    crate::storage::atomic::atomic_write(path, bytes)?;
    Ok(())
}

/// Reads all values from a registry key (`HKCU` or `HKLM`).
pub fn read_key_values(
    hive: &str,
    subkey: &str,
) -> Result<BTreeMap<String, RegistryEntry>, HoyoverseError> {
    if let Some(mock_path) = mock_registry_path() {
        let store = load_mock_registry(&mock_path);
        let key_id = format!("{hive}\\{subkey}").to_lowercase();
        for (k, v) in store {
            if k.to_lowercase() == key_id {
                return Ok(v);
            }
        }
        return Ok(BTreeMap::new());
    }

    #[cfg(windows)]
    {
        let full_key = format!("{hive}\\{subkey}");
        let output = hidden_command("reg.exe")
            .args(["query", &full_key])
            .output()?;
        if !output.status.success() {
            return Ok(BTreeMap::new());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        return Ok(parse_reg_query_output(&text));
    }

    #[cfg(not(windows))]
    Ok(BTreeMap::new())
}

/// Reads a specific value from a registry key.
pub fn read_value(
    hive: &str,
    subkey: &str,
    value_name: &str,
) -> Result<Option<RegistryEntry>, HoyoverseError> {
    let mut values = read_key_values(hive, subkey)?;
    Ok(values.remove(value_name))
}

/// Writes a specific value into the registry key.
pub fn write_value(
    hive: &str,
    subkey: &str,
    value_name: &str,
    entry: &RegistryEntry,
) -> Result<(), HoyoverseError> {
    if let Some(mock_path) = mock_registry_path() {
        let mut store = load_mock_registry(&mock_path);
        let key_id = format!("{hive}\\{subkey}");
        store.entry(key_id).or_default().insert(value_name.to_string(), entry.clone());
        return save_mock_registry(&mock_path, &store);
    }

    #[cfg(windows)]
    {
        let full_key = format!("{hive}\\{subkey}");
        let mut cmd = hidden_command("reg.exe");
        cmd.arg("add").arg(&full_key).arg("/v").arg(value_name).arg("/f");

        match entry {
            RegistryEntry::Binary(bytes) => {
                let hex_str: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                cmd.arg("/t").arg("REG_BINARY").arg("/d").arg(if hex_str.is_empty() { "00" } else { &hex_str });
            }
            RegistryEntry::String(text) => {
                cmd.arg("/t").arg("REG_SZ").arg("/d").arg(text);
            }
            RegistryEntry::DWord(val) => {
                cmd.arg("/t").arg("REG_DWORD").arg("/d").arg(val.to_string());
            }
            RegistryEntry::QWord(val) => {
                cmd.arg("/t").arg("REG_QWORD").arg("/d").arg(val.to_string());
            }
        }

        let output = cmd.output()?;
        if !output.status.success() {
            return Err(HoyoverseError::Invalid(format!(
                "reg add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    Ok(())
}

/// Deletes a specific value from the registry.
pub fn delete_value(hive: &str, subkey: &str, value_name: &str) -> Result<(), HoyoverseError> {
    if let Some(mock_path) = mock_registry_path() {
        let mut store = load_mock_registry(&mock_path);
        let key_id = format!("{hive}\\{subkey}");
        if let Some(key_map) = store.get_mut(&key_id) {
            key_map.remove(value_name);
        }
        return save_mock_registry(&mock_path, &store);
    }

    #[cfg(windows)]
    {
        let full_key = format!("{hive}\\{subkey}");
        let _ = hidden_command("reg.exe")
            .args(["delete", &full_key, "/v", value_name, "/f"])
            .output();
    }

    Ok(())
}

/// Parses the output of `reg.exe query ...` into typed RegistryEntry items.
pub fn parse_reg_query_output(text: &str) -> BTreeMap<String, RegistryEntry> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("HKEY_") {
            continue;
        }

        // Standard format: <ValueName>    <REG_TYPE>    <Data>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        // Handle value name with possible spaces or types
        let (val_name, reg_type, data_slice) = if let Some((name, rest)) = split_reg_line(line) {
            let rest_trimmed = rest.trim_start();
            if let Some((rtype, data)) = rest_trimmed.split_once(char::is_whitespace) {
                (name.trim().to_string(), rtype.trim(), data.trim())
            } else {
                (name.trim().to_string(), rest_trimmed, "")
            }
        } else {
            continue;
        };

        match reg_type {
            "REG_BINARY" => {
                let clean_hex: String = data_slice.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                if let Ok(bytes) = hex_decode(&clean_hex) {
                    map.insert(val_name, RegistryEntry::Binary(bytes));
                }
            }
            "REG_SZ" | "REG_EXPAND_SZ" => {
                map.insert(val_name, RegistryEntry::String(data_slice.to_string()));
            }
            "REG_DWORD" => {
                let cleaned = data_slice.trim_start_matches("0x").trim_start_matches("0X");
                if let Ok(val) = u32::from_str_radix(cleaned, 16) {
                    map.insert(val_name, RegistryEntry::DWord(val));
                } else if let Ok(val) = data_slice.parse::<u32>() {
                    map.insert(val_name, RegistryEntry::DWord(val));
                }
            }
            "REG_QWORD" => {
                let cleaned = data_slice.trim_start_matches("0x").trim_start_matches("0X");
                if let Ok(val) = u64::from_str_radix(cleaned, 16) {
                    map.insert(val_name, RegistryEntry::QWord(val));
                } else if let Ok(val) = data_slice.parse::<u64>() {
                    map.insert(val_name, RegistryEntry::QWord(val));
                }
            }
            _ => {}
        }
    }
    map
}

fn split_reg_line(line: &str) -> Option<(&str, &str)> {
    // Look for REG_ markers
    for type_marker in [
        "REG_BINARY",
        "REG_SZ",
        "REG_DWORD",
        "REG_QWORD",
        "REG_MULTI_SZ",
        "REG_EXPAND_SZ",
    ] {
        if let Some(idx) = line.find(type_marker) {
            let name = &line[..idx];
            let rest = &line[idx..];
            return Some((name, rest));
        }
    }
    None
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}
