use rand::RngCore;
use std::net::{ToSocketAddrs, UdpSocket};
use std::process::Command;
use std::time::{Duration, Instant};

pub fn icmp_ping(host: &str) -> (bool, f64) {
    let t0 = Instant::now();
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("ping");
        c.args(["-n", "1", "-w", "2000", host]);
        c
    } else {
        let mut c = Command::new("ping");
        c.args(["-c", "1", "-W", "2", host]);
        c
    };

    match cmd.output() {
        Ok(out) => {
            let rtt = t0.elapsed().as_secs_f64() * 1000.0;
            (out.status.success(), rtt)
        }
        Err(_) => (false, -1.0),
    }
}

pub fn run_connectivity_test<F>(
    host: &str,
    port: u16,
    is_host_side: bool,
    local_server_port: Option<u16>,
    proxy_running: bool,
    mut log_fn: F,
) where
    F: FnMut(&str, &str),
{
    log_fn("--- Connectivity Test ---", "info");
    log_fn("  (For full tunnel verification, use Echo Test)", "dim");

    let mut passed = 0;
    let mut warned = 0;
    let mut failed = 0;


    log_fn(&format!("[1/4]  Resolving  {} ...", host), "warn");
    let ip_addr = match format!("{}:{}", host, port).to_socket_addrs() {
        Ok(mut addrs) => match addrs.next() {
            Some(a) => a.ip().to_string(),
            None => {
                log_fn(&format!("  [fail]  DNS FAILED - host not resolved: {}", host), "err");
                failed += 1;
                print_summary(&mut log_fn, passed, warned, failed);
                return;
            }
        },
        Err(e) => {
            log_fn(&format!("  [fail]  DNS FAILED - {}", e), "err");
            failed += 1;
            print_summary(&mut log_fn, passed, warned, failed);
            return;
        }
    };

    log_fn(
        &format!("  [ok]  DNS OK   {} -> {}", host, ip_addr),
        "ok",
    );
    log_fn(
        "       (this only means the hostname exists - not that your tunnel is active)",
        "dim",
    );
    passed += 1;


    log_fn(&format!("[2/4]  ICMP ping -> {} ...", ip_addr), "warn");
    let (alive, ping_rtt) = icmp_ping(&ip_addr);
    if alive {
        log_fn(
            &format!("  [ok]  Relay server is reachable  (ping ~ {:.0} ms)", ping_rtt),
            "ok",
        );
        log_fn(
            "       (this proves the relay IP is up - not that your tunnel is forwarding)",
            "dim",
        );
        passed += 1;
    } else {
        log_fn(&format!("  [fail]  ICMP ping failed - {} unreachable", ip_addr), "err");
        log_fn("    -> Tunnel address may be wrong, or relay is offline", "err");
        log_fn("    -> (Some relays block ICMP - continue to check UDP)", "warn");
        warned += 1;
    }


    let target_ip: String;
    let target_port: u16;

    if is_host_side {
        let lp = local_server_port.unwrap_or(port);
        log_fn(&format!("[3/4]  Checking local Studio port {} ...", lp), "warn");
        let pb = UdpSocket::bind("0.0.0.0:0");
        let mut port_ok = false;
        if let Ok(sock) = pb {
            let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
            let _ = sock.send_to(b"\xff\x00\x00", ("127.0.0.1", lp));
            let mut b = [0u8; 64];
            match sock.recv_from(&mut b) {
                Ok(_) => port_ok = true,
                Err(ref e)
                    if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                {

                    port_ok = true;
                }
                Err(ref e) if e.raw_os_error() == Some(10054) => {
                    port_ok = false;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    port_ok = false;
                }
                Err(_) => port_ok = true,
            }
        }

        if port_ok {
            log_fn(
                &format!("  [ok]  Port {} accepts UDP (no ICMP reject)", lp),
                "ok",
            );
            passed += 1;
        } else {
            log_fn(
                &format!("  [fail]  Port {} ICMP unreachable - Studio not running or OS firewall blocking it", lp),
                "err",
            );
            failed += 1;
        }

        target_ip = ip_addr;
        target_port = port;
    } else {
        log_fn("[3/4]  Checking local proxy on 127.0.0.1:55555 ...", "warn");
        if proxy_running {
            log_fn("  [ok]  Proxy active on port 55555", "ok");
            passed += 1;
        } else {
            log_fn("  [fail]  Proxy is NOT running - Connect first", "err");
            failed += 1;
        }

        target_ip = "127.0.0.1".to_string();
        target_port = 55555;
    }


    let probe_count = 5usize;
    log_fn(
        &format!("[4/4]  UDP probe burst -> {}:{} ({} packets) ...", target_ip, target_port, probe_count),
        "warn",
    );

    let mut sent_ok = 0;
    let mut icmp_err = false;

    if let Ok(pb2) = UdpSocket::bind("0.0.0.0:0") {
        let _ = pb2.set_read_timeout(Some(Duration::from_millis(350)));
        let mut rng = rand::thread_rng();

        for i in 0..probe_count {
            let mut payload = vec![0xff, 0x00, 0xaa, (i & 0xff) as u8];
            let mut rand_bytes = [0u8; 12];
            rng.fill_bytes(&mut rand_bytes);
            payload.extend_from_slice(&rand_bytes);

            if pb2.send_to(&payload, (&target_ip[..], target_port)).is_ok() {
                sent_ok += 1;
            }

            let mut b = [0u8; 64];
            match pb2.recv_from(&mut b) {
                Ok(_) => {}
                Err(ref e) if e.raw_os_error() == Some(10054) => {
                    icmp_err = true;
                    break;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    icmp_err = true;
                    break;
                }
                Err(_) => {}
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if icmp_err {
        log_fn(
            &format!("  [fail]  ICMP Port Unreachable - port {} is actively closed", target_port),
            "err",
        );
        failed += 1;
    } else if sent_ok == probe_count {
        log_fn(
            &format!("  [ok]  {}/{} probes sent, no ICMP errors", sent_ok, probe_count),
            "ok",
        );
        log_fn(
            "  [info]  No reply expected here - use Echo Test to confirm end-to-end path",
            "dim",
        );
        passed += 1;
    } else {
        log_fn(
            &format!("  [fail]  Only {}/{} probes sent", sent_ok, probe_count),
            "err",
        );
        failed += 1;
    }

    print_summary(&mut log_fn, passed, warned, failed);
}

fn print_summary<F>(log_fn: &mut F, passed: usize, warned: usize, failed: usize)
where
    F: FnMut(&str, &str),
{
    let (verdict, tag) = if failed > 0 {
        ("ISSUES DETECTED", "err")
    } else if warned > 0 {
        ("PARTIALLY OK - check warnings", "warn")
    } else {
        ("ALL CLEAR", "ok")
    };

    log_fn(
        &format!(
            "Result: {}  ({} passed | {} warnings | {} failed)",
            verdict, passed, warned, failed
        ),
        tag,
    );
    log_fn("-----------------------", "dim");
}
