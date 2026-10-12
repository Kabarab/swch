//! Pass a validated desktop OAuth session to GALAXY without passwords or shell arguments.
use super::{local, valid_id, GogError};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LoginState {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub refresh_token: Option<String>,
}

pub trait GalaxyClient: Send + Sync {
    fn available(&self) -> bool;
    fn state(&self) -> Result<LoginState, GogError>;
    fn running(&self) -> bool;
    fn close(&self) -> Result<(), GogError>;
    fn write(&self, state: &LoginState) -> Result<(), GogError>;
    fn open(&self) -> Result<(), GogError>;
    fn backup(&self, state: &LoginState) -> Result<(), GogError>;
    fn timeout(&self) -> Duration {
        Duration::from_secs(40)
    }
    fn confirmed(&self, target: &str, injected: &str) -> Result<bool, GogError> {
        let state = self.state()?;
        Ok(state.user_id.as_deref() == Some(target)
            && state
                .refresh_token
                .as_deref()
                .is_some_and(|token| token != injected)
            && self.running())
    }
}

#[derive(Default)]
pub struct WindowsGalaxy {
    login_log_start: std::sync::Mutex<Option<(std::path::PathBuf, u64)>>,
}

const CLIENT_NAMES: &[&str] = &[
    "GalaxyClient.exe",
    "GalaxyClientService.exe",
    "GalaxyCommunication.exe",
    "GOG Galaxy Notifications Renderer.exe",
];

impl WindowsGalaxy {
    fn processes(&self, system: &sysinfo::System) -> Vec<sysinfo::Pid> {
        let Some(root) = local::launcher_dir() else {
            return Vec::new();
        };
        system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                (CLIENT_NAMES
                    .iter()
                    .any(|name| process.name().eq_ignore_ascii_case(name))
                    && process.exe().is_some_and(|exe| exe.starts_with(&root)))
                .then_some(*pid)
            })
            .collect()
    }
}

impl GalaxyClient for WindowsGalaxy {
    fn available(&self) -> bool {
        cfg!(windows) && local::launcher_dir().is_some()
    }
    fn state(&self) -> Result<LoginState, GogError> {
        registry::read()
    }
    fn running(&self) -> bool {
        let mut system = sysinfo::System::new();
        system.refresh_processes();
        self.processes(&system).iter().any(|pid| {
            system
                .process(*pid)
                .is_some_and(|p| p.name().eq_ignore_ascii_case("GalaxyClient.exe"))
        })
    }
    fn close(&self) -> Result<(), GogError> {
        let mut system = sysinfo::System::new();
        system.refresh_processes();
        let pids = self.processes(&system);
        let games = local::scan_games()?;
        if !pids.is_empty()
            && system.processes().values().any(|process| {
                process.exe().is_some_and(|exe| {
                    games.iter().any(|g| {
                        g.install_path
                            .as_ref()
                            .is_some_and(|dir| exe.starts_with(dir))
                    })
                })
            })
        {
            return Err(GogError::Invalid(
                "Заверши игру GOG перед переключением аккаунта.".into(),
            ));
        }
        crate::sys::process::terminate_pids_blocking(&mut system, &pids, &crate::sys::process::TerminateOptions { include_descendants:false,..Default::default() })
            .map_err(|_| GogError::Invalid("Не удалось закрыть GALAXY. Заверши клиент через значок в трее и повтори переключение.".into()))?;
        system.refresh_processes();
        if !self.processes(&system).is_empty() {
            return Err(GogError::Invalid(
                "GALAXY ещё работает. Повтори переключение после завершения клиента.".into(),
            ));
        }
        Ok(())
    }
    fn write(&self, state: &LoginState) -> Result<(), GogError> {
        registry::write(state)
    }
    fn open(&self) -> Result<(), GogError> {
        let root = std::env::var_os("ProgramData")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:/ProgramData".into());
        let log = root.join("GOG.com/Galaxy/logs/GalaxyClient.log");
        let size = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
        *self
            .login_log_start
            .lock()
            .map_err(|_| GogError::Response)? = Some((log, size));
        local::open_client(None)
    }
    fn confirmed(&self, target: &str, _: &str) -> Result<bool, GogError> {
        let state = self.state()?;
        if state.user_id.as_deref() != Some(target) || state.refresh_token.is_none() {
            return Ok(false);
        }
        let bookmark = self
            .login_log_start
            .lock()
            .map_err(|_| GogError::Response)?;
        let Some((path, start)) = bookmark.as_ref() else {
            return Ok(false);
        };
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(path) else {
            return Ok(false);
        };
        let size = file.metadata()?.len();
        let offset = if size >= *start { *start } else { 0 };
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        Ok(text
            .lines()
            .any(|line| line.contains(&format!("Authenticated user ID: {target}."))))
    }
    fn backup(&self, state: &LoginState) -> Result<(), GogError> {
        if state.refresh_token.is_none() {
            return Ok(());
        }
        let id = state
            .user_id
            .as_deref()
            .filter(|id| valid_id(id))
            .unwrap_or("previous");
        let json = serde_json::to_string(state).map_err(|_| GogError::Response)?;
        keyring::Entry::new("swch:gog:galaxy-backup", id)?.set_password(&json)?;
        Ok(())
    }
}

#[cfg(not(windows))]
mod registry {
    use super::*;
    pub fn read() -> Result<LoginState, GogError> {
        Ok(LoginState::default())
    }
    pub fn write(_: &LoginState) -> Result<(), GogError> {
        Err(GogError::Invalid(
            "Переключение GALAXY поддерживается на Windows.".into(),
        ))
    }
}

#[cfg(windows)]
mod registry {
    use super::*;
    use std::ffi::c_void;
    type Hkey = *mut c_void;
    const HKCU: Hkey = 0x80000001u32 as i32 as isize as Hkey;
    const ROOT: &str = "Software\\GOG.com\\Galaxy";

    #[link(name = "advapi32")]
    extern "system" {
        fn RegCreateKeyExW(
            key: Hkey,
            subkey: *const u16,
            reserved: u32,
            class: *mut u16,
            options: u32,
            access: u32,
            attributes: *const c_void,
            result: *mut Hkey,
            disposition: *mut u32,
        ) -> i32;
        fn RegOpenKeyExW(
            key: Hkey,
            subkey: *const u16,
            options: u32,
            access: u32,
            result: *mut Hkey,
        ) -> i32;
        fn RegQueryValueExW(
            key: Hkey,
            name: *const u16,
            reserved: *mut u32,
            kind: *mut u32,
            data: *mut u8,
            size: *mut u32,
        ) -> i32;
        fn RegSetValueExW(
            key: Hkey,
            name: *const u16,
            reserved: u32,
            kind: u32,
            data: *const u8,
            size: u32,
        ) -> i32;
        fn RegDeleteValueW(key: Hkey, name: *const u16) -> i32;
        fn RegCloseKey(key: Hkey) -> i32;
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    struct Key(Hkey);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn failure() -> GogError {
        GogError::Invalid("Не удалось обновить данные входа GALAXY в реестре Windows.".into())
    }
    fn key(path: &str, write: bool) -> Result<Option<Key>, GogError> {
        let mut handle = std::ptr::null_mut();
        let name = wide(path);
        let result = unsafe {
            if write {
                RegCreateKeyExW(
                    HKCU,
                    name.as_ptr(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    0x0003,
                    std::ptr::null(),
                    &mut handle,
                    std::ptr::null_mut(),
                )
            } else {
                RegOpenKeyExW(HKCU, name.as_ptr(), 0, 0x0001, &mut handle)
            }
        };
        match result {
            0 => Ok(Some(Key(handle))),
            2 if !write => Ok(None),
            _ => Err(failure()),
        }
    }
    fn get(path: &str, name: &str) -> Result<Option<String>, GogError> {
        let Some(key) = key(path, false)? else {
            return Ok(None);
        };
        let name = wide(name);
        let mut kind = 0;
        let mut size = 0;
        let result = unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if result == 2 {
            return Ok(None);
        }
        if result != 0 || kind != 1 || size > 16 * 1024 || size % 2 != 0 {
            return Err(failure());
        }
        let mut data = vec![0u16; (size as usize) / 2];
        if unsafe {
            RegQueryValueExW(
                key.0,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        } != 0
        {
            return Err(failure());
        }
        while data.last() == Some(&0) {
            data.pop();
        }
        String::from_utf16(&data).map(Some).map_err(|_| failure())
    }
    fn set(path: &str, name: &str, value: Option<&str>) -> Result<(), GogError> {
        let key = key(path, true)?.ok_or_else(failure)?;
        let name = wide(name);
        let result = unsafe {
            if let Some(value) = value {
                let data = wide(value);
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    1,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            } else {
                RegDeleteValueW(key.0, name.as_ptr())
            }
        };
        if result == 0 || (value.is_none() && result == 2) {
            Ok(())
        } else {
            Err(failure())
        }
    }
    pub fn read() -> Result<LoginState, GogError> {
        Ok(LoginState {
            user_id: get(&format!("{ROOT}\\settings"), "userId")?.filter(|id| valid_id(id)),
            username: get(&format!("{ROOT}\\settings"), "username")?,
            refresh_token: get(ROOT, "refreshToken")?.filter(|s| !s.is_empty()),
        })
    }
    pub fn write(state: &LoginState) -> Result<(), GogError> {
        if state.user_id.as_ref().is_some_and(|id| !valid_id(id))
            || state
                .refresh_token
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 4096 || s.contains('\0'))
        {
            return Err(failure());
        }
        set(ROOT, "refreshToken", state.refresh_token.as_deref())?;
        set(
            &format!("{ROOT}\\settings"),
            "userId",
            state.user_id.as_deref(),
        )?;
        set(
            &format!("{ROOT}\\settings"),
            "username",
            state.username.as_deref(),
        )
    }
}
