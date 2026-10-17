use crate::community::crypto::{self, CommunityEnvelope};
use crate::community::types::Post;
use crate::identity::{self, hex, unhex, unix_now, Identity};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

pub const DEFAULT_INSTANCE: &str = "roblox.puppytunnel.ma";

pub const FEDERATION_MAX_AGE_SECS: u64 = 300;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FederatedAddress {
    pub username: String,
    pub instance: String,
}

impl FederatedAddress {
    pub fn new(username: &str, instance: &str) -> Result<Self, &'static str> {
        let u = username.trim().trim_start_matches('@').to_ascii_lowercase();
        let i = instance.trim().to_ascii_lowercase();

        if u.is_empty() || u.len() > 32 {
            return Err("username must be between 1 and 32 characters");
        }
        if !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err("username contains invalid characters");
        }

        if i.is_empty() || i.len() > 128 {
            return Err("instance must be between 1 and 128 characters");
        }
        if !i.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == ':') {
            return Err("instance contains invalid characters");
        }

        Ok(Self { username: u, instance: i })
    }

    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        let trimmed = raw.trim().trim_start_matches('@');
        if trimmed.is_empty() {
            return Err("address cannot be empty");
        }

        if let Some((user, inst)) = trimmed.split_once('@') {
            Self::new(user, inst)
        } else {
            Self::new(trimmed, DEFAULT_INSTANCE)
        }
    }

    pub fn is_local_to(&self, instance: &str) -> bool {
        self.instance.eq_ignore_ascii_case(instance)
    }
}

impl fmt::Display for FederatedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.username, self.instance)
    }
}

impl FromStr for FederatedAddress {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NodeInfoSoftware {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub repository: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NodeInfoUsage {
    pub users: u64,
    pub posts: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct InstanceMetadata {
    pub instance_name: String,
    pub description: String,
    pub public_key: String,
    pub maintainer: String,
    #[serde(default)]
    pub peer_instances: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub version: String,
    pub software: NodeInfoSoftware,
    pub protocols: Vec<String>,
    pub usage: NodeInfoUsage,
    pub open_registrations: bool,
    pub metadata: InstanceMetadata,
}

pub fn build_nodeinfo(
    instance_name: &str,
    description: &str,
    pubkey_hex: &str,
    users: u64,
    posts: u64,
    peers: Vec<String>,
) -> NodeInfo {
    NodeInfo {
        version: "2.1".to_string(),
        software: NodeInfoSoftware {
            name: "puppytunnel".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            repository: "https://github.com/puppytunnel/puppytunnel".to_string(),
        },
        protocols: vec!["puppytunnel-federation/1.0".to_string()],
        usage: NodeInfoUsage { users, posts },
        open_registrations: true,
        metadata: InstanceMetadata {
            instance_name: instance_name.to_string(),
            description: description.to_string(),
            public_key: pubkey_hex.to_string(),
            maintainer: "PuppyTunnel Network".to_string(),
            peer_instances: peers,
        },
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FederationPeer {
    pub instance: String,
    pub public_key: String,
    pub name: String,
    pub last_synced: u64,
    pub trusted: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HandshakeRequest {
    pub origin_instance: String,
    pub target_instance: String,
    pub instance_pubkey: String,
    pub ts: u64,
    pub nonce: String,
    pub sig: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HandshakeResponse {
    pub instance_name: String,
    pub instance_pubkey: String,
    pub status: String,
    pub node_info: NodeInfo,
    pub ts: u64,
    pub sig: String,
}

fn handshake_req_bytes(origin: &str, target: &str, ts: u64, nonce: &str) -> Vec<u8> {
    let mut msg = b"puppy-fed-req\0".to_vec();
    msg.extend_from_slice(origin.as_bytes());
    msg.push(0);
    msg.extend_from_slice(target.as_bytes());
    msg.push(0);
    msg.extend_from_slice(&ts.to_be_bytes());
    msg.extend_from_slice(nonce.as_bytes());
    msg
}

fn handshake_resp_bytes(origin: &str, target: &str, ts: u64, status: &str) -> Vec<u8> {
    let mut msg = b"puppy-fed-resp\0".to_vec();
    msg.extend_from_slice(origin.as_bytes());
    msg.push(0);
    msg.extend_from_slice(target.as_bytes());
    msg.push(0);
    msg.extend_from_slice(&ts.to_be_bytes());
    msg.extend_from_slice(status.as_bytes());
    msg
}

impl HandshakeRequest {
    pub fn sign(
        id: &Identity,
        origin_instance: &str,
        target_instance: &str,
    ) -> Self {
        let ts = unix_now();
        let nonce = Uuid::new_v4().to_string();
        let bytes = handshake_req_bytes(origin_instance, target_instance, ts, &nonce);
        let sig = id.sign(&bytes);
        Self {
            origin_instance: origin_instance.to_string(),
            target_instance: target_instance.to_string(),
            instance_pubkey: hex(&id.public()),
            ts,
            nonce,
            sig: hex(&sig),
        }
    }

    pub fn verify(&self, expected_target: &str, max_age_secs: u64) -> Result<[u8; 32], &'static str> {
        let pk = unhex::<32>(&self.instance_pubkey).ok_or("bad instance public key")?;
        let sig = unhex::<64>(&self.sig).ok_or("bad handshake signature")?;

        if !self.target_instance.eq_ignore_ascii_case(expected_target) {
            return Err("handshake target mismatch");
        }

        let now = unix_now();
        if now.abs_diff(self.ts) > max_age_secs {
            return Err("handshake timestamp expired");
        }

        let bytes = handshake_req_bytes(
            &self.origin_instance,
            &self.target_instance,
            self.ts,
            &self.nonce,
        );

        if !identity::verify(&pk, &bytes, &sig) {
            return Err("handshake signature invalid");
        }

        Ok(pk)
    }
}

impl HandshakeResponse {
    pub fn sign(
        id: &Identity,
        node_info: NodeInfo,
        origin_instance: &str,
        target_instance: &str,
        status: &str,
    ) -> Self {
        let ts = unix_now();
        let bytes = handshake_resp_bytes(origin_instance, target_instance, ts, status);
        let sig = id.sign(&bytes);
        Self {
            instance_name: node_info.metadata.instance_name.clone(),
            instance_pubkey: hex(&id.public()),
            status: status.to_string(),
            node_info,
            ts,
            sig: hex(&sig),
        }
    }

    pub fn verify(
        &self,
        origin_instance: &str,
        target_instance: &str,
        max_age_secs: u64,
    ) -> Result<[u8; 32], &'static str> {
        let pk = unhex::<32>(&self.instance_pubkey).ok_or("bad instance public key")?;
        let sig = unhex::<64>(&self.sig).ok_or("bad handshake signature")?;

        let now = unix_now();
        if now.abs_diff(self.ts) > max_age_secs {
            return Err("handshake response timestamp expired");
        }

        let bytes = handshake_resp_bytes(origin_instance, target_instance, self.ts, &self.status);
        if !identity::verify(&pk, &bytes, &sig) {
            return Err("handshake response signature invalid");
        }

        Ok(pk)
    }
}

pub fn verify_federated_post(
    envelope: &CommunityEnvelope<Post>,
    sending_peer: Option<&FederationPeer>,
    max_age_secs: u64,
) -> Result<(), &'static str> {
    crypto::verify_envelope(envelope, max_age_secs).map_err(|_| "invalid author envelope")?;

    let post = &envelope.payload;

    let addr = FederatedAddress::parse(&post.author_handle)?;
    if !post.origin_instance.is_empty() && !addr.instance.eq_ignore_ascii_case(&post.origin_instance) {
        return Err("author handle domain does not match post origin_instance");
    }

    if let Some(peer) = sending_peer {
        if !peer.trusted && !post.origin_instance.is_empty() {
            if !post.origin_instance.eq_ignore_ascii_case(&peer.instance) {
                return Err("untrusted peer relayed post from different origin instance");
            }
        }
    }

    Ok(())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build()
}

pub fn sync_posts_from_peer(
    peer_url: &str,
    since_ts: u64,
) -> Result<Vec<CommunityEnvelope<Post>>, String> {
    let base = peer_url.trim().trim_end_matches('/');
    let url = if base.contains("://") {
        format!("{}/api/community/federation/posts", base)
    } else {
        format!("https://{}/api/community/federation/posts", base)
    };

    let mut req = agent().get(&url);
    if since_ts > 0 {
        req = req.query("since", &since_ts.to_string());
    }

    req.call()
        .map_err(|e| format!("peer sync request failed: {}", e))?
        .into_json::<Vec<CommunityEnvelope<Post>>>()
        .map_err(|e| format!("failed to parse peer posts: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_federated_address_parsing() {
        let addr = FederatedAddress::parse("alice@roblox.puppytunnel.ma").unwrap();
        assert_eq!(addr.username, "alice");
        assert_eq!(addr.instance, "roblox.puppytunnel.ma");
        assert_eq!(addr.to_string(), "alice@roblox.puppytunnel.ma");

        let addr2 = FederatedAddress::parse("@bob@custom.domain").unwrap();
        assert_eq!(addr2.username, "bob");
        assert_eq!(addr2.instance, "custom.domain");

        let addr3 = FederatedAddress::parse("carol").unwrap();
        assert_eq!(addr3.username, "carol");
        assert_eq!(addr3.instance, DEFAULT_INSTANCE);

        assert!(FederatedAddress::parse("bad username!@domain").is_err());
        assert!(FederatedAddress::parse("").is_err());
    }

    #[test]
    fn test_federation_handshake() {
        let id_a = Identity::stable("instance-a-secret", 5001);
        let id_b = Identity::stable("instance-b-secret", 5002);

        let req = HandshakeRequest::sign(&id_a, "alpha.instance", "beta.instance");
        assert_eq!(req.origin_instance, "alpha.instance");
        assert_eq!(req.target_instance, "beta.instance");

        let pk = req.verify("beta.instance", 60).unwrap();
        assert_eq!(pk, id_a.public());

        assert!(req.verify("gamma.instance", 60).is_err());

        let info_b = build_nodeinfo("Beta Node", "Second instance", &hex(&id_b.public()), 10, 50, vec![]);
        let resp = HandshakeResponse::sign(&id_b, info_b, "alpha.instance", "beta.instance", "accepted");
        let pk_b = resp.verify("alpha.instance", "beta.instance", 60).unwrap();
        assert_eq!(pk_b, id_b.public());
    }

    #[test]
    fn test_anti_spoofing() {
        let author_id = Identity::profile("user-author");
        let post = Post {
            id: "post-1".to_string(),
            author_pubkey: hex(&author_id.public()),
            author_handle: "alice@alpha.net".to_string(),
            author_display: None,
            author_avatar_url: None,
            author_accent: None,
            author_roblox_id: None,
            author_roblox_verified: false,
            content: "Real post".to_string(),
            attachments: vec![],
            reply_to_id: None,
            repost_of_id: None,
            likes_count: 0,
            reposts_count: 0,
            replies_count: 0,
            created_at: unix_now(),
            edited_at: None,
            tags: vec![],
            visibility: "public".to_string(),
            federated: true,
            origin_instance: "alpha.net".to_string(),
            is_pinned: false,
            liked_by_me: false,
        };

        let env = crypto::sign_envelope(&author_id, "post:create", post).unwrap();

        let peer = FederationPeer {
            instance: "alpha.net".to_string(),
            public_key: "00".repeat(32),
            name: "Alpha".to_string(),
            last_synced: 0,
            trusted: false,
        };

        assert!(verify_federated_post(&env, Some(&peer), 60).is_ok());

        let rogue_peer = FederationPeer {
            instance: "beta.net".to_string(),
            public_key: "00".repeat(32),
            name: "Beta".to_string(),
            last_synced: 0,
            trusted: false,
        };
        assert_eq!(
            verify_federated_post(&env, Some(&rogue_peer), 60).unwrap_err(),
            "untrusted peer relayed post from different origin instance"
        );
    }
}
