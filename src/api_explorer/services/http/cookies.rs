//! Parsing `Set-Cookie` response headers into something the Cookies tab shows.
//!
//! A `Set-Cookie` value is the cookie's `name=value` followed by `;`-separated
//! attributes (`Path=/`, `HttpOnly`, `Expires=…`). This reads that structure —
//! it does not validate or apply the cookie, only presents what the server
//! sent. Pure and testable; the header list comes from the [`Exchange`].
//!
//! [`Exchange`]: crate::api_explorer::models::exchange::Exchange

/// One attribute of a cookie: a flag (`HttpOnly`) or a name/value (`Path=/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookieAttribute {
    pub name: String,
    pub value: Option<String>,
}

/// A parsed `Set-Cookie` header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub attributes: Vec<CookieAttribute>,
}

/// Parses one `Set-Cookie` value. Returns `None` when there is no `name=value`
/// pair at the front, which is the one thing a cookie cannot be without.
pub fn parse_set_cookie(header: &str) -> Option<Cookie> {
    let mut parts = header.split(';');

    let first = parts.next()?.trim();
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    let attributes = parts
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            Some(match part.split_once('=') {
                Some((name, value)) => CookieAttribute {
                    name: name.trim().to_string(),
                    value: Some(value.trim().to_string()),
                },
                None => CookieAttribute {
                    name: part.to_string(),
                    value: None,
                },
            })
        })
        .collect();

    Some(Cookie {
        name: name.to_string(),
        value: value.trim().to_string(),
        attributes,
    })
}

/// Every parseable cookie set by a response, in header order.
pub fn cookies_from_headers(headers: &[(String, String)]) -> Vec<Cookie> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, value)| parse_set_cookie(value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{CookieAttribute, cookies_from_headers, parse_set_cookie};

    #[test]
    fn a_bare_pair_parses_with_no_attributes() {
        let cookie = parse_set_cookie("session=abc123").expect("parses");
        assert_eq!(cookie.name, "session");
        assert_eq!(cookie.value, "abc123");
        assert!(cookie.attributes.is_empty());
    }

    #[test]
    fn attributes_split_into_flags_and_pairs() {
        let cookie = parse_set_cookie("id=42; Path=/; HttpOnly; Max-Age=3600").expect("parses");
        assert_eq!(cookie.name, "id");
        assert_eq!(cookie.value, "42");
        assert_eq!(
            cookie.attributes,
            vec![
                CookieAttribute {
                    name: "Path".into(),
                    value: Some("/".into())
                },
                CookieAttribute {
                    name: "HttpOnly".into(),
                    value: None
                },
                CookieAttribute {
                    name: "Max-Age".into(),
                    value: Some("3600".into())
                },
            ]
        );
    }

    #[test]
    fn a_value_may_itself_contain_an_equals_sign() {
        let cookie = parse_set_cookie("token=a=b=c; Secure").expect("parses");
        assert_eq!(cookie.value, "a=b=c");
        assert_eq!(cookie.attributes[0].name, "Secure");
    }

    #[test]
    fn a_header_without_a_pair_is_skipped() {
        assert!(parse_set_cookie("   ").is_none());
        assert!(parse_set_cookie("=novalue").is_none());
    }

    #[test]
    fn only_set_cookie_headers_are_read_and_case_is_ignored() {
        let headers = vec![
            ("Content-Type".to_string(), "text/html".to_string()),
            ("set-cookie".to_string(), "a=1".to_string()),
            ("Set-Cookie".to_string(), "b=2; Path=/".to_string()),
        ];
        let cookies = cookies_from_headers(&headers);
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "a");
        assert_eq!(cookies[1].name, "b");
    }
}
