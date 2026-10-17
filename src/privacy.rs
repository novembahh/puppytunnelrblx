use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::Path;

const MEDIA_EXTS: &[&str] = &[
    "mp3", "ogg", "oga", "wav", "flac", "m4a", "aac", "opus", "mp4", "webm", "mkv", "mov", "ogv",
];

pub fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])))
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00
                || (seg[0] & 0xffc0) == 0xfe80)
        }
    }
}

pub fn public_only_resolver(netloc: &str) -> io::Result<Vec<SocketAddr>> {
    let addrs: Vec<SocketAddr> = netloc
        .to_socket_addrs()?
        .filter(|a| is_public_ip(&a.ip()))
        .collect();
    if addrs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to connect to a local or private address",
        ));
    }
    Ok(addrs)
}

pub fn is_http_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub fn sanitize_media_name(name: &str) -> Option<String> {
    let base = Path::new(name.trim()).file_name()?.to_str()?.to_string();
    if base.is_empty() || base.starts_with('.') || base.chars().any(|c| c.is_control() || "<>:\"|?*\\/".contains(c)) {
        return None;
    }
    let ext = Path::new(&base).extension()?.to_str()?.to_ascii_lowercase();
    MEDIA_EXTS.contains(&ext.as_str()).then_some(base)
}

pub fn tunnel_addr_warnings(addr: &str) -> Vec<String> {
    let addr = addr.trim();
    let mut out = Vec::new();
    if addr.is_empty() || addr.starts_with('<') {
        return out;
    }
    if addr.starts_with("puppy://") {
        return out;
    }
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    let host = host.trim_matches(|c| c == '[' || c == ']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_public_ip(&ip) {
            out.push(
                "This is a raw public IP. If it is your own, everyone you share it with learns your real IP. Use a tunnel/relay hostname or --relay instead."
                    .to_string(),
            );
        } else {
            out.push("Private/loopback address: only reachable from your own network.".to_string());
        }
    }
    out
}

pub fn is_local_request(req: &tiny_http::Request, port: u16) -> bool {
    let mut host_ok = false;
    for h in req.headers() {
        let name = h.field.as_str().as_str();
        if name.eq_ignore_ascii_case("origin") || name.eq_ignore_ascii_case("sec-fetch-site") {
            return false;
        }
        if name.eq_ignore_ascii_case("host") {
            let v = h.value.as_str().to_ascii_lowercase();
            host_ok = v == format!("127.0.0.1:{}", port) || v == format!("localhost:{}", port);
        }
    }
    host_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_names() {
        assert_eq!(sanitize_media_name("song.mp3").as_deref(), Some("song.mp3"));
        assert_eq!(sanitize_media_name("../../evil.mp3").as_deref(), Some("evil.mp3"));
        assert_eq!(sanitize_media_name("..\\..\\evil.dll"), None);
        assert_eq!(sanitize_media_name("x.exe"), None);
        assert_eq!(sanitize_media_name(".mp3"), None);
    }

    #[test]
    fn public_ips() {
        assert!(!is_public_ip(&"127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip(&"192.168.1.5".parse().unwrap()));
        assert!(!is_public_ip(&"100.64.0.1".parse().unwrap()));
        assert!(is_public_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!tunnel_addr_warnings("8.8.8.8:55555").is_empty());
        assert!(tunnel_addr_warnings("my.tunnel.example:55555").is_empty());
    }
}
