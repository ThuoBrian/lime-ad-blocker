//! Download blocklists over HTTPS with a timeout and a size limit.
//!
//! Built on ureq 3.x, which uses an `Agent` created from a
//! `ureq::config::Config`. The default response body read limit is 10 MB;
//! we raise it explicitly so larger sources are accepted while still
//! bounding memory.

use std::time::Duration;

use crate::error::{AdBlockerError, Result};

/// Hard cap on a single response body. `StevenBlack`'s unified hosts is
/// ~2.2 MB; 20 MB gives headroom for additional sources without allowing
/// runaway downloads.
const MAX_RESPONSE_SIZE: u64 = 20 * 1024 * 1024;

/// Fetch `url` over HTTPS and return its body as a string.
///
/// Enforces a 30-second global timeout and the [`MAX_RESPONSE_SIZE`] body
/// limit. The body limit is enforced both via the `Content-Length` header
/// (early rejection) and while reading (in case of chunked transfer).
pub fn fetch_url(url: &str) -> Result<String> {
    let config = ureq::config::Config::builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .max_response_header_size(64 * 1024)
        .build();
    let agent = ureq::Agent::new_with_config(config);

    let mut response = agent.get(url).call().map_err(|e| AdBlockerError::Fetch {
        url: url.to_string(),
        source: e,
    })?;

    if let Some(len) = response.body().content_length()
        && len > MAX_RESPONSE_SIZE
    {
        return Err(AdBlockerError::ResponseTooLarge {
            url: url.to_string(),
            limit: MAX_RESPONSE_SIZE,
        });
    }

    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_SIZE)
        .read_to_string()
        .map_err(|e| AdBlockerError::Fetch {
            url: url.to_string(),
            source: e,
        })?;

    Ok(body)
}
