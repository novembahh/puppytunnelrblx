use rand::Rng;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn warm_tunnel(
    proxy_port: u16,
    packets: usize,
    interval_secs: f32,
    running: Option<Arc<AtomicBool>>,
) -> usize {
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(300)));

    let mut sent = 0;
    let mut rng = rand::thread_rng();

    for _ in 0..packets {
        if let Some(ref r) = running {
            if !r.load(Ordering::SeqCst) {
                break;
            }
        }

        let rand_len = rng.gen_range(6..=20);
        let mut payload = Vec::with_capacity(2 + rand_len);
        payload.push(0xff);
        payload.push(0x00);
        let mut rand_bytes = vec![0u8; rand_len];
        rng.fill(&mut rand_bytes[..]);
        payload.extend_from_slice(&rand_bytes);

        if sock
            .send_to(&payload, ("127.0.0.1", proxy_port))
            .is_ok()
        {
            sent += 1;
        }

        thread::sleep(Duration::from_secs_f32(interval_secs));
    }

    sent
}
