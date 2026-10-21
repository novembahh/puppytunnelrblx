use crate::identity::{reg_message, token_of, unix_now, verify, Identity};
use rand::RngCore;
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
pub const T_OPEN: u8 = 0x07;
pub const T_OPEN_HOST: u8 = 0x0a;
pub const T_SLOT: u8 = 0x8a;

const HOST_TTL: Duration = Duration::from_secs(30);
const CLIENT_TTL: Duration = Duration::from_secs(30);
const REG_MAX_SKEW_SECS: u64 = 300;
const MAX_CLIENTS: usize = 1024;
const MAX_HOSTS: usize = 256;
const MAX_OPEN: usize = 64;
const MAX_FORWARD_PER_SEC: u32 = 120;
const MAX_FORWARD_BYTES: usize = 8192;

pub fn session_label(token: &Token) -> String {
    token_hex(token)[..10].to_string()
}

pub fn game_address(zone: &str, token: &Token, port: u16) -> String {
    let zone = zone.trim().trim_matches('.');
    if zone.is_empty() {
        return String::new();
    }
    format!("{}.{}:{}", session_label(token), zone, port)
}

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

const BIND_TTL_SECS: u64 = 60;
const BIND_CHALLENGE_TTL_SECS: u64 = 300;
const MAX_PENDING_CHALLENGES: usize = 4096;

pub struct BindRecord {
    pub addr: SocketAddr,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub challenge: String,
    pub verified: bool,
}

pub struct BindRegistry {
    binds: HashMap<Token, HashMap<SocketAddr, BindRecord>>,
    challenges: HashMap<String, (Token, SocketAddr, Instant)>,
}

impl BindRegistry {
    pub fn new() -> Self {
        Self { binds: HashMap::new(), challenges: HashMap::new() }
    }
    pub fn hello(&mut self, token: Token, addr: SocketAddr) -> String {
        let now = Instant::now();
        let entry = self.binds.entry(token).or_default();
        if let Some(r) = entry.get_mut(&addr) {
            r.last_seen = now;
            self.challenges.insert(r.challenge.clone(), (token, addr, now));
            return r.challenge.clone();
        }
        let challenge = token_hex(&new_token());
        entry.insert(addr, BindRecord { addr, first_seen: now, last_seen: now, challenge: challenge.clone(), verified: false });
        self.challenges.insert(challenge.clone(), (token, addr, now));
        challenge
    }
    pub fn confirm(&mut self, challenge: &str) -> bool {
        let Some((token, addr, issued)) = self.challenges.remove(challenge) else { return false; };
        if issued.elapsed() > Duration::from_secs(BIND_CHALLENGE_TTL_SECS) {
            return false;
        }
        if let Some(r) = self.binds.get_mut(&token).and_then(|m| m.get_mut(&addr)) {
            r.verified = true;
            return true;
        }
        false
    }
    pub fn bindings(&self, token: &Token) -> Vec<SocketAddr> {
        self.binds.get(token).map(|m| m.keys().copied().collect()).unwrap_or_default()
    }
    pub fn remove(&mut self, token: &Token, addr: &SocketAddr) {
        if let Some(m) = self.binds.get_mut(token) {
            m.remove(addr);
            if m.is_empty() {
                self.binds.remove(token);
            }
        }
    }
    pub fn prune(&mut self) {
        self.binds.retain(|_, m| {
            m.retain(|_, r| r.last_seen.elapsed() < Duration::from_secs(BIND_TTL_SECS));
            !m.is_empty()
        });
        self.challenges.retain(|_, (_, _, t)| t.elapsed() < Duration::from_secs(BIND_CHALLENGE_TTL_SECS));
        if self.challenges.len() > MAX_PENDING_CHALLENGES {
            let mut ages: Vec<Instant> = self.challenges.values().map(|(_, _, t)| *t).collect();
            ages.sort_by_key(|t| std::cmp::Reverse(t.elapsed()));
            let cutoff = ages[MAX_PENDING_CHALLENGES - 1];
            self.challenges.retain(|_, (_, _, t)| *t >= cutoff);
        }
    }
}

impl Default for BindRegistry {
    fn default() -> Self {
        Self::new()
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

#[allow(clippy::too_many_arguments)]
fn route_unframed(
    sock: &UdpSocket,
    hosts: &mut HashMap<Token, (SocketAddr, Instant)>,
    open_hosts: &mut std::collections::HashSet<Token>,
    clients: &mut HashMap<SocketAddr, ClientEntry>,
    cids: &mut HashMap<u32, SocketAddr>,
    next_cid: &mut u32,
    n: usize,
    src: SocketAddr,
    buf: &mut [u8],
    last_warn: &mut Instant,
) {
    open_hosts.retain(|t| hosts.contains_key(t));
    let mut live = open_hosts.iter().filter_map(|t| hosts.get(t).map(|(a, _)| (*t, *a)));
    let Some((token, host)) = live.next() else { return };
    if live.next().is_some() {
        if last_warn.elapsed() >= Duration::from_secs(10) {
            *last_warn = Instant::now();
            eprintln!("relay: unframed Neptunnel traffic dropped, more than one open host is registered");
        }
        return;
    }
    let cid = match clients.get(&src) {
        Some(c) if c.token == token => c.cid,
        _ => {
            if clients.len() >= MAX_CLIENTS {
                return;
            }
            let id = *next_cid;
            *next_cid = next_cid.wrapping_add(1).max(1);
            cids.insert(id, src);
            clients.insert(src, ClientEntry { token, cid: id, last: Instant::now(), open: true });
            id
        }
    };
    if let Some(c) = clients.get_mut(&src) {
        c.last = Instant::now();
        c.open = true;
    }
    let mut payload = cid.to_be_bytes().to_vec();
    payload.extend_from_slice(&frame(T_OPEN, &buf[..n]));
    let _ = sock.send_to(&frame(T_TO_HOST, &payload), host);
}

struct ClientEntry {
    token: Token,
    cid: u32,
    last: Instant,
    open: bool,
}

pub struct RelayServer {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    binds: Arc<Mutex<BindRegistry>>,
}

impl RelayServer {
    pub fn start<L>(listen: SocketAddr, log: L) -> io::Result<Self>
    where
        L: Fn(&str) + Send + 'static,
    {
        Self::start_with_ingress(listen, 0, log)
    }

    pub fn start_with_ingress<L>(listen: SocketAddr, ingress: usize, log: L) -> io::Result<Self>
    where
        L: Fn(&str) + Send + 'static,
    {
        let sock = UdpSocket::bind(listen)?;
        sock.set_read_timeout(Some(Duration::from_millis(500)))?;
        let running = Arc::new(AtomicBool::new(true));
        let run = Arc::clone(&running);

        let binds = Arc::new(Mutex::new(BindRegistry::new()));
        let thread_binds = Arc::clone(&binds);
        let thread = thread::spawn(move || {
            let mut hosts: HashMap<Token, (SocketAddr, Instant)> = HashMap::new();
            let mut open_hosts: std::collections::HashSet<Token> = std::collections::HashSet::new();
            let mut slots: HashMap<u16, Token> = HashMap::new();
            let (slot_tx, slot_rx) = std::sync::mpsc::channel::<(u16, Vec<u8>, SocketAddr)>();
            let mut slot_workers: Vec<JoinHandle<()>> = Vec::new();
            let base_port = listen.port();
            for i in 1..=ingress {
                let port = match base_port.checked_add(i as u16) {
                    Some(p) => p,
                    None => break,
                };
                let Ok(sk) = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port))) else { continue };
                let _ = sk.set_read_timeout(Some(Duration::from_millis(200)));
                let sk = Arc::new(sk);
                let tx = slot_tx.clone();
                let run_s = Arc::clone(&run);
                slot_workers.push(thread::spawn(move || {
                    let mut b = vec![0u8; 65536];
                    while run_s.load(Ordering::SeqCst) {
                        match sk.recv_from(&mut b) {
                            Ok((n, from)) if n > 0 => {
                                let _ = tx.send((port, b[..n].to_vec(), from));
                            }
                            Ok(_) => {}
                            Err(ref e) if is_transient(e) => continue,
                            Err(_) => break,
                        }
                    }
                }));
            }
            let mut last_unframed_warn = Instant::now() - Duration::from_secs(60);
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
                    open_hosts.retain(|t| hosts.contains_key(t));
                    clients.retain(|_, c| c.last.elapsed() < CLIENT_TTL);
                    cids.retain(|_, a| clients.contains_key(a));
                    client_gates.retain(|a, _| clients.contains_key(a));
                    thread_binds.lock().unwrap_or_else(|e| e.into_inner()).prune();
                    host_gates.retain(|_, g| g.start.elapsed() < Duration::from_secs(10));
                    slots.retain(|_, t| hosts.contains_key(t));
                }

                while let Ok((port, data, from)) = slot_rx.try_recv() {
                    let Some(token) = slots.get(&port).copied() else { continue };
                    let Some((host, _)) = hosts.get(&token) else { continue };
                    let cid = match clients.get(&from) {
                        Some(c) if c.token == token => c.cid,
                        _ => {
                            if clients.len() >= MAX_CLIENTS {
                                continue;
                            }
                            let id = next_cid;
                            next_cid = next_cid.wrapping_add(1).max(1);
                            cids.insert(id, from);
                            clients.insert(from, ClientEntry { token, cid: id, last: Instant::now(), open: true });
                            id
                        }
                    };
                    if let Some(c) = clients.get_mut(&from) {
                        c.last = Instant::now();
                        c.open = true;
                    }
                    let mut payload = cid.to_be_bytes().to_vec();
                    payload.extend_from_slice(&frame(T_OPEN, &data));
                    let _ = sock.send_to(&frame(T_TO_HOST, &payload), host);
                }

                let (n, src) = match sock.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(ref e) if is_transient(e) => continue,
                    Err(_) => break,
                };
                if n < 5 || &buf[..4] != MAGIC {
                    route_unframed(
                        &sock, &mut hosts, &mut open_hosts, &mut clients, &mut cids, &mut next_cid,
                        n, src, &mut buf, &mut last_unframed_warn,
                    );
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
                            None => hosts.len() < MAX_HOSTS || hosts.values().any(|(_, seen)| seen.elapsed() >= HOST_TTL),
                        };
                        if allowed {
                            hosts.retain(|_, (_, seen)| seen.elapsed() < HOST_TTL);
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
                    T_OPEN_HOST if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        let registered = hosts.get(&token).map_or(false, |(a, t)| *a == src && t.elapsed() < HOST_TTL);
                        if !registered {
                            continue;
                        }
                        open_hosts.insert(token);
                        if ingress == 0 {
                            continue;
                        }
                        let assigned = slots.iter().find(|(_, t)| **t == token).map(|(p, _)| *p);
                        let port = match assigned {
                            Some(p) => p,
                            None => {
                                if slots.len() >= ingress {
                                    let _ = sock.send_to(&frame(T_SLOT, &0u16.to_be_bytes()), src);
                                    continue;
                                }
                                let mut found = 0;
                                for i in 1..=ingress {
                                    let p = match base_port.checked_add(i as u16) {
                                        Some(p) => p,
                                        None => continue,
                                    };
                                    if !slots.contains_key(&p) {
                                        found = p;
                                        break;
                                    }
                                }
                                if found == 0 {
                                    let _ = sock.send_to(&frame(T_SLOT, &0u16.to_be_bytes()), src);
                                    continue;
                                }
                                slots.insert(found, token);
                                log(&format!("game slot udp/{} assigned", found));
                                found
                            }
                        };
                        let _ = sock.send_to(&frame(T_SLOT, &port.to_be_bytes()), src);
                    }
                    T_OPEN if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        let live = hosts.get(&token).map_or(false, |(_, t)| t.elapsed() < HOST_TTL);
                        if !live {
                            let _ = sock.send_to(&frame(T_NO_HOST, &[]), src);
                            continue;
                        }
                        let known = clients.get(&src).map(|c| c.token == token).unwrap_or(false);
                        if !known {
                            let open_count = clients.values().filter(|c| c.open).count();
                            if open_count >= MAX_OPEN || clients.len() >= MAX_CLIENTS {
                                continue;
                            }
                            let cid = next_cid;
                            next_cid = next_cid.wrapping_add(1).max(1);
                            cids.insert(cid, src);
                            clients.insert(src, ClientEntry { token, cid, last: Instant::now(), open: true });
                            log(&format!("open client joined from {}", src));
                        }
                        if let Some(c) = clients.get_mut(&src) {
                            c.last = Instant::now();
                        }
                        let _ = sock.send_to(&frame(T_ACK_CLIENT, &[]), src);
                    }
                    T_HELLO if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        if !hosts.get(&token).map_or(false, |(_, t)| t.elapsed() < HOST_TTL) {
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
                            clients.insert(src, ClientEntry { token, cid, last: Instant::now(), open: false });
                            log(&format!("client joined from {}", src));
                        }
                        if let Some(c) = clients.get_mut(&src) {
                            c.token = token;
                            c.last = Instant::now();
                        }
                        thread_binds.lock().unwrap_or_else(|e| e.into_inner()).hello(token, src);
                        let _ = sock.send_to(&frame(T_ACK_CLIENT, &[]), src);
                    }
                    T_BYE if body.len() == 16 => {
                        let mut token = [0u8; 16];
                        token.copy_from_slice(body);
                        let before = clients.len();
                        clients.retain(|addr, c| !(addr == &src && c.token == token));
                        if clients.len() != before {
                            cids.retain(|_, a| clients.contains_key(a));
                            thread_binds.lock().unwrap_or_else(|e| e.into_inner()).remove(&token, &src);
                            log(&format!("client left from {}", src));
                        }
                    }
                    T_C_DATA => {
                        if !forward_payload_ok(body.len()) {
                            continue;
                        }
                        let Some(entry) = clients.get_mut(&src) else { continue };
                        let gate_ok = client_gates
                            .entry(src)
                            .or_insert(FloodGate { start: Instant::now(), count: 0 })
                            .allow(MAX_FORWARD_PER_SEC);
                        if !gate_ok {
                            continue;
                        }
                        entry.last = Instant::now();
                        entry.open = false;
                        if let Some((host, _)) = hosts.get(&entry.token) {
                            let mut payload = entry.cid.to_be_bytes().to_vec();
                            if entry.open {
                                payload.extend_from_slice(&frame(T_OPEN, body));
                            } else {
                                payload.extend_from_slice(body);
                            }
                            let _ = sock.send_to(&frame(T_TO_HOST, &payload), host);
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
                                let open = clients.get(&caddr).map(|c| c.open).unwrap_or(false);
                                if open {
                                    let _ = sock.send_to(&body[4..], caddr);
                                } else {
                                    let _ = sock.send_to(&frame(T_S_DATA, &body[4..]), caddr);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self { running, thread: Some(thread), binds })
    }

    pub fn client_bindings(&self, token: &Token) -> Vec<SocketAddr> {
        self.binds.lock().unwrap_or_else(|e| e.into_inner()).bindings(token)
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
pub fn udp_port_bound(port: u16) -> bool {
    super::udp_port_in_use(port)
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
    run_host_agent_on(relay, identity, studio_port, stop, players, false, log)
}

pub fn run_host_agent_open<L>(
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
    run_host_agent_on(relay, identity, studio_port, stop, players, true, log)
}

fn run_host_agent_on<L>(
    relay: SocketAddr,
    identity: Identity,
    studio_port: u16,
    stop: Arc<AtomicBool>,
    players: Arc<AtomicUsize>,
    open_ingress: bool,
    log: L,
) -> io::Result<()>
where
    L: Fn(&str, &str),
{
    run_host_agent_slot(relay, identity, studio_port, stop, players, open_ingress, None, log)
}

pub fn run_host_agent_slot<L>(
    relay: SocketAddr,
    identity: Identity,
    studio_port: u16,
    stop: Arc<AtomicBool>,
    players: Arc<AtomicUsize>,
    open_ingress: bool,
    game_port: Option<Arc<AtomicUsize>>,
    log: L,
) -> io::Result<()>
where
    L: Fn(&str, &str),
{
    let sock = Arc::new(UdpSocket::bind("0.0.0.0:0")?);
    sock.connect(relay)?;
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
            let _ = sock.send(&frame(T_REG, &reg));
            last_reg = Some(Instant::now());
            if connected && last_ack.map_or(true, |t| t.elapsed() > Duration::from_secs(20)) {
                connected = false;
                log("Relay is not answering; will keep retrying.", "warn");
            }
        }

        let n = match sock.recv(&mut buf) {
            Ok(v) => v,
            Err(ref e) if is_transient(e) => continue,
            Err(_) => break,
        };
        if n < 5 || &buf[..4] != MAGIC {
            continue;
        }
        match buf[4] {
            T_ACK_HOST => {
                last_ack = Some(Instant::now());
                if !connected {
                    connected = true;
                    log("Registered with relay.", "ok");
                }
                if open_ingress {
                    let _ = sock.send(&frame(T_OPEN_HOST, &identity.token()));
                }
            }
            T_SLOT if n == 7 => {
                let port = u16::from_be_bytes([buf[5], buf[6]]);
                if let Some(slot) = &game_port {
                    slot.store(port as usize, Ordering::Relaxed);
                }
                if port == 0 {
                    log("Relay has no free game port; plain Neptunnel players cannot join.", "warn");
                } else {
                    log(&format!("Game port {} assigned by the relay.", port), "ok");
                }
            }
            T_TO_HOST if n >= 9 => {
                let cid = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]);
                let inner = &buf[9..n];
                let payload = if inner.len() >= 5 && &inner[..4] == MAGIC && inner[4] == T_OPEN {
                    &inner[5..]
                } else {
                    inner
                };
                sessions.retain(|_, s| s.alive.load(Ordering::SeqCst));
                if !sessions.contains_key(&cid) {
                    if sessions.len() >= MAX_CLIENTS {
                        log("Too many remote players; refusing a new relay session.", "warn");
                        continue;
                    }
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
                    let _ = s.sock.send(payload);
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

    #[test]
    fn bind_challenge_single_use() {
        let mut r = BindRegistry::new();
        let t = new_token();
        let addr: SocketAddr = ([127, 0, 0, 1], 9).into();
        let c = r.hello(t, addr);
        assert!(!c.is_empty());
        assert_eq!(r.bindings(&t), vec![addr]);
        assert!(r.confirm(&c));
        assert!(!r.confirm(&c));
        assert!(!r.confirm("missing"));
    }

    #[test]
    fn hello_records_client_binding() {
        let relay_port = free_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();
        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();
        let host = Identity::ephemeral();
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
        let ts = unix_now();
        let mut reg = host.public().to_vec();
        reg.extend_from_slice(&ts.to_be_bytes());
        reg.extend_from_slice(&host.sign(&reg_message(ts)));
        sock.send_to(&frame(T_REG, &reg), relay_addr).unwrap();
        let mut b = [0u8; 64];
        assert!(sock.recv(&mut b).is_ok());
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client.send_to(&frame(T_HELLO, &host.token()), relay_addr).unwrap();
        let want = client.local_addr().unwrap().port();
        let mut seen = false;
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(50));
            if relay.client_bindings(&host.token()).iter().any(|a| a.port() == want) {
                seen = true;
                break;
            }
        }
        assert!(seen, "hello did not record a client binding");
        relay.stop();
    }

    fn free_port() -> u16 {
        UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn slot_lets_two_open_hosts_coexist() {
        let base = free_port();
        if base > 60000 {
            return;
        }
        let listen: SocketAddr = ([127, 0, 0, 1], base).into();
        let studio_a = free_port();
        let studio_b = free_port();
        let echo_a = spawn_echo(studio_a);
        let echo_b = spawn_echo(studio_b);

        let mut relay = RelayServer::start_with_ingress(listen, 2, |_| {}).unwrap();

        let id_a = Identity::ephemeral();
        let id_b = Identity::ephemeral();
        let stop = Arc::new(AtomicBool::new(false));
        let (sa, sb) = (Arc::clone(&stop), Arc::clone(&stop));
        let (ia, ib) = (id_a.clone(), id_b.clone());
        let slot_a = Arc::new(AtomicUsize::new(0));
        let slot_b = Arc::new(AtomicUsize::new(0));
        let (ga, gb) = (Arc::clone(&slot_a), Arc::clone(&slot_b));
        let a = thread::spawn(move || {
            run_host_agent_slot(listen, ia, studio_a, sa, Arc::new(AtomicUsize::new(0)), true, Some(ga), |_, _| {})
        });
        let b = thread::spawn(move || {
            run_host_agent_slot(listen, ib, studio_b, sb, Arc::new(AtomicUsize::new(0)), true, Some(gb), |_, _| {})
        });

        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && (slot_a.load(Ordering::Relaxed) == 0 || slot_b.load(Ordering::Relaxed) == 0) {
            thread::sleep(Duration::from_millis(100));
        }
        let pa = slot_a.load(Ordering::Relaxed);
        let pb = slot_b.load(Ordering::Relaxed);
        assert!(pa != 0 && pb != 0, "relay did not assign both game slots");
        assert_ne!(pa, pb, "both hosts got the same game slot");

        for (port, want) in [(pa as u16, "host-a"), (pb as u16, "host-b")] {
            let c = UdpSocket::bind("127.0.0.1:0").unwrap();
            c.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            let mut got = false;
            let deadline = Instant::now() + Duration::from_secs(6);
            while Instant::now() < deadline && !got {
                let _ = c.send_to(want.as_bytes(), addr);
                let mut b = [0u8; 128];
                if let Ok(n) = c.recv(&mut b) {
                    got = b[..n] == want.as_bytes()[..];
                }
            }
            assert!(got, "slot {} did not reach its own host", port);
        }

        stop.store(true, Ordering::SeqCst);
        let _ = a.join();
        let _ = b.join();
        echo_a.store(false, Ordering::SeqCst);
        echo_b.store(false, Ordering::SeqCst);
        relay.stop();
    }

    #[test]
    fn session_labels_are_stable_and_distinct() {
        let a = new_token();
        let b = new_token();
        assert_eq!(session_label(&a), session_label(&a));
        assert_ne!(session_label(&a), session_label(&b));
        assert_eq!(session_label(&a).len(), 10);
        assert_eq!(game_address("games.puppytunnel.vip", &a, 7777), format!("{}.games.puppytunnel.vip:7777", session_label(&a)));
        assert!(game_address("", &a, 7777).is_empty());
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

    fn spawn_echo(port: u16) -> Arc<AtomicBool> {
        let sock = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], port))).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
        let run = Arc::new(AtomicBool::new(true));
        let r = Arc::clone(&run);
        thread::spawn(move || {
            let mut b = [0u8; 2048];
            while r.load(Ordering::SeqCst) {
                match sock.recv_from(&mut b) {
                    Ok((n, from)) => {
                        let _ = sock.send_to(&b[..n], from);
                    }
                    Err(_) => continue,
                }
            }
        });
        run
    }

    #[test]
    fn open_client_reaches_studio_as_neptunnel() {
        let relay_port = free_port();
        let studio_port = free_port();
        let relay_addr: SocketAddr = ([127, 0, 0, 1], relay_port).into();

        let studio = spawn_echo(studio_port);
        let mut relay = RelayServer::start(relay_addr, |_| {}).unwrap();

        let identity = Identity::ephemeral();
        let token = identity.token();
        let stop = Arc::new(AtomicBool::new(false));
        let st = Arc::clone(&stop);
        let agent = thread::spawn(move || {
            run_host_agent_open(relay_addr, identity, studio_port, st, Arc::new(AtomicUsize::new(0)), |_, _| {})
        });

        for _ in 0..30 {
            thread::sleep(Duration::from_millis(100));
            if query_stats(relay_addr, &token).map_or(false, |(reg, _)| reg) {
                break;
            }
        }

        let c = UdpSocket::bind("127.0.0.1:0").unwrap();
        c.set_read_timeout(Some(Duration::from_millis(400))).unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut raw = false;
        while Instant::now() < deadline && !raw {
            let _ = c.send_to(b"plain-datagram", relay_addr);
            let mut b = [0u8; 256];
            if let Ok(n) = c.recv(&mut b) {
                if b[..n] == b"plain-datagram"[..] {
                    raw = true;
                }
            }
        }
        assert!(raw, "unframed Neptunnel datagram did not round-trip");

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut framed = false;
        while Instant::now() < deadline && !framed {
            let _ = c.send_to(&frame(T_C_DATA, b"framed-payload"), relay_addr);
            let mut b = [0u8; 256];
            if let Ok(n) = c.recv(&mut b) {
                if n >= 5 && &b[..4] == MAGIC && b[4] == T_S_DATA && b[5..n] == b"framed-payload"[..] {
                    framed = true;
                }
            }
        }
        assert!(framed, "framed Neptunnel datagram did not survive the relay");

        stop.store(true, Ordering::SeqCst);
        agent.join().unwrap().unwrap();
        studio.store(false, Ordering::SeqCst);
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
