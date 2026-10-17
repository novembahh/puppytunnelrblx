CREATE TABLE IF NOT EXISTS users (
    pubkey TEXT PRIMARY KEY,
    roblox_id INTEGER DEFAULT 0,
    username TEXT NOT NULL DEFAULT '',
    display_name TEXT NOT NULL DEFAULT '',
    bio TEXT DEFAULT '',
    accent TEXT DEFAULT '#5fe3ac',
    avatar_url TEXT DEFAULT '',
    banner_url TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    roblox_verified INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_roblox_id ON users(roblox_id);

CREATE TABLE IF NOT EXISTS follows (
    follower_pubkey TEXT NOT NULL,
    following_pubkey TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (follower_pubkey, following_pubkey),
    FOREIGN KEY (follower_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (following_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_follows_following ON follows(following_pubkey);

CREATE TABLE IF NOT EXISTS posts (
    id TEXT PRIMARY KEY,
    author_pubkey TEXT NOT NULL,
    content TEXT NOT NULL,
    attachments_json TEXT DEFAULT '[]',
    reply_to_id TEXT DEFAULT NULL,
    repost_of_id TEXT DEFAULT NULL,
    likes_count INTEGER NOT NULL DEFAULT 0,
    reposts_count INTEGER NOT NULL DEFAULT 0,
    replies_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    is_pinned INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (author_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (reply_to_id) REFERENCES posts(id) ON DELETE SET NULL,
    FOREIGN KEY (repost_of_id) REFERENCES posts(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_posts_author_created ON posts(author_pubkey, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_posts_reply_to ON posts(reply_to_id);
CREATE INDEX IF NOT EXISTS idx_posts_repost_of ON posts(repost_of_id);
CREATE INDEX IF NOT EXISTS idx_posts_created ON posts(created_at DESC);

CREATE TABLE IF NOT EXISTS likes (
    post_id TEXT NOT NULL,
    user_pubkey TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (post_id, user_pubkey),
    FOREIGN KEY (post_id) REFERENCES posts(id) ON DELETE CASCADE,
    FOREIGN KEY (user_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_likes_user ON likes(user_pubkey);

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    is_group INTEGER NOT NULL DEFAULT 0,
    name TEXT DEFAULT '',
    description TEXT DEFAULT '',
    icon_url TEXT DEFAULT '',
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (created_by) REFERENCES users(pubkey)
);

CREATE INDEX IF NOT EXISTS idx_conv_updated ON conversations(updated_at DESC);

CREATE TABLE IF NOT EXISTS conversation_members (
    conversation_id TEXT NOT NULL,
    user_pubkey TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    nickname TEXT DEFAULT '',
    joined_at INTEGER NOT NULL,
    last_read_message_id TEXT DEFAULT NULL,
    PRIMARY KEY (conversation_id, user_pubkey),
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (user_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_conv_members_user ON conversation_members(user_pubkey);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    sender_pubkey TEXT NOT NULL,
    content TEXT NOT NULL,
    attachments_json TEXT DEFAULT '[]',
    reply_to_id TEXT DEFAULT NULL,
    is_edited INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
    FOREIGN KEY (sender_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (reply_to_id) REFERENCES messages(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_conv_created ON messages(conversation_id, created_at ASC);

CREATE TABLE IF NOT EXISTS guilds (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT DEFAULT '',
    icon_url TEXT DEFAULT '',
    banner_url TEXT DEFAULT '',
    owner_pubkey TEXT NOT NULL,
    is_private INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (owner_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_guilds_owner ON guilds(owner_pubkey);

CREATE TABLE IF NOT EXISTS guild_channels (
    id TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL,
    name TEXT NOT NULL,
    topic TEXT DEFAULT '',
    kind TEXT NOT NULL DEFAULT 'text',
    position INTEGER NOT NULL DEFAULT 0,
    is_private INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_guild_channels_guild_pos ON guild_channels(guild_id, position ASC);

CREATE TABLE IF NOT EXISTS guild_members (
    guild_id TEXT NOT NULL,
    user_pubkey TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'member',
    nickname TEXT DEFAULT '',
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_pubkey),
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (user_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_guild_members_user ON guild_members(user_pubkey);

CREATE TABLE IF NOT EXISTS guild_bans (
    guild_id TEXT NOT NULL,
    user_pubkey TEXT NOT NULL,
    banned_by TEXT NOT NULL,
    reason TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (guild_id, user_pubkey),
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (user_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (banned_by) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS guild_invites (
    code TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL,
    created_by TEXT NOT NULL,
    max_uses INTEGER NOT NULL DEFAULT 0,
    uses INTEGER NOT NULL DEFAULT 0,
    expires_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_guild_invites_guild ON guild_invites(guild_id);

CREATE TABLE IF NOT EXISTS guild_messages (
    id TEXT PRIMARY KEY,
    channel_id TEXT NOT NULL,
    guild_id TEXT NOT NULL,
    sender_pubkey TEXT NOT NULL,
    content TEXT NOT NULL,
    attachments_json TEXT DEFAULT '[]',
    reply_to_id TEXT DEFAULT NULL,
    is_edited INTEGER NOT NULL DEFAULT 0,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES guild_channels(id) ON DELETE CASCADE,
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (sender_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (reply_to_id) REFERENCES guild_messages(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_guild_messages_channel_created ON guild_messages(channel_id, created_at ASC);

CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY,
    recipient_pubkey TEXT NOT NULL,
    actor_pubkey TEXT NOT NULL,
    kind TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    content TEXT DEFAULT '',
    is_read INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (recipient_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (actor_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_notifications_recipient_created ON notifications(recipient_pubkey, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_notifications_recipient_unread ON notifications(recipient_pubkey, is_read);

CREATE TABLE IF NOT EXISTS moderation_reports (
    id TEXT PRIMARY KEY,
    reporter_pubkey TEXT NOT NULL,
    reported_pubkey TEXT NOT NULL,
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    handled_by TEXT DEFAULT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (reporter_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (reported_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE,
    FOREIGN KEY (handled_by) REFERENCES users(pubkey) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_mod_reports_status ON moderation_reports(status, created_at DESC);

CREATE TABLE IF NOT EXISTS moderation_logs (
    id TEXT PRIMARY KEY,
    guild_id TEXT DEFAULT NULL,
    actor_pubkey TEXT NOT NULL,
    action TEXT NOT NULL,
    target_pubkey TEXT DEFAULT NULL,
    details TEXT DEFAULT '',
    created_at INTEGER NOT NULL,
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (actor_pubkey) REFERENCES users(pubkey) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_mod_logs_guild ON moderation_logs(guild_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mod_logs_actor ON moderation_logs(actor_pubkey, created_at DESC);

CREATE TABLE IF NOT EXISTS federated_instances (
    domain TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    public_key TEXT DEFAULT '',
    trust_level TEXT NOT NULL DEFAULT 'trusted',
    last_synced_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
