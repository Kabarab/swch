//! GOG ownership is synced online; GALAXY remains responsible for game launches.
pub mod auth;
pub mod local;
pub mod galaxy;

use crate::storage::Game;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum GogError {
    #[error("{0}")]
    Invalid(String),
    #[error("Вход GOG истёк. Подключи аккаунт заново.")]
    Unauthorized,
    #[error("Не удалось связаться с GOG. Проверь соединение и повтори попытку.")]
    Network,
    #[error("GOG вернул ошибку HTTP {0}.")]
    Http(u16),
    #[error("Некорректный ответ GOG.")]
    Response,
    #[error("Не удалось сохранить доступ GOG в системном хранилище: {0}")]
    Credentials(#[from] keyring::Error),
    #[error("Ошибка файлов GOG: {0}")]
    Io(#[from] std::io::Error),
    #[error("Ошибка библиотеки GOG: {0}")]
    Database(#[from] crate::storage::DbError),
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|c| c.is_ascii_digit())
}

pub fn json_id(value: &Value) -> Option<String> {
    let id = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|n| n.to_string()))?;
    valid_id(&id).then_some(id)
}

pub fn game(id: &str, name: String, banner_url: Option<String>) -> Game {
    Game {
        id: format!("gog_{id}"),
        launcher: "gog".into(),
        app_id: id.into(),
        name,
        install_path: None,
        executable_path: None,
        size_bytes: 0,
        is_installed: false,
        banner_url,
        launch_uri: None,
        namespace: None,
    }
}

pub fn product_game(id: &str, product: &Value) -> Result<Option<Game>, GogError> {
    if !valid_id(id) || json_id(&product["id"]).as_deref() != Some(id) {
        return Err(GogError::Response);
    }
    if product["game_type"]
        .as_str()
        .is_some_and(|t| t != "game" && t != "pack")
    {
        return Ok(None);
    }
    let title = product["title"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or(GogError::Response)?;
    let banner = ["background", "logo", "icon"].iter().find_map(|key| {
        let url = product["images"][key].as_str()?;
        let url = if url.starts_with("//") {
            format!("https:{url}")
        } else {
            url.into()
        };
        reqwest::Url::parse(&url)
            .ok()
            .filter(|u| u.scheme() == "https")
            .map(|u| u.to_string())
    });
    Ok(Some(game(id, title.into(), banner)))
}
