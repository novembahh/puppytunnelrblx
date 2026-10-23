use crate::community::client::CommunityClient;
use crate::community::types::{Conversation, Guild, Notification, Post, Report, AuditLogEntry, GuildChannel, GuildMember, DirectMessage, GuildMessage, UserProfile};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Job {
    Feed(usize),
    Replies(String),
    Conversations,
    Messages(String),
    Guilds,
    GuildChannels(String),
    GuildMembers(String),
    GuildMessages(String),
    Notifications,
    Social,
    SelfProfile,
    User(String),
    Reports,
    Audit,
}

#[derive(Debug)]
pub enum Done {
    Feed(Result<Vec<Post>, String>),
    Replies(String, Result<Vec<Post>, String>),
    Conversations(Result<Vec<Conversation>, String>),
    Messages(String, Result<Vec<DirectMessage>, String>),
    Guilds(Result<Vec<Guild>, String>),
    GuildChannels(String, Result<Vec<GuildChannel>, String>),
    GuildMembers(String, Result<Vec<GuildMember>, String>),
    GuildMessages(String, Result<Vec<GuildMessage>, String>),
    Notifications(Result<Vec<Notification>, String>),
    Social(Result<(Vec<String>, Vec<String>, Vec<String>), String>),
    SelfProfile(Result<UserProfile, String>),
    User(String, Result<UserProfile, String>),
    Reports(Result<Vec<Report>, String>),
    Audit(Result<Vec<AuditLogEntry>, String>),
}

pub struct Worker {
    jobs: Sender<Job>,
    results: Receiver<Done>,
}

impl Worker {
    pub fn spawn(client: CommunityClient) -> Self {
        let (jobs, job_rx) = channel::<Job>();
        let (result_tx, results) = channel::<Done>();
        std::thread::Builder::new()
            .name("community-sync".into())
            .spawn(move || {
                while let Ok(job) = job_rx.recv() {
                    let out = run(&client, job);
                    if result_tx.send(out).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self { jobs, results }
    }

    pub fn request(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    pub fn drain(&self) -> Vec<Done> {
        let mut out = Vec::new();
        loop {
            match self.results.try_recv() {
                Ok(d) => out.push(d),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

fn run(client: &CommunityClient, job: Job) -> Done {
    match job {
        Job::Feed(n) => Done::Feed(client.get_feed(n, None)),
        Job::Replies(id) => {
            let out = client.get_replies(&id);
            Done::Replies(id, out)
        }
        Job::Conversations => Done::Conversations(client.get_conversations()),
        Job::Messages(id) => {
            let out = client.get_messages(&id, 50);
            Done::Messages(id, out)
        }
        Job::Guilds => Done::Guilds(client.list_guilds()),
        Job::GuildChannels(id) => {
            let out = client.get_guild(&id).map(|g| g.channels);
            Done::GuildChannels(id, out)
        }
        Job::GuildMembers(id) => Done::GuildMembers(id.clone(), client.get_guild_members(&id)),
        Job::GuildMessages(id) => {
            let out = client.get_guild_messages(&id, 50);
            Done::GuildMessages(id, out)
        }
        Job::Notifications => Done::Notifications(client.get_notifications()),
        Job::Social => Done::Social(client.get_social_lists()),
        Job::SelfProfile => Done::SelfProfile(
            client
                .identity_pubkey()
                .ok_or_else(|| "no identity".to_string())
                .and_then(|pk| client.get_profile(&pk)),
        ),
        Job::User(key) => Done::User(key.clone(), client.get_profile(&key)),
        Job::Reports => Done::Reports(client.get_reports()),
        Job::Audit => Done::Audit(client.get_audit_log()),
    }
}
