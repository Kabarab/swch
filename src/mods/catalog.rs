use super::{types::*, ModManager};
use futures_util::StreamExt;
use serde_json::json;
use std::fs;

pub const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/Kabarab/swch/mod-catalog/index.json";

pub fn github_download(url: &str) -> bool {
    reqwest::Url::parse(url).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("github.com")
            && u.username().is_empty()
            && u.password().is_none()
            && u.path().split('/').collect::<Vec<_>>().get(3..5)
                == Some(&["releases", "download"][..])
    })
}

pub async fn bounded_download(url: &str, max: u64) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .user_agent("SWCH-Mods/1")
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|s| s > max) {
        return Err("Download too large".into());
    }
    let mut out = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if out.len() as u64 + chunk.len() as u64 > max {
            return Err("Download too large".into());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

impl ModManager {
    pub async fn catalog(&self, refresh: bool) -> Result<serde_json::Value> {
        let path = self.root.join("catalog.json");
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|t| t.as_secs() < 21600);
        let mut warning = None;
        if refresh || !fresh {
            match bounded_download(CATALOG_URL, 2 * 1024 * 1024).await {
                Ok(bytes) => {
                    let catalog: Catalog =
                        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    validate(&catalog)?;
                    super::write_json(&path, &catalog)?;
                }
                Err(e) => {
                    warning = Some(format!("GitHub catalog unavailable: {e}"));
                }
            }
        }
        let catalog: Catalog = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            Err(_) => Catalog {
                schema: 1,
                ..Default::default()
            },
        };
        validate(&catalog)?;
        let compatibility: std::collections::HashMap<_, _> = catalog
            .mods
            .iter()
            .map(|e| {
                (
                    e.manifest.id.clone(),
                    e.manifest.validate(Some(target())).is_ok(),
                )
            })
            .collect();
        Ok(
            json!({"catalog":catalog,"compatibility":compatibility,"warning":warning,"target":target(),"api":API_VERSION}),
        )
    }
    pub async fn install_catalog(&self, id: &str) -> Result<String> {
        let catalog: Catalog = serde_json::from_value(self.catalog(true).await?["catalog"].clone())
            .map_err(|e| e.to_string())?;
        if catalog.blocked.iter().any(|b| b == id) {
            return Err("Mod blocked by catalog owner".into());
        }
        let entry = catalog
            .mods
            .into_iter()
            .find(|e| e.manifest.id == id)
            .ok_or("Mod not found in catalog")?;
        entry.manifest.validate(Some(target()))?;
        let bytes = bounded_download(&entry.download_url, MAX_PACKAGE).await?;
        self.install_bytes(&bytes, &entry.sha256.clone(), Some(entry))
            .await
    }
}

fn validate(catalog: &Catalog) -> Result<()> {
    if catalog.schema != 1 {
        return Err("Unsupported catalog schema".into());
    }
    let mut ids = std::collections::HashSet::new();
    for entry in &catalog.mods {
        if !valid_id(&entry.manifest.id)
            || !ids.insert(&entry.manifest.id)
            || !github_download(&entry.download_url)
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("Invalid catalog entry".into());
        }
        // Other API generations may remain visible in the catalog but cannot be installed.
        semver::Version::parse(&entry.manifest.version).map_err(|e| e.to_string())?;
    }
    Ok(())
}
