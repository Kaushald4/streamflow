//! Blocks the companion's outbound HTTP (adapter code's `http` bridge in
//! `js-host`, and the stream proxy's upstream fetches) from reaching
//! private/loopback/link-local addresses.
//!
//! Adapters get real, unrestricted-looking network access so they can talk
//! to arbitrary CDN/site hosts, but without this, a malicious or
//! CSRF-triggered adapter could use that access to probe the user's LAN
//! (router admin panels, other local services). Real CDN/site hosts always
//! resolve to public IPs, so blocking private ranges doesn't break any
//! legitimate adapter.
//!
//! Two layers, both needed:
//! - [`SsrfSafeResolver`], a custom `reqwest::dns::Resolve` rather than a
//!   pre-request string check on the hostname, specifically to avoid
//!   DNS-rebinding: the filtered address list *is* what the connection
//!   uses, so there's no window between "checked" and "connected" where a
//!   re-resolve could return something different.
//! - [`check_target_url`], because an HTTP client given a literal IP in the
//!   URL (e.g. `http://127.0.0.1/`) never calls the resolver at all, there's
//!   nothing to resolve. Callers must run this check *before* every outbound
//!   request; the resolver alone does not see this case.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

#[derive(Debug, Default, Clone, Copy)]
pub struct SsrfSafeResolver;

impl Resolve for SsrfSafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_string();
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let filtered: Vec<SocketAddr> = resolved.filter(|addr| is_public_ip(addr.ip())).collect();
            if filtered.is_empty() {
                return Err(format!("blocked: \"{host}\" did not resolve to any public address").into());
            }
            Ok(Box::new(filtered.into_iter()) as Addrs)
        })
    }
}

/// Call before making any outbound request to a caller-influenced URL.
/// Rejects a literal IP host that isn't public outright (the DNS resolver
/// never sees these, see the module docs). A hostname is let through here;
/// [`SsrfSafeResolver`] is what protects it once it resolves.
pub fn check_target_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    let ip = match parsed.host() {
        Some(url::Host::Ipv4(v4)) => Some(IpAddr::V4(v4)),
        Some(url::Host::Ipv6(v6)) => Some(IpAddr::V6(v6)),
        Some(url::Host::Domain(_)) => None,
        None => return Err("URL has no host".to_string()),
    };
    if let Some(ip) = ip {
        if !is_public_ip(ip) {
            return Err(format!("blocked: \"{ip}\" is not a public address"));
        }
    }
    Ok(())
}

pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast())
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || is_unique_local_v6(ip)
        || is_unicast_link_local_v6(ip))
}

/// fc00::/7, IPv6 equivalent of RFC1918 private ranges.
fn is_unique_local_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

/// fe80::/10.
fn is_unicast_link_local_v6(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr as V4;

    #[test]
    fn blocks_private_and_loopback_v4() {
        assert!(!is_public_ip(IpAddr::V4(V4::new(127, 0, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(V4::new(10, 0, 0, 5))));
        assert!(!is_public_ip(IpAddr::V4(V4::new(192, 168, 1, 1))));
        assert!(!is_public_ip(IpAddr::V4(V4::new(172, 16, 0, 1))));
        assert!(!is_public_ip(IpAddr::V4(V4::new(169, 254, 1, 1))));
    }

    #[test]
    fn allows_public_v4() {
        assert!(is_public_ip(IpAddr::V4(V4::new(93, 184, 216, 34))));
        assert!(is_public_ip(IpAddr::V4(V4::new(1, 1, 1, 1))));
    }

    #[test]
    fn blocks_loopback_and_unique_local_v6() {
        assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_public_ip(IpAddr::V6("fc00::1".parse().unwrap())));
        assert!(!is_public_ip(IpAddr::V6("fe80::1".parse().unwrap())));
    }

    #[test]
    fn allows_public_v6() {
        assert!(is_public_ip(IpAddr::V6("2606:4700:4700::1111".parse().unwrap())));
    }

    #[test]
    fn check_target_url_blocks_literal_private_ips() {
        assert!(check_target_url("http://127.0.0.1/secret").is_err());
        assert!(check_target_url("http://192.168.1.1/admin").is_err());
        assert!(check_target_url("http://[::1]/").is_err());
        assert!(check_target_url("http://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn check_target_url_allows_hostnames_and_public_ips() {
        // Hostnames pass this check, SsrfSafeResolver is what protects them
        // once resolved, since checking here can't see the resolved address.
        assert!(check_target_url("https://example.com/movie.m3u8").is_ok());
        assert!(check_target_url("http://1.1.1.1/").is_ok());
    }
}
