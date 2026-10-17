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
