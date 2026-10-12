//! Read-only recovery of the user's remembered Steam login, followed by a short
//! CM session to obtain a store cookie. The client refresh token is never saved
//! by SWCH, logged, returned by the API, or sent outside Valve's servers.
use super::steam_auth::SteamSession;
use crate::{
    launchers::steam::{library::valid_steam_id, SteamPaths},
    parsers::vdf::{VdfDocument, VdfTable},
    storage::Account,
};
use base64::Engine;
use prost::Message;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use steamroom::{
    client::{msg::ClientMsg, LoggedIn, SteamClient, PROTOCOL_VERSION},
    connection::{CmServer, CmServerAddr, Protocol},
    generated,
    messages::EMsg,
    transport::websocket::WebSocketTransport,
};

struct Credential {
    account_id: String,
    username: String,
    refresh: String,
}
pub struct ConnectedSession {
    pub session: SteamSession,
    // Hold the short CM connection until the store has verified the cookie.
    // Afterwards closing the socket leaves the independent store session valid.
    _client: SteamClient<LoggedIn>,
}

fn payload(token: &str) -> Option<Value> {
    if token.len() > 16384
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return None;
    }
    let data = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1)?)
        .ok()?;
    serde_json::from_slice(&data).ok()
}
fn validate_token(token: &str, id: &str, refresh: bool, now: i64) -> bool {
    payload(token).is_some_and(|p| {
        // Client refresh tokens use "steam". Access tokens are received over
        // an authenticated Valve CM connection and may use another issuer tag;
        // the store then verifies their signature and account independently.
        (!refresh || p["iss"].as_str() == Some("steam"))
            && p["sub"].as_str() == Some(id)
            && p["exp"].as_i64().is_some_and(|v| v > now + 60)
            && p["aud"].as_array().is_some_and(|a| {
                a.iter().any(|v| {
                    if refresh {
                        v.as_str() == Some("derive")
                    } else {
                        matches!(v.as_str(), Some("web" | "web:store"))
                    }
                }) && (!refresh || a.iter().any(|v| v.as_str() == Some("client")))
            })
    })
}
fn cache<'a>(root: &'a VdfTable) -> Option<&'a VdfTable> {
    let root = root
        .get_obj("MachineUserConfigStore")
        .or_else(|| root.get_obj("InstallConfigStore"))
        .unwrap_or(root);
    root.get_obj("Software")?
        .get_obj("Valve")?
        .get_obj("Steam")?
        .get_obj("ConnectCache")
}
fn cached_blobs(doc: &VdfDocument, username: &str) -> Vec<Vec<u8>> {
    let prefix = format!(
        "{:08x}",
        crc32fast::hash(username.to_ascii_lowercase().as_bytes())
    );
    cache(&doc.root)
        .into_iter()
        .flat_map(|v| v.iter())
        .filter(|(key, _)| key.to_ascii_lowercase().starts_with(&prefix))
        .filter_map(|(_, v)| hex(v.as_str()?))
        .collect()
}
fn hex(value: &str) -> Option<Vec<u8>> {
    if value.len() < 16 || value.len() > 131072 || value.len() % 2 != 0 {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some((high * 16 + low) as u8)
        })
        .collect()
}
fn account_name(steam: &Path, id: &str) -> Option<String> {
    if !valid_steam_id(id) {
        return None;
    }
    let users = VdfDocument::from_file(steam.join("config/loginusers.vdf")).ok()?;
    let name = users
        .get_obj("users")?
        .get_obj(id)?
        .get_str("AccountName")?
        .to_ascii_lowercase();
    if name.is_empty()
        || name.len() > 128
        || !name.bytes().all(|v| v.is_ascii_alphanumeric() || v == b'_')
    {
        return None;
    }
    Some(name)
}
fn local_files(steam: &Path) -> Vec<PathBuf> {
    let mut files = vec![];
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        files.push(local.join("Steam/local.vdf"));
    }
    files.extend([
        steam.join("local.vdf"),
        steam.join("config/local.vdf"),
        steam.join("config/config.vdf"),
    ]);
    files
}
pub fn available(account: &Account) -> bool {
    if account.launcher != "steam" {
        return false;
    }
    let Ok(paths) = SteamPaths::detect() else {
        return false;
    };
    available_at(
        &paths.steam_dir,
        &account.id,
        &local_files(&paths.steam_dir),
    )
}
pub fn accounts() -> Vec<Account> {
    let Ok(paths) = SteamPaths::detect() else {
        return vec![];
    };
    let Ok(doc) = VdfDocument::from_file(paths.loginusers_path()) else {
        return vec![];
    };
    let Some(users) = doc.get_obj("users") else {
        return vec![];
    };
    users
        .iter()
        .filter_map(|(id, value)| {
            if !valid_steam_id(id) {
                return None;
            }
            let user = value.as_obj()?;
            let username = user.get_str("AccountName")?.to_string();
            Some(Account {
                id: id.clone(),
                launcher: "steam".into(),
                username,
                display_name: user.get_str("PersonaName").map(str::to_string),
                email: None,
                is_active: user.get_str("MostRecent") == Some("1"),
                last_used: None,
            })
        })
        .collect()
}
pub fn merge_accounts(saved: Vec<Account>, local: Vec<Account>) -> Vec<Account> {
    let mut result = local.clone();
    for account in saved {
        if account.launcher == "steam"
            && local
                .iter()
                .any(|a| a.id == account.id || a.username.eq_ignore_ascii_case(&account.username))
        {
            continue;
        }
        if !result
            .iter()
            .any(|a| a.launcher == account.launcher && a.id == account.id)
        {
            result.push(account);
        }
    }
    result
}
fn available_at(steam: &Path, id: &str, files: &[PathBuf]) -> bool {
    let Some(name) = account_name(steam, id) else {
        return false;
    };
    files
        .iter()
        .any(|file| VdfDocument::from_file(file).is_ok_and(|v| !cached_blobs(&v, &name).is_empty()))
}
fn recover(account: &Account) -> Result<Credential, String> {
    let paths = SteamPaths::detect().map_err(|_| {
        "Steam не найден. Установи официальный клиент и войди с «Запомнить меня».".to_string()
    })?;
    let name = account_name(&paths.steam_dir, &account.id)
        .ok_or("Войди в этот аккаунт в официальном Steam с «Запомнить меня».")?;
    for file in local_files(&paths.steam_dir) {
        let Ok(doc) = VdfDocument::from_file(file) else {
            continue;
        };
        for blob in cached_blobs(&doc, &name) {
            if let Some(bytes) = decrypt(&blob, &name) {
                if let Ok(token) = String::from_utf8(bytes) {
                    let token = token.trim_end_matches('\0').to_string();
                    if validate_token(&token, &account.id, true, chrono::Utc::now().timestamp()) {
                        return Ok(Credential {
                            account_id: account.id.clone(),
                            username: name,
                            refresh: token,
                        });
                    }
                }
            }
        }
    }
    Err("В Steam нет действующего сохранённого входа для этого аккаунта. Войди в официальный Steam с «Запомнить меня»; SWCH подхватит вход автоматически.".into())
}
#[cfg(windows)]
fn decrypt(bytes: &[u8], username: &str) -> Option<Vec<u8>> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let input = Blob {
        len: bytes.len() as u32,
        data: bytes.as_ptr() as *mut u8,
    };
    let entropy = Blob {
        len: username.len() as u32,
        data: username.as_ptr() as *mut u8,
    };
    let mut output = Blob {
        len: 0,
        data: ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            &entropy,
            ptr::null_mut(),
            ptr::null_mut(),
            1,
            &mut output,
        )
    };
    if ok == 0 || output.data.is_null() {
        return None;
    }
    let plain = unsafe { std::slice::from_raw_parts(output.data, output.len as usize) }.to_vec();
    // Clear Windows' plaintext allocation before releasing it.
    unsafe {
        ptr::write_bytes(output.data, 0, output.len as usize);
        LocalFree(output.data.cast());
    }
    Some(plain)
}
#[cfg(not(windows))]
fn decrypt(bytes: &[u8], username: &str) -> Option<Vec<u8>> {
    use sha2::{Digest, Sha256};
    if bytes.len() < 32 {
        return None;
    }
    let key = Sha256::digest(username.as_bytes());
    let iv = steamroom::crypto::symmetric_decrypt_ecb_nopad(&bytes[..16], &key).ok()?;
    steamroom::crypto::symmetric_decrypt_cbc(&bytes[16..], &key, &iv).ok()
}
fn trusted_cm(server: &CmServer) -> bool {
    matches!(&server.addr,CmServerAddr::Dns{host,port} if *port==443 &&
        (host.ends_with(".steamserver.net") || host.ends_with(".steampowered.com")))
        && server.protocol == Protocol::WebSocket
}
fn logon(credential: &Credential, instance: u32) -> generated::CMsgClientLogon {
    generated::CMsgClientLogon {
        protocol_version: Some(PROTOCOL_VERSION),
        account_name: Some(credential.username.clone()),
        access_token: Some(credential.refresh.clone()),
        client_os_type: Some(16),
        client_language: Some("english".into()),
        should_remember_password: Some(true),
        client_supplied_steam_id: credential.account_id.parse().ok(),
        obfuscated_private_ip: Some(generated::CMsgIpAddress {
            ip: Some(generated::c_msg_ip_address::Ip::V4(instance)),
        }),
        // Each background connection has its own session ID and never announces a game.
        machine_name: Some("SWCH Giveaways".into()),
        supports_rate_limit_response: Some(true),
        ..Default::default()
    }
}
pub async fn connect(account: &Account) -> Result<ConnectedSession, String> {
    let account = account.clone();
    let credential = tokio::task::spawn_blocking(move || recover(&account))
        .await
        .map_err(|_| "Не удалось прочитать сохранённый вход Steam.".to_string())??;
    tokio::time::timeout(Duration::from_secs(40), connect_credential(credential))
        .await
        .map_err(|_| {
            "Steam не ответил на автоматическое подключение. Проверка будет повторена.".to_string()
        })?
}
async fn connect_credential(credential: Credential) -> Result<ConnectedSession, String> {
    let servers = CmServer::fetch()
        .await
        .map_err(|_| "Серверы Steam недоступны. Проверка будет повторена.".to_string())?;
    let mut random = [0u8; 4];
    getrandom::getrandom(&mut random)
        .map_err(|_| "Не удалось подготовить вход Steam.".to_string())?;
    let instance = u32::from_le_bytes(random) | 0x80000000;
    for server in servers.iter().filter(|s| trusted_cm(s)).take(3) {
        let transport =
            match tokio::time::timeout(Duration::from_secs(8), WebSocketTransport::connect(server))
                .await
            {
                Ok(Ok(v)) => v,
                _ => continue,
            };
        let (client, _) = SteamClient::connect_ws(transport)
            .await
            .map_err(|_| "Не удалось подключиться к Steam.".to_string())?;
        let client = client
            .prepare()
            .await
            .map_err(|_| "Не удалось подготовить соединение Steam.".to_string())?;
        let body = logon(&credential, instance).encode_to_vec();
        let mut message = ClientMsg::with_body(EMsg::CLIENT_LOGON, &body);
        message.header.steamid = credential.account_id.parse().ok();
        message.header.client_sessionid = Some(0);
        let (client,login)=client.login(message).await.map_err(|error| {
            use steamroom::{error::{Error,ConnectionError},enums::EResultError};
            if let Error::Connection(ConnectionError::LogonFailed(code))=&error {
                if std::env::var("SWCH_NATIVE_AUTH_DIAGNOSTICS").as_deref()==Ok("1") {eprintln!("Steam logon status: {code:?}");}
                if matches!(code,EResultError::RateLimitExceeded|EResultError::LoginDeniedThrottle|EResultError::Busy|EResultError::ServiceUnavailable) {
                    return "Steam временно ограничил подключение. Проверка будет повторена позже.".to_string();
                }
                return "Steam отклонил сохранённый вход. Войди в официальный клиент с «Запомнить меня».".to_string();
            }
            "Связь с Steam прервалась. Подключение будет повторено.".to_string()
        })?;
        if login.header.steamid.map(|v| v.to_string()).as_deref() != Some(&credential.account_id) {
            return Err("Steam подтвердил другой аккаунт; подключение отменено.".into());
        }
        let details = generated::CMsgClientLogonResponse::decode(&*login.body)
            .map_err(|_| "Не удалось проверить ответ входа Steam.".to_string())?;
        let country = details
            .user_country
            .map(|v| v.to_ascii_uppercase())
            .filter(|v| v.len() == 2 && v.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or("Steam не сообщил страну аккаунта; проверка будет повторена.")?;
        let request = generated::CAuthenticationAccessTokenGenerateForAppRequest {
            refresh_token: Some(credential.refresh.clone()),
            steamid: credential.account_id.parse().ok(),
            renewal_type: Some(0),
        };
        let response=client.call_service_method("Authentication.GenerateAccessTokenForApp#1",&request.encode_to_vec()).await
            .map_err(|_| "Steam не разрешил автоматический вход в магазин. Повтори вход в официальный клиент.".to_string())?;
        let response: generated::CAuthenticationAccessTokenGenerateForAppResponse = response
            .decode()
            .map_err(|_| "Steam вернул неизвестный формат сессии магазина.".to_string())?;
        let token = response
            .access_token
            .ok_or("Steam не выдал сессию магазина для этого аккаунта.")?;
        if !validate_token(
            &token,
            &credential.account_id,
            false,
            chrono::Utc::now().timestamp(),
        ) {
            if std::env::var("SWCH_NATIVE_AUTH_DIAGNOSTICS").as_deref() == Ok("1") {
                let p = payload(&token).unwrap_or(Value::Null);
                let aud = |name: &str| {
                    p["aud"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)))
                };
                eprintln!(
                    "{}",
                    serde_json::json!({"native_auth_metadata":{"jwt_parsed":!p.is_null(),"issuer_matches":p["iss"].as_str()==Some("steam"),"account_matches":p["sub"].as_str()==Some(&credential.account_id),"expires_in":p["exp"].as_i64().map(|v|v-chrono::Utc::now().timestamp()),"aud_client":aud("client"),"aud_web":aud("web"),"aud_store":aud("web:store"),"aud_community":aud("web:community")}})
                );
            }
            return Err("Steam не выдал действующую сессию магазина для этого аккаунта.".into());
        }
        let mut csrf = [0u8; 12];
        getrandom::getrandom(&mut csrf)
            .map_err(|_| "Не удалось подготовить сессию магазина Steam.".to_string())?;
        let session = SteamSession {
            account_id: credential.account_id.clone(),
            login_secure: format!("{}%7C%7C{}", credential.account_id, token),
            session_id: csrf.iter().map(|v| format!("{v:02x}")).collect(),
            country,
            refresh_token: None,
            auth_source: "native".into(),
        };
        return Ok(ConnectedSession {
            session,
            _client: client,
        });
    }
    Err("Не удалось подключиться к серверам Steam. Проверка будет повторена.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn legacy_database_names_map_to_current_steamids_without_duplicates() {
        let account = |id: &str, name: &str| Account {
            id: id.into(),
            launcher: "steam".into(),
            username: name.into(),
            display_name: None,
            email: None,
            is_active: false,
            last_used: None,
        };
        let saved = vec![
            account("steam_account_a", "ACCOUNT_A"),
            account("76561197960265730", "account_b"),
        ];
        let local = vec![
            account("76561197960265729", "account_a"),
            account("76561197960265730", "account_b"),
            account("76561197960265731", "new_account"),
        ];
        let merged = merge_accounts(saved, local);
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().all(|a| valid_steam_id(&a.id)));
        assert!(merged.iter().any(|a| a.username == "new_account"));
    }
    #[test]
    fn tokens_cannot_cross_accounts_or_expire_silently() {
        let token = |id: &str, aud: Value, exp: i64| {
            format!(
                "header.{}.signature",
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(json!({"iss":"steam","sub":id,"aud":aud,"exp":exp}).to_string())
            )
        };
        let id = "76561197960265729";
        assert!(validate_token(
            &token(id, json!(["derive", "client"]), 1000),
            id,
            true,
            1
        ));
        assert!(!validate_token(
            &token("76561197960265730", json!(["derive", "client"]), 1000),
            id,
            true,
            1
        ));
        assert!(!validate_token(
            &token(id, json!(["derive", "client"]), 1),
            id,
            true,
            1
        ));
        assert!(!validate_token(
            &token(id, json!(["web"]), 1000),
            id,
            true,
            1
        ));
        assert!(validate_token(
            &token(id, json!(["web"]), 1000),
            id,
            false,
            1
        ));
        assert!(validate_token(
            &token(id, json!(["client", "web:store"]), 1000),
            id,
            false,
            1
        ));
        assert!(!validate_token(
            &token(id, json!(["web:community"]), 1000),
            id,
            false,
            1
        ));
    }
    #[test]
    fn lookup_only_uses_the_requested_accounts_cache() {
        let a = format!("{:08x}", crc32fast::hash(b"account_a"));
        let b = format!("{:08x}", crc32fast::hash(b"account_b"));
        let doc=VdfDocument::parse(&format!("\"MachineUserConfigStore\" {{ \"Software\" {{ \"Valve\" {{ \"Steam\" {{ \"ConnectCache\" {{ \"{a}suffix\" \"0102030405060708\" \"{b}suffix\" \"090a0b0c0d0e0f10\" }} }} }} }} }}")).unwrap();
        assert_eq!(
            cached_blobs(&doc, "ACCOUNT_A"),
            vec![vec![1, 2, 3, 4, 5, 6, 7, 8]]
        );
        assert!(cached_blobs(&doc, "missing").is_empty());
        assert!(hex("secret").is_none());
    }
    #[test]
    fn background_logon_has_an_independent_session_and_no_password() {
        let credential = Credential {
            account_id: "76561197960265729".into(),
            username: "account".into(),
            refresh: "fixture-token".into(),
        };
        let request = logon(&credential, 0xf1234567);
        assert_eq!(
            request.obfuscated_private_ip.unwrap().ip,
            Some(generated::c_msg_ip_address::Ip::V4(0xf1234567))
        );
        assert!(request.password.is_none());
        assert!(request.two_factor_code.is_none());
        assert_eq!(request.should_remember_password, Some(true));
    }
    #[test]
    fn arbitrary_cm_hosts_are_rejected() {
        let server = |host: &str| CmServer {
            addr: CmServerAddr::Dns {
                host: host.into(),
                port: 443,
            },
            protocol: Protocol::WebSocket,
        };
        assert!(trusted_cm(&server("cmp1-fra1.steamserver.net")));
        assert!(!trusted_cm(&server("steamserver.net.evil.example")));
        assert!(!trusted_cm(&server("127.0.0.1")));
    }
}
