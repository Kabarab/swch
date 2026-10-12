//! Store capabilities. Discovery is not a licence receipt.
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct Platform {
    pub id: &'static str,
    pub name: &'static str,
    pub login_url: &'static str,
    pub giveaways_url: &'static str,
    pub delivery: &'static str,
    pub requires_subscription: bool,
}
pub const PLATFORMS: &[Platform] = &[
    Platform {
        id: "steam",
        name: "Steam",
        login_url: "https://store.steampowered.com/login/",
        giveaways_url: "https://store.steampowered.com/search/?maxprice=free&specials=1",
        delivery: "native",
        requires_subscription: false,
    },
    Platform {
        id: "epic",
        name: "Epic Games",
        login_url: "https://www.epicgames.com/id/login",
        giveaways_url: "https://store.epicgames.com/en-US/free-games",
        delivery: "native_and_browser",
        requires_subscription: false,
    },
    Platform {
        id: "gog",
        name: "GOG",
        login_url: "https://www.gog.com/account",
        giveaways_url: "https://www.gog.com/",
        delivery: "browser",
        requires_subscription: false,
    },
    Platform {
        id: "prime",
        name: "Prime Gaming",
        login_url: "https://luna.amazon.com/claims/home",
        giveaways_url: "https://luna.amazon.com/claims/home",
        delivery: "library_and_codes",
        requires_subscription: true,
    },
    Platform {
        id: "microsoft",
        name: "Microsoft / Xbox",
        login_url: "https://account.microsoft.com/",
        giveaways_url: "https://apps.microsoft.com/",
        delivery: "browser_and_codes",
        requires_subscription: false,
    },
    Platform {
        id: "ubisoft",
        name: "Ubisoft",
        login_url: "https://account.ubisoft.com/",
        giveaways_url: "https://www.ubisoft.com/en-us/games/free",
        delivery: "browser",
        requires_subscription: false,
    },
    Platform {
        id: "fanatical",
        name: "Fanatical",
        login_url: "https://www.fanatical.com/en/login",
        giveaways_url: "https://www.fanatical.com/",
        delivery: "steam_codes",
        requires_subscription: false,
    },
    Platform {
        id: "itchio",
        name: "Itch.io",
        login_url: "https://itch.io/login",
        giveaways_url: "https://itch.io/",
        delivery: "browser",
        requires_subscription: false,
    },
    Platform {
        id: "indiegala",
        name: "IndieGala",
        login_url: "https://www.indiegala.com/login",
        giveaways_url: "https://freebies.indiegala.com/",
        delivery: "browser",
        requires_subscription: false,
    },
    Platform {
        id: "alienware",
        name: "Alienware Arena",
        login_url: "https://www.alienwarearena.com/",
        giveaways_url: "https://www.alienwarearena.com/ucf/Giveaway",
        delivery: "notify_only",
        requires_subscription: false,
    },
];
pub fn platform(id: &str) -> Option<&'static Platform> {
    PLATFORMS.iter().find(|p| p.id == id)
}
/// An existing API session is not proof of a logged-in store page.
pub fn connection_url(id: &str, return_url: &str) -> Result<String, String> {
    let spec = platform(id).ok_or("Неизвестный магазин.")?;
    if !trusted_url(id, return_url) {
        return Err("Адрес не относится к магазину.".into());
    }
    if id != "epic" {
        return Ok(spec.login_url.into());
    }
    let mut url = reqwest::Url::parse(spec.login_url).map_err(|_| "Некорректный адрес входа.")?;
    url.query_pairs_mut().append_pair("redirectUrl", return_url);
    Ok(url.into())
}
/// Locale and display-language queries do not change an Epic product SKU.
pub fn same_product_url(id: &str, left: &str, right: &str) -> bool {
    if !product_url(id, left) || !product_url(id, right) {
        return false;
    }
    if id != "epic" {
        return left == right;
    }
    fn slug(value: &str) -> Option<String> {
        let url = reqwest::Url::parse(value).ok()?;
        let mut segments = url.path_segments()?;
        while let Some(segment) = segments.next() {
            if segment == "p" {
                let product = segments.next()?.to_owned();
                if product.is_empty() || segments.any(|s| !s.is_empty()) {
                    return None;
                }
                return Some(product);
            }
        }
        None
    }
    match (slug(left), slug(right)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}
pub fn login_host_allowed(id: &str, value: &str) -> bool {
    if !trusted_url(id, value) {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    match id {
        "steam" => matches!(host, "store.steampowered.com" | "login.steampowered.com"),
        "epic" => host == "www.epicgames.com",
        "gog" => matches!(host, "login.gog.com" | "www.gog.com"),
        "prime" => [
            "amazon.com",
            "amazon.co.uk",
            "amazon.de",
            "amazon.fr",
            "amazon.it",
            "amazon.es",
            "amazon.ca",
            "amazon.co.jp",
        ]
        .iter()
        .any(|d| host == *d || host == format!("www.{d}")),
        "microsoft" => matches!(
            host,
            "login.live.com"
                | "login.microsoftonline.com"
                | "account.microsoft.com"
                | "account.xbox.com"
        ),
        "ubisoft" => matches!(
            host,
            "account.ubisoft.com" | "login.ubisoft.com" | "connect.ubisoft.com"
        ),
        "fanatical" => matches!(host, "www.fanatical.com" | "fanatical.com"),
        "itchio" => host == "itch.io",
        "indiegala" => matches!(
            host,
            "www.indiegala.com" | "indiegala.com" | "accounts.indiegala.com"
        ),
        _ => false,
    }
}
pub fn trusted_url(id: &str, value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let domains: &[&str] = match id {
        "steam" => &["steampowered.com", "steamcommunity.com"],
        "epic" => &["epicgames.com"],
        "gog" => &["gog.com"],
        "prime" => &[
            "amazon.com",
            "amazon.co.uk",
            "amazon.de",
            "amazon.fr",
            "amazon.it",
            "amazon.es",
            "amazon.ca",
            "amazon.co.jp",
        ],
        "microsoft" => &[
            "microsoft.com",
            "live.com",
            "microsoftonline.com",
            "xbox.com",
        ],
        "ubisoft" => &["ubisoft.com", "ubi.com"],
        "fanatical" => &["fanatical.com"],
        "itchio" => &["itch.io"],
        "indiegala" => &["indiegala.com"],
        "alienware" => &["alienwarearena.com"],
        _ => return false,
    };
    domains
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

pub fn product_url(id: &str, value: &str) -> bool {
    if !trusted_url(id, value) {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    if url.query_pairs().any(|(key, _)| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "sid"
                | "exchangecode"
                | "access_token"
                | "refresh_token"
                | "code"
                | "token"
                | "sessionid"
        )
    }) {
        return false;
    }
    match id {
        "steam" => ["/app/", "/sub/", "/bundle/"]
            .iter()
            .any(|p| path.starts_with(p)),
        "epic" => path.starts_with("/p/") || path.contains("/p/"),
        "gog" => path.contains("/game/") || path == "/",
        "prime" => {
            path.starts_with("/loot/")
                || path.starts_with("/games/")
                || path.starts_with("/claims/")
                || path == "/"
                || path == "/home"
        }
        "microsoft" => {
            path.contains("/detail/") || path.contains("/games/store/") || path.contains("/p/")
        }
        "ubisoft" => {
            path.contains("/game/")
                || path.contains("/games/")
                || url.host_str() == Some("register.ubisoft.com")
        }
        "fanatical" => {
            path.contains("/game/") || path.contains("/bundle/") || path.contains("/giveaway/")
        }
        "itchio" => {
            !path.starts_with("/login")
                && !path.starts_with("/register")
                && !path.starts_with("/settings")
                && !path.starts_with("/user/")
                && !path.contains("/delete")
        }
        "indiegala" => {
            url.host_str() == Some("freebies.indiegala.com")
                || path.contains("/giveaway")
                || path.contains("/freebie")
        }
        "alienware" => path.contains("/ucf/") || path == "/",
        _ => false,
    }
}
