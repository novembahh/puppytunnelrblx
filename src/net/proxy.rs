use super::bind_udp;
use super::relay::{frame, Token, MAGIC, T_BYE, T_C_DATA, T_HELLO, T_S_DATA};
use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SESSION_IDLE_SECS: u64 = 45;
const HELLO_INTERVAL: Duration = Duration::from_secs(10);

pub struct ProxyHandle {
    port: u16,
    running: Arc<AtomicBool>,
    server_thread: Option<JoinHandle<()>>,
    relay_threads: Arc<Mutex<Vec<JoinHandle<()>>>>,
    token: Option<Token>,
    target: Option<SocketAddr>,
}

fn send_bye(target: SocketAddr, token: Token) {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.send_to(&frame(T_BYE, &token), target);
    }
}

impl ProxyHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }


    pub fn live_sessions(&self) -> usize {
        self.relay_threads
            .lock()
            .map(|mut ths| {
                ths.retain(|h| !h.is_finished());
                ths.len()
            })
            .unwrap_or(0)
    }

    pub fn stop(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        if let (Some(t), Some(target)) = (self.token, self.target) {
            send_bye(target, t);
        }

        if let Ok(waker) = UdpSocket::bind("127.0.0.1:0") {
            let _ = waker.send_to(b"\x00", SocketAddr::from(([127, 0, 0, 1], self.port)));
        }
        if let Some(handle) = self.server_thread.take() {
            let _ = handle.join();
        }
        if let Ok(mut relays) = self.relay_threads.lock() {
            for handle in relays.drain(..) {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.stop();
    }
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

struct Session {
    sock: Arc<UdpSocket>,
    alive: Arc<AtomicBool>,
    activity: Arc<AtomicU64>,
}


pub fn start_proxy(
    proxy_port: u16,
    dst_host: &str,
    dst_port: u16,
) -> Result<ProxyHandle, io::Error> {
    start(proxy_port, dst_host, dst_port, None)
}


pub fn start_proxy_relay(
    proxy_port: u16,
    relay_host: &str,
    relay_port: u16,
    token: Token,
) -> Result<ProxyHandle, io::Error> {
    start(proxy_port, relay_host, relay_port, Some(token))
}

fn start(
    proxy_port: u16,
    dst_host: &str,
    dst_port: u16,
    token: Option<Token>,
) -> Result<ProxyHandle, io::Error> {
    let target_addr: SocketAddr = (dst_host, dst_port)
        .to_socket_addrs()?
        .find(|a| a.is_ipv4())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Host resolution failed"))?;

    let (server_sock, actual_port) = match bind_udp(SocketAddr::from(([127, 0, 0, 1], proxy_port)), true) {
        Ok(s) => (s, proxy_port),
        Err(orig_err) => {
            let mut found = None;
            for offset in 1..=15 {
                let candidate = proxy_port.saturating_add(offset);
                if let Ok(s) = bind_udp(SocketAddr::from(([127, 0, 0, 1], candidate)), true) {
                    found = Some((s, candidate));
                    break;
                }
            }
            match found {
                Some(pair) => pair,
                None => return Err(orig_err),
            }
        }
    };
    server_sock.set_read_timeout(Some(Duration::from_millis(500)))?;
    let server_sock = Arc::new(server_sock);

    let running = Arc::new(AtomicBool::new(true));
    let relay_threads: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));

    let running_c = Arc::clone(&running);
    let relay_threads_c = Arc::clone(&relay_threads);

    let server_thread = thread::spawn(move || {
        let mut sessions: HashMap<SocketAddr, Session> = HashMap::new();
        let mut buf = vec![0u8; 65536];
        let base = Instant::now();

        while running_c.load(Ordering::SeqCst) {
            let (n, client_addr) = match server_sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(ref e) if is_transient(e) => continue,
                Err(_) => break,
            };

            if !running_c.load(Ordering::SeqCst) {
                break;
            }


            if n == 1 && buf[0] == 0 && client_addr.ip().is_loopback() {
                continue;
            }

            sessions.retain(|_, s| s.alive.load(Ordering::SeqCst));
            if !sessions.contains_key(&client_addr) {
                let up = match bind_udp(SocketAddr::from(([0, 0, 0, 0], 0)), false) {
                    Ok(s) => Arc::new(s),
                    Err(_) => continue,
                };
                let _ = up.set_read_timeout(Some(Duration::from_millis(500)));
                if let Some(t) = token {
                    let _ = up.send_to(&frame(T_HELLO, &t), target_addr);
                }
                let alive = Arc::new(AtomicBool::new(true));
                let activity = Arc::new(AtomicU64::new(base.elapsed().as_secs()));

                let up_r = Arc::clone(&up);
                let activity_r = Arc::clone(&activity);
                let down = Arc::clone(&server_sock);
                let run = Arc::clone(&running_c);
                let alive_r = Arc::clone(&alive);
                let handle = thread::spawn(move || {
                    let mut rb = vec![0u8; 65536];
                    let mut last_hello = Instant::now() - HELLO_INTERVAL;
                    loop {
                        if !run.load(Ordering::SeqCst) {
                            break;
                        }


                        let idle = base.elapsed().as_secs().saturating_sub(activity_r.load(Ordering::Relaxed));
                        if idle >= SESSION_IDLE_SECS {
                            break;
                        }
                        if let Some(t) = token {
                            if last_hello.elapsed() >= HELLO_INTERVAL {
                                let _ = up_r.send_to(&frame(T_HELLO, &t), target_addr);
                                last_hello = Instant::now();
                            }
                        }
                        match up_r.recv_from(&mut rb) {
                            Ok((len, from)) => {
                                if token.is_some() {
                                    if from == target_addr
                                        && len >= 5
                                        && &rb[..4] == MAGIC
                                        && rb[4] == T_S_DATA
                                    {
                                        let _ = down.send_to(&rb[5..len], client_addr);
                                    }
                                } else {
                                    let _ = down.send_to(&rb[..len], client_addr);
                                }
                            }
                            Err(ref e) if is_transient(e) => continue,
                            Err(_) => break,
                        }
                    }
                    if let Some(t) = token {
                        send_bye(target_addr, t);
                    }
                    alive_r.store(false, Ordering::SeqCst);
                });

                if let Ok(mut ths) = relay_threads_c.lock() {
                    ths.retain(|h| !h.is_finished());
                    ths.push(handle);
                }
                sessions.insert(client_addr, Session { sock: up, alive, activity });
            }

            if let Some(s) = sessions.get(&client_addr) {
                s.activity.store(base.elapsed().as_secs(), Ordering::Relaxed);
                let _ = if token.is_some() {
                    s.sock.send_to(&frame(T_C_DATA, &buf[..n]), target_addr)
                } else {
                    s.sock.send_to(&buf[..n], target_addr)
                };
            }
        }
        for s in sessions.values() {
            s.alive.store(false, Ordering::SeqCst);
        }
    });

    Ok(ProxyHandle {
        port: actual_port,
        running,
        server_thread: Some(server_thread),
        relay_threads,
        token,
        target: Some(target_addr),
    })
}
