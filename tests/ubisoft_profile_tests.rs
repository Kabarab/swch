use serde_json::json;
use std::fs;
use swch::launchers::ubisoft::{
    profile::{cached_name, name_from_cache},
    session::SessionManager,
};
use tempfile::tempdir;

const A: &str = "12345678-1111-2222-3333-123456789abc";
const B: &str = "87654321-1111-2222-3333-123456789abc";

#[test]
fn cached_name_requires_matching_identity_and_ubisoft_platform() {
    let records = [
        json!({"userId":B,"profileId":B,"platformType":"uplay","nameOnPlatform":"Other"}),
        json!({"userId":A,"platformType":"xbl","nameOnPlatform":"XboxTag"}),
        json!({"userId":A,"profileId":A,"platformType":"uplay","nameOnPlatform":"UbiPlayer","ticket":"private"}),
    ];
    let mut bytes = vec![0xff, 0, 0x01];
    for record in records {
        bytes.extend(record.to_string().as_bytes());
        bytes.extend([0, 0xff]);
    }
    assert_eq!(
        name_from_cache(&bytes, &format!("ubisoft_{A}")),
        Some("UbiPlayer".into())
    );
    assert_eq!(name_from_cache(&bytes, B), Some("Other".into()));
    assert_eq!(name_from_cache(&bytes, "../unsafe"), None);
}

#[test]
fn unrelated_nearby_names_and_secrets_cannot_become_the_name() {
    for record in [
        json!({"userId":A,"friends":[{"userId":B,"nameOnPlatform":"Friend"}]}),
        json!({"userId":B,"nameOnPlatform":"Wrong","another":{"userId":A}}),
        json!({"userId":A,"nameOnPlatform":"email@example.com"}),
        json!({"userId":A,"nameOnPlatform":"bad\nname"}),
        json!({"userId":A,"nameOnPlatform":""}),
        json!({"userId":A,"ticket":"secret","password":"secret"}),
    ] {
        assert_eq!(name_from_cache(record.to_string().as_bytes(), A), None);
    }
    assert_eq!(name_from_cache(b"\"userId\":\"12345678-1111-2222-3333-123456789abc\",broken \"nameOnPlatform\":\"Wrong\"",A),None);
}

#[test]
fn latest_matching_record_supplies_updated_name() {
    let bytes = format!(
        "{}\0{}",
        json!({"userId":A,"nameOnPlatform":"Old"}),
        json!({"userId":A,"nameOnPlatform":"New"})
    );
    assert_eq!(name_from_cache(bytes.as_bytes(), A), Some("New".into()));
}

#[test]
fn save_uses_detected_name_and_keeps_custom_label_on_refresh() {
    let dir = tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let cache = data_dir.join("cache/http2/Default/Local Storage/leveldb");
    fs::create_dir_all(&cache).unwrap();
    let client = dir.path().join("client");
    fs::create_dir_all(client.join("logs")).unwrap();
    fs::write(client.join("logs/launcher_log.txt"), format!("User: {A}\n")).unwrap();
    fs::write(data_dir.join("user.dat"), "native-login").unwrap();
    fs::write(cache.join("000001.log"),json!({"userId":A,"profileId":A,"nameOnPlatform":"RealNickname","platformType":"uplay","ticket":"never-return-this"}).to_string()).unwrap();
    let manager = SessionManager {
        data_dir,
        launcher_dir: Some(client),
        sessions_dir: dir.path().join("sessions"),
    };
    assert_eq!(
        cached_name(&manager.data_dir, A),
        Some("RealNickname".into())
    );
    let account = manager.save_current(None).unwrap();
    assert_eq!(account.display_name.as_deref(), Some("RealNickname"));
    assert_eq!(account.username, "RealNickname");
    assert!(!serde_json::to_string(&account)
        .unwrap()
        .contains("never-return-this"));
    manager.save_current(Some("My custom label")).unwrap();
    assert_eq!(
        manager.save_current(None).unwrap().display_name.as_deref(),
        Some("My custom label")
    );
}

#[test]
fn old_uuid_placeholder_is_replaced_when_name_becomes_available() {
    let dir = tempdir().unwrap();
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let client = dir.path().join("client");
    fs::create_dir_all(client.join("logs")).unwrap();
    fs::write(client.join("logs/launcher_log.txt"), format!("User: {A}\n")).unwrap();
    fs::write(data_dir.join("user.dat"), "native-login").unwrap();
    let manager = SessionManager {
        data_dir,
        launcher_dir: Some(client),
        sessions_dir: dir.path().join("sessions"),
    };
    assert_eq!(
        manager.save_current(None).unwrap().display_name.as_deref(),
        Some("Ubisoft 12345678")
    );
    let cache = manager
        .data_dir
        .join("cache/http2/Default/Local Storage/leveldb");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("000001.log"),
        json!({"userId":A,"nameOnPlatform":"NewName"}).to_string(),
    )
    .unwrap();
    let path = manager.sessions_dir.join(format!("{A}.json"));
    let before: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    // A delayed nickname must repair the saved label without saving live credentials.
    fs::write(manager.data_dir.join("user.dat"), "different-live-login").unwrap();
    manager.refresh_names().unwrap();
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(after["account"]["display_name"], "NewName");
    assert_eq!(after["account"]["username"], "NewName");
    assert_eq!(before["files"], after["files"]);
    assert_eq!(
        before["account"]["last_used"],
        after["account"]["last_used"]
    );
    assert_eq!(
        fs::read(manager.data_dir.join("user.dat")).unwrap(),
        b"different-live-login"
    );
}

fn vi(mut n: usize, out: &mut Vec<u8>) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}

// Minimal real SST layout: a Snappy-compressed data block, index and 48-byte footer.
fn compressed_table(value: &[u8]) -> Vec<u8> {
    let mut data = vec![0, 1];
    vi(value.len(), &mut data);
    data.push(b'k');
    data.extend_from_slice(value);
    data.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]); // One restart at offset zero.
    let mut compressed = Vec::new();
    vi(data.len(), &mut compressed);
    let len = data.len() - 1;
    compressed.push(61 << 2); // Two-byte extended Snappy literal length.
    compressed.extend_from_slice(&(len as u16).to_le_bytes());
    compressed.extend_from_slice(&data);
    let size = compressed.len();
    compressed.extend_from_slice(&[1, 0, 0, 0, 0]);
    let index_offset = compressed.len();
    let mut handle = vec![0];
    vi(size, &mut handle);
    let mut index = vec![0, 1];
    vi(handle.len(), &mut index);
    index.push(b'k');
    index.extend_from_slice(&handle);
    index.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]);
    let index_size = index.len();
    compressed.extend_from_slice(&index);
    compressed.extend_from_slice(&[0, 0, 0, 0, 0]);
    let mut footer = vec![0, 0];
    vi(index_offset, &mut footer);
    vi(index_size, &mut footer);
    footer.resize(40, 0);
    footer.extend_from_slice(&[0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]);
    compressed.extend_from_slice(&footer);
    compressed
}

#[test]
fn compressed_leveldb_and_utf16_profiles_supply_only_matching_names() {
    let record =
        json!({"userId":A,"nameOnPlatform":"Игрок_Ubi","platformType":"uplay"}).to_string();
    for utf16 in [false, true] {
        let value = if utf16 {
            let mut bytes = vec![0];
            for c in record.encode_utf16() {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
            bytes
        } else {
            record.as_bytes().to_vec()
        };
        let table = compressed_table(&value);
        assert_eq!(name_from_cache(&table, A), Some("Игрок_Ubi".into()));
        assert_eq!(name_from_cache(&table, B), None);
        assert_eq!(name_from_cache(&table[..table.len() - 10], B), None);
    }
}

#[test]
fn http_profile_response_repairs_generic_label_and_preserves_custom_label() {
    let dir = tempdir().unwrap();
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    let client = dir.path().join("client");
    fs::create_dir_all(client.join("logs")).unwrap();
    fs::write(client.join("logs/launcher_log.txt"), format!("User: {A}\n")).unwrap();
    fs::write(data_dir.join("user.dat"), "remembered-login").unwrap();
    let manager = SessionManager {
        data_dir,
        launcher_dir: Some(client),
        sessions_dir: dir.path().join("sessions"),
    };
    manager.save_current(Some("My label")).unwrap();
    let cache = manager
        .data_dir
        .join("cache/http2/Default/Cache/Cache_Data");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("data_2"),
        format!(
            "binary-header\0{}\0end",
            json!({"userId":A,"username":"Nickname","nameOnPlatform":"Nickname"})
        ),
    )
    .unwrap();
    manager.refresh_names().unwrap();
    let account = manager.accounts().unwrap().remove(0);
    assert_eq!(account.username, "Nickname");
    assert_eq!(account.display_name.as_deref(), Some("My label"));
}
