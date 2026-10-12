//! Untrusted discovery metadata. A candidate is never proof that a store will
//! charge zero or grant a permanent game licence.
use chrono::{DateTime, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GiveawayCandidate {
    pub source_id: String,
    pub title: String,
    pub source_url: String,
    /// Only a direct, validated store URL. GamerPower redirects stay in source_url.
    pub claim_url: Option<String>,
    pub platform: String,
    /// GamerPower's estimated value in USD cents; never a checkout price.
    pub worth: i64,
    pub ends_at: i64,
    pub needs_store_validation: bool,
}
/// Only literal redirects; never executes the source site's JavaScript.
pub fn html_redirect(html: &str) -> Option<String> {
    let mut targets = BTreeSet::new();
    let mut insert = |url: &str| {
        if let Some((platform, url)) = trusted_claim_url(url) {
            if super::platforms::product_url(&platform, &url) {
                targets.insert(url);
            }
        }
    };
    for tag in html
        .split('<')
        .map(|s| s.split('>').next().unwrap_or_default())
    {
        if tag.to_ascii_lowercase().starts_with("meta ")
            && super::offers::attr(tag, "http-equiv")
                .is_some_and(|s| s.eq_ignore_ascii_case("refresh"))
        {
            if let Some(content) = super::offers::attr(tag, "content") {
                if let Some((_, url)) = content
                    .split_once("url=")
                    .or_else(|| content.split_once("URL="))
                {
                    insert(url.trim().trim_matches(['\'', '"']));
                }
            }
        }
    }
    for prefix in [
        "window.location.replace(",
        "window.location.assign(",
        "window.location.href = ",
        "window.location.href=",
    ] {
        for tail in html.split(prefix).skip(1) {
            let tail = tail.trim_start();
            if let Some(quote) = tail.chars().next().filter(|q| *q == '\'' || *q == '"') {
                if let Some(url) = tail[1..].split(quote).next() {
                    insert(url);
                }
            }
        }
    }
    if targets.len() == 1 {
        targets.pop_first()
    } else {
        None
    }
}

/// Parse paid, active, finite full-game giveaways. Unknown stores and incomplete
/// entries are excluded. Naive feed dates are interpreted as UTC for discovery;
/// the actual store must revalidate the promotion and its local deadline.
pub fn gamerpower_candidates(data: &Value, now: i64) -> Result<Vec<GiveawayCandidate>, String> {
    let entries = data
        .as_array()
        .ok_or("GamerPower вернул неизвестный формат списка раздач.")?;
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for entry in entries {
        let Some(candidate) = candidate(entry, now) else {
            continue;
        };
        if seen.insert((candidate.platform.clone(), candidate.source_id.clone())) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

fn candidate(entry: &Value, now: i64) -> Option<GiveawayCandidate> {
    if !entry["status"]
        .as_str()?
        .trim()
        .eq_ignore_ascii_case("active")
        || !entry["type"].as_str()?.trim().eq_ignore_ascii_case("game")
        || ["is_free_to_play", "is_f2p", "is_permanent"]
            .iter()
            .any(|field| entry[*field].as_bool() == Some(true))
    {
        return None;
    }
    let source_id = source_id(&entry["id"])?;
    let title = entry["title"].as_str()?.trim();
    if title.is_empty() || title.len() > 1024 || disallowed_content(entry) {
        return None;
    }
    let worth = usd_cents(entry["worth"].as_str()?)?;
    let start = timestamp(entry.get("start_date").unwrap_or(&entry["published_date"]))?;
    let ends_at = timestamp(&entry["end_date"])?;
    if start > now || ends_at <= now || ends_at <= start {
        return None;
    }

    let labels: BTreeSet<&str> = entry["platforms"]
        .as_str()?
        .split([',', ';'])
        .filter_map(platform_label)
        .collect();
    let mut source_url = match entry.get("gamerpower_url") {
        None | Some(Value::Null) => None,
        Some(value) => Some(gamerpower_url(value.as_str()?)?),
    };
    let mut direct = None;
    for field in ["claim_link", "open_giveaway_url", "open_giveaway"] {
        let Some(value) = entry.get(field) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let input = value.as_str()?;
        if input.trim().is_empty() {
            continue;
        }
        if let Some((platform, url)) = trusted_claim_url(input) {
            if direct.is_none() {
                direct = Some((platform, url));
            }
        } else {
            let url = gamerpower_url(input)?;
            // The redirect is only a discovery source. Consumers must validate
            // every Location before resolving it, without account credentials.
            if direct.is_none() {
                source_url = Some(url);
            }
        }
    }
    let platform = if labels.len() == 1 {
        *labels.first()?
    } else {
        let host_platform = direct.as_ref()?.0.as_str();
        if !labels.is_empty() && !labels.contains(host_platform) {
            return None;
        }
        host_platform
    };
    let claim_url = direct
        .as_ref()
        .filter(|(host_platform, _)| host_platform == platform)
        .map(|(_, url)| url.clone());
    Some(GiveawayCandidate {
        source_url: source_url
            .unwrap_or_else(|| format!("https://www.gamerpower.com/api/giveaway?id={source_id}")),
        source_id,
        title: canonical_game_title(title),
        claim_url,
        platform: platform.to_string(),
        worth,
        ends_at,
        needs_store_validation: true,
    })
}
/// Strip only feed decorations, retaining edition and remaster names. Matching
/// the resulting title in the store is still required before a browser action.
pub fn canonical_game_title(value: &str) -> String {
    let tags = [
        " (Steam)",
        " (GOG)",
        " (Epic Games)",
        " (Prime Gaming)",
        " (PC)",
        " (Itch.io)",
        " (IndieGala)",
        " (Xbox)",
        " (Microsoft)",
    ];
    let mut title = value.trim().to_string();
    let strip_tags = |title: &mut String| {
        for _ in 0..2 {
            let lower = title.to_ascii_lowercase();
            if let Some(tag) = tags
                .iter()
                .find(|t| lower.ends_with(&t.to_ascii_lowercase()))
            {
                title.truncate(title.len() - tag.len());
                *title = title.trim().to_string();
            } else {
                break;
            }
        }
    };
    strip_tags(&mut title);
    if title.to_ascii_lowercase().ends_with(" giveaway") {
        title.truncate(title.len() - " giveaway".len());
        title = title.trim().to_string();
    }
    for suffix in [" Steam Key", " GOG Key", " Epic Games Key", " Key"] {
        if title
            .to_ascii_lowercase()
            .ends_with(&suffix.to_ascii_lowercase())
        {
            title.truncate(title.len() - suffix.len());
            title = title.trim().to_string();
            break;
        }
    }
    strip_tags(&mut title);
    title
}

/// A store-host allowlist, suitable for checking each hop of a separate redirect
/// resolver. This function neither follows URLs nor authorizes claims. Explicit
/// ports (including :443), credentials, encoded hosts and non-HTTPS are rejected.
pub fn trusted_claim_url(input: &str) -> Option<(String, String)> {
    let url = strict_https_url(input)?;
    let host = url.host_str()?;
    let platform = match host {
        "store.steampowered.com" => "steam",
        "store.epicgames.com" | "www.epicgames.com" | "epicgames.com" => "epic",
        "www.gog.com" | "gog.com" => "gog",
        "gaming.amazon.com" | "luna.amazon.com" => "prime",
        "apps.microsoft.com"
        | "www.microsoft.com"
        | "microsoft.com"
        | "redeem.microsoft.com"
        | "www.xbox.com"
        | "xbox.com" => "microsoft",
        "store.ubisoft.com" | "www.ubisoft.com" | "ubisoft.com" => "ubisoft",
        "www.fanatical.com" | "fanatical.com" => "fanatical",
        "itch.io" => "itchio",
        "www.indiegala.com" | "indiegala.com" | "freebies.indiegala.com" => "indiegala",
        "alienwarearena.com" | "www.alienwarearena.com" => "alienware",
        _ if host.ends_with(".itch.io") => "itchio",
        _ if host.ends_with(".alienwarearena.com") => "alienware",
        _ => return None,
    };
    Some((platform.to_string(), url.to_string()))
}

fn strict_https_url(input: &str) -> Option<reqwest::Url> {
    if input.is_empty()
        || input.len() > 8192
        || input.trim() != input
        || input.chars().any(|c| c.is_control() || c == '\\')
    {
        return None;
    }
    let (scheme, tail) = input.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = tail.split(['/', '?', '#']).next()?;
    if authority.is_empty() || !authority.is_ascii() || authority.contains([':', '@', '%']) {
        return None;
    }
    let url = reqwest::Url::parse(input).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !authority.eq_ignore_ascii_case(url.host_str()?)
    {
        return None;
    }
    Some(url)
}

fn gamerpower_url(input: &str) -> Option<String> {
    let url = strict_https_url(input)?;
    if !matches!(
        url.host_str(),
        Some("www.gamerpower.com" | "gamerpower.com")
    ) {
        return None;
    }
    Some(url.to_string())
}

fn platform_label(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "steam" => Some("steam"),
        "epic" | "epic games" | "epic games store" | "epic-games-store" => Some("epic"),
        "gog" | "gog.com" => Some("gog"),
        "prime"
        | "prime gaming"
        | "amazon prime gaming"
        | "amazon games"
        | "amazon luna"
        | "luna" => Some("prime"),
        "microsoft" | "microsoft store" | "windows store" | "xbox" | "xbox one"
        | "xbox series x/s" => Some("microsoft"),
        "ubisoft" | "ubisoft connect" => Some("ubisoft"),
        "fanatical" => Some("fanatical"),
        "itch.io" | "itchio" => Some("itchio"),
        "indiegala" | "indie gala" => Some("indiegala"),
        "alienware" | "alienware arena" => Some("alienware"),
        _ => None,
    }
}

fn source_id(value: &Value) -> Option<String> {
    let id = value.as_u64().or_else(|| {
        let text = value.as_str()?;
        (!text.is_empty() && text.bytes().all(|c| c.is_ascii_digit()))
            .then(|| text.parse::<u64>().ok())
            .flatten()
    })?;
    (id > 0).then(|| id.to_string())
}

fn timestamp(value: &Value) -> Option<i64> {
    let text = value.as_str()?.trim();
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date| date.timestamp())
        .or_else(|| {
            NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|date| date.and_utc().timestamp())
        })
}

fn usd_cents(value: &str) -> Option<i64> {
    let value = value.trim().strip_prefix('$')?;
    let mut parts = value.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("00");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.is_empty()
        || fraction.len() > 2
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let cents =
        whole.parse::<i64>().ok()?.checked_mul(100)?.checked_add(
            fraction.parse::<i64>().ok()? * if fraction.len() == 1 { 10 } else { 1 },
        )?;
    (cents > 0).then_some(cents)
}

fn disallowed_content(entry: &Value) -> bool {
    let text = ["title", "description", "instructions"]
        .iter()
        .filter_map(|field| entry[*field].as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let words: Vec<_> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.iter().any(|word| {
        matches!(
            *word,
            "dlc" | "demo" | "beta" | "playtest" | "trial" | "f2p"
        )
    }) {
        return true;
    }
    let normalized = format!(" {} ", words.join(" "));
    [
        " free to play ",
        " free weekend ",
        " free weekends ",
        " permanently free ",
        " permanent free ",
        " free forever ",
        " always free ",
        " early access ",
        " weekend pass ",
    ]
    .iter()
    .any(|phrase| normalized.contains(phrase))
}
