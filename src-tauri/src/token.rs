//! The Instagram access token and where it is kept.
//!
//! The token is a credential: it is a newtype whose `Debug` output is redacted, and the only
//! persistent copy lives in the OS credential store behind [`TokenStore`].

use std::fmt;

use thiserror::Error;

const KEYRING_SERVICE: &str = "recipe-sync";
const KEYRING_USER: &str = "instagram-token";

/// Why a token was rejected or could not be stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TokenError {
    #[error("the access token is empty")]
    Empty,
    #[error("the access token contains whitespace; paste only the token")]
    ContainsWhitespace,
    /// The OS credential store failed; `reason` is its own message and never the secret.
    #[error("the credential store failed to {action}: {reason}")]
    Store {
        action: &'static str,
        reason: String,
    },
}

/// A validated access token. Never printed in full.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken(String);

impl AccessToken {
    /// Trims `raw` and rejects an empty value or one with whitespace inside, which means more
    /// than the token was pasted.
    pub fn new(raw: &str) -> Result<Self, TokenError> {
        let token = raw.trim();
        if token.is_empty() {
            return Err(TokenError::Empty);
        }
        if token.chars().any(char::is_whitespace) {
            return Err(TokenError::ContainsWhitespace);
        }
        Ok(Self(token.to_owned()))
    }

    /// The raw value, only for building a request.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(<redacted>)")
    }
}

/// Persistent storage for the access token.
pub trait TokenStore {
    /// The saved token, or `None` when there is none.
    fn load(&self) -> Result<Option<AccessToken>, TokenError>;
    /// Replaces any saved token.
    fn save(&self, token: &AccessToken) -> Result<(), TokenError>;
    /// Removing a token that is not there is not an error.
    fn clear(&self) -> Result<(), TokenError>;
}

/// [`TokenStore`] backed by the OS credential store (Windows Credential Manager, macOS
/// Keychain, Secret Service). Not unit-tested: it needs a real OS session.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(action: &'static str) -> Result<keyring::Entry, TokenError> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| store_error(action, &e))
    }
}

fn store_error(action: &'static str, error: &keyring::Error) -> TokenError {
    TokenError::Store {
        action,
        reason: error.to_string(),
    }
}

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<AccessToken>, TokenError> {
        match Self::entry("read the token")?.get_password() {
            // A stored value that no longer validates counts as no token: the user is asked to
            // paste it again and the save overwrites the bad entry.
            Ok(raw) => Ok(AccessToken::new(&raw).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error("read the token", &e)),
        }
    }

    fn save(&self, token: &AccessToken) -> Result<(), TokenError> {
        Self::entry("save the token")?
            .set_password(token.expose())
            .map_err(|e| store_error("save the token", &e))
    }

    fn clear(&self) -> Result<(), TokenError> {
        match Self::entry("remove the token")?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error("remove the token", &e)),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use super::*;

    /// In-memory store shared by the tests of the modules that need a [`TokenStore`].
    #[derive(Default)]
    pub(crate) struct MemoryStore(pub(crate) RefCell<Option<AccessToken>>);

    impl TokenStore for MemoryStore {
        fn load(&self) -> Result<Option<AccessToken>, TokenError> {
            Ok(self.0.borrow().clone())
        }
        fn save(&self, token: &AccessToken) -> Result<(), TokenError> {
            *self.0.borrow_mut() = Some(token.clone());
            Ok(())
        }
        fn clear(&self) -> Result<(), TokenError> {
            *self.0.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn trims_the_pasted_token() {
        let token = AccessToken::new("  IGAAabc123 \n").unwrap();
        assert_eq!(token.expose(), "IGAAabc123");
    }

    #[test]
    fn rejects_an_empty_token() {
        assert_eq!(AccessToken::new("  \n"), Err(TokenError::Empty));
    }

    #[test]
    fn rejects_a_token_with_whitespace_inside() {
        assert_eq!(
            AccessToken::new("IGAA abc"),
            Err(TokenError::ContainsWhitespace)
        );
    }

    #[test]
    fn debug_output_never_shows_the_token() {
        let token = AccessToken::new("IGAAsecret").unwrap();
        assert_eq!(format!("{token:?}"), "AccessToken(<redacted>)");
    }

    #[test]
    fn memory_store_round_trips_and_clears() {
        let store = MemoryStore::default();
        assert_eq!(store.load(), Ok(None));
        let token = AccessToken::new("IGAAabc").unwrap();
        store.save(&token).unwrap();
        assert_eq!(store.load(), Ok(Some(token)));
        store.clear().unwrap();
        assert_eq!(store.load(), Ok(None));
    }
}
