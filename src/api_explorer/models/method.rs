//! The HTTP methods the request bar offers, and how each one is coloured.

use gpui::{App, Hsla};
use gpui_component::ActiveTheme as _;

/// The nine methods in the method dropdown, in the order they are listed.
///
/// Deliberately a closed enum rather than a free-text field: an HTTP method is
/// a token from a fixed set here, and keeping it closed is what lets
/// [`HttpMethod::color`] be exhaustive.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    Connect,
    Trace,
}

impl HttpMethod {
    pub const ALL: [HttpMethod; 9] = [
        HttpMethod::Get,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Options,
        HttpMethod::Head,
        HttpMethod::Connect,
        HttpMethod::Trace,
    ];

    /// The wire spelling, which is also what is shown on screen.
    ///
    /// This is not localized and never will be: `GET` is a protocol token, not
    /// a word, and translating it would produce an invalid request.
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Head => "HEAD",
            HttpMethod::Connect => "CONNECT",
            HttpMethod::Trace => "TRACE",
        }
    }

    /// The accent the method is drawn in, taken from the active theme rather
    /// than from fixed hex values, so the colour coding survives a theme
    /// change instead of clashing with it.
    ///
    /// The mapping follows the convention every HTTP client shares: read is
    /// green, create is amber, update is blue, destroy is red, and the
    /// metadata methods stay quiet.
    pub fn color(self, cx: &App) -> Hsla {
        match self {
            HttpMethod::Get => cx.theme().success,
            HttpMethod::Post => cx.theme().warning,
            HttpMethod::Put | HttpMethod::Patch => cx.theme().info,
            HttpMethod::Delete => cx.theme().danger,
            HttpMethod::Options | HttpMethod::Head | HttpMethod::Connect | HttpMethod::Trace => {
                cx.theme().muted_foreground
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::HttpMethod;

    #[test]
    fn all_methods_are_listed_once() {
        let mut seen = Vec::new();
        for method in HttpMethod::ALL {
            assert!(
                !seen.contains(&method.as_str()),
                "{} appears twice in HttpMethod::ALL",
                method.as_str()
            );
            seen.push(method.as_str());
        }
        assert_eq!(seen.len(), 9);
    }

    #[test]
    fn wire_spelling_is_uppercase_ascii() {
        for method in HttpMethod::ALL {
            let name = method.as_str();
            assert!(
                name.chars().all(|c| c.is_ascii_uppercase()),
                "{name} is not a bare uppercase HTTP token"
            );
        }
    }
}
