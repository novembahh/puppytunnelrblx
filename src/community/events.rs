use crate::identity::unix_now;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "details")]
pub enum PresenceStatus {
    Online,
    InGame {
        studio_place: String,
        place_id: Option<u64>,
    },
    Idle,
    Offline,
}

impl PresenceStatus {
    pub fn label(&self) -> &'static str {
        match self {
            PresenceStatus::Online => "Online",
            PresenceStatus::InGame { .. } => "In Studio",
            PresenceStatus::Idle => "Away / Idle",
            PresenceStatus::Offline => "Offline",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommunityMessage {
    pub id: String,
    pub guild_id: Option<String>,
    pub channel_id: String,
    pub author_pubkey: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub attachment_ids: Vec<String>,
    pub timestamp: u64,
    pub edited_at: Option<u64>,
    pub signature: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "event", content = "data")]
pub enum CommunityEvent {
    MessageCreated(CommunityMessage),
    MessageUpdated {
        guild_id: Option<String>,
        channel_id: String,
        message_id: String,
        new_content: String,
        edited_at: u64,
    },
    MessageDeleted {
        guild_id: Option<String>,
        channel_id: String,
        message_id: String,
    },
    ReactionAdded {
        channel_id: String,
        message_id: String,
        emoji: String,
        user_pubkey: String,
    },
    ReactionRemoved {
        channel_id: String,
        message_id: String,
        emoji: String,
        user_pubkey: String,
    },
    MemberJoined {
        guild_id: String,
        member_pubkey: String,
        joined_at: u64,
    },
    MemberLeft {
        guild_id: String,
        member_pubkey: String,
    },
    MemberUpdated {
        guild_id: String,
        member_pubkey: String,
        roles: Vec<String>,
    },
    PresenceUpdated {
        user_pubkey: String,
        status: PresenceStatus,
        last_heartbeat: u64,
    },
    TypingStarted {
        channel_id: String,
        user_pubkey: String,
        expires_at: u64,
    },
    ChannelCreated {
        guild_id: String,
        channel_id: String,
        name: String,
    },
    ChannelUpdated {
        guild_id: String,
        channel_id: String,
        name: String,
        topic: String,
    },
    ChannelDeleted {
        guild_id: String,
        channel_id: String,
    },
    AuditLogCreated {
        guild_id: String,
        entry_id: String,
    },
    ReportFiled {
        report_id: String,
        guild_id: Option<String>,
    },
    NotificationCreated {
        id: String,
        recipient_pubkey: String,
        title: String,
        body: String,
        category: String,
        timestamp: u64,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SequencedEvent {
    pub seq: u64,
    pub timestamp: u64,
    pub event: CommunityEvent,
}

#[derive(Debug, Clone)]
pub struct EventQueue {
    events: Vec<SequencedEvent>,
    next_seq: u64,
    max_history: usize,
}

impl EventQueue {
    pub fn new(max_history: usize) -> Self {
        Self {
            events: Vec::new(),
            next_seq: 1,
            max_history,
        }
    }

    pub fn push(&mut self, event: CommunityEvent) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        let sequenced = SequencedEvent {
            seq,
            timestamp: unix_now(),
            event,
        };
        self.events.push(sequenced);
        if self.events.len() > self.max_history {
            self.events.remove(0);
        }
        seq
    }

    pub fn get_since(&self, since_seq: u64) -> Vec<SequencedEvent> {
        self.events
            .iter()
            .filter(|e| e.seq > since_seq)
            .cloned()
            .collect()
    }

    pub fn latest_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }
}

pub type SharedEventQueue = Arc<Mutex<EventQueue>>;
