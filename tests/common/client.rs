#![allow(dead_code)]

use axum::{body::Body, Router};
use http_body_util::BodyExt;
use reqwest::{header, Client, StatusCode};
use std::collections::HashMap;
use thiserror::Error;
use tower::ServiceExt;

use super::models::*;

#[derive(Error, Debug)]
pub enum EpicClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error response: {0:?}")]
    Api(EpicApiError),
    #[error("HTTP status {0}: {1}")]
    Status(StatusCode, String),
    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Missing expected field in response: {0}")]
    MissingField(String),
}

#[derive(Clone)]
pub struct EpicClient {
    client: Client,
    base_url: String,
    router: Option<Router>,
}

impl EpicClient {
    pub fn new(base_url: Option<&str>) -> Self {
        let base = base_url
            .unwrap_or("https://account-public-service-prod03.ol.epicgames.com")
            .trim_end_matches('/')
            .to_string();

        let client = Client::builder()
            .no_proxy()
            .user_agent("UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit")
            .build()
            .expect("Failed to build reqwest Client");

        Self {
            client,
            base_url: base,
            router: None,
        }
    }

    pub fn from_router(router: Router) -> Self {
        let mut client = Self::new(Some("http://mock.epicgames.local"));
        client.router = Some(router);
        client
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn dispatch(
        &self,
        method: &'static str,
        path: &str,
        headers: &[(&'static str, String)],
        body: Option<String>,
    ) -> Result<(StatusCode, String), EpicClientError> {
        if let Some(router) = &self.router {
            let mut req_builder = axum::http::Request::builder()
                .method(method)
                .uri(path);

            for (k, v) in headers {
                req_builder = req_builder.header(*k, v.as_str());
            }

            let req = if let Some(b) = body {
                req_builder.body(Body::from(b)).unwrap()
            } else {
                req_builder.body(Body::empty()).unwrap()
            };

            let response = router
                .clone()
                .oneshot(req)
                .await
                .map_err(|e| EpicClientError::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let status = response.status();
            let body_bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|e| EpicClientError::Status(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .to_bytes();

            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            Ok((status, body_str))
        } else {
            let url = format!("{}{}", self.base_url, path);
            let mut req = match method {
                "GET" => self.client.get(&url),
                "POST" => self.client.post(&url),
                "DELETE" => self.client.delete(&url),
                _ => unreachable!(),
            };

            for (k, v) in headers {
                req = req.header(*k, v.as_str());
            }

            if let Some(b) = body {
                req = req.body(b);
            }

            let resp = req.send().await?;
            let status = resp.status();
            let body_str = resp.text().await?;
            Ok((status, body_str))
        }
    }

    /// Exchanges an authorization code for initial OAuth2 session tokens
    pub async fn exchange_code_for_token(
        &self,
        code: &str,
    ) -> Result<OAuthTokenResponse, EpicClientError> {
        let form_body = format!(
            "grant_type=authorization_code&code={}&token_type=eg1",
            code
        );
        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_LAUNCHER_BASIC_AUTH.to_string()),
            (header::CONTENT_TYPE.as_str(), "application/x-www-form-urlencoded".to_string()),
        ];

        let (status, body_text) = self
            .dispatch("POST", "/account/api/oauth/token", &headers, Some(form_body))
            .await?;

        if status.is_success() {
            let token_resp: OAuthTokenResponse = serde_json::from_str(&body_text)?;
            Ok(token_resp)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Creates a permanent DeviceAuth credential
    pub async fn create_device_auth(
        &self,
        account_id: &str,
        access_token: &str,
    ) -> Result<DeviceAuth, EpicClientError> {
        let path = format!("/account/api/public/account/{}/deviceAuth", account_id);
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::CONTENT_TYPE.as_str(), "application/json".to_string()),
        ];

        let (status, body_text) = self
            .dispatch("POST", &path, &headers, Some("{}".to_string()))
            .await?;

        if status.is_success() {
            let auth: DeviceAuth = serde_json::from_str(&body_text)?;
            Ok(auth)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Headless login/refresh via DeviceAuth credentials
    pub async fn login_with_device_auth(
        &self,
        auth: &DeviceAuth,
    ) -> Result<OAuthTokenResponse, EpicClientError> {
        let form_body = format!(
            "grant_type=device_auth&account_id={}&device_id={}&secret={}&token_type=eg1",
            auth.account_id, auth.device_id, auth.secret
        );
        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_LAUNCHER_BASIC_AUTH.to_string()),
            (header::CONTENT_TYPE.as_str(), "application/x-www-form-urlencoded".to_string()),
        ];

        let (status, body_text) = self
            .dispatch("POST", "/account/api/oauth/token", &headers, Some(form_body))
            .await?;

        if status.is_success() {
            let token_resp: OAuthTokenResponse = serde_json::from_str(&body_text)?;
            Ok(token_resp)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Deletes a registered DeviceAuth credential
    pub async fn delete_device_auth(
        &self,
        account_id: &str,
        device_id: &str,
        access_token: &str,
    ) -> Result<(), EpicClientError> {
        let path = format!(
            "/account/api/public/account/{}/deviceAuth/{}",
            account_id, device_id
        );
        let headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token))];

        let (status, body_text) = self.dispatch("DELETE", &path, &headers, None).await?;

        if status.is_success() || status == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Obtains a short-lived (300s) one-time game launch exchange code
    pub async fn get_exchange_code(
        &self,
        access_token: &str,
    ) -> Result<ExchangeCodeResponse, EpicClientError> {
        let headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token))];

        let (status, body_text) = self
            .dispatch("GET", "/account/api/oauth/exchange", &headers, None)
            .await?;

        if status.is_success() {
            let exchange: ExchangeCodeResponse = serde_json::from_str(&body_text)?;
            Ok(exchange)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Fetches a single page of entitlements
    pub async fn fetch_entitlements(
        &self,
        access_token: &str,
        account_id: &str,
        start: usize,
        count: usize,
    ) -> Result<Vec<Entitlement>, EpicClientError> {
        let path = format!(
            "/entitlement/api/account/{}/entitlements?start={}&count={}",
            account_id, start, count
        );
        let headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token))];

        let (status, body_text) = self.dispatch("GET", &path, &headers, None).await?;

        if status.is_success() {
            let entitlements: Vec<Entitlement> = serde_json::from_str(&body_text)?;
            Ok(entitlements)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }

    /// Fetches all entitlements across all pages
    pub async fn fetch_all_entitlements(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> Result<Vec<Entitlement>, EpicClientError> {
        let mut all = Vec::new();
        let mut start = 0;
        let count = 1000;

        loop {
            let batch = self
                .fetch_entitlements(access_token, account_id, start, count)
                .await?;
            let fetched = batch.len();
            all.extend(batch);

            if fetched < count {
                break;
            }
            start += count;
        }

        Ok(all)
    }

    /// Fetches catalog items metadata for games and DLCs
    pub async fn fetch_catalog_items(
        &self,
        access_token: &str,
        namespace: &str,
        item_ids: &[&str],
    ) -> Result<HashMap<String, CatalogItem>, EpicClientError> {
        let mut path = format!(
            "/catalog/api/shared/namespace/{}/bulk/items",
            namespace
        );

        if !item_ids.is_empty() {
            let query: Vec<String> = item_ids.iter().map(|id| format!("id={}", id)).collect();
            path = format!("{}?{}", path, query.join("&"));
        }
        let headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token))];

        let (status, body_text) = self.dispatch("GET", &path, &headers, None).await?;

        if status.is_success() {
            let items: HashMap<String, CatalogItem> = serde_json::from_str(&body_text)?;
            Ok(items)
        } else if let Ok(api_err) = serde_json::from_str::<EpicApiError>(&body_text) {
            Err(EpicClientError::Api(api_err))
        } else {
            Err(EpicClientError::Status(status, body_text))
        }
    }
}
