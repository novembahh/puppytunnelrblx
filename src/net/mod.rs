use std::net::SocketAddr;

pub mod echo;
pub mod proxy;
pub mod relay;
pub mod test;
pub mod warm;

pub use echo::{run_echo_test, EchoServer};
pub use proxy::{start_proxy, start_proxy_relay, ProxyHandle};
pub use test::{icmp_ping, run_connectivity_test};
pub use warm::warm_tunnel;

pub fn udp_port_in_use(port: u16) -> bool {
    for ip in [std::net::IpAddr::from([0, 0, 0, 0]), std::net::IpAddr::from([127, 0, 0, 1])] {
        match std::net::UdpSocket::bind(SocketAddr::new(ip, port)) {
            Ok(s) => drop(s),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => return true,
            Err(_) => {}
        }
    }
    false
}

pub fn bind_udp(addr: SocketAddr, reuse: bool) -> std::io::Result<std::net::UdpSocket> {
    let domain = if addr.is_ipv6() {
        socket2::Domain::IPV6
    } else {
        socket2::Domain::IPV4
    };
    let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, Some(socket2::Protocol::UDP))?;
    if reuse {
        let _ = socket.set_reuse_address(true);
        #[cfg(unix)]
        let _ = socket.set_reuse_port(true);
    }
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_probe_sees_bound_port() {
        for host in ["127.0.0.1:0", "0.0.0.0:0"] {
            let held = std::net::UdpSocket::bind(host).unwrap();
            let port = held.local_addr().unwrap().port();
            assert!(udp_port_in_use(port), "holder on {}", host);
            drop(held);
            assert!(!udp_port_in_use(port));
        }
    }
}
