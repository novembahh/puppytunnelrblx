use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Permissions(pub u64);

impl Permissions {
    pub const NONE: Self = Self(0);

    pub const VIEW_CHANNELS: Self = Self(1 << 0);
    pub const SEND_MESSAGES: Self = Self(1 << 1);
    pub const ATTACH_FILES: Self = Self(1 << 2);
    pub const EMBED_LINKS: Self = Self(1 << 3);
    pub const ADD_REACTIONS: Self = Self(1 << 4);
    pub const PIN_MESSAGES: Self = Self(1 << 5);
    pub const USE_VOICE: Self = Self(1 << 6);

    pub const MANAGE_MESSAGES: Self = Self(1 << 7);
    pub const MENTION_EVERYONE: Self = Self(1 << 8);

    pub const MANAGE_CHANNELS: Self = Self(1 << 9);
    pub const MANAGE_GUILD: Self = Self(1 << 10);
    pub const CREATE_INVITES: Self = Self(1 << 11);

    pub const KICK_MEMBERS: Self = Self(1 << 12);
    pub const BAN_MEMBERS: Self = Self(1 << 13);
    pub const MODERATE_MEMBERS: Self = Self(1 << 14);
    pub const MANAGE_ROLES: Self = Self(1 << 15);
    pub const VIEW_AUDIT_LOG: Self = Self(1 << 16);

    pub const ADMINISTRATOR: Self = Self(1 << 30);

    pub const ALL: Self = Self((1 << 31) - 1);

    pub const DEFAULT_MEMBER: Self = Self(
        Self::VIEW_CHANNELS.0
            | Self::SEND_MESSAGES.0
            | Self::ATTACH_FILES.0
            | Self::EMBED_LINKS.0
            | Self::ADD_REACTIONS.0
            | Self::CREATE_INVITES.0
            | Self::USE_VOICE.0,
    );

    pub const DEFAULT_MODERATOR: Self = Self(
        Self::DEFAULT_MEMBER.0
            | Self::MANAGE_MESSAGES.0
            | Self::PIN_MESSAGES.0
            | Self::KICK_MEMBERS.0
            | Self::BAN_MEMBERS.0
            | Self::MODERATE_MEMBERS.0
            | Self::VIEW_AUDIT_LOG.0,
    );

    pub const DEFAULT_ADMIN: Self = Self(
        Self::DEFAULT_MODERATOR.0
            | Self::MANAGE_CHANNELS.0
            | Self::MANAGE_GUILD.0
            | Self::MANAGE_ROLES.0
            | Self::MENTION_EVERYONE.0
            | Self::ADMINISTRATOR.0,
    );

    pub fn bits(&self) -> u64 {
        self.0
    }

    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub fn has(&self, perm: Self) -> bool {
        if self.contains(Self::ADMINISTRATOR) {
            return true;
        }
        self.contains(perm)
    }
}

impl std::ops::BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl std::ops::BitAnd for Permissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        self.intersect(rhs)
    }
}

impl std::fmt::Debug for Permissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Permissions(0x{:08X})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HierarchyRank {
    Banned,
    Muted,
    Member,
    CustomRole(u32),
    Moderator,
    Administrator,
    Owner,
}

impl HierarchyRank {
    pub fn numeric_value(&self) -> u32 {
        match self {
            HierarchyRank::Banned => 0,
            HierarchyRank::Muted => 10,
            HierarchyRank::Member => 20,
            HierarchyRank::CustomRole(r) => *r,
            HierarchyRank::Moderator => 80,
            HierarchyRank::Administrator => 90,
            HierarchyRank::Owner => 100,
        }
    }

    pub fn can_moderate(&self, target: &HierarchyRank) -> bool {
        if *target == HierarchyRank::Owner {
            return false;
        }
        self.numeric_value() > target.numeric_value()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub color_hex: String,
    pub position: u32,
    pub permissions: Permissions,
    pub hoist: bool,
}

impl Role {
    pub fn new(id: impl Into<String>, name: impl Into<String>, position: u32, permissions: Permissions) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            color_hex: "#5fe3ac".into(),
            position,
            permissions,
            hoist: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelPermissionOverride {
    pub allow: Permissions,
    pub deny: Permissions,
}

impl ChannelPermissionOverride {
    pub fn apply(&self, current: Permissions) -> Permissions {
        let after_deny = Permissions(current.0 & !self.deny.0);
        Permissions(after_deny.0 | self.allow.0)
    }
}

#[derive(Debug, Clone)]
pub struct PermissionContext<'a> {
    pub user_pubkey: &'a str,
    pub guild_owner_pubkey: &'a str,
    pub is_system_admin: bool,
    pub is_banned: bool,
    pub is_muted: bool,
    pub user_roles: &'a [&'a Role],
    pub everyone_permissions: Permissions,
    pub channel_override: Option<&'a ChannelPermissionOverride>,
}

pub struct PermissionResolver;

impl PermissionResolver {
    pub fn resolve(ctx: &PermissionContext) -> Permissions {
        if ctx.is_system_admin {
            return Permissions::ALL;
        }

        if ctx.user_pubkey == ctx.guild_owner_pubkey {
            return Permissions::ALL;
        }

        if ctx.is_banned {
            return Permissions::NONE;
        }

        let mut perms = ctx.everyone_permissions;

        for role in ctx.user_roles {
            perms.insert(role.permissions);
        }

        if perms.contains(Permissions::ADMINISTRATOR) {
            return Permissions::ALL;
        }

        if let Some(ch_override) = ctx.channel_override {
            perms = ch_override.apply(perms);
        }

        if ctx.is_muted {
            perms.remove(
                Permissions::SEND_MESSAGES
                    | Permissions::ATTACH_FILES
                    | Permissions::ADD_REACTIONS
                    | Permissions::USE_VOICE,
            );
        }

        perms
    }

    pub fn can_moderate(
        actor_pubkey: &str,
        actor_rank: HierarchyRank,
        actor_is_sysadmin: bool,
        target_pubkey: &str,
        target_rank: HierarchyRank,
        guild_owner_pubkey: &str,
    ) -> bool {
        if actor_is_sysadmin {
            return true;
        }

        if target_pubkey == guild_owner_pubkey {
            return false;
        }

        if actor_pubkey == guild_owner_pubkey {
            return true;
        }

        if actor_pubkey == target_pubkey {
            return false;
        }

        actor_rank.can_moderate(&target_rank)
    }
}

#[derive(Debug, Default, Clone)]
pub struct SystemAdminRegistry {
    pub admin_pubkeys: HashMap<String, String>,
}

impl SystemAdminRegistry {
    pub fn new() -> Self {
        Self {
            admin_pubkeys: HashMap::new(),
        }
    }

    pub fn add_admin(&mut self, pubkey: impl Into<String>, label: impl Into<String>) {
        self.admin_pubkeys.insert(pubkey.into(), label.into());
    }

    pub fn remove_admin(&mut self, pubkey: &str) {
        self.admin_pubkeys.remove(pubkey);
    }

    pub fn is_admin(&self, pubkey: &str) -> bool {
        self.admin_pubkeys.contains_key(pubkey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissions_bitflags() {
        let mut p = Permissions::DEFAULT_MEMBER;
        assert!(p.has(Permissions::SEND_MESSAGES));
        assert!(p.has(Permissions::VIEW_CHANNELS));
        assert!(!p.has(Permissions::BAN_MEMBERS));

        p.insert(Permissions::BAN_MEMBERS);
        assert!(p.has(Permissions::BAN_MEMBERS));

        p.remove(Permissions::SEND_MESSAGES);
        assert!(!p.has(Permissions::SEND_MESSAGES));

        let admin = Permissions::ADMINISTRATOR;
        assert!(admin.has(Permissions::MANAGE_CHANNELS));
    }

    #[test]
    fn test_hierarchy_moderation() {
        let owner = HierarchyRank::Owner;
        let admin = HierarchyRank::Administrator;
        let moderator = HierarchyRank::Moderator;
        let member = HierarchyRank::Member;
        let custom_role = HierarchyRank::CustomRole(50);

        assert!(owner.can_moderate(&admin));
        assert!(admin.can_moderate(&moderator));
        assert!(moderator.can_moderate(&custom_role));
        assert!(moderator.can_moderate(&member));

        assert!(!moderator.can_moderate(&moderator));
        assert!(!member.can_moderate(&moderator));
        assert!(!admin.can_moderate(&owner));
    }

    #[test]
    fn test_permission_resolver() {
        let role = Role::new("mod", "Moderator", 80, Permissions::DEFAULT_MODERATOR);
        let roles = vec![&role];

        let ctx = PermissionContext {
            user_pubkey: "mod_pk",
            guild_owner_pubkey: "owner_pk",
            is_system_admin: false,
            is_banned: false,
            is_muted: false,
            user_roles: &roles,
            everyone_permissions: Permissions::DEFAULT_MEMBER,
            channel_override: None,
        };

        let perms = PermissionResolver::resolve(&ctx);
        assert!(perms.has(Permissions::KICK_MEMBERS));
        assert!(perms.has(Permissions::SEND_MESSAGES));

        let mut muted_ctx = ctx.clone();
        muted_ctx.is_muted = true;
        let muted_perms = PermissionResolver::resolve(&muted_ctx);
        assert!(!muted_perms.has(Permissions::SEND_MESSAGES));
        assert!(muted_perms.has(Permissions::VIEW_CHANNELS));

        let mut banned_ctx = ctx.clone();
        banned_ctx.is_banned = true;
        let banned_perms = PermissionResolver::resolve(&banned_ctx);
        assert_eq!(banned_perms, Permissions::NONE);
    }
}
