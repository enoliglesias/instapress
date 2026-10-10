//! The Instagram access token and where it is kept.
//!
//! The token is a credential: it is a newtype whose `Debug` output is redacted, and the only
//! persistent copy lives in the OS credential store behind [`TokenStore`].

use std::fmt;

use thiserror::Error;

const KEYRING_SERVICE: &str = "recipe-sync";
const KEYRING_USER: &str = "instagram-token";
const KEYRING_EXPIRY_USER: &str = "instagram-token-expiry";

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
    /// Removing a token that is not there is not an error. The remembered expiry goes with it.
    fn clear(&self) -> Result<(), TokenError>;
    /// When the saved token stops working, in seconds since 1970, if it is known.
    fn expires_at(&self) -> Result<Option<u64>, TokenError>;
    /// Remembers when the saved token stops working.
    fn save_expiry(&self, unix_seconds: u64) -> Result<(), TokenError>;
}

/// One named secret in the OS credential store (Windows Credential Manager, macOS Keychain,
/// Secret Service), under the `recipe-sync` service. Not unit-tested: it needs a real OS session.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OsSecret(&'static str);

impl OsSecret {
    pub(crate) const fn new(user: &'static str) -> Self {
        Self(user)
    }

    fn entry(&self, action: &'static str) -> Result<keyring::Entry, TokenError> {
        keyring::Entry::new(KEYRING_SERVICE, self.0).map_err(|e| store_error(action, &e))
    }

    /// The stored text, or `None` when there is none.
    pub(crate) fn load(&self, what: &'static str) -> Result<Option<String>, TokenError> {
        match self.entry(what)?.get_password() {
            Ok(raw) => Ok(Some(raw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(what, &e)),
        }
    }

    pub(crate) fn save(&self, what: &'static str, secret: &str) -> Result<(), TokenError> {
        self.entry(what)?
            .set_password(secret)
            .map_err(|e| store_error(what, &e))
    }

    /// Removing a secret that is not there is not an error.
    pub(crate) fn clear(&self, what: &'static str) -> Result<(), TokenError> {
        match self.entry(what)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(what, &e)),
        }
    }
}

fn store_error(action: &'static str, error: &keyring::Error) -> TokenError {
    TokenError::Store {
        action,
        reason: error.to_string(),
    }
}

/// [`TokenStore`] backed by the OS credential store.
#[derive(Debug, Default)]
pub struct KeyringStore;

const INSTAGRAM_SECRET: OsSecret = OsSecret::new(KEYRING_USER);
const INSTAGRAM_EXPIRY: OsSecret = OsSecret::new(KEYRING_EXPIRY_USER);

impl TokenStore for KeyringStore {
    fn load(&self) -> Result<Option<AccessToken>, TokenError> {
        // A stored value that no longer validates counts as no token: the user is asked to
        // paste it again and the save overwrites the bad entry.
        Ok(INSTAGRAM_SECRET
            .load("read the token")?
            .and_then(|raw| AccessToken::new(&raw).ok()))
    }

    fn save(&self, token: &AccessToken) -> Result<(), TokenError> {
        INSTAGRAM_SECRET.save("save the token", token.expose())
    }

    fn clear(&self) -> Result<(), TokenError> {
        INSTAGRAM_SECRET.clear("remove the token")?;
        INSTAGRAM_EXPIRY.clear("remove the token expiry")
    }

    fn expires_at(&self) -> Result<Option<u64>, TokenError> {
        // A value that is not a number counts as unknown: the next renewal writes a good one.
        Ok(INSTAGRAM_EXPIRY
            .load("read the token expiry")?
            .and_then(|raw| raw.trim().parse().ok()))
    }

    fn save_expiry(&self, unix_seconds: u64) -> Result<(), TokenError> {
        INSTAGRAM_EXPIRY.save("save the token expiry", &unix_seconds.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;

    use super::*;

    /// In-memory store shared by the tests of the modules that need a [`TokenStore`].
    #[derive(Default)]
    pub(crate) struct MemoryStore(
        pub(crate) RefCell<Option<AccessToken>>,
        pub(crate) RefCell<Option<u64>>,
    );

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
            *self.1.borrow_mut() = None;
            Ok(())
        }
        fn expires_at(&self) -> Result<Option<u64>, TokenError> {
            Ok(*self.1.borrow())
        }
        fn save_expiry(&self, unix_seconds: u64) -> Result<(), TokenError> {
            *self.1.borrow_mut() = Some(unix_seconds);
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

    #[test]
    fn memory_store_remembers_the_expiry_and_forgets_it_with_the_token() {
        let store = MemoryStore::default();
        assert_eq!(store.expires_at(), Ok(None));
        store.save(&AccessToken::new("IGAAabc").unwrap()).unwrap();
        store.save_expiry(1_800_000_000).unwrap();
        assert_eq!(store.expires_at(), Ok(Some(1_800_000_000)));
        store.clear().unwrap();
        assert_eq!(store.expires_at(), Ok(None));
    }
}
