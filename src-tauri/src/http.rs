//! The blocking HTTP client shared by the Instagram and WordPress code.
//!
//! HTTPS only, no redirects (credentials travel in headers and must not follow one to another
//! host), a global timeout, and 4xx/5xx returned instead of raised so error bodies can be read.
//! Failures are reduced to fixed phrases because a `ureq` error text can carry request details.

use std::time::Duration;

use thiserror::Error;

/// Short calls that exchange a little JSON.
const API_TIMEOUT: Duration = Duration::from_secs(20);
/// Calls that move an image of up to 10 MB or wait for WordPress to process one: a slow uplink
/// must not cut a transfer that a retry would then duplicate on the site.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(120);

/// What the HTTP layer got back, whatever the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// The kind of transport failure, never the underlying error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct TransportError(&'static str);

/// A downloaded body with the type the server declared for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub status: u16,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

/// Blocking client over `ureq`.
pub struct UreqClient {
    agent: ureq::Agent,
}

impl Default for UreqClient {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(0)
            .timeout_global(Some(API_TIMEOUT))
            .build();
        Self {
            agent: config.into(),
        }
    }
}

impl UreqClient {
    /// `GET` with a bearer token, returning the body as text.
    pub(crate) fn get_text(
        &self,
        url: &str,
        query: &[(&str, &str)],
        bearer_token: &str,
    ) -> Result<HttpResponse, TransportError> {
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", format!("Bearer {bearer_token}"));
        for (key, value) in query {
            request = request.query(key, value);
        }
        let mut response = request.call().map_err(transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(HttpResponse { status, body })
    }

    /// `GET` with the given headers, returning the body as text.
    pub(crate) fn get_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, TransportError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.call().map_err(transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(HttpResponse { status, body })
    }

    /// `POST` of raw bytes with the given headers, returning the body as text. It may carry an
    /// image, so it gets the longer transfer timeout.
    pub(crate) fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<HttpResponse, TransportError> {
        let mut request = self
            .agent
            .post(url)
            .config()
            .timeout_global(Some(TRANSFER_TIMEOUT))
            .build();
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let mut response = request.send(body).map_err(transport_error)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(transport_error)?;
        Ok(HttpResponse { status, body })
    }

    /// `GET` without credentials, reading at most `max_bytes` of the body.
    pub(crate) fn get_bytes(&self, url: &str, max_bytes: u64) -> Result<Download, TransportError> {
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(TRANSFER_TIMEOUT))
            .build()
            .call()
            .map_err(transport_error)?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = response
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(transport_error)?;
        Ok(Download {
            status,
            content_type,
            bytes,
        })
    }
}

/// Keeps only the kind of failure.
fn transport_error(error: ureq::Error) -> TransportError {
    TransportError(match error {
        ureq::Error::Timeout(_) => "timed out",
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => "could not connect",
        ureq::Error::BodyExceedsLimit(_) => "response too large",
        _ => "transport error",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_errors_are_reduced_to_a_fixed_phrase() {
        for (error, phrase) in [
            (ureq::Error::HostNotFound, "could not connect"),
            (ureq::Error::ConnectionFailed, "could not connect"),
            (ureq::Error::Timeout(ureq::Timeout::Global), "timed out"),
            (ureq::Error::BodyExceedsLimit(10), "response too large"),
            (ureq::Error::TooManyRedirects, "transport error"),
        ] {
            assert_eq!(transport_error(error).to_string(), phrase);
        }
    }
}
