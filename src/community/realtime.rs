use crate::community::events::{CommunityEvent, EventQueue, PresenceStatus, SharedEventQueue};
pub use crate::community::events::CommunityMessage;
use crate::identity::unix_now;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const TYPING_DECAY_SECS: u64 = 5;

pub const IDLE_TIMEOUT_SECS: u64 = 30;

pub const OFFLINE_TIMEOUT_SECS: u64 = 90;

pub const DEFAULT_SYNC_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Default, Clone)]
pub struct TypingTracker {
    typing_entries: HashMap<(String, String), u64>,
}

impl TypingTracker {
    pub fn new() -> Self {
        Self {
            typing_entries: HashMap::new(),
        }
    }

    pub fn start_typing(&mut self, channel_id: impl Into<String>, user_pubkey: impl Into<String>) {
        let key = (channel_id.into(), user_pubkey.into());
        let expires_at = unix_now() + TYPING_DECAY_SECS;
        self.typing_entries.insert(key, expires_at);
    }

    pub fn stop_typing(&mut self, channel_id: &str, user_pubkey: &str) {
        self.typing_entries
            .remove(&(channel_id.to_string(), user_pubkey.to_string()));
    }

    pub fn get_typing_users(&mut self, channel_id: &str) -> Vec<String> {
        let now = unix_now();
        self.prune(now);

        self.typing_entries
            .iter()
            .filter(|((ch, _), exp)| ch == channel_id && **exp > now)
            .map(|((_, pk), _)| pk.clone())
            .collect()
    }

    pub fn prune(&mut self, now: u64) {
        self.typing_entries.retain(|_, exp| *exp > now);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct UserPresence {
    pub user_pubkey: String,
    pub status: PresenceStatus,
    pub custom_status: Option<String>,
    pub last_heartbeat: u64,
}

#[derive(Debug, Default, Clone)]
pub struct PresenceTracker {
    presences: HashMap<String, UserPresence>,
}

impl PresenceTracker {
    pub fn new() -> Self {
        Self {
            presences: HashMap::new(),
        }
    }

    pub fn update_presence(
        &mut self,
        user_pubkey: impl Into<String>,
        status: PresenceStatus,
        custom_status: Option<String>,
    ) {
        let pk = user_pubkey.into();
        let now = unix_now();
        self.presences.insert(
            pk.clone(),
            UserPresence {
                user_pubkey: pk,
                status,
                custom_status,
                last_heartbeat: now,
            },
        );
    }

    pub fn get_presence(&self, user_pubkey: &str) -> PresenceStatus {
        let now = unix_now();
        if let Some(p) = self.presences.get(user_pubkey) {
            let elapsed = now.saturating_sub(p.last_heartbeat);
            if elapsed > OFFLINE_TIMEOUT_SECS {
                PresenceStatus::Offline
            } else if elapsed > IDLE_TIMEOUT_SECS {
                match p.status {
                    PresenceStatus::Offline => PresenceStatus::Offline,
                    _ => PresenceStatus::Idle,
                }
            } else {
                p.status.clone()
            }
        } else {
            PresenceStatus::Offline
        }
    }

    pub fn get_active_users(&self) -> Vec<UserPresence> {
        let now = unix_now();
        self.presences
            .values()
            .filter(|p| now.saturating_sub(p.last_heartbeat) <= OFFLINE_TIMEOUT_SECS)
            .cloned()
            .collect()
    }
}

#[derive(Debug, Default, Clone)]
pub struct ReadReceiptTracker {
    read_cursors: HashMap<(String, String), u64>,
    channel_latest_seq: HashMap<String, u64>,
    unread_mentions: HashMap<(String, String), Vec<u64>>,
}

impl ReadReceiptTracker {
    pub fn new() -> Self {
        Self {
            read_cursors: HashMap::new(),
            channel_latest_seq: HashMap::new(),
            unread_mentions: HashMap::new(),
        }
    }

    pub fn record_incoming_message(
        &mut self,
        channel_id: &str,
        seq: u64,
        mentioned_pubkeys: &[&str],
    ) {
        let cur = self.channel_latest_seq.entry(channel_id.to_string()).or_insert(0);
        if seq > *cur {
            *cur = seq;
        }

        for pk in mentioned_pubkeys {
            let key = (pk.to_string(), channel_id.to_string());
            self.unread_mentions.entry(key).or_default().push(seq);
        }
    }

    pub fn mark_channel_read(&mut self, user_pubkey: &str, channel_id: &str, up_to_seq: u64) {
        let key = (user_pubkey.to_string(), channel_id.to_string());
        self.read_cursors.insert(key.clone(), up_to_seq);

        if let Some(mentions) = self.unread_mentions.get_mut(&key) {
            mentions.retain(|&seq| seq > up_to_seq);
        }
    }

    pub fn get_channel_unread(&self, user_pubkey: &str, channel_id: &str) -> (usize, usize) {
        let key = (user_pubkey.to_string(), channel_id.to_string());
        let last_read = self.read_cursors.get(&key).copied().unwrap_or(0);
        let latest = self.channel_latest_seq.get(channel_id).copied().unwrap_or(0);

        let unread_messages = latest.saturating_sub(last_read) as usize;
        let unread_mentions = self
            .unread_mentions
            .get(&key)
            .map(|list| list.len())
            .unwrap_or(0);

        (unread_messages, unread_mentions)
    }

    pub fn get_total_unread(&self, user_pubkey: &str, channel_ids: &[&str]) -> (usize, usize) {
        let mut total_msgs = 0;
        let mut total_mentions = 0;
        for ch in channel_ids {
            let (m, men) = self.get_channel_unread(user_pubkey, ch);
            total_msgs += m;
            total_mentions += men;
        }
        (total_msgs, total_mentions)
    }
}

#[derive(Clone, Debug)]
pub struct SyncConfig {
    pub poll_interval: Duration,
    pub master_url: String,
    pub user_pubkey: String,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_SYNC_INTERVAL,
            master_url: crate::master::DEFAULT_MASTER.to_string(),
            user_pubkey: String::new(),
        }
    }
}

pub struct CommunitySyncWorker {
    running: Arc<AtomicBool>,
    event_receiver: Receiver<CommunityEvent>,
    event_queue: SharedEventQueue,
    thread_handle: Option<JoinHandle<()>>,
}

impl CommunitySyncWorker {
    pub fn start(config: SyncConfig) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = channel::<CommunityEvent>();
        let event_queue: SharedEventQueue = Arc::new(Mutex::new(EventQueue::new(500)));

        let r_flag = Arc::clone(&running);
        let q_clone = Arc::clone(&event_queue);

        let thread_handle = thread::spawn(move || {
            let mut last_seq = 0;
            while r_flag.load(Ordering::Relaxed) {
                let polled_events = Self::poll_remote_batch(&config, last_seq);

                for event in polled_events {
                    if let Ok(mut q) = q_clone.lock() {
                        let seq = q.push(event.clone());
                        if seq > last_seq {
                            last_seq = seq;
                        }
                    }
                    let _ = tx.send(event);
                }

                let total_sleep = config.poll_interval;
                let step = Duration::from_millis(50);
                let mut elapsed = Duration::ZERO;
                while elapsed < total_sleep && r_flag.load(Ordering::Relaxed) {
                    thread::sleep(step);
                    elapsed += step;
                }
            }
        });

        Self {
            running,
            event_receiver: rx,
            event_queue,
            thread_handle: Some(thread_handle),
        }
    }

    fn poll_remote_batch(config: &SyncConfig, _since_seq: u64) -> Vec<CommunityEvent> {
        let events = Vec::new();

        if !config.master_url.is_empty() && config.master_url.starts_with("http") {
        }

        events
    }

    pub fn dispatch_local_event(&self, event: CommunityEvent) -> u64 {
        if let Ok(mut q) = self.event_queue.lock() {
            q.push(event)
        } else {
            0
        }
    }

    pub fn drain_events(&self) -> Vec<CommunityEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.event_receiver.try_recv() {
            out.push(event);
        }
        out
    }

    pub fn get_events_since(&self, cursor: u64) -> Vec<crate::community::events::SequencedEvent> {
        if let Ok(q) = self.event_queue.lock() {
            q.get_since(cursor)
        } else {
            Vec::new()
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.thread_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for CommunitySyncWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::community::CommunityMessage;

    #[test]
    fn test_typing_tracker() {
        let mut tracker = TypingTracker::new();
        tracker.start_typing("ch-general", "alice_pk");
        tracker.start_typing("ch-general", "bob_pk");
        tracker.start_typing("ch-other", "charlie_pk");

        let typing = tracker.get_typing_users("ch-general");
        assert_eq!(typing.len(), 2);
        assert!(typing.contains(&"alice_pk".to_string()));
        assert!(typing.contains(&"bob_pk".to_string()));

        tracker.stop_typing("ch-general", "alice_pk");
        let after_stop = tracker.get_typing_users("ch-general");
        assert_eq!(after_stop.len(), 1);
        assert_eq!(after_stop[0], "bob_pk");
    }

    #[test]
    fn test_presence_tracker() {
        let mut tracker = PresenceTracker::new();
        tracker.update_presence("alice_pk", PresenceStatus::Online, None);
        assert_eq!(tracker.get_presence("alice_pk"), PresenceStatus::Online);

        tracker.update_presence(
            "bob_pk",
            PresenceStatus::InGame {
                studio_place: "Crossroads".into(),
                place_id: Some(1818),
            },
            Some("Working on script".into()),
        );

        match tracker.get_presence("bob_pk") {
            PresenceStatus::InGame { studio_place, .. } => {
                assert_eq!(studio_place, "Crossroads");
            }
            _ => panic!("Expected InGame presence"),
        }

        assert_eq!(tracker.get_presence("unknown_pk"), PresenceStatus::Offline);
    }

    #[test]
    fn test_read_receipt_and_unread_badges() {
        let mut receipts = ReadReceiptTracker::new();
        receipts.record_incoming_message("channel-1", 1, &[]);
        receipts.record_incoming_message("channel-1", 2, &["alice_pk"]);
        receipts.record_incoming_message("channel-1", 3, &[]);

        let (unread, mentions) = receipts.get_channel_unread("alice_pk", "channel-1");
        assert_eq!(unread, 3);
        assert_eq!(mentions, 1);

        receipts.mark_channel_read("alice_pk", "channel-1", 2);
        let (unread_after, mentions_after) = receipts.get_channel_unread("alice_pk", "channel-1");
        assert_eq!(unread_after, 1);
        assert_eq!(mentions_after, 0);
    }

    #[test]
    fn test_sync_worker_lifecycle() {
        let config = SyncConfig {
            poll_interval: Duration::from_millis(50),
            master_url: String::new(),
            user_pubkey: "test-user".into(),
        };

        let mut worker = CommunitySyncWorker::start(config);
        let dummy_msg = CommunityMessage {
            id: "msg-1".into(),
            guild_id: None,
            channel_id: "general".into(),
            author_pubkey: "pk-1".into(),
            content: "Hello!".into(),
            reply_to: None,
            attachment_ids: vec![],
            timestamp: unix_now(),
            edited_at: None,
            signature: "sig".into(),
        };

        worker.dispatch_local_event(CommunityEvent::MessageCreated(dummy_msg));
        let events = worker.get_events_since(0);
        assert_eq!(events.len(), 1);

        worker.stop();
    }
}
