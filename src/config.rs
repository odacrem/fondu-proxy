//! Settings from the `config` config store.

use std::time::Duration;

use fastly::ConfigStore;

const STORE_NAME: &str = "config";

pub struct Config {
    /// Prefix for the request path on the fondu backend, e.g. `/directives` sends a
    /// request for `/news/story` to fondu as `/directives/news/story`.
    pub fondu_path_prefix: String,
    /// How long to wait for fondu, from the start of the request, before failing open.
    /// `None` waits as long as the fondu backend takes.
    pub fondu_budget: Option<Duration>,
}

impl Config {
    pub fn load() -> Self {
        let store = ConfigStore::open(STORE_NAME);
        Config {
            fondu_path_prefix: store.get("fondu_path_prefix").unwrap_or_default(),
            fondu_budget: store
                .get("fondu_budget_ms")
                .and_then(|ms| parse_budget(&ms)),
        }
    }
}

/// Parses `fondu_budget_ms`, logging and ignoring invalid values.
fn parse_budget(ms: &str) -> Option<Duration> {
    match ms.parse() {
        Ok(ms) => Some(Duration::from_millis(ms)),
        Err(e) => {
            println!("Ignoring invalid fondu_budget_ms {ms:?}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_budget() {
        assert_eq!(parse_budget("50"), Some(Duration::from_millis(50)));
        assert_eq!(parse_budget("0"), Some(Duration::ZERO));
    }

    #[test]
    fn ignores_invalid_budget() {
        assert_eq!(parse_budget(""), None);
        assert_eq!(parse_budget("50ms"), None);
        assert_eq!(parse_budget("-1"), None);
    }
}
