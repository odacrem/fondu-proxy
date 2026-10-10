//! Caching headers for a page combined from the content and fondu responses.

use fastly::http::header::{self, HeaderName};
use fastly::Response;

/// Makes `content`'s caching headers describe the rewritten page, which is built from both
/// responses: it may be cached no longer and no more widely than either allows, it varies on
/// whatever either varies on, and the content response's validators no longer apply to it.
pub fn combine(content: &mut Response, fondu: &Response) {
    let cache_control = CacheControl::of(content).stricter(CacheControl::of(fondu));
    set_or_remove(content, header::CACHE_CONTROL, cache_control.to_header());

    let vary = merge_vary(
        header_list(content, header::VARY).as_deref(),
        header_list(fondu, header::VARY).as_deref(),
    );
    set_or_remove(content, header::VARY, vary);

    // the page is as old as its older part
    let age = [&*content, fondu]
        .into_iter()
        .filter_map(|resp| resp.get_header_str(header::AGE)?.trim().parse::<u64>().ok())
        .max();
    set_or_remove(content, header::AGE, age.map(|age| age.to_string()));

    // validators and length describe the original body, not the rewritten one
    content.remove_header(header::ETAG);
    content.remove_header(header::LAST_MODIFIED);
    content.remove_header(header::CONTENT_LENGTH);
}

/// All values of a header joined into one list, or `None` if it's absent.
fn header_list(resp: &Response, name: HeaderName) -> Option<String> {
    let values: Vec<&str> = resp
        .get_header_all(name)
        .filter_map(|value| value.to_str().ok())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

fn set_or_remove(resp: &mut Response, name: HeaderName, value: Option<String>) {
    match value {
        Some(value) => resp.set_header(name, value),
        None => {
            resp.remove_header(name);
        }
    }
}

/// The `Cache-Control` directives that limit caching. Other directives are dropped when
/// combining, which errs on the side of caching less.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one flag per directive")]
struct CacheControl {
    no_store: bool,
    no_cache: bool,
    private: bool,
    public: bool,
    must_revalidate: bool,
    max_age: Option<u64>,
    s_maxage: Option<u64>,
}

impl CacheControl {
    /// A response's policy; no header means no limits.
    fn of(resp: &Response) -> Self {
        header_list(resp, header::CACHE_CONTROL)
            .map(|value| CacheControl::parse(&value))
            .unwrap_or_default()
    }

    fn parse(value: &str) -> Self {
        let mut cc = CacheControl::default();
        for directive in value.split(',') {
            let (name, arg) = match directive.split_once('=') {
                Some((name, arg)) => (name, Some(arg.trim().trim_matches('"'))),
                None => (directive, None),
            };
            let seconds = || arg.and_then(|arg| arg.parse().ok());
            match name.trim().to_ascii_lowercase().as_str() {
                "no-store" => cc.no_store = true,
                // field-qualified forms like private="Set-Cookie" are treated as unqualified
                "no-cache" => cc.no_cache = true,
                "private" => cc.private = true,
                "public" => cc.public = true,
                "must-revalidate" => cc.must_revalidate = true,
                "max-age" => cc.max_age = seconds(),
                "s-maxage" => cc.s_maxage = seconds(),
                _ => {}
            }
        }
        cc
    }

    /// A policy that satisfies both `self` and `other`.
    fn stricter(self, other: Self) -> Self {
        CacheControl {
            no_store: self.no_store || other.no_store,
            no_cache: self.no_cache || other.no_cache,
            private: self.private || other.private,
            public: self.public && other.public,
            must_revalidate: self.must_revalidate || other.must_revalidate,
            max_age: min_present(self.max_age, other.max_age),
            s_maxage: min_present(self.s_maxage, other.s_maxage),
        }
    }

    fn to_header(self) -> Option<String> {
        if self.no_store {
            return Some("no-store".to_owned());
        }
        let mut directives = Vec::new();
        if self.private {
            directives.push("private".to_owned());
        } else if self.public {
            directives.push("public".to_owned());
        }
        if self.no_cache {
            directives.push("no-cache".to_owned());
        }
        if self.must_revalidate {
            directives.push("must-revalidate".to_owned());
        }
        if let Some(max_age) = self.max_age {
            directives.push(format!("max-age={max_age}"));
        }
        // s-maxage only applies to shared caches, which private responses never reach
        if let Some(s_maxage) = self.s_maxage.filter(|_| !self.private) {
            directives.push(format!("s-maxage={s_maxage}"));
        }
        (!directives.is_empty()).then(|| directives.join(", "))
    }
}

/// The smaller of the values that are present.
fn min_present(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Every header name either response varies on, without duplicates.
fn merge_vary(a: Option<&str>, b: Option<&str>) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for name in a.into_iter().chain(b).flat_map(|value| value.split(',')) {
        let name = name.trim();
        if name == "*" {
            return Some("*".to_owned());
        }
        if !name.is_empty() && !names.iter().any(|seen| seen.eq_ignore_ascii_case(name)) {
            names.push(name);
        }
    }
    (!names.is_empty()).then(|| names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combined(content: Option<&str>, fondu: Option<&str>) -> Option<String> {
        let parse = |value: Option<&str>| value.map(CacheControl::parse).unwrap_or_default();
        parse(content).stricter(parse(fondu)).to_header()
    }

    #[test]
    fn takes_the_shorter_lifetime() {
        assert_eq!(
            combined(Some("public, max-age=3600"), Some("public, max-age=60")),
            Some("public, max-age=60".to_owned())
        );
    }

    #[test]
    fn private_on_either_side_makes_the_page_private() {
        assert_eq!(
            combined(Some("public, max-age=3600, s-maxage=600"), Some("private")),
            Some("private, max-age=3600".to_owned())
        );
    }

    #[test]
    fn no_store_on_either_side_wins() {
        assert_eq!(
            combined(Some("max-age=3600"), Some("no-store")),
            Some("no-store".to_owned())
        );
        assert_eq!(
            combined(Some("No-Store"), None),
            Some("no-store".to_owned())
        );
    }

    #[test]
    fn public_needs_both_sides() {
        assert_eq!(
            combined(Some("public, max-age=60"), None),
            Some("max-age=60".to_owned())
        );
    }

    #[test]
    fn a_missing_header_adds_no_limits() {
        assert_eq!(
            combined(None, Some("max-age=60")),
            Some("max-age=60".to_owned())
        );
        assert_eq!(combined(None, None), None);
    }

    #[test]
    fn keeps_revalidation_directives() {
        assert_eq!(
            combined(Some("no-cache"), Some("must-revalidate, max-age=0")),
            Some("no-cache, must-revalidate, max-age=0".to_owned())
        );
    }

    #[test]
    fn treats_field_qualified_private_as_private() {
        assert!(CacheControl::parse(r#"private="Set-Cookie", max-age=60"#).private);
    }

    #[test]
    fn drops_directives_that_do_not_limit_caching() {
        assert_eq!(
            combined(
                Some("max-age=60, immutable, stale-while-revalidate=30"),
                None
            ),
            Some("max-age=60".to_owned())
        );
    }

    #[test]
    fn merges_vary_without_duplicates() {
        assert_eq!(
            merge_vary(Some("Accept-Language, Cookie"), Some("cookie, User-Agent")),
            Some("Accept-Language, Cookie, User-Agent".to_owned())
        );
        assert_eq!(merge_vary(Some("Cookie"), Some("*")), Some("*".to_owned()));
        assert_eq!(merge_vary(None, None), None);
    }
}
