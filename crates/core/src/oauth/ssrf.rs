//! SSRF guard for outbound fetches to admin-supplied URLs.
//!
//! OIDC discovery, token, userinfo, and JWKS endpoints all come from
//! admin-entered config. Without a guard, an admin (or an attacker who has
//! compromised an admin) could point them at internal hosts — cloud metadata
//! (`169.254.169.254`), loopback services, or RFC1918 ranges — and have the
//! server fetch them. [`guard_url`] runs *before* each fetch and rejects those
//! targets.
//!
//! `allow_private` is wired from the deployment environment: development
//! permits loopback/private so a local IdP works; production blocks them.
//!
//! Caveat (DNS rebinding): the host is resolved here, but `reqwest` re-resolves
//! at connect time, so a TOCTOU window exists. It is mitigated by pairing this
//! guard with `redirect::Policy::none()` on every guarded client (a malicious
//! redirect can't bounce to an internal host). A fully robust fix pins the
//! validated IP via a custom connector — tracked as a follow-up.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use crate::error::{CoreError, CoreResult};

/// Validate that `url` is safe to fetch. In production (`allow_private =
/// false`) requires HTTPS and rejects any host that resolves to a
/// loopback/private/link-local/metadata address. In development
/// (`allow_private = true`) the checks are relaxed so a local IdP works.
pub async fn guard_url(url: &str, allow_private: bool) -> CoreResult<()> {
    let parsed = reqwest::Url::parse(url.trim())
        .map_err(|_| CoreError::BadRequest(format!("invalid URL: {url}")))?;

    match parsed.scheme() {
        "https" => {}
        "http" if allow_private => {}
        _ => {
            return Err(CoreError::BadRequest(
                "URL must use HTTPS".into(),
            ));
        }
    }

    if allow_private {
        return Ok(());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| CoreError::BadRequest("URL has no host".into()))?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);

    // DNS resolution blocks; run it off the async runtime. Resolve every
    // address the host maps to and reject if *any* is disallowed (an attacker
    // can't smuggle an internal IP behind a multi-A record).
    let addrs = tokio::task::spawn_blocking(move || {
        (host.as_str(), port)
            .to_socket_addrs()
            .map(|it| it.map(|sa| sa.ip()).collect::<Vec<_>>())
    })
    .await
    .map_err(|_| CoreError::BadRequest("could not resolve host".into()))?
    .map_err(|_| CoreError::BadRequest("could not resolve host".into()))?;

    if addrs.is_empty() {
        return Err(CoreError::BadRequest("host did not resolve".into()));
    }
    for ip in addrs {
        if is_disallowed(ip) {
            return Err(CoreError::BadRequest(
                "URL resolves to a disallowed (private/loopback/link-local) address".into(),
            ));
        }
    }
    Ok(())
}

/// `true` for any address we refuse to fetch in production.
fn is_disallowed(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        // Only unwrap genuine IPv4-*mapped* addresses (`::ffff:a.b.c.d`) and
        // re-check as v4. `to_ipv4()` also folds IPv4-*compatible* forms like
        // `::1` → `0.0.0.1`, which would defeat the loopback check, so it isn't
        // used here — native v6 checks handle `::1`, `fe80::`, `fc00::`, etc.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_disallowed_v4(v4),
            None => is_disallowed_v6(v6),
        },
    }
}

fn is_disallowed_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local() // 169.254.0.0/16 — covers cloud metadata
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        // Carrier-grade NAT 100.64.0.0/10 (not covered by std helpers).
        || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
}

fn is_disallowed_v6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        // Unique local fc00::/7.
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        // Link-local fe80::/10.
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_metadata_and_private_v4() {
        assert!(is_disallowed("169.254.169.254".parse().unwrap()));
        assert!(is_disallowed("127.0.0.1".parse().unwrap()));
        assert!(is_disallowed("10.0.0.5".parse().unwrap()));
        assert!(is_disallowed("192.168.1.1".parse().unwrap()));
        assert!(is_disallowed("172.16.0.1".parse().unwrap()));
        assert!(is_disallowed("100.64.0.1".parse().unwrap()));
    }

    #[test]
    fn allows_public_v4() {
        assert!(!is_disallowed("8.8.8.8".parse().unwrap()));
        assert!(!is_disallowed("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn blocks_private_v6_and_mapped() {
        assert!(is_disallowed("::1".parse().unwrap()));
        assert!(is_disallowed("fe80::1".parse().unwrap()));
        assert!(is_disallowed("fc00::1".parse().unwrap()));
        // IPv4-mapped loopback.
        assert!(is_disallowed("::ffff:127.0.0.1".parse().unwrap()));
    }

    #[tokio::test]
    async fn rejects_non_https_in_production() {
        assert!(guard_url("http://example.com", false).await.is_err());
    }

    #[tokio::test]
    async fn allows_http_in_development() {
        assert!(guard_url("http://localhost:8080", true).await.is_ok());
    }
}
