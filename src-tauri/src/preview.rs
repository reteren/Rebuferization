//! Link previews: asking the site that owns a URL what it points at.
//!
//! OWNER: this module is the only place in the crate that makes an outbound
//! request. That is deliberate and worth keeping true. A clipboard manager sees
//! everything you copy, so every link it looks up is a link it has told someone
//! else about — which is why nothing here runs unless `privacy.linkPreviews` is
//! on, and why only hosts we recognise are ever contacted.
//!
//! Today that means YouTube, through its public oEmbed endpoint: no key, no
//! quota, no account. The URL is handed over whole, because YouTube already
//! sees it in full the moment the link is opened.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::Agent;

use crate::error::{AppError, AppResult};

/// oEmbed replies are a few hundred bytes. Anything near this cap is not an
/// answer to our question.
const METADATA_LIMIT: u64 = 64 * 1024;

/// Preview thumbnails are ~20 KB JPEGs. The cap is what we refuse to download,
/// not what we expect.
const THUMBNAIL_LIMIT: u64 = 4 * 1024 * 1024;

/// Short on purpose. A preview is a nicety; a request that has not answered in
/// ten seconds has already failed at being one.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What a link turned out to be. The thumbnail is separate from the title
/// because either can arrive without the other, and a title alone is already
/// most of the value.
#[derive(Debug, Clone)]
pub struct LinkPreview {
    pub title: String,
    pub thumbnail: Option<Vec<u8>>,
}

/// Whether this URL is one we are willing to look up. Host-only: no attempt to
/// parse out a video id, because oEmbed takes the whole URL and every shape
/// YouTube hands out (`watch?v=`, `youtu.be/`, `/shorts/`, `/embed/`) then
/// works without this having to know about it.
pub fn is_supported(url: &str) -> bool {
    matches!(
        host_of(url).as_deref(),
        Some("youtube.com") | Some("youtu.be") | Some("youtube-nocookie.com")
    )
}

/// The registrable host of an absolute http(s) URL, lowercased and with the
/// subdomains YouTube serves the same content on folded away. `None` for
/// anything that is not plainly an http(s) URL — a relative path, a `file:`,
/// or something we should not be guessing about.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    // Userinfo before the last '@', port after the first ':' of what is left.
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        return None;
    }
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    for prefix in ["www.", "m.", "music.", "gaming."] {
        if let Some(bare) = host.strip_prefix(prefix) {
            return Some(bare.to_string());
        }
    }
    Some(host)
}

/// Looks the link up and returns what the site said about it. Only ever called
/// for a URL `is_supported` accepted.
pub fn fetch(url: &str) -> AppResult<LinkPreview> {
    let agent = agent();
    let endpoint = format!(
        "https://www.youtube.com/oembed?url={}&format=json",
        urlencoding::encode(url)
    );

    let body = agent
        .get(&endpoint)
        .call()
        .map_err(net)?
        .body_mut()
        .with_config()
        .limit(METADATA_LIMIT)
        .read_to_string()
        .map_err(net)?;

    let json: serde_json::Value = serde_json::from_str(&body)?;
    let title = json
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if title.is_empty() {
        return Err(AppError::Net("the reply carried no title".into()));
    }

    // A missing thumbnail is not a failed preview: the title is the part the
    // user asked for, and a card with a name and no picture is still better
    // than a bare hostname.
    let thumbnail =
        json.get("thumbnail_url")
            .and_then(|t| t.as_str())
            .and_then(|u| match download(&agent, u) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    tracing::debug!("link preview: thumbnail for {url} not fetched: {e}");
                    None
                }
            });

    Ok(LinkPreview { title, thumbnail })
}

fn download(agent: &Agent, url: &str) -> AppResult<Vec<u8>> {
    agent
        .get(url)
        .call()
        .map_err(net)?
        .body_mut()
        .with_config()
        .limit(THUMBNAIL_LIMIT)
        .read_to_vec()
        .map_err(net)
}

/// A fresh agent per lookup. Previews are rare and unbatched, so a pooled
/// connection would only be a socket held open against a third party between
/// two copies that may be hours apart.
fn agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent("Rebuffer")
        .tls_config(
            TlsConfig::builder()
                // ureq never picks native-tls up on its own, even as the only
                // backend compiled in. Schannel is the right one here: it is
                // already on every Windows box, needs no build toolchain, and
                // trusts what the machine trusts — including a corporate proxy
                // the user cannot opt out of.
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

fn net(e: ureq::Error) -> AppError {
    AppError::Net(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only test here that leaves the machine, so it does not run by
    /// default: `cargo test -- --ignored a_real_youtube_link`. Run it when the
    /// endpoint or the TLS stack is in doubt; everything else about the
    /// feature is exercised without a network.
    #[test]
    #[ignore = "hits youtube.com"]
    fn a_real_youtube_link_comes_back_with_a_title_and_a_picture() {
        let p = fetch("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            .unwrap_or_else(|e| panic!("lookup failed: {e}"));
        assert!(!p.title.is_empty(), "no title");
        let bytes = p.thumbnail.expect("no thumbnail");
        assert!(bytes.len() > 1000, "thumbnail is {} bytes", bytes.len());
        assert!(
            image::load_from_memory(&bytes).is_ok(),
            "the thumbnail is not a decodable image"
        );
        println!("title: {} ({} thumbnail bytes)", p.title, bytes.len());
    }

    #[test]
    fn only_youtube_hosts_are_ever_contacted() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLabc&index=7",
            "http://youtube.com/watch?v=x",
            "https://m.youtube.com/watch?v=x",
            "https://music.youtube.com/watch?v=x",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://www.youtube.com/shorts/tPEE9ZwTmy0",
            "https://www.youtube-nocookie.com/embed/x",
            "https://WWW.YouTube.COM/watch?v=x",
            "https://www.youtube.com:443/watch?v=x",
        ] {
            assert!(is_supported(url), "should be looked up: {url}");
        }

        for url in [
            // Neighbours that are emphatically not YouTube. The last two are
            // the reason this matches on a parsed host and not on `contains`.
            "https://google.com/search?q=youtube.com",
            "https://vimeo.com/12345",
            "https://youtube.com.evil.example/watch?v=x",
            "https://evil.example/?youtube.com/watch?v=x",
            "https://user@evil.example/youtu.be",
            "file:///C:/youtube.com/watch",
            "youtube.com/watch?v=x",
            "not a url at all",
            "",
        ] {
            assert!(!is_supported(url), "should be left alone: {url}");
        }
    }
}
