use crate::config::{ECHO_REQ, ECHO_RESP};
use rand::RngCore;
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct EchoServer {
    running: Arc<AtomicBool>,
    echoed: Arc<AtomicUsize>,
    clients: Arc<std::sync::Mutex<HashSet<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl EchoServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            echoed: Arc::new(AtomicUsize::new(0)),
            clients: Arc::new(std::sync::Mutex::new(HashSet::new())),
            thread: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn echoed_count(&self) -> usize {
        self.echoed.load(Ordering::SeqCst)
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().map(|c| c.len()).unwrap_or(0)
    }

    pub fn start(&mut self, port: u16) -> Result<(), std::io::Error> {
        self.stop();

        let bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
        let socket = super::bind_udp(bind_addr, true)?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        self.running.store(true, Ordering::SeqCst);
        self.echoed.store(0, Ordering::SeqCst);
        if let Ok(mut c) = self.clients.lock() {
            c.clear();
        }

        let running = Arc::clone(&self.running);
        let echoed = Arc::clone(&self.echoed);
        let clients = Arc::clone(&self.clients);

        self.thread = Some(thread::spawn(move || {
            let mut buf = [0u8; 512];
            while running.load(Ordering::SeqCst) {
                match socket.recv_from(&mut buf) {
                    Ok((n, addr)) => {
                        let data = &buf[..n];
                        if data.starts_with(ECHO_REQ) {
                            let nonce = &data[ECHO_REQ.len()..];
                            let mut resp = Vec::with_capacity(ECHO_RESP.len() + nonce.len());
                            resp.extend_from_slice(ECHO_RESP);
                            resp.extend_from_slice(nonce);

                            let _ = socket.send_to(&resp, addr);
                            echoed.fetch_add(1, Ordering::SeqCst);
                            if let Ok(mut c) = clients.lock() {
                                c.insert(addr.ip().to_string());
                            }
                        }
                    }
                    Err(ref e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::Interrupted
                        ) || e.raw_os_error() == Some(10054) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }));

        Ok(())
    }

    pub fn stop(&mut self) {
        if self.running.swap(false, Ordering::SeqCst) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

impl Drop for EchoServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn run_echo_test<F>(
    tunnel_host: &str,
    tunnel_port: u16,
    max_successes: usize,
    timeout_secs: f64,
    mut log_fn: F,
) where
    F: FnMut(&str, &str),
{
    log_fn("--- Echo Round-Trip Test ---", "info");
    log_fn(&format!("Target: {}:{}", tunnel_host, tunnel_port), "dim");
    log_fn(
        "Sending probes directly to tunnel (bypassing local proxy)...",
        "warn",
    );
    log_fn(
        "Note: Tunnels can take a few seconds to \"wake up\". Please wait...",
        "dim",
    );

    let target_addr: SocketAddr = match format!("{}:{}", tunnel_host, tunnel_port).to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => a,
            None => {
                log_fn("[fail] FAILED: DNS lookup produced no addresses.", "err");
                return;
            }
        },
        Err(e) => {
            log_fn(&format!("[fail] FAILED: Could not resolve tunnel host: {}", e), "err");
            return;
        }
    };

    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            log_fn(&format!("[fail] Socket bind failed: {}", e), "err");
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));

    let mut sent: HashMap<[u8; 8], Instant> = HashMap::new();
    let mut received = 0usize;
    let mut rtts: Vec<f64> = Vec::new();
    let start_time = Instant::now();
    let probe_interval = Duration::from_millis(400);
    let mut next_probe_time = Instant::now();
    let mut icmp_reject = false;
    let timeout_duration = Duration::from_secs_f64(timeout_secs);

    let mut rng = rand::thread_rng();
    let mut buf = [0u8; 512];

    while start_time.elapsed() < timeout_duration && received < max_successes {
        let now = Instant::now();
        if now >= next_probe_time {
            let mut nonce = [0u8; 8];
            rng.fill_bytes(&mut nonce);

            let mut packet = Vec::with_capacity(ECHO_REQ.len() + 8);
            packet.extend_from_slice(ECHO_REQ);
            packet.extend_from_slice(&nonce);

            match sock.send_to(&packet, target_addr) {
                Ok(_) => {
                    sent.insert(nonce, now);
                    next_probe_time = now + probe_interval;
                }
                Err(e) => {
                    log_fn(&format!("[fail] Send error: {}", e), "err");
                    break;
                }
            }
        }

        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                let data = &buf[..n];
                if data.starts_with(ECHO_RESP) && data.len() >= ECHO_RESP.len() + 8 {
                    let nonce_bytes = &data[ECHO_RESP.len()..ECHO_RESP.len() + 8];
                    let mut key = [0u8; 8];
                    key.copy_from_slice(nonce_bytes);

                    if let Some(sent_time) = sent.remove(&key) {
                        let rtt = sent_time.elapsed().as_secs_f64() * 1000.0;
                        rtts.push(rtt);
                        received += 1;

                        if received == 1 {
                            log_fn(
                                &format!("[ok] First echo received! ({:.0} ms) Tunnel is waking up...", rtt),
                                "ok",
                            );
                        } else if received == 2 {
                            log_fn("[ok] Second echo received. Connection stabilizing...", "ok");
                        }
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(ref e) if e.raw_os_error() == Some(10054) => {

                log_fn(
                    "[fail] ICMP Port Unreachable. Tunnel endpoint is actively rejecting.",
                    "err",
                );
                icmp_reject = true;
                break;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                log_fn(
                    "[fail] ICMP Port Unreachable. Tunnel endpoint is actively rejecting.",
                    "err",
                );
                icmp_reject = true;
                break;
            }
            Err(_) => {
                continue;
            }
        }
    }

    log_fn("-----------------------", "dim");
    if received >= max_successes {
        let sum: f64 = rtts.iter().sum();
        let avg = sum / (rtts.len() as f64);
        let min = rtts.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        log_fn(
            &format!("[ok] SUCCESS: {} echoes received. Tunnel is LIVE and stable.", received),
            "ok",
        );
        log_fn(
            &format!(
                "  RTT: avg {:.0} ms | min {:.0} ms | max {:.0} ms",
                avg, min, max
            ),
            "ok",
        );
        log_fn("  You can now safely start your session.", "info");
    } else if received > 0 {
        let sum: f64 = rtts.iter().sum();
        let avg = sum / (rtts.len() as f64);
        log_fn(
            &format!("[warn] PARTIAL: {}/{} echoes. Tunnel is unstable.", received, max_successes),
            "warn",
        );
        log_fn(&format!("  RTT: avg {:.0} ms", avg), "warn");
        log_fn("  You might experience lag or disconnects in-game.", "warn");
    } else if icmp_reject {
        log_fn("[fail] FAILED: Tunnel port is closed or host firewall is blocking it.", "err");
    } else {
        log_fn("[fail] FAILED: No echoes received within timeout.", "err");
        log_fn("  Possible causes:", "dim");
        log_fn("  1. Host has not started the Echo Server yet.", "dim");
        log_fn("  2. Tunnel is down or misconfigured.", "dim");
        log_fn("  3. Host firewall is blocking the tunnel agent.", "dim");
    }
}
