//! What a request tab is called when nobody has named it.
//!
//! A tab strip is read sideways, at a glance, next to a coloured method token.
//! The host is the least distinguishing part of that line — every tab in a
//! session usually shares it — so the derived title is the **path**:
//! `https://google.com.vn/api/123` reads as `/api/123`.
//!
//! Plain data with no GPUI in it, so the rules below are unit tested rather
//! than eyeballed in a running window. `state::request::RequestState::display_name`
//! is the only caller; an explicitly named tab never reaches here.

use reqwest::Url;

/// The longest derived title before the middle is elided.
///
/// Generous enough that ordinary REST paths are never touched, short enough
/// that one pathological URL cannot push every other tab off the strip. The
/// middle goes rather than the tail because both ends of a path carry meaning —
/// the resource at the front, the identifier at the back.
const MAX_TITLE_CHARS: usize = 48;

/// The character standing in for what was elided.
const ELLIPSIS: char = '…';

/// The tab title `url` implies, or `None` when there is nothing to derive one
/// from and the caller should show its own "untitled" wording.
///
/// The rules, in the order they are tried:
///
/// 1. Nothing but whitespace → `None`.
/// 2. A URL that parses and has a host → its path, with the scheme, host, port
///    and query string dropped. `https://example.com/api/123?x=1` → `/api/123`.
/// 3. A URL that parses to a bare host → the host. `https://example.com` →
///    `example.com`, because a lone `/` names nothing.
/// 4. Anything else — half-typed, or a `{{variable}}` where the host goes → the
///    raw text, trimmed. A title that lags reality beats a title that vanishes.
///
/// A missing scheme is not "anything else": `example.com/api` is completed to
/// `https://` first, exactly as [`services::http::prepare`] does before
/// sending, so the tab title and the request agree about what was typed.
///
/// [`services::http::prepare`]: crate::services::http::prepare
pub fn derive(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let absolute = if has_scheme(trimmed) {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let title = match Url::parse(&absolute) {
        Ok(parsed) => match parsed.host_str().filter(|host| is_real_host(host)) {
            Some(host) => {
                let path = parsed.path();
                if path.is_empty() || path == "/" {
                    host.to_string()
                } else {
                    path.to_string()
                }
            }
            None => trimmed.to_string(),
        },
        Err(_) => trimmed.to_string(),
    };

    Some(elide(&title))
}

/// Whether the text already opens with something shaped like a URL scheme.
///
/// Checked against the grammar rather than for `"://"`, because a URL is
/// half-typed far more often than it is finished: `https:/` must not be
/// completed to `https://https:/`, which would leave the tab reading `https`.
fn has_scheme(text: &str) -> bool {
    let Some(colon) = text.find(':') else {
        return false;
    };
    let scheme = &text[..colon];
    !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Whether a parsed host is a host rather than a placeholder waiting to be
/// filled in.
///
/// `{` and `}` are not forbidden host code points, so `https://{{base}}/api`
/// parses happily with `{{base}}` as its host — and a tab reading `/api` would
/// claim the request is further along than it is. A host with a brace in it is
/// treated as unresolved, and the raw text is shown instead.
fn is_real_host(host: &str) -> bool {
    !host.is_empty() && !host.contains(['{', '}'])
}

/// Shortens an over-long title by taking out its middle.
fn elide(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= MAX_TITLE_CHARS {
        return title.to_string();
    }
    // One character of the budget goes to the ellipsis itself; the head keeps
    // the smaller half so the identifying tail survives intact.
    let tail = MAX_TITLE_CHARS / 2;
    let head = MAX_TITLE_CHARS - tail - 1;
    let mut out: String = chars[..head].iter().collect();
    out.push(ELLIPSIS);
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{ELLIPSIS, MAX_TITLE_CHARS, derive};

    #[test]
    fn the_path_is_the_title() {
        assert_eq!(
            derive("https://google.com.vn/api/123").as_deref(),
            Some("/api/123")
        );
    }

    #[test]
    fn the_scheme_host_and_port_all_go() {
        assert_eq!(
            derive("http://localhost:8080/v1/users").as_deref(),
            Some("/v1/users")
        );
    }

    #[test]
    fn the_query_string_is_left_out() {
        assert_eq!(
            derive("https://example.com/search?q=rust&page=2").as_deref(),
            Some("/search")
        );
        // …and so is a fragment.
        assert_eq!(
            derive("https://example.com/docs#install").as_deref(),
            Some("/docs")
        );
    }

    #[test]
    fn an_empty_path_falls_back_to_the_host_rather_than_a_bare_slash() {
        assert_eq!(
            derive("https://example.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            derive("https://example.com/").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            derive("https://example.com/?q=1").as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn a_url_with_no_scheme_is_still_split() {
        assert_eq!(derive("google.com.vn/api/123").as_deref(), Some("/api/123"));
        assert_eq!(derive("example.com").as_deref(), Some("example.com"));
    }

    #[test]
    fn a_half_typed_url_degrades_to_what_was_typed() {
        for typed in ["h", "https:/", "https://", "http://:"] {
            assert_eq!(
                derive(typed).as_deref(),
                Some(typed),
                "{typed} should have shown itself rather than nothing"
            );
        }
    }

    #[test]
    fn a_variable_where_the_host_goes_degrades_to_the_raw_text() {
        let typed = "{{base_url}}/api/123";
        assert_eq!(derive(typed).as_deref(), Some(typed));
        let with_scheme = "https://{{base_url}}/api/123";
        assert_eq!(derive(with_scheme).as_deref(), Some(with_scheme));
    }

    #[test]
    fn nothing_typed_derives_nothing() {
        assert_eq!(derive(""), None);
        assert_eq!(derive("   \t "), None);
    }

    #[test]
    fn surrounding_whitespace_never_reaches_the_strip() {
        assert_eq!(derive("  https://example.com/x  ").as_deref(), Some("/x"));
    }

    #[test]
    fn a_long_path_is_elided_in_the_middle_and_keeps_its_tail() {
        let path = format!("/{}/{}", "segment".repeat(12), "42");
        let title = derive(&format!("https://example.com{path}")).expect("has a title");
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
        assert!(title.contains(ELLIPSIS));
        assert!(title.starts_with("/segment"));
        assert!(title.ends_with("/42"), "the identifier was elided: {title}");
    }

    #[test]
    fn a_path_that_exactly_fits_is_left_alone() {
        let path = format!("/{}", "a".repeat(MAX_TITLE_CHARS - 1));
        let title = derive(&format!("https://example.com{path}")).expect("has a title");
        assert_eq!(title, path);
    }

    #[test]
    fn a_percent_encoded_path_is_shown_as_typed() {
        assert_eq!(
            derive("https://example.com/a%20b/c").as_deref(),
            Some("/a%20b/c")
        );
    }
}
