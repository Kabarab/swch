//! Persist a validated language choice for browser windows and native startup.
use axum::{
    extract::{DefaultBodyLimit, Path},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Deserialize, Serialize)]
pub struct Selection {
    pub locale: String,
    pub revision: String,
    pub machine: bool,
    pub messages: BTreeMap<String, String>,
}
pub fn directory() -> PathBuf {
    std::env::var_os("SWCH_I18N_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("swch")
                .join("i18n")
        })
}
pub fn router() -> Router<super::AppState> {
    Router::new()
        .route("/api/i18n/selection", get(selection).post(save))
        .route("/api/i18n/packs/:locale", get(pack))
        .layer(DefaultBodyLimit::max(4_000_000))
        .layer(axum::middleware::from_fn(super::mods::local_origin))
}
fn snapshot() -> Option<PathBuf> {
    let active = std::fs::read_to_string(directory().join("active.txt")).ok()?;
    if active.len() != 64 || !active.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(directory().join("versions").join(active))
}
async fn selection() -> Json<Value> {
    let value = snapshot()
        .and_then(|dir| std::fs::read(dir.join("selection.json")).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    Json(value.unwrap_or_else(|| json!({"locale":"ru"})))
}
async fn pack(Path(locale): Path<String>) -> Result<Json<Value>, StatusCode> {
    if !valid_locale(&locale) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let bytes = std::fs::read(directory().join("packs").join(locale + ".json"))
        .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_slice(&bytes)
        .map(Json)
        .map_err(|_| StatusCode::NOT_FOUND)
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn valid_locale(locale: &str) -> bool {
    let parts: Vec<_> = locale.split('-').collect();
    parts.len() <= 3
        && (2..=3).contains(&parts[0].len())
        && parts[0].bytes().all(|b| b.is_ascii_alphabetic())
        && parts[1..]
            .iter()
            .all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}
fn slots(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    for suffix in text.split('{').skip(1) {
        if let Some((candidate, _)) = suffix.split_once('}') {
            if !candidate.is_empty() && candidate.bytes().all(|b| b.is_ascii_digit()) {
                result.push(candidate.to_owned());
            }
        }
    }
    result.sort();
    result
}
pub fn validate(selection: &Selection, source: &Value) -> bool {
    let Some(messages) = source["messages"].as_object() else {
        return false;
    };
    valid_locale(&selection.locale)
        && source["revision"].as_str() == Some(selection.revision.as_str())
        && selection.messages.len() == messages.len()
        && messages.keys().all(|key| {
            selection.messages.get(key).is_some_and(|text| {
                !text.trim().is_empty() && text.len() <= 64_000 && slots(text) == slots(key)
            })
        })
}
pub fn startup_html(selection: &Selection, error: bool, windows: bool) -> String {
    let text = |key: &str| {
        escape(
            selection
                .messages
                .get(key)
                .map(String::as_str)
                .unwrap_or(key),
        )
    };
    let title = text(if error {
        "Не удалось запустить SWCH"
    } else {
        "Запуск SWCH..."
    });
    let description = text(if error {
        "Серверный движок не ответил вовремя. Проверьте запуск приложения или повторите попытку."
    } else {
        "Инициализация игрового пространства"
    });
    let retry = if error {
        format!(
            "<button onclick=\"{}\">{}</button>",
            if windows {
                "window.chrome.webview.postMessage('retry')"
            } else {
                "location.href='http://127.0.0.1:4242/'"
            },
            text("Повторить попытку")
        )
    } else {
        String::new()
    };
    let rtl = [
        "ar", "fa", "iw", "he", "ur", "ps", "sd", "ug", "dv", "yi", "ckb", "bal",
    ]
    .iter()
    .any(|code| selection.locale.split('-').next() == Some(*code))
        || selection.locale.contains("-Arab")
        || selection.locale.contains("-Nkoo");
    format!("<!doctype html><html lang=\"{}\" dir=\"{}\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>SWCH</title><style>body{{margin:0;background:#111214;color:#f2f2f4;font:16px/1.5 system-ui,sans-serif;display:grid;min-height:100vh;place-items:center}}main{{text-align:center;max-width:440px;padding:32px;overflow-wrap:anywhere}}p{{color:#adb0bb}}button{{background:#ff642d;border:0;padding:12px 22px;border-radius:8px;font:inherit;white-space:normal}}</style><main><h2>{}</h2><p>{}</p>{}</main></html>",escape(&selection.locale),if rtl {"rtl"} else {"ltr"},title,description,retry)
}
async fn save(Json(choice): Json<Selection>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let source: Value =
        serde_json::from_str(include_str!("../../ui/lang/source.json")).unwrap_or_default();
    if !validate(&choice, &source) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid-language-pack"})),
        ));
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let root = directory();
        let id = format!("{:x}", Sha256::digest(serde_json::to_vec(&choice)?));
        let dir = root.join("versions").join(&id);
        std::fs::create_dir_all(&dir)?;
        for (name, error, windows) in [
            ("loading-windows.html", false, true),
            ("error-windows.html", true, true),
            ("loading-macos.html", false, false),
            ("error-macos.html", true, false),
        ] {
            crate::storage::atomic::atomic_write(
                dir.join(name),
                startup_html(&choice, error, windows),
            )?;
        }
        crate::storage::atomic::atomic_write(
            dir.join("selection.json"),
            serde_json::to_vec(&json!({"locale":choice.locale,"revision":choice.revision}))?,
        )?;
        std::fs::create_dir_all(root.join("packs"))?;
        crate::storage::atomic::atomic_write(
            root.join("packs").join(choice.locale.clone() + ".json"),
            serde_json::to_vec(
                &json!({"schema":1,"locale":choice.locale,"revision":choice.revision,"machine":choice.machine,"messages":choice.messages}),
            )?,
        )?;
        // A single atomic pointer commits all native templates and the selection together.
        crate::storage::atomic::atomic_write(root.join("active.txt"), id)?;
        Ok(())
    })();
    result.map(|_| Json(json!({"status":"ok"}))).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"language-save-failed"})),
        )
    })
}
