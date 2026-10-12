use std::collections::HashMap;
use std::time::Duration;
use axum::{body::Body, Router};
use reqwest::{header, Client, StatusCode};
use serde::Deserialize;
use tower::ServiceExt;

use crate::launchers::epic::models::*;

fn form(fields: &[(&str, &str)]) -> String {
    let mut url = reqwest::Url::parse("https://epicgames.com/").expect("constant URL");
    url.query_pairs_mut().extend_pairs(fields.iter().copied());
    url.query().unwrap_or_default().to_string()
}
fn checkout_http_error(status: StatusCode, stage: &str) -> EpicError {
    EpicError::Api { status, code: Some(format!("swch.epic.checkout.{stage}")),
        message: "Epic checkout is unavailable".into(), numeric: None }
}

/// Keep the HTTP domain-cookie flag when translating into Chromium's cookie
/// representation. reqwest strips a Domain attribute's leading dot, whereas
/// DevTools interprets a domain without that dot as a host-only cookie.
fn capture_epic_web_cookies(
    response: &reqwest::Response,
    cookies: &mut Vec<crate::auto_claim::web::Cookie>,
    now: f64,
) {
    let source = response.url();
    let Some(host) = source.host_str() else {
        return;
    };
    if !crate::auto_claim::platforms::trusted_url("epic", source.as_str()) {
        return;
    }
    cookies.retain(|c| c.expires.is_none_or(|expires| expires > now));
    for raw in response.headers().get_all(header::SET_COOKIE) {
        // Parse each header separately so its attributes stay associated even
        // when another Set-Cookie header is malformed. No network is involved.
        let mut single = axum::http::Response::new(String::new());
        single.headers_mut().insert(header::SET_COOKIE, raw.clone());
        let single = reqwest::Response::from(single);
        let Some(cookie) = single.cookies().next() else {
            continue;
        };
        let domain = if let Some(domain) = cookie.domain() {
            let domain = domain.to_ascii_lowercase();
            if !(host == domain || host.ends_with(&format!(".{domain}")))
                || !crate::auto_claim::platforms::trusted_url("epic", &format!("https://{domain}/"))
            {
                continue;
            }
            format!(".{domain}")
        } else {
            host.to_string()
        };
        let default_path = || {
            source
                .path()
                .rfind('/')
                .filter(|&position| position > 0)
                .map(|position| &source.path()[..position])
                .unwrap_or("/")
        };
        let path = cookie
            .path()
            .filter(|path| path.starts_with('/'))
            .unwrap_or_else(default_path)
            .to_string();
        let expires = cookie
            .max_age()
            .map(|age| now + age.as_secs_f64())
            .or_else(|| {
                cookie.expires().map(|expires| {
                    expires
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_secs_f64())
                        .unwrap_or(0.0)
                })
            });
        let same_site = if cookie.same_site_lax() {
            Some("Lax")
        } else if cookie.same_site_strict() {
            Some("Strict")
        } else if raw.to_str().ok().is_some_and(|raw| {
            raw.split(';')
                .skip(1)
                .filter_map(|attribute| attribute.trim().split_once('='))
                .any(|(key, value)| {
                    key.trim().eq_ignore_ascii_case("samesite")
                        && value.trim().eq_ignore_ascii_case("none")
                })
        }) {
            // reqwest exposes Lax/Strict predicates, but no SameSite=None one.
            Some("None")
        } else {
            None
        };
        cookies.retain(|existing| {
            !(existing.name == cookie.name() && existing.domain == domain && existing.path == path)
        });
        if expires.is_some_and(|expires| expires <= now) {
            continue;
        }
        cookies.push(crate::auto_claim::web::Cookie {
            name: cookie.name().into(),
            value: cookie.value().into(),
            domain,
            path,
            secure: cookie.secure(),
            http_only: cookie.http_only(),
            expires,
            same_site: same_site.map(str::to_string),
        });
    }
}

fn capture_epic_web_response(
    response: &reqwest::Response,
    cookies: &mut Vec<crate::auto_claim::web::Cookie>,
) {
    capture_epic_web_cookies(
        response,
        cookies,
        chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
    );
}

fn epic_web_bearer(
    cookies: &[crate::auto_claim::web::Cookie],
    now: f64,
) -> Result<&crate::auto_claim::web::Cookie, EpicError> {
    let mut matching = cookies.iter().filter(|cookie| {
        cookie.name == "EPIC_BEARER_TOKEN"
            && cookie.domain.eq_ignore_ascii_case(".epicgames.com")
            && cookie.path == "/"
            && cookie.secure
            && !cookie.value.is_empty()
            && cookie
                .expires
                .is_none_or(|expires| expires.is_finite() && (expires == -1.0 || expires > now))
    });
    let bearer = matching
        .next()
        .ok_or_else(|| EpicError::Auth("Epic browser session needs verification".into()))?;
    if matching.any(|cookie| cookie.value != bearer.value) {
        return Err(EpicError::Auth("Epic browser session is ambiguous".into()));
    }
    Ok(bearer)
}

/// Configurable endpoints for Epic Games backend services.
/// Allows overriding base URLs for unit, integration, and offline mock server tests.
#[derive(Debug, Clone)]
pub struct EpicEndpoints {
    pub account_base_url: String,
    pub entitlement_base_url: String,
    pub catalog_base_url: String,
    pub fn_mcp_base_url: String,
}

impl Default for EpicEndpoints {
    fn default() -> Self {
        Self {
            account_base_url: "https://account-public-service-prod03.ol.epicgames.com".to_string(),
            entitlement_base_url: "https://entitlement-public-service-prod08.ol.epicgames.com".to_string(),
            catalog_base_url: "https://catalog-public-service-prod06.ol.epicgames.com".to_string(),
            fn_mcp_base_url: "https://fortnite-public-service-prod11.ol.epicgames.com".to_string(),
        }
    }
}

impl EpicEndpoints {
    /// Creates endpoints with all services pointing to a single mock server URL.
    pub fn new_mock(base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        Self {
            account_base_url: base.to_string(),
            entitlement_base_url: base.to_string(),
            catalog_base_url: base.to_string(),
            fn_mcp_base_url: base.to_string(),
        }
    }

    /// Checks the `SWCH_EPIC_API_URL` environment variable or defaults to production URLs.
    pub fn from_env_or_default() -> Self {
        if let Ok(url) = std::env::var("SWCH_EPIC_API_URL") {
            if !url.trim().is_empty() {
                return Self::new_mock(&url);
            }
        }
        Self::default()
    }
}

/// Asynchronous HTTP client for Epic Games services.
#[derive(Clone)]
pub struct EpicClient {
    client: Client,
    endpoints: EpicEndpoints,
    router: Option<Router>,
}

impl Default for EpicClient {
    fn default() -> Self {
        Self::new()
    }
}

impl EpicClient {
    /// Official one-use code exchange. Cookies stay in memory or the caller's
    /// OS vault; no passwords, browser profile copying or challenge solving.
    pub async fn store_web_session(
        &self,
        token: &str,
        account_id: &str,
    ) -> Result<(Client, Vec<crate::auto_claim::web::Cookie>), EpicError> {
        let client = Client::builder()
            .no_proxy()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .user_agent(EPIC_USER_AGENT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut cookies = Vec::new();
        let response = client
            .get("https://www.epicgames.com/id/api/reputation")
            .send()
            .await?;
        capture_epic_web_response(&response, &mut cookies);
        let mut xsrf = response
            .cookies()
            .find(|c| c.name() == "XSRF-TOKEN")
            .map(|c| c.value().to_string());
        if xsrf.is_none() {
            let response = client
                .get("https://www.epicgames.com/id/api/csrf")
                .send()
                .await?;
            capture_epic_web_response(&response, &mut cookies);
            xsrf = response
                .cookies()
                .find(|c| c.name() == "XSRF-TOKEN")
                .map(|c| c.value().to_string());
        }
        let xsrf =
            xsrf.ok_or_else(|| EpicError::Auth("Epic browser session needs verification".into()))?;
        let exchange = self.get_exchange_code(token).await?;
        let response = client
            .post("https://www.epicgames.com/id/api/exchange")
            .header("x-xsrf-token", xsrf)
            .json(&serde_json::json!({"exchangeCode":exchange.code}))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(checkout_http_error(response.status(), "exchange_api"));
        }
        capture_epic_web_response(&response, &mut cookies);
        let response = client
            .get("https://www.epicgames.com/id/api/redirect")
            .send()
            .await?;
        capture_epic_web_response(&response, &mut cookies);
        let data: serde_json::Value = Self::parse_json(response.status(), &response.text().await?)?;
        let sid = data["sid"]
            .as_str()
            .filter(|s| s.len() <= 1024 && !s.is_empty())
            .ok_or_else(|| EpicError::Auth("Epic browser session needs verification".into()))?;
        let response = client
            .get("https://www.epicgames.com/id/api/set-sid")
            .query(&[("sid", sid)])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(checkout_http_error(response.status(), "session_api"));
        }
        capture_epic_web_response(&response, &mut cookies);
        let bearer = epic_web_bearer(
            &cookies,
            chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
        )?;
        let (status, body) = self
            .dispatch(
                "GET",
                "/account/api/oauth/verify",
                &[
                    (
                        header::AUTHORIZATION.as_str(),
                        format!("Bearer {}", bearer.value),
                    ),
                    (header::USER_AGENT.as_str(), EPIC_USER_AGENT.into()),
                ],
                None,
            )
            .await?;
        let verified: serde_json::Value = Self::parse_json(status, &body)?;
        if verified["account_id"].as_str() != Some(account_id) {
            return Err(EpicError::Auth(
                "Epic browser session belongs to another account".into(),
            ));
        }
        Ok((client, cookies))
    }
    pub async fn verify_web_identity(
        &self,
        cookies: &[crate::auto_claim::web::Cookie],
        account_id: &str,
    ) -> Result<(), EpicError> {
        let bearer = epic_web_bearer(
            cookies,
            chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
        )?;
        let (status, body) = self
            .dispatch(
                "GET",
                "/account/api/oauth/verify",
                &[(
                    header::AUTHORIZATION.as_str(),
                    format!("Bearer {}", bearer.value),
                )],
                None,
            )
            .await?;
        let verified: serde_json::Value = Self::parse_json(status, &body)?;
        if verified["account_id"].as_str() != Some(account_id) {
            return Err(EpicError::Auth(
                "Epic browser session belongs to another account".into(),
            ));
        }
        Ok(())
    }
    /// Instantiates client with endpoints resolved from environment or production defaults.
    pub fn new() -> Self {
        Self::with_endpoints(EpicEndpoints::from_env_or_default())
    }

    /// Instantiates client targeting a single test mock server URL.
    pub fn with_base_url(base_url: &str) -> Self {
        Self::with_endpoints(EpicEndpoints::new_mock(base_url))
    }

    /// Instantiates client wrapping an in-process Axum mock router (enables socket-free, proxy-free testing).
    pub fn from_router(router: Router) -> Self {
        let mut client = Self::new();
        client.router = Some(router);
        client
    }

    /// Returns true if this client is configured with an in-process mock router.
    pub fn is_mock(&self) -> bool {
        self.router.is_some()
    }

    /// Instantiates client with custom endpoint configuration.
    pub fn with_endpoints(endpoints: EpicEndpoints) -> Self {
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            client,
            endpoints,
            router: None,
        }
    }

    /// Reference to configured endpoints.
    pub fn endpoints(&self) -> &EpicEndpoints {
        &self.endpoints
    }

    /// Standard browser authorization URL for Epic account authentication.
    pub fn auth_url() -> &'static str {
        EPIC_AUTH_URL
    }

    async fn dispatch(
        &self,
        method: &'static str,
        path: &str,
        headers: &[(&'static str, String)],
        body: Option<(String, &'static str)>,
    ) -> Result<(StatusCode, String), EpicError> {
        if let Some(router) = &self.router {
            let mut req_builder = axum::http::Request::builder()
                .method(method)
                .uri(path);

            for (k, v) in headers {
                req_builder = req_builder.header(*k, v.as_str());
            }

            let req = if let Some((b, ct)) = body {
                req_builder
                    .header(header::CONTENT_TYPE.as_str(), ct)
                    .body(Body::from(b))
                    .map_err(|e| EpicError::Io(e.to_string()))?
            } else {
                req_builder
                    .body(Body::empty())
                    .map_err(|e| EpicError::Io(e.to_string()))?
            };

            let response = router
                .clone()
                .oneshot(req)
                .await
                .map_err(|e| EpicError::Io(e.to_string()))?;

            let status = response.status();
            let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
                .await
                .map_err(|e| EpicError::Io(e.to_string()))?;
            let body_str = String::from_utf8_lossy(&body_bytes).to_string();
            Ok((status, body_str))
        } else {
            let base_url = if path.starts_with("/orderprocessor/") && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
                "https://orderprocessor-public-service-ecomprod01.ol.epicgames.com"
            } else if matches!(path,"/purchase/order-preview" | "/purchase/confirm-order") && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
                "https://payment-website-pci.ol.epicgames.com"
            } else if path.starts_with("/purchase") && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
                "https://launcher-website-prod07.ol.epicgames.com"
            } else if path.starts_with("/id/") && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
                "https://www.epicgames.com"
            } else if path.starts_with("/library/") && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
                "https://library-service.live.use1a.on.epicgames.com"
            } else if path.starts_with("/entitlement/") {
                &self.endpoints.entitlement_base_url
            } else if path.starts_with("/catalog/") {
                &self.endpoints.catalog_base_url
            } else if path.starts_with("/fortnite/") {
                &self.endpoints.fn_mcp_base_url
            } else {
                &self.endpoints.account_base_url
            };
            let url = format!("{}{}", base_url, path);
            let mut req = match method {
                "GET" => self.client.get(&url),
                "POST" => self.client.post(&url),
                "DELETE" => self.client.delete(&url),
                _ => unreachable!(),
            };

            for (k, v) in headers {
                req = req.header(*k, v.as_str());
            }

            if let Some((b, ct)) = body {
                req = req.header(header::CONTENT_TYPE, ct).body(b);
            }

            let resp = req.send().await?;
            let status = resp.status();
            let diagnostic = std::env::var("SWCH_EPIC_CHECKOUT_DIAGNOSTICS").as_deref() == Ok("1") && path.starts_with("/id/exchange?");
            let final_stage = match resp.url().path() {
                "/id/exchange" => "exchange",
                "/site/account" => "account_landing",
                "/id/login" => "login_page",
                _ => "other",
            };
            let body_str = resp.text().await.unwrap_or_default();
            if diagnostic {
                let lower = body_str.to_ascii_lowercase();
                eprintln!("{}",serde_json::json!({"epic_sso_metadata":{"http":status.as_u16(),"stage":final_stage,"cloudflare":lower.contains("cloudflare"),"challenge":lower.contains("challenge-platform") || lower.contains("cf-chl-"),"captcha":lower.contains("captcha"),"json":serde_json::from_str::<serde_json::Value>(&body_str).is_ok()}}));
            }
            Ok((status, body_str))
        }
    }

    fn parse_json<T: serde::de::DeserializeOwned>(status: StatusCode, text: &str) -> Result<T, EpicError> {
        if status.is_success() {
            let parsed = serde_json::from_str::<T>(text)?;
            Ok(parsed)
        } else {
            if let Ok(api_err) = serde_json::from_str::<EpicApiErrorResponse>(text) {
                Err(EpicError::Api {
                    status,
                    code: api_err.error_code,
                    message: api_err.error_message.unwrap_or_else(|| "Unknown Epic API error".to_string()),
                    numeric: api_err.numeric_error_code,
                })
            } else {
                Err(EpicError::Api {
                    status,
                    code: None,
                    message: if text.is_empty() {
                        status.canonical_reason().unwrap_or("HTTP error").to_string()
                    } else {
                        text.to_string()
                    },
                    numeric: None,
                })
            }
        }
    }

    /// A complimentary order only; checkout confirmation refuses nonzero totals.
    pub async fn claim_free_offer(&self, token: &str, account_id: &str, namespace: &str, offer_id: &str, country: &str) -> Result<(), EpicError> {
        if [account_id, namespace, offer_id].iter().any(|v| !crate::auto_claim::offers::identifier(v))
            || country.len() != 2 || !country.bytes().all(|v| v.is_ascii_uppercase()) {
            return Err(EpicError::Auth("Invalid free offer identity or country".into()));
        }
        let body = serde_json::json!({
            "salesChannel": "Launcher-purchase-client", "entitlementSource": "Launcher-purchase-client",
            "returnSplitPaymentItems": false,
            "lineOffers": [{"offerId": offer_id, "namespace": namespace, "quantity": 1}]
        });
        let headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {token}")),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string())];
        let path = format!("/orderprocessor/api/shared/accounts/{account_id}/orders/quickPurchase?country={country}&locale=en-US");
        let (status, text) = self.dispatch("POST", &path, &headers, Some((body.to_string(), "application/json"))).await?;
        let result: serde_json::Value = Self::parse_json(status, &text)?;
        match result["quickPurchaseStatus"].as_str() {
            Some("SUCCESS") => return Ok(()),
            Some("CHECKOUT") => {},
            _ => return Err(EpicError::Auth("Epic rejected this free order".into())),
        }
        // A fresh jar belongs to this order/account only. Launcher OAuth is
        // exchanged for web SSO; cookies are discarded with this local client.
        let mut checkout = self.clone();
        if self.router.is_none() && self.endpoints.account_base_url == EpicEndpoints::default().account_base_url {
            checkout.client = Client::builder().no_proxy().timeout(Duration::from_secs(30)).user_agent(EPIC_USER_AGENT)
                .cookie_store(true).redirect(reqwest::redirect::Policy::custom(|attempt| {
                    let u = attempt.url();
                    if attempt.previous().len() >= 10 { return attempt.error("Epic checkout redirect limit"); }
                    if u.scheme() == "https" && u.port_or_known_default() == Some(443)
                        && u.username().is_empty() && u.password().is_none()
                        && u.host_str().is_some_and(|h|h == "epicgames.com" || h.ends_with(".epicgames.com")) {
                        attempt.follow()
                    } else { attempt.stop() }
                })).build()?;
            let exchange = self.get_exchange_code(token).await?;
            let query = form(&[("exchangeCode",exchange.code.as_str()),("redirectUrl","https://www.epicgames.com/site/account")]);
            let (status, _) = checkout.dispatch("GET",&format!("/id/exchange?{query}"),&[],None).await?;
            if !status.is_success() {
                return Err(checkout_http_error(status,"login"));
            }
        }
        let path = format!("/purchase?showNavigation=true&namespace={namespace}&offers={offer_id}");
        let (status, html) = checkout.dispatch("GET", &path, &headers, None).await?;
        if !status.is_success() { return Err(checkout_http_error(status,"page")); }
        let purchase_token = html.split("<input").find(|tag| {
            let tag = tag.split('>').next().unwrap_or_default();
            crate::auto_claim::offers::attr(tag, "id") == Some("purchaseToken")
        }).and_then(|tag| crate::auto_claim::offers::attr(tag.split('>').next().unwrap_or_default(), "value"))
            .filter(|v| !v.is_empty() && v.len() <= 8192)
            .ok_or_else(|| EpicError::Auth("Epic checkout needs browser verification".into()))?;
        let checkout_headers = [(header::AUTHORIZATION.as_str(), format!("Bearer {token}")),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()), ("x-requested-with", purchase_token.to_string())];
        let preview = form(&[("useDefault","false"),("setDefault","false"),("namespace",namespace),
            ("country",country),("countryName",""),("orderId",""),("orderComplete",""),("orderError",""),("orderPending",""),
            ("offers[0]",offer_id),("offerPrice","")]);
        let (status, text) = checkout.dispatch("POST", "/purchase/order-preview", &checkout_headers,
            Some((preview, "application/x-www-form-urlencoded"))).await?;
        let preview: serde_json::Value = Self::parse_json(status, &text)?;
        let total = &preview["orderResponse"]["totalPrice"];
        let zero = total.as_f64().or_else(|| total.as_str().and_then(|v| v.parse::<f64>().ok())) == Some(0.0);
        if !zero || preview["namespace"].as_str() != Some(namespace) || preview["country"].as_str() != Some(country)
            || preview["offers"].as_array().is_none_or(|v| v.len() != 1 || v[0].as_str() != Some(offer_id)) {
            return Err(EpicError::Auth("Epic checkout is not a zero-price order for this offer".into()));
        }
        let sync = preview["syncToken"].as_str().filter(|v| !v.is_empty())
            .ok_or_else(|| EpicError::Auth("Epic checkout preview incomplete".into()))?;
        let confirm = form(&[("useDefault","false"),("setDefault","false"),("namespace",namespace),
            ("country",country),("countryName",preview["countryName"].as_str().unwrap_or("")),
            ("orderId",""),("orderComplete",""),("orderError",""),("orderPending",""),("offers[0]",offer_id),
            ("includeAccountBalance","false"),("totalAmount","0"),("affiliateId",""),("creatorSource",""),("syncToken",sync)]);
        let (status, text) = checkout.dispatch("POST", "/purchase/confirm-order", &checkout_headers,
            Some((confirm, "application/x-www-form-urlencoded"))).await?;
        let confirmed: serde_json::Value = Self::parse_json(status, &text)?;
        if confirmed["confirmation"].as_bool() == Some(true) { Ok(()) }
        else { Err(EpicError::Auth("Epic checkout needs browser verification".into())) }
    }

    /// Exchanges an authorization code (or raw user input) for OAuth2 tokens.
    pub async fn exchange_code_for_token(&self, raw_code: &str) -> Result<OAuthTokenResponse, EpicError> {
        let code = sanitize_auth_code(raw_code)?;
        let form_body = format!(
            "grant_type=authorization_code&code={}&token_type=eg1",
            code
        );

        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_BASIC_AUTH.to_string()),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch(
                "POST",
                "/account/api/oauth/token",
                &headers,
                Some((form_body, "application/x-www-form-urlencoded")),
            )
            .await?;

        Self::parse_json(status, &text)
    }

    /// Creates a separate native launcher session without handing SWCH's rotating
    /// refresh token to another process.
    pub async fn redeem_launcher_exchange(&self, code: &str) -> Result<OAuthTokenResponse, EpicError> {
        let body = format!("grant_type=exchange_code&exchange_code={code}&token_type=eg1");
        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_BASIC_AUTH.to_string()),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];
        let (status, text) = self.dispatch("POST", "/account/api/oauth/token", &headers,
            Some((body, "application/x-www-form-urlencoded"))).await?;
        Self::parse_json(status, &text)
    }

    /// Creates permanent DeviceAuth credentials for the given account.
    pub async fn create_device_auth(&self, account_id: &str, access_token: &str) -> Result<DeviceAuth, EpicError> {
        let path = format!("/account/api/public/account/{}/deviceAuth", account_id);
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch("POST", &path, &headers, Some(("{}".to_string(), "application/json")))
            .await?;

        Self::parse_json(status, &text)
    }

    /// Headlessly refreshes an OAuth session using permanent DeviceAuth credentials.
    pub async fn login_with_device_auth(&self, auth: &DeviceAuth) -> Result<OAuthTokenResponse, EpicError> {
        let form_body = format!(
            "grant_type=device_auth&account_id={}&device_id={}&secret={}&token_type=eg1",
            auth.account_id, auth.device_id, auth.secret
        );

        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_BASIC_AUTH.to_string()),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch(
                "POST",
                "/account/api/oauth/token",
                &headers,
                Some((form_body, "application/x-www-form-urlencoded")),
            )
            .await?;

        Self::parse_json(status, &text)
    }

    /// Refreshes an OAuth session using a standard refresh token.
    pub async fn login_with_refresh_token(&self, refresh_token: &str) -> Result<OAuthTokenResponse, EpicError> {
        let form_body = format!(
            "grant_type=refresh_token&refresh_token={}&token_type=eg1",
            refresh_token
        );

        let headers = [
            (header::AUTHORIZATION.as_str(), EPIC_BASIC_AUTH.to_string()),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch(
                "POST",
                "/account/api/oauth/token",
                &headers,
                Some((form_body, "application/x-www-form-urlencoded")),
            )
            .await?;

        Self::parse_json(status, &text)
    }

    /// Retrieves a single-use exchange code (valid for 300s) for game CLI launching.
    pub async fn get_exchange_code(&self, access_token: &str) -> Result<ExchangeCodeResponse, EpicError> {
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch("GET", "/account/api/oauth/exchange", &headers, None)
            .await?;

        Self::parse_json(status, &text)
    }

    /// Revokes a specific DeviceAuth credential from Epic's backend (account logout).
    pub async fn delete_device_auth(&self, account_id: &str, device_id: &str, access_token: &str) -> Result<(), EpicError> {
        let path = format!("/account/api/public/account/{}/deviceAuth/{}", account_id, device_id);
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch("DELETE", &path, &headers, None)
            .await?;

        if status.is_success() || status == StatusCode::NO_CONTENT {
            Ok(())
        } else {
            if let Ok(err_payload) = serde_json::from_str::<EpicApiErrorResponse>(&text) {
                Err(EpicError::Api {
                    status,
                    code: err_payload.error_code,
                    message: err_payload.error_message.unwrap_or_else(|| "Failed to delete device auth".to_string()),
                    numeric: err_payload.numeric_error_code,
                })
            } else {
                Err(EpicError::Api {
                    status,
                    code: None,
                    message: if text.is_empty() {
                        status.canonical_reason().unwrap_or("HTTP error").to_string()
                    } else {
                        text
                    },
                    numeric: None,
                })
            }
        }
    }

    /// Fetches a single paginated batch of cloud-owned entitlements for an account.
    pub async fn fetch_entitlements(
        &self,
        access_token: &str,
        account_id: &str,
        start: u32,
        count: u32,
    ) -> Result<Vec<Entitlement>, EpicError> {
        let path = format!(
            "/entitlement/api/account/{}/entitlements?start={}&count={}",
            account_id, start, count
        );
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self.dispatch("GET", &path, &headers, None).await?;
        Self::parse_json(status, &text)
    }

    /// Fetches all entitlements across all pages by automatically looping with batch size 1000.
    pub async fn fetch_all_entitlements(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> Result<Vec<Entitlement>, EpicError> {
        let mut all = Vec::new();
        let mut start: u32 = 0;
        let count: u32 = 1000;

        loop {
            let batch = self
                .fetch_entitlements(access_token, account_id, start, count)
                .await?;
            let fetched = batch.len() as u32;
            all.extend(batch);

            if fetched < count {
                break;
            }
            start += count;
        }

        Ok(all)
    }

    /// Fetches bulk catalog item metadata for a specific namespace and list of item IDs.
    /// Resiliently accepts either Map (`{"id": {...}}`) or Array (`[{...}]`) wire JSON.
    pub async fn fetch_catalog_items(
        &self,
        access_token: &str,
        namespace: &str,
        item_ids: &[&str],
    ) -> Result<HashMap<String, CatalogItem>, EpicError> {
        let mut path = format!(
            "/catalog/api/shared/namespace/{}/bulk/items",
            namespace
        );

        if !item_ids.is_empty() {
            let query: Vec<String> = item_ids.iter().map(|id| format!("id={}", id)).collect();
            path = format!("{}?{}", path, query.join("&"));
        }

        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];

        let (status, text) = self.dispatch("GET", &path, &headers, None).await?;

        if status.is_success() {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum CatalogWireResponse {
                Map(HashMap<String, CatalogItem>),
                List(Vec<CatalogItem>),
            }

            let parsed: CatalogWireResponse = serde_json::from_str(&text)?;
            match parsed {
                CatalogWireResponse::Map(map) => Ok(map),
                CatalogWireResponse::List(list) => {
                    let map = list.into_iter().map(|item| (item.id.clone(), item)).collect();
                    Ok(map)
                }
            }
        } else {
            Self::parse_json::<HashMap<String, CatalogItem>>(status, &text)
        }
    }

    /// Read-only launcher lifetime playtime. Uses the selected account's bearer token,
    /// without changing the active/native launcher session.
    pub async fn fetch_playtime(&self, access_token: &str, account_id: &str) -> Result<serde_json::Value, EpicError> {
        let path = format!("/library/api/public/playtime/account/{account_id}/all");
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {access_token}")),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];
        let (status, text) = self.dispatch("GET", &path, &headers, None).await?;
        Self::parse_json(status, &text)
    }

    /// Fetches detailed public account profile (2FA, country, email verification, external platform links).
    pub async fn fetch_account_details(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> Result<EpicAccountDetails, EpicError> {
        let path = format!("/account/api/public/account/{}", account_id);
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), EPIC_USER_AGENT.to_string()),
        ];
        let (status, text) = self.dispatch("GET", &path, &headers, None).await?;
        Self::parse_json(status, &text)
    }

    /// Exchanges an Epic Games launcher access token for a specialized Fortnite game client token
    /// that possesses `fortnite:profile:<accountId>:commands ALL` permissions.
    pub async fn exchange_for_fortnite_token(&self, launcher_access_token: &str) -> Result<String, EpicError> {
        let exchange_resp = self.get_exchange_code(launcher_access_token).await?;
        let form_body = format!(
            "grant_type=exchange_code&exchange_code={}&token_type=eg1",
            exchange_resp.code
        );

        let headers = [
            (header::AUTHORIZATION.as_str(), FORTNITE_BASIC_AUTH.to_string()),
            (header::USER_AGENT.as_str(), FORTNITE_USER_AGENT.to_string()),
        ];

        let (status, text) = self
            .dispatch(
                "POST",
                "/account/api/oauth/token",
                &headers,
                Some((form_body, "application/x-www-form-urlencoded")),
            )
            .await?;

        let token_resp: OAuthTokenResponse = Self::parse_json(status, &text)?;
        Ok(token_resp.access_token)
    }

    /// Queries a Fortnite Microservice Client Platform (MCP) profile (e.g. "athena" or "common_core").
    pub async fn query_fortnite_profile(
        &self,
        access_token: &str,
        account_id: &str,
        profile_id: &str,
    ) -> Result<FortniteMcpResponse, EpicError> {
        let path = format!(
            "/fortnite/api/game/v2/profile/{}/client/QueryProfile?profileId={}&rvn=-1",
            account_id, profile_id
        );
        let headers = [
            (header::AUTHORIZATION.as_str(), format!("Bearer {}", access_token)),
            (header::USER_AGENT.as_str(), FORTNITE_USER_AGENT.to_string()),
        ];
        let (status, text) = self
            .dispatch("POST", &path, &headers, Some(("{}".to_string(), "application/json")))
            .await?;
        Self::parse_json(status, &text)
    }

    /// Fetches a complete account and Fortnite summary matching LZT Market's checker.
    /// Combines account details, Battle Royale cosmetic locker (athena), and wallet/V-Bucks (common_core).
    pub async fn fetch_fortnite_summary(
        &self,
        access_token: &str,
        account_id: &str,
    ) -> Result<FortniteAccountSummary, EpicError> {
        // 1. Fetch Epic account details (using launcher token)
        let details = match self.fetch_account_details(access_token, account_id).await {
            Ok(d) => d,
            Err(EpicError::Api { status, .. }) if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN => {
                return Err(EpicError::Auth(format!("Epic authentication failed ({status})")));
            }
            Err(_) => EpicAccountDetails {
                id: account_id.to_string(),
                ..Default::default()
            },
        };

        // 2. Obtain specialized Fortnite game token (which has `fortnite:profile:commands ALL` scope).
        // Fall back to launcher access_token if exchange fails (e.g. in offline unit tests).
        let fn_token = match self.exchange_for_fortnite_token(access_token).await {
            Ok(token) => token,
            Err(e) => {
                tracing::debug!("Using launcher token directly for Fortnite queries: {}", e);
                access_token.to_string()
            }
        };

        let mut summary = FortniteAccountSummary {
            account_id: account_id.to_string(),
            display_name: details.display_name.unwrap_or_else(|| account_id.to_string()),
            email: details.email,
            email_verified: details.email_verified,
            tfa_enabled: details.tfa_enabled,
            country: details.country,
            can_update_display_name: details.can_update_display_name,
            last_login: details.last_login,
            account_created: details.created,
            external_auths: details.external_auths,
            updated_at: chrono::Utc::now().timestamp(),
            ..Default::default()
        };

        // 3. Query common_core profile for V-Bucks and account flags using Fortnite token
        match self.query_fortnite_profile(&fn_token, account_id, "common_core").await {
            Ok(common_core) => {
                summary.has_fortnite = true;
                for change in common_core.profile_changes {
                    if let Some(prof) = change.profile {
                        for (_k, item) in prof.items {
                            let tmpl = item.template_id.as_str();
                            let tmpl_lower = tmpl.to_lowercase();
                            let qty = item.quantity.unwrap_or(0);
                            if tmpl_lower.starts_with("currency:mtx") {
                                summary.total_vbucks += qty;
                                if tmpl_lower.contains("purchased") {
                                    summary.vbucks_purchased += qty;
                                } else if tmpl_lower.contains("earned") {
                                    summary.vbucks_earned += qty;
                                }
                            } else if tmpl_lower.contains("campaignaccess") || tmpl_lower.contains("founder") || tmpl_lower.contains("founderspack") {
                                summary.has_save_the_world = true;
                                if tmpl_lower.contains("founder") || tmpl_lower.contains("founderspack") {
                                    summary.is_stw_founder = true;
                                }
                            }
                        }
                    }
                }
            }
            Err(EpicError::Api { status, message, .. }) if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN => {
                return Err(EpicError::Auth(format!("Epic authentication failed on common_core ({status}): {message}")));
            }
            Err(_) => {}
        }

        // 4. Query athena profile for locker items, level, battle pass, and seasons using Fortnite token
        match self.query_fortnite_profile(&fn_token, account_id, "athena").await {
            Ok(athena) => {
                summary.has_fortnite = true;
                let mut cosmetics_map: HashMap<String, FortniteCosmeticItem> = HashMap::new();

                for change in athena.profile_changes {
                    if let Some(prof) = change.profile {
                        // Extract stats
                        if let Some(stats) = prof.stats {
                            if let Some(attrs) = stats.attributes {
                                if let Some(lvl) = attrs.level {
                                    summary.season_level = lvl;
                                }
                                if let Some(acc_lvl) = attrs.account_level {
                                    summary.account_level = acc_lvl;
                                }
                                if let Some(b_lvl) = attrs.book_level {
                                    summary.battle_pass_level = b_lvl;
                                }
                                if let Some(b_pur) = attrs.book_purchased {
                                    summary.battle_pass_purchased = b_pur;
                                }
                                if let Some(wins) = attrs.lifetime_wins {
                                    summary.lifetime_wins = wins;
                                }
                                if let Some(past) = attrs.past_seasons {
                                    summary.past_seasons = past;
                                }
                                if let Some(last_match) = attrs.last_match_end_datetime {
                                    summary.last_match_end_datetime = Some(last_match);
                                }
                            }
                        }

                        // Extract locker items
                        for (_guid, raw) in prof.items {
                            let tmpl = &raw.template_id;
                            let tmpl_lower = tmpl.to_lowercase();
                            if tmpl_lower.starts_with("athenacharacter:")
                                || tmpl_lower.starts_with("athenabackpack:")
                                || tmpl_lower.starts_with("athenapickaxe:")
                                || tmpl_lower.starts_with("athenaglider:")
                                || tmpl_lower.starts_with("athenadance:")
                                || tmpl_lower.starts_with("athenaitemwrap:")
                                || tmpl_lower.starts_with("athenamusicpack:")
                                || tmpl_lower.starts_with("athenaskydivecontrail:")
                                || tmpl_lower.starts_with("athenaloadingscreen:")
                            {
                                let parts: Vec<&str> = tmpl.splitn(2, ':').collect();
                                let clean_id = parts.get(1).unwrap_or(&tmpl.as_str()).to_lowercase();
                                let (name, cosmetic_type, rarity, is_exclusive) = resolve_cosmetic_metadata(tmpl);

                                let is_favorite = raw.attributes.as_ref().and_then(|a| a.favorite).unwrap_or(false);
                                let date_acquired = raw.attributes.as_ref().and_then(|a| a.creation_time.clone());

                                let mut variant_styles_set = std::collections::HashSet::new();
                                if let Some(ref attrs) = raw.attributes {
                                    if let Some(ref vars) = attrs.variants {
                                        for v in vars {
                                            if let Some(ref act) = v.active {
                                                variant_styles_set.insert(act.clone());
                                            }
                                            if let Some(ref owned) = v.owned {
                                                for o in owned {
                                                    variant_styles_set.insert(o.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                                let mut variant_styles: Vec<String> = variant_styles_set.into_iter().collect();
                                variant_styles.sort();

                                // Check founder indicators
                                if tmpl_lower.contains("founder") {
                                    summary.has_save_the_world = true;
                                    summary.is_stw_founder = true;
                                }

                                let image_url = format!(
                                    "https://fortnite-api.com/images/cosmetics/br/{}/icon.png",
                                    clean_id
                                );

                                let cosmetic = FortniteCosmeticItem {
                                    id: clean_id.clone(),
                                    template_id: tmpl.clone(),
                                    name,
                                    cosmetic_type,
                                    rarity,
                                    description: None,
                                    image_url,
                                    is_favorite,
                                    is_exclusive,
                                    variants_count: variant_styles.len(),
                                    variant_styles,
                                    date_acquired,
                                };

                                cosmetics_map.insert(clean_id, cosmetic);
                            }
                        }
                    }
                }

                let mut cosmetics: Vec<FortniteCosmeticItem> = cosmetics_map.into_values().collect();

                // Order: Favorites first, then Exclusives, then by rarity tier, then alphabetically
                let rarity_rank = |r: &str| match r.to_lowercase().as_str() {
                    "mythic" => 10,
                    "gaminglegends" | "marvel" | "dc" | "starwars" | "icon" | "lava" | "frozen" | "shadow" | "slurp" => 9,
                    "legendary" => 8,
                    "epic" => 7,
                    "rare" => 6,
                    "uncommon" => 5,
                    _ => 1,
                };

                cosmetics.sort_by(|a, b| {
                    b.is_favorite
                        .cmp(&a.is_favorite)
                        .then_with(|| b.is_exclusive.cmp(&a.is_exclusive))
                        .then_with(|| rarity_rank(&b.rarity).cmp(&rarity_rank(&a.rarity)))
                        .then_with(|| a.name.cmp(&b.name))
                });

                // Compute counts
                summary.total_cosmetics = cosmetics.len();
                summary.outfits_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Outfit).count();
                summary.backpacks_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Backpack).count();
                summary.pickaxes_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Pickaxe).count();
                summary.gliders_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Glider).count();
                summary.emotes_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Emote).count();
                summary.wraps_count = cosmetics.iter().filter(|c| c.cosmetic_type == FortniteCosmeticType::Wrap).count();
                summary.exclusive_count = cosmetics.iter().filter(|c| c.is_exclusive).count();
                summary.cosmetics = cosmetics;
            }
            Err(EpicError::Api { status, message, .. }) if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN => {
                return Err(EpicError::Auth(format!("Epic authentication failed on athena ({status}): {message}")));
            }
            Err(_) => {}
        }

        // Fallback account_level to sum of past seasons + current season if not reported explicitly
        if summary.account_level == 0 && summary.season_level > 0 {
            let past_sum: i64 = summary.past_seasons.iter().filter_map(|s| s.season_level).sum();
            summary.account_level = summary.season_level + past_sum;
        }

        // Calculate days inactive from last match or last login
        if let Some(ref ts) = summary.last_match_end_datetime.as_ref().or(summary.last_login.as_ref()) {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                let diff = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_days();
                summary.days_inactive = Some(diff.max(0));
            }
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod web_cookie_tests {
    use super::*;
    use reqwest::ResponseBuilderExt;

    fn response(url: &str, headers: &[&str]) -> reqwest::Response {
        let mut builder = axum::http::Response::builder().url(reqwest::Url::parse(url).unwrap());
        for value in headers {
            builder = builder.header(header::SET_COOKIE, *value);
        }
        builder.body(String::new()).unwrap().into()
    }

    fn bearer(domain: &str, value: &str) -> crate::auto_claim::web::Cookie {
        crate::auto_claim::web::Cookie {
            name: "EPIC_BEARER_TOKEN".into(),
            value: value.into(),
            domain: domain.into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: None,
            same_site: None,
        }
    }

    #[test]
    fn epic_web_bearer_ignores_legacy_scope_and_expired_duplicates() {
        let mut expired = bearer(".epicgames.com", "expired");
        expired.expires = Some(1000.0);
        let cookies = [
            bearer("epicgames.com", "legacy"),
            expired,
            bearer(".epicgames.com", "fresh"),
        ];
        assert_eq!(epic_web_bearer(&cookies, 1000.0).unwrap().value, "fresh");
        for expires in [0.0, -2.0, 999.0] {
            let mut cookie = bearer(".epicgames.com", "expired");
            cookie.expires = Some(expires);
            assert!(epic_web_bearer(&[cookie], 1000.0).is_err());
        }
        let mut cookie = bearer(".epicgames.com", "session");
        cookie.expires = Some(-1.0);
        assert!(epic_web_bearer(&[cookie], 1000.0).is_ok());
        let mut cookie = bearer(".epicgames.com", "insecure");
        cookie.secure = false;
        assert!(epic_web_bearer(&[cookie], 1000.0).is_err());
        assert!(epic_web_bearer(&[bearer(".epicgames.com", "")], 1000.0).is_err());
    }

    #[test]
    fn epic_web_bearer_rejects_conflicting_active_domain_values() {
        assert!(epic_web_bearer(
            &[
                bearer(".epicgames.com", "first"),
                bearer(".epicgames.com", "other")
            ],
            1000.0
        )
        .is_err());
        assert!(epic_web_bearer(
            &[
                bearer(".epicgames.com", "same"),
                bearer(".epicgames.com", "same")
            ],
            1000.0
        )
        .is_ok());
    }

    #[tokio::test]
    async fn epic_web_owner_verification_uses_scoped_bearer_and_rejects_other_account() {
        let client = EpicClient::from_router(Router::new().route(
            "/account/api/oauth/verify",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                assert_eq!(headers.get(header::AUTHORIZATION).unwrap(), "Bearer fresh");
                axum::Json(serde_json::json!({"account_id": "expected-account"}))
            }),
        ));
        let cookies = [
            bearer("epicgames.com", "legacy"),
            bearer(".epicgames.com", "fresh"),
        ];
        assert!(client
            .verify_web_identity(&cookies, "expected-account")
            .await
            .is_ok());
        assert!(client
            .verify_web_identity(&cookies, "another-account")
            .await
            .is_err());
    }

    #[test]
    fn epic_web_cookies_preserve_domain_and_host_only_scopes() {
        let response = response(
            "https://www.epicgames.com/id/api/set-sid",
            &[
                "EPIC_SSO=fixture; Domain=.epicgames.com; Path=/; Secure; HttpOnly",
                "parent=fixture; Domain=epicgames.com; Path=/",
                "host=fixture; Path=/",
                "host_domain=fixture; Domain=www.epicgames.com; Path=/",
                "default_path=fixture",
            ],
        );
        let mut cookies = vec![];
        capture_epic_web_cookies(&response, &mut cookies, 1700000000.0);
        let cookie = |name: &str| cookies.iter().find(|c| c.name == name).unwrap();
        assert_eq!(cookie("EPIC_SSO").domain, ".epicgames.com");
        assert_eq!(cookie("parent").domain, ".epicgames.com");
        assert_eq!(cookie("host").domain, "www.epicgames.com");
        assert_eq!(cookie("host_domain").domain, ".www.epicgames.com");
        assert_eq!(cookie("default_path").path, "/id/api");
        assert!(cookie("EPIC_SSO").secure);
        assert!(cookie("EPIC_SSO").http_only);
    }

    #[test]
    fn epic_web_cookies_merge_the_full_chain_by_scope_and_apply_deletions() {
        let mut cookies = vec![];
        for (stage, values) in [
            (
                "reputation",
                vec![
                    "reputation=fixture; Path=/",
                    "XSRF-TOKEN=fixture; Path=/id/api",
                ],
            ),
            ("csrf", vec!["XSRF-TOKEN=new; Path=/id/api"]),
            (
                "exchange",
                vec!["EPIC_SESSION_AP=fixture; Domain=epicgames.com; Path=/"],
            ),
            (
                "redirect",
                vec!["EPIC_BEARER_TOKEN=old; Domain=epicgames.com; Path=/"],
            ),
            (
                "set-sid",
                vec![
                    "EPIC_SSO=fixture; Domain=epicgames.com; Path=/",
                    "EPIC_BEARER_TOKEN=fresh; Domain=.epicgames.com; Path=/",
                    "XSRF-TOKEN=; Path=/id/api; Max-Age=0",
                ],
            ),
        ] {
            capture_epic_web_cookies(
                &response(
                    &format!("https://www.epicgames.com/id/api/{stage}"),
                    &values,
                ),
                &mut cookies,
                1700000000.0,
            );
        }
        assert_eq!(cookies.len(), 4);
        assert!(cookies.iter().any(|c| c.name == "reputation"));
        assert!(cookies.iter().any(|c| c.name == "EPIC_SESSION_AP"));
        assert!(cookies.iter().any(|c| c.name == "EPIC_SSO"));
        assert_eq!(
            cookies
                .iter()
                .find(|c| c.name == "EPIC_BEARER_TOKEN")
                .unwrap()
                .value,
            "fresh"
        );
        assert!(!cookies.iter().any(|c| c.name == "XSRF-TOKEN"));

        capture_epic_web_cookies(
            &response(
                "https://www.epicgames.com/id/api/set-sid",
                &[
                    "EPIC_SSO=host; Path=/",
                    "EPIC_SSO=child; Domain=epicgames.com; Path=/id",
                ],
            ),
            &mut cookies,
            1700000000.0,
        );
        assert_eq!(
            cookies.iter().filter(|c| c.name == "EPIC_SSO").count(),
            3,
            "Different HTTP domain/path scopes must not overwrite each other"
        );
    }

    #[test]
    fn epic_web_cookies_preserve_samesite_and_max_age_precedence() {
        let now = 1700000000.0;
        let mut cookies = vec![];
        capture_epic_web_cookies(&response("https://www.epicgames.com/id/api/set-sid", &[
            "lax=fixture; SameSite=Lax; Max-Age=60; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/",
            "strict=fixture; SameSite=Strict; Path=/",
            "cross_site=fixture; samesite=none; Secure; Path=/",
            "unspecified=fixture; Path=/",
        ]), &mut cookies, now);
        let cookie = |name: &str| cookies.iter().find(|c| c.name == name).unwrap();
        assert_eq!(cookie("lax").expires, Some(now + 60.0));
        assert_eq!(cookie("lax").same_site.as_deref(), Some("Lax"));
        assert_eq!(cookie("strict").same_site.as_deref(), Some("Strict"));
        assert_eq!(cookie("cross_site").same_site.as_deref(), Some("None"));
        assert_eq!(cookie("unspecified").same_site, None);
        capture_epic_web_cookies(
            &response("https://www.epicgames.com/id/api/set-sid", &[]),
            &mut cookies,
            now + 60.0,
        );
        assert!(!cookies.iter().any(|c| c.name == "lax"));
    }

    #[test]
    fn epic_web_cookies_drop_expired_negative_age_and_unrelated_domains() {
        let mut cookies = vec![];
        let url = "https://www.epicgames.com/id/api/set-sid";
        capture_epic_web_cookies(
            &response(
                url,
                &[
                    "remove=fixture; Domain=epicgames.com; Path=/",
                    "past=fixture; Expires=Wed, 31 Dec 1969 23:59:59 GMT; Path=/",
                    "foreign=fixture; Domain=other.example; Path=/",
                    "sibling=fixture; Domain=store.epicgames.com; Path=/",
                    "malformed header",
                ],
            ),
            &mut cookies,
            1700000000.0,
        );
        assert_eq!(cookies.len(), 1);
        capture_epic_web_cookies(&response(url, &[
            "remove=; Domain=.epicgames.com; Path=/; Max-Age=-1; Expires=Thu, 01 Jan 2038 00:00:00 GMT",
        ]), &mut cookies, 1700000001.0);
        assert!(cookies.is_empty());
        capture_epic_web_cookies(
            &response(
                "https://other.example/id/api/set-sid",
                &["foreign=fixture; Path=/"],
            ),
            &mut cookies,
            1700000002.0,
        );
        assert!(cookies.is_empty());
    }
}
