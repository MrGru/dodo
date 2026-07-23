//! What the Auth tab holds, as plain data.
//!
//! Turning this into an `Authorization` header or a query parameter is the
//! service layer's job (`services::http::auth`); nothing here knows what a
//! header is.

use crate::i18n::Str;

/// The authorization schemes the Auth tab offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum AuthType {
    #[default]
    None,
    Bearer,
    Basic,
    ApiKey,
    OAuth2,
}

impl AuthType {
    pub const ALL: [AuthType; 5] = [
        AuthType::None,
        AuthType::Bearer,
        AuthType::Basic,
        AuthType::ApiKey,
        AuthType::OAuth2,
    ];

    pub fn label(self) -> Str {
        match self {
            AuthType::None => Str::AuthTypeNone,
            AuthType::Bearer => Str::AuthTypeBearer,
            AuthType::Basic => Str::AuthTypeBasic,
            AuthType::ApiKey => Str::AuthTypeApiKey,
            AuthType::OAuth2 => Str::AuthTypeOAuth2,
        }
    }

    /// Whether this phase can actually perform the scheme.
    ///
    /// OAuth 2.0 needs a browser redirect, a token store and a refresh path —
    /// a feature, not a field. It is shown disabled with a tooltip rather than
    /// omitted, so a user looking for it learns where it stands.
    pub fn is_available(self) -> bool {
        !matches!(self, AuthType::OAuth2)
    }
}

/// Where an API key rides: as a header, or as a query parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

impl ApiKeyLocation {
    pub const ALL: [ApiKeyLocation; 2] = [ApiKeyLocation::Header, ApiKeyLocation::Query];

    pub fn label(self) -> Str {
        match self {
            ApiKeyLocation::Header => Str::ApiKeyInHeader,
            ApiKeyLocation::Query => Str::ApiKeyInQuery,
        }
    }
}

/// A snapshot of the Auth tab, taken when Send is pressed.
///
/// Every scheme's fields are carried regardless of `kind`, for the same reason
/// [`BodyDraft`] carries both surfaces: switching scheme and switching back
/// must not wipe what was typed.
///
/// [`BodyDraft`]: crate::api_explorer::models::body::BodyDraft
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AuthDraft {
    pub kind: AuthType,
    pub token: String,
    pub username: String,
    pub password: String,
    pub key_name: String,
    pub key_value: String,
    pub key_location: ApiKeyLocation,
}

#[cfg(test)]
mod tests {
    use super::{ApiKeyLocation, AuthType};

    #[test]
    fn every_scheme_is_listed_once() {
        for scheme in AuthType::ALL {
            assert_eq!(
                AuthType::ALL
                    .iter()
                    .filter(|other| **other == scheme)
                    .count(),
                1,
                "{scheme:?} appears more than once in AuthType::ALL"
            );
        }
    }

    #[test]
    fn only_oauth2_is_deferred() {
        for scheme in AuthType::ALL {
            assert_eq!(
                scheme.is_available(),
                scheme != AuthType::OAuth2,
                "{scheme:?} is not marked as this phase expects"
            );
        }
    }

    #[test]
    fn an_api_key_defaults_to_a_header() {
        assert_eq!(ApiKeyLocation::default(), ApiKeyLocation::Header);
    }
}
