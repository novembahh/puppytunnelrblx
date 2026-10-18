use crate::identity::{reg_message, token_of, unix_now, verify, Identity};
use rand::RngCore;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const MAGIC: &[u8; 4] = b"NEPR";
pub const T_REG: u8 = 0x01;
pub const T_HELLO: u8 = 0x02;
pub const T_C_DATA: u8 = 0x03;
pub const T_BYE: u8 = 0x06;
pub const T_STATS: u8 = 0x05;
pub const T_STATS_REPLY: u8 = 0x85;
pub const T_TO_HOST: u8 = 0x10;
pub const T_FROM_HOST: u8 = 0x11;
pub const T_ACK_HOST: u8 = 0x81;
pub const T_ACK_CLIENT: u8 = 0x82;
pub const T_S_DATA: u8 = 0x83;
pub const T_NO_HOST: u8 = 0x84;

const HOST_TTL: Duration = Duration::from_secs(30);
const CLIENT_TTL: Duration = Duration::from_secs(30);
const REG_MAX_SKEW_SECS: u64 = 300;
const MAX_CLIENTS: usize = 1024;
const MAX_HOSTS: usize = 256;
const MAX_FORWARD_PER_SEC: u32 = 120;
const MAX_FORWARD_BYTES: usize = 8192;

pub fn forward_payload_ok(len: usize) -> bool {
    len <= MAX_FORWARD_BYTES
}

struct FloodGate {
    start: Instant,
    count: u32,
}

impl FloodGate {
    fn allow(&mut self, limit: u32) -> bool {
        if self.start.elapsed() >= Duration::from_secs(1) {
            self.start = Instant::now();
            self.count = 0;
        }
        if self.count >= limit {
            return false;
        }
        self.count += 1;
        true
    }
}

pub type Token = [u8; 16];

pub fn new_token() -> Token {
    let mut t = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut t);
    t
}

pub fn token_hex(t: &Token) -> String {
    t.iter().map(|b| format!("{:02x}", b)).collect()
}

fn parse_token(s: &str) -> Option<Token> {
    if s.len() != 32 || !s.is_ascii() {
        return None;
    }
    let mut t = [0u8; 16];
    for (i, b) in t.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(t)
}

pub fn make_invite(relay: &str, token: &Token) -> String {
    format!("puppy://{}/{}", relay, token_hex(token))
}


pub fn parse_invite(s: &str) -> Option<(String, u16, Token)> {
    let rest = s.trim().strip_prefix("puppy://")?;
    let (authority, tok) = rest.split_once('/')?;
    let (host, port) = authority.rsplit_once(':')?;
    let host = host.trim_matches(|c| c == '[' || c == ']');
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?, parse_token(tok.trim_end_matches('/'))?))
}

pub fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(5 + payload.len());
    v.extend_from_slice(MAGIC);
    v.push(kind);
    v.extend_from_slice(payload);
    v
}

fn is_transient(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
    ) || e.raw_os_error() == Some(10054)
}

struct ClientEntry {
    token: Token,
    cid: u32,
    last: Instant,
}

pub struct RelayServer {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RelayServer {
    pub fn start<L>(listen: SocketAddr, log: L) -> io::Result<Self>
    where
        L: Fn(&str) + Send + 'static,
    {
        let sock = UdpSocket::bind(listen)?;
        sock.set_read_timeout(Some(Duration::from_millis(500)))?;
        let running = Arc::new(AtomicBool::new(true));
        let run = Arc::clone(&running);

        let thread = thread::spawn(move || {
            let mut hosts: HashMap<Token, (SocketAddr, Instant)> = HashMap::new();
            let mut clients: HashMap<SocketAddr, ClientEntry> = HashMap::new();
            let mut cids: HashMap<u32, SocketAddr> = HashMap::new();
            let mut client_gates: HashMap<SocketAddr, FloodGate> = HashMap::new();
            let mut host_gates: HashMap<SocketAddr, FloodGate> = HashMap::new();
            let mut next_cid: u32 = 1;
            let mut last_prune = Instant::now();
            let mut buf = vec![0u8; 65536];

            while run.load(Ordering::SeqCst) {
                if last_prune.elapsed() >= Duration::from_secs(5) {
                    last_prune = Instant::now();
                    hosts.retain(|_, (_, t)| t.elapsed() < HOST_TTL);
                    clients.retain(|_, c| c.last.elapsed() < CLIENT_TTL);
                    cids.retain(|_, a| clients.contains_key(a));
                    client_gates.retain(|a, _| clients.contains_key(a));
                    host_gates.retain(|_, g| g.start.elapsed() < Duration::from_secs(10));
                }

                let (n, src) = match sock.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(ref e) if is_transient(e) => continue,
                    Err(_) => break,
                };
                if n < 5 || &buf[..4] != MAGIC {
                    continue;
                }
                let body = &buf[5..n];
                match buf[4] {
                    T_REG if body.len() == 104 => {
                        let mut pk = [0u8; 32];
                        pk.copy_from_slice(&body[..32]);
                        let ts = u64::from_be_bytes(body[32..40].try_into().unwrap());
                        let mut sig = [0u8; 64];
                        sig.copy_from_slice(&body[40..104]);
                        if unix_now().abs_diff(ts) > REG_MAX_SKEW_SECS || !verify(&pk, &reg_message(ts), &sig) {
                            continue;
                        }
                        let token = token_of(&pk);
                        let allowed = match hosts.get(&token) {
                            Some((addr, seen)) => *addr == src || seen.elapsed() >= HOST_TTL,
                            None => hosts.len() < MAX_HOSTS,
                        };
                        if allowed {
                            if !hosts.contains_key(&token) {
                                log(&format!("host registered from {}", src));
                            }
                            hosts.insert(token, (src, Instant::now()));
                            let _ = sock.send_to(&frame(T_ACK_HOST, &[]), src);
                        }
                    }
                    T_STATS if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        let registered = hosts.get(&token).map_or(false, |(_, t)| t.elapsed() < HOST_TTL);


                        let mut ips = std::collections::HashSet::new();
                        for (addr, c) in clients.iter() {
                            if c.token == token {
                                ips.insert(addr.ip());
                            }
                        }
                        let count = ips.len() as u32;
                        let mut reply = vec![registered as u8];
                        reply.extend_from_slice(&count.to_be_bytes());
                        let _ = sock.send_to(&frame(T_STATS_REPLY, &reply), src);
                    }
                    T_HELLO if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        if !hosts.contains_key(&token) {
                            let _ = sock.send_to(&frame(T_NO_HOST, &[]), src);
                            continue;
                        }
                        if !clients.contains_key(&src) {
                            if clients.len() >= MAX_CLIENTS {
                                continue;
                            }
                            let cid = next_cid;
                            next_cid = next_cid.wrapping_add(1).max(1);
                            cids.insert(cid, src);
                            clients.insert(src, ClientEntry { token, cid, last: Instant::now() });
                            log(&format!("client joined from {}", src));
                        }
                        if let Some(c) = clients.get_mut(&src) {
                            c.token = token;
                            c.last = Instant::now();
                        }
                        let _ = sock.send_to(&frame(T_ACK_CLIENT, &[]), src);
                    }
                    T_BYE if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        let before = clients.len();
                        clients.retain(|addr, c| !(addr == &src && c.token == token));
                        if clients.len() != before {
                            cids.retain(|_, a| clients.contains_key(a));
                            log(&format!("client left from {}", src));
                        }
                    }
                    T_C_DATA => {
                        if !forward_payload_ok(body.len()) {
                            continue;
                        }
                        let gate_ok = client_gates
                            .entry(src)
                            .or_insert(FloodGate { start: Instant::now(), count: 0 })
                            .allow(MAX_FORWARD_PER_SEC);
                        if !gate_ok {
                            continue;
                        }
                        if let Some(c) = clients.get_mut(&src) {
                            c.last = Instant::now();
                            if let Some((host, _)) = hosts.get(&c.token) {
                                let mut payload = c.cid.to_be_bytes().to_vec();
                                payload.extend_from_slice(body);
                                let _ = sock.send_to(&frame(T_TO_HOST, &payload), host);
                            }
                        }
                    }
                    T_FROM_HOST if body.len() >= 4 => {
                        if body.len() > MAX_FORWARD_BYTES + 4 {
                            continue;
                        }
                        let gate_ok = host_gates
                            .entry(src)
                            .or_insert(FloodGate { start: Instant::now(), count: 0 })
                            .allow(MAX_FORWARD_PER_SEC * 2);
                        if !gate_ok {
                            continue;
                        }
                        let cid = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                        let target = cids.get(&cid).copied();
                        if let Some(caddr) = target {
                            let authorised = clients
                                .get(&caddr)
                                .and_then(|c| hosts.get(&c.token))
                                .map_or(false, |(h, _)| *h == src);
                            if authorised {
                                let _ = sock.send_to(&frame(T_S_DATA, &body[4..]), caddr);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self { running, thread: Some(thread) })
    }

    pub fn stop(&mut self) {
        if self.running.swap(false, Ordering::SeqCst) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for RelayServer {
    fn drop(&mut self) {
        self.stop();
    }
}




pub fn check_invite_live(invite: &str) -> Result<(String, u16, Token), String> {
    let (host, port, token) = parse_invite(invite.trim())
        .ok_or_else(|| "Not a puppy:// invite.".to_string())?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("Cannot resolve relay {}:{}: {}", host, port, e))?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| format!("No IPv4 address for relay {}:{}", host, port))?;
    match query_stats(addr, &token) {
        Some((true, _)) => Ok((host, port, token)),
        Some((false, _)) => Err("Server is offline: the relay has no host registered for this invite.".to_string()),
        None => Err(format!("Relay {}:{} did not answer; it may be down or unreachable.", host, port)),
    }
}



#[cfg(target_os = "linux")]
pub fn udp_port_bound(port: u16) -> bool {
    let want = format!("{:04X}", port);
    for f in ["/proc/net/udp", "/proc/net/udp6"] {
        if let Ok(content) = std::fs::read_to_string(f) {
            for line in content.lines().skip(1) {
                let mut cols = line.split_whitespace();
                let pair = (cols.next(), cols.next());
                if let (Some(_), Some(local)) = pair {
                    if local.rsplit(':').next().map_or(false, |p| p.eq_ignore_ascii_case(&want)) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(not(target_os = "linux"))]
pub fn udp_port_bound(_port: u16) -> bool {
    true
}


pub fn query_stats(relay: SocketAddr, token: &Token) -> Option<(bool, u32)> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    sock.send_to(&frame(T_STATS, token), relay).ok()?;
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) if from == relay && n == 10 && &buf[..4] == MAGIC && buf[4] == T_STATS_REPLY => {
                return Some((buf[5] != 0, u32::from_be_bytes([buf[6], buf[7], buf[8], buf[9]])));
            }
            Ok(_) => continue,
            Err(ref e) if is_transient(e) => continue,
            Err(_) => return None,
        }
    }
    None
}

struct LocalSession {
    sock: Arc<UdpSocket>,
    alive: Arc<AtomicBool>,


    last_client: Arc<AtomicU64>,
}


pub fn run_host_agent<L>(
    relay: SocketAddr,
    identity: Identity,
    studio_port: u16,
    stop: Arc<AtomicBool>,
    players: Arc<AtomicUsize>,
    log: L,
) -> io::Result<()>
where
    L: Fn(&str, &str),
{
    let sock = Arc::new(UdpSocket::bind("0.0.0.0:0")?);
    sock.set_read_timeout(Some(Duration::from_millis(500)))?;

    let mut sessions: HashMap<u32, LocalSession> = HashMap::new();
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let mut last_reg: Option<Instant> = None;
    let mut last_ack: Option<Instant> = None;
    let mut connected = false;
    let mut buf = vec![0u8; 65536];

    while !stop.load(Ordering::SeqCst) {
        players.store(sessions.values().filter(|s| s.alive.load(Ordering::SeqCst)).count(), Ordering::Relaxed);
        if last_reg.map_or(true, |t| t.elapsed() >= Duration::from_secs(5)) {
            let ts = unix_now();
            let mut reg = identity.public().to_vec();
            reg.extend_from_slice(&ts.to_be_bytes());
            reg.extend_from_slice(&identity.sign(&reg_message(ts)));
            let _ = sock.send_to(&frame(T_REG, &reg), relay);
            last_reg = Some(Instant::now());
            if connected && last_ack.map_or(true, |t| t.elapsed() > Duration::from_secs(20)) {
                connected = false;
                log("Relay is not answering; will keep retrying.", "warn");
            }
        }

        let (n, src) = match sock.recv_from(&mut buf) {
            Ok(v) => v,
            Err(ref e) if is_transient(e) => continue,
            Err(_) => break,
        };
        if src != relay || n < 5 || &buf[..4] != MAGIC {
            continue;
        }
        match buf[4] {
            T_ACK_HOST => {
                last_ack = Some(Instant::now());
                if !connected {
                    connected = true;
                    log("Registered with relay.", "ok");
                }
            }
            T_TO_HOST if n >= 9 => {
                let cid = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
                sessions.retain(|_, s| s.alive.load(Ordering::SeqCst));
                if !sessions.contains_key(&cid) {
                    let local = match UdpSocket::bind("127.0.0.1:0") {
                        Ok(s) => Arc::new(s),
                        Err(_) => continue,
                    };
                    if local.connect(("127.0.0.1", studio_port)).is_err() {
                        continue;
                    }
                    let _ = local.set_read_timeout(Some(Duration::from_millis(500)));
                    let alive = Arc::new(AtomicBool::new(true));
                    let last_client = Arc::new(AtomicU64::new(unix_now()));
                    let (l, r, a, st, lc) = (
                        Arc::clone(&local),
                        Arc::clone(&sock),
                        Arc::clone(&alive),
                        Arc::clone(&stop),
                        Arc::clone(&last_client),
                    );
                    threads.retain(|h| !h.is_finished());
                    threads.push(thread::spawn(move || {
                        let mut b = vec![0u8; 65536];
                        let mut idle = Instant::now();
                        while !st.load(Ordering::SeqCst) && idle.elapsed() < Duration::from_secs(120) {


                            if unix_now().saturating_sub(lc.load(Ordering::Relaxed)) > 120 {
                                break;
                            }
                            match l.recv(&mut b) {
                                Ok(len) => {
                                    idle = Instant::now();
                                    let mut p = cid.to_be_bytes().to_vec();
                                    p.extend_from_slice(&b[..len]);
                                    let _ = r.send_to(&frame(T_FROM_HOST, &p), relay);
                                }
                                Err(ref e) if is_transient(e) => continue,
                                Err(_) => break,
                            }
                        }
                        a.store(false, Ordering::SeqCst);
                    }));
                    log(&format!("Player #{} connected through relay.", cid), "info");
                    sessions.insert(cid, LocalSession { sock: local, alive, last_client });
                }
                if let Some(s) = sessions.get(&cid) {
                    s.last_client.store(unix_now(), Ordering::Relaxed);
                    let _ = s.sock.send(&buf[9..n]);
                }
            }
            _ => {}
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in threads {
        let _ = h.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ECHO_REQ, ECHO_RESP};
    use crate::net::{start_proxy_relay, EchoServer};

    #[test]
    fn forward_gate_caps_burst_then_resets() {
        assert!(forward_payload_ok(0));
        assert!(forward_payload_ok(MAX_FORWARD_BYTES));
        assert!(!forward_payload_ok(MAX_FORWARD_BYTES + 1));
        let mut g = FloodGate { start: Instant::now(), count: 0 };
        for _ in 0..MAX_FORWARD_PER_SEC {
            assert!(g.allow(MAX_FORWARD_PER_SEC));
        }
        assert!(!g.allow(MAX_FORWARD_PER_SEC));
        g.start = Instant::now() - Duration::from_secs(2);
        assert!(g.allow(MAX_FORWARD_PER_SEC));
    }

    fn free_port() -> u16 {
        UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn invite_round_trip() {
        let t = new_token();
        let inv = make_invite("relay.example.com:7777", &t);
        assert_eq!(parse_invite(&inv), Some(("relay.example.com".to_string(), 7777, t)));
        assert_eq!(parse_invite("puppy://x:1/zz"), None);
    }

    #[test]
    fn relay_rejects_forged_registration() {
        let relay_port = free_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();
        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();
        let victim = Identity::ephemeral();
        let attacker = Identity::ephemeral();

        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        let ts = unix_now();
        let mut b = [0u8; 64];


        let mut forged = victim.public().to_vec();
        forged.extend_from_slice(&ts.to_be_bytes());
        forged.extend_from_slice(&attacker.sign(&reg_message(ts)));
        sock.send_to(&frame(T_REG, &forged), relay_addr).unwrap();
        assert!(sock.recv(&mut b).is_err(), "forged registration was acknowledged");
        assert_eq!(query_stats(relay_addr, &victim.token()), Some((false, 0)));


        let mut good = victim.public().to_vec();
        good.extend_from_slice(&ts.to_be_bytes());
        good.extend_from_slice(&victim.sign(&reg_message(ts)));
        sock.send_to(&frame(T_REG, &good), relay_addr).unwrap();
        assert!(sock.recv(&mut b).is_ok());
        assert_eq!(query_stats(relay_addr, &victim.token()), Some((true, 0)));
        relay.stop();
    }

    #[test]
    fn echo_through_relay() {
        let relay_port = free_port();
        let studio_port = free_port();
        let proxy_port = free_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();

        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();
        let mut echo = EchoServer::new();
        echo.start(studio_port).unwrap();

        let identity = Identity::ephemeral();
        let token = identity.token();
        let stop = Arc::new(AtomicBool::new(false));
        let st = Arc::clone(&stop);
        let agent = thread::spawn(move || run_host_agent(relay_addr, identity, studio_port, st, Arc::new(AtomicUsize::new(0)), |_, _| {}));
        thread::sleep(Duration::from_millis(300));
        assert_eq!(query_stats(relay_addr, &token), Some((true, 0)));

        let mut proxy = start_proxy_relay(proxy_port, "127.0.0.1", relay_port, token).unwrap();
        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut req = ECHO_REQ.to_vec();
        req.extend_from_slice(b"12345678");
        c.send_to(&req, ("127.0.0.1", proxy_port)).unwrap();
        let mut b = [0u8; 64];
        let n = c.recv(&mut b).expect("no echo through relay");
        assert!(b[..n].starts_with(ECHO_RESP) && b[..n].ends_with(b"12345678"));

        assert_eq!(query_stats(relay_addr, &token), Some((true, 1)));
        proxy.stop();
        stop.store(true, Ordering::SeqCst);
        agent.join().unwrap().unwrap();
        echo.stop();
        relay.stop();
    }
}

#[cfg(test)]
mod host_gate_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    use std::thread;
    use std::time::Duration;

    fn test_port() -> u16 {
        UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn unregistered_invite_is_refused() {
        let relay_port = test_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();
        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();
        let id = Identity::ephemeral();
        let inv = make_invite(&format!("127.0.0.1:{}", relay_port), &id.token());
        let err = check_invite_live(&inv).expect_err("unregistered host must be refused");
        assert!(err.contains("offline"), "unexpected message: {err}");
        relay.stop();
    }

    #[test]
    fn registered_invite_passes() {
        let relay_port = test_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();
        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();
        let id = Identity::ephemeral();
        let stop = Arc::new(AtomicBool::new(false));
        let players = Arc::new(AtomicUsize::new(0));
        let studio_port = test_port();
        let agent_stop = Arc::clone(&stop);
        let agent_players = Arc::clone(&players);
        let agent_id = id.clone();
        let handle = thread::spawn(move || {
            let _ = run_host_agent(relay_addr, agent_id, studio_port, agent_stop, agent_players, |_, _| {});
        });
        let inv = make_invite(&format!("127.0.0.1:{}", relay_port), &id.token());
        let mut ok = false;
        for _ in 0..30 {
            if check_invite_live(&inv).is_ok() {
                ok = true;
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        stop.store(true, Ordering::SeqCst);
        let _ = handle.join();
        relay.stop();
        assert!(ok, "registered host should pass the liveness check");
    }
}
