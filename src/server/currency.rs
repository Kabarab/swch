//! Display-only conversion of USD estimates. Steam regional pricing is separate.
use super::handlers::ErrorResponse;
use crate::storage::Database;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRates {
    pub base: String,
    pub date: String,
    pub source: String,
    pub rates: BTreeMap<String, f64>,
    pub cached: bool,
    pub warning: Option<String>,
}
pub fn parse_rates(value: &Value) -> Result<ExchangeRates, String> {
    let list = value.as_array().ok_or("Неверный формат курсов валют")?;
    let usd = list
        .iter()
        .find(|v| v["Cur_Abbreviation"] == "USD")
        .ok_or("Нет курса USD")?;
    let date = usd["Date"]
        .as_str()
        .and_then(|v| v.get(..10))
        .filter(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok())
        .ok_or("Нет даты курса")?;
    let unit = |v: &Value| -> Option<f64> {
        let rate = v["Cur_OfficialRate"].as_f64()?;
        let scale = v["Cur_Scale"].as_f64()?;
        if rate.is_finite() && rate > 0.0 && scale.is_finite() && scale >= 1.0 {
            Some(rate / scale)
        } else {
            None
        }
    };
    let usd_byn = unit(usd).ok_or("Некорректный курс USD")?;
    let mut rates = BTreeMap::from([("USD".into(), 1.0)]);
    for currency in ["EUR", "RUB", "KZT", "UAH", "PLN", "GBP", "CNY"] {
        if let Some(v) = list.iter().find(|v| v["Cur_Abbreviation"] == currency) {
            if v["Date"].as_str().and_then(|v| v.get(..10)) != Some(date) {
                continue;
            }
            if let Some(target_byn) = unit(v) {
                let rate = usd_byn / target_byn;
                if rate.is_finite() && rate > 0.0 {
                    rates.insert(currency.into(), rate);
                }
            }
        }
    }
    Ok(ExchangeRates {
        base: "USD".into(),
        date: date.into(),
        source: "НБРБ".into(),
        rates,
        cached: false,
        warning: None,
    })
}
fn default_rates() -> ExchangeRates {
    let today = (chrono::Utc::now() + chrono::Duration::hours(3))
        .format("%Y-%m-%d")
        .to_string();
    let mut rates = BTreeMap::new();
    rates.insert("USD".into(), 1.0);
    rates.insert("EUR".into(), 0.92);
    rates.insert("RUB".into(), 91.5);
    rates.insert("KZT".into(), 480.0);
    rates.insert("UAH".into(), 41.2);
    rates.insert("PLN".into(), 3.95);
    rates.insert("GBP".into(), 0.78);
    rates.insert("CNY".into(), 7.23);
    ExchangeRates {
        base: "USD".into(),
        date: today,
        source: "Резервные данные".into(),
        rates,
        cached: true,
        warning: Some("Сервис курсов временно недоступен. Используются резервные значения.".into()),
    }
}

async fn fetch_rates() -> Result<ExchangeRates, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|_| "Не удалось загрузить курсы")?;
    let value = client
        .get("https://api.nbrb.by/exrates/rates")
        .query(&[("periodicity", "0")])
        .send()
        .await
        .map_err(|_| "Сервис курсов НБРБ недоступен")?
        .error_for_status()
        .map_err(|_| "Сервис курсов НБРБ недоступен")?
        .json::<Value>()
        .await
        .map_err(|_| "Не удалось прочитать курсы НБРБ")?;
    parse_rates(&value)
}
pub async fn exchange_rates(
    State(db): State<Database>,
) -> Result<Json<ExchangeRates>, (StatusCode, Json<ErrorResponse>)> {
    let error = |message: String| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse { error: message }),
        )
    };
    let cached = db
        .steam_cache_get("fx:nbrb:steam:USD:v1")
        .await
        .map_err(|_| error("Не удалось прочитать сохранённые курсы".into()))?
        .and_then(|(s, t)| {
            serde_json::from_str::<ExchangeRates>(&s)
                .ok()
                .map(|v| (v, t))
        });
    let today = (chrono::Utc::now() + chrono::Duration::hours(3))
        .format("%Y-%m-%d")
        .to_string();
    if let Some((mut rates, time)) = cached.clone() {
        if rates.date == today && chrono::Utc::now().timestamp() - time < 3600 {
            rates.cached = true;
            return Ok(Json(rates));
        }
    }
    match fetch_rates().await {
        Ok(mut rates) => {
            if rates.date != today {
                rates.warning=Some("Источник вернул курс за другую дату. Используется дата, указанная рядом с валютой.".into());
            }
            let _ = db.steam_cache_put(
                "fx:nbrb:steam:USD:v1",
                &serde_json::to_string(&rates).unwrap_or_default(),
            ).await;
            Ok(Json(rates))
        }
        Err(message) => {
            if let Some((mut rates, _)) = cached {
                rates.cached = true;
                rates.warning = Some(format!(
                    "{message}. Используется сохранённый курс за {}.",
                    rates.date
                ));
                Ok(Json(rates))
            } else {
                let fallback = default_rates();
                let _ = db.steam_cache_put(
                    "fx:nbrb:steam:USD:v1",
                    &serde_json::to_string(&fallback).unwrap_or_default(),
                ).await;
                Ok(Json(fallback))
            }
        }
    }
}
