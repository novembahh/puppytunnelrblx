const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, POST, PUT, PATCH, DELETE, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization, X-Puppy-Pubkey, X-Puppy-Timestamp, X-Puppy-Signature",
  "Access-Control-Max-Age": "86400",
};

const json = (data, status = 200) =>
  new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json", ...CORS_HEADERS },
  });

const errorJson = (message, status = 400) => json({ success: false, error: message }, status);
const nowSec = () => Math.floor(Date.now() / 1000);
const genId = (prefix = "") => `${prefix ? prefix + "_" : ""}${Date.now().toString(36)}_${crypto.randomUUID().replace(/-/g, "").slice(0, 8)}`;

const hexToBytes = (hex) => {
  if (typeof hex !== "string" || hex.length % 2 !== 0) return null;
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    const b = parseInt(hex.substr(i, 2), 16);
    if (isNaN(b)) return null;
    bytes[i / 2] = b;
  }
  return bytes;
};

const bytesToHex = (bytes) => Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join("");

const sha256Hex = async (data) => {
  const buf = typeof data === "string" ? new TextEncoder().encode(data) : data;
  return bytesToHex(new Uint8Array(await crypto.subtle.digest("SHA-256", buf)));
};

async function verifyEd25519(pubkeyHex, msgBytes, sigHex) {
  try {
    const pk = hexToBytes(pubkeyHex);
    const sig = hexToBytes(sigHex);
    if (!pk || pk.length !== 32 || !sig || sig.length !== 64) return false;
    const key = await crypto.subtle.importKey("raw", pk, { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify({ name: "Ed25519" }, key, sig, msgBytes);
  } catch (e) {
    return false;
  }
}

async function getOrCreateUser(db, pubkey) {
  const pk = pubkey.toLowerCase().trim();
  let user = await db.prepare("SELECT * FROM users WHERE pubkey = ?").bind(pk).first();
  if (user) {
    if (pk === "d5a864d4363d596eeccb2e4b3ebadcc7eec5d36e2f1e29202a0a38096cf8f9bf" && !user.is_admin) {
      await db.prepare("UPDATE users SET is_admin = 1 WHERE pubkey = ?").bind(pk).run();
      user.is_admin = 1;
    }
    return { ok: true, user };
  }

  const now = nowSec();
  const shortId = pk.slice(0, 8);
  const isAdmin = pk === "d5a864d4363d596eeccb2e4b3ebadcc7eec5d36e2f1e29202a0a38096cf8f9bf" ? 1 : 0;
  await db
    .prepare(
      `INSERT INTO users (pubkey, roblox_id, username, display_name, bio, accent, avatar_url, banner_url, created_at, updated_at, is_admin, roblox_verified)
       VALUES (?, 0, ?, ?, '', '#5fe3ac', '', '', ?, ?, ?, 0)`
    )
    .bind(pk, `pup_${shortId}`, `Puppy ${shortId}`, now, now, isAdmin)
    .run();

  user = await db.prepare("SELECT * FROM users WHERE pubkey = ?").bind(pk).first();
  return { ok: true, user };
}

function makeEnvelopeSigningMessage(action, ts, nonce, payload_hash) {
  const enc = new TextEncoder();
  const p1 = enc.encode("puppy-community\0" + action + "\0");
  const tsBuf = new Uint8Array(8);
  const view = new DataView(tsBuf.buffer);
  view.setBigUint64(0, BigInt(ts), false);
  const p2 = enc.encode(nonce + "\0" + payload_hash);

  const msg = new Uint8Array(p1.length + 8 + p2.length);
  msg.set(p1, 0);
  msg.set(tsBuf, p1.length);
  msg.set(p2, p1.length + 8);
  return msg;
}

async function authenticate(request, env, rawBody = null, parsedBody = null) {
  if (parsedBody && parsedBody.pubkey && parsedBody.sig && parsedBody.ts && parsedBody.action && parsedBody.nonce) {
    const ts = parseInt(parsedBody.ts, 10);
    if (Math.abs(nowSec() - ts) <= 600) {
      const msg = makeEnvelopeSigningMessage(parsedBody.action, ts, parsedBody.nonce, parsedBody.payload_hash || "");
      if (await verifyEd25519(parsedBody.pubkey, msg, parsedBody.sig)) {
        return await getOrCreateUser(env.DB, parsedBody.pubkey);
      }
    }
  }

  const auth = request.headers.get("Authorization");
  if (auth && auth.startsWith("Bearer ")) {
    const token = auth.slice(7).trim();
    if (token) {
      const pubkey = await env.KV.get(`session:${token}`);
      if (pubkey) return await getOrCreateUser(env.DB, pubkey);
    }
  }

  const pubkey = request.headers.get("X-Puppy-Pubkey");
  const tsStr = request.headers.get("X-Puppy-Timestamp");
  const sig = request.headers.get("X-Puppy-Signature");
  if (pubkey && tsStr && sig) {
    const ts = parseInt(tsStr, 10);
    if (Math.abs(nowSec() - ts) <= 600) {
      const url = new URL(request.url);
      const method = request.method.toUpperCase();
      const bodyHash = rawBody && rawBody.length > 0 ? `:${await sha256Hex(rawBody)}` : "";

      const candidates = [
        new TextEncoder().encode(`puppy-auth:${method}:${url.pathname}:${ts}${bodyHash}`),
        new TextEncoder().encode(`puppy-auth:${method}:${url.pathname}:${ts}`),
        new TextEncoder().encode(`puppy-auth:${ts}`),
      ];

      for (const msg of candidates) {
        if (await verifyEd25519(pubkey, msg, sig)) {
          return await getOrCreateUser(env.DB, pubkey);
        }
      }
    }
  }

  return null;
}

async function notify(db, recipient, actor, kind, entityType, entityId, content) {
  if (!recipient || recipient === actor) return;
  try {
    await db
      .prepare(
        `INSERT INTO notifications (id, recipient_pubkey, actor_pubkey, kind, entity_type, entity_id, content, is_read, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)`
      )
      .bind(genId("notif"), recipient, actor, kind, entityType, entityId, content, nowSec())
      .run();
  } catch (e) {}
}

export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    const url = new URL(request.url);
    const path = url.pathname;
    const method = request.method;
    const domain = env.INSTANCE_DOMAIN || "roblox.puppytunnel.ma";
    const name = env.INSTANCE_NAME || "Puppytunnel Community";

    let rawBody = null;
    let body = {};
    if (["POST", "PUT", "PATCH"].includes(method)) {
      try {
        rawBody = await request.text();
        if (rawBody) body = JSON.parse(rawBody);
      } catch (e) {
        return errorJson("Invalid JSON body", 400);
      }
    }

    const isEnvelope = !!(body && body.action && body.pubkey && body.sig);
    const actualBody = (isEnvelope && body.payload !== undefined && typeof body.payload === "object" && body.payload !== null)
      ? body.payload
      : body;

    try {
      if (path === "/" || path === "/health" || path === "/api/community") {
        const stats = await env.DB.batch([
          env.DB.prepare("SELECT COUNT(*) AS c FROM users"),
          env.DB.prepare("SELECT COUNT(*) AS c FROM posts"),
          env.DB.prepare("SELECT COUNT(*) AS c FROM guilds"),
        ]);
        return json({
          status: "healthy",
          service: "puppytunnel-community",
          domain,
          name,
          stats: {
            users: stats[0].results[0].c,
            posts: stats[1].results[0].c,
            guilds: stats[2].results[0].c,
          },
        });
      }

      if (path === "/.well-known/nodeinfo") {
        return json({
          links: [{ rel: "http://nodeinfo.diaspora.software/ns/schema/2.1", href: `https://${url.host}/nodeinfo/2.1` }],
        });
      }

      if (path === "/nodeinfo/2.1" || path === "/.well-known/nodeinfo/2.1") {
        const u = (await env.DB.prepare("SELECT COUNT(*) as c FROM users").first("c")) || 0;
        const p = (await env.DB.prepare("SELECT COUNT(*) as c FROM posts").first("c")) || 0;
        return json({
          version: "2.1",
          software: { name: "puppytunnel-community", version: "1.0.0" },
          protocols: ["puppytunnel", "activitypub"],
          services: { inbound: [], outbound: [] },
          openRegistrations: true,
          usage: { users: { total: u, activeMonth: u }, localPosts: p, localComments: 0 },
          metadata: { nodeName: name, federation: true },
        });
      }

      if (path === "/api/community/federation/instance" && method === "GET") {
        return json({ domain, name, version: "1.0.0", protocols: ["puppytunnel", "activitypub"] });
      }

      if (path === "/api/community/federation/peers") {
        if (method === "GET") {
          const peers = await env.DB.prepare("SELECT * FROM federated_instances ORDER BY created_at DESC").all();
          return json({ peers: peers.results });
        }
        if (method === "POST") {
          const a = await authenticate(request, env, rawBody);
          if (!a || !a.user || !a.user.is_admin) return errorJson("Admin required", 403);
          const { domain: d, name: n, public_key = "", trust_level = "trusted" } = body;
          if (!d || !n) return errorJson("domain and name required", 400);
          await env.DB.prepare(
            `INSERT INTO federated_instances (domain, name, public_key, trust_level, last_synced_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(domain) DO UPDATE SET name=excluded.name, public_key=excluded.public_key, trust_level=excluded.trust_level, last_synced_at=excluded.last_synced_at`
          )
            .bind(d.toLowerCase(), n, public_key, trust_level, nowSec(), nowSec())
            .run();
          return json({ success: true, domain: d });
        }
      }

      if (path === "/api/community/auth/session" && method === "POST") {
        const { pubkey, timestamp, signature } = body;
        if (!pubkey || !timestamp || !signature) return errorJson("pubkey, timestamp, signature required", 400);
        const ts = parseInt(timestamp, 10);
        if (Math.abs(nowSec() - ts) > 600) return errorJson("Timestamp skewed", 400);

        const ok =
          (await verifyEd25519(pubkey, new TextEncoder().encode(`puppy-auth:session:${ts}`), signature)) ||
          (await verifyEd25519(pubkey, new TextEncoder().encode(`puppy-auth:${ts}`), signature));
        if (!ok) return errorJson("Invalid cryptographic signature", 401);

        const token = crypto.randomUUID().replace(/-/g, "") + crypto.randomUUID().replace(/-/g, "");
        const userRes = await getOrCreateUser(env.DB, pubkey);
        await env.KV.put(`session:${token}`, pubkey.toLowerCase(), { expirationTtl: 1209600 });
        return json({ success: true, token, user: userRes.user, expires_in: 1209600 });
      }

      if (path === "/api/community/auth/me" && method === "GET") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);
        return json({ success: true, user: a.user });
      }

      if ((path === "/api/community/users/profile" || path === "/api/community/profile") && ["POST", "PUT"].includes(method)) {
        const a = await authenticate(request, env, rawBody, body);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const pb = actualBody || body;
        const { username, display_name, bio, accent, avatar_url, banner_url, roblox_id } = pb;
        const cur = a.user;
        const newU = username ? username.trim().toLowerCase() : cur.username;
        if (newU !== cur.username) {
          const conflict = await env.DB.prepare("SELECT pubkey FROM users WHERE username = ? AND pubkey != ?").bind(newU, cur.pubkey).first();
          if (conflict) return errorJson("Username taken", 409);
        }

        await env.DB.prepare(
          `UPDATE users SET username=?, display_name=?, bio=?, accent=?, avatar_url=?, banner_url=?, roblox_id=?, updated_at=? WHERE pubkey=?`
        )
          .bind(
            newU,
            display_name !== undefined ? display_name.trim() : cur.display_name,
            bio !== undefined ? bio.trim() : cur.bio,
            accent !== undefined ? accent.trim() : cur.accent,
            avatar_url !== undefined ? avatar_url.trim() : cur.avatar_url,
            banner_url !== undefined ? banner_url.trim() : cur.banner_url,
            roblox_id !== undefined ? parseInt(roblox_id, 10) || 0 : cur.roblox_id,
            nowSec(),
            cur.pubkey
          )
          .run();

        const updated = await env.DB.prepare("SELECT * FROM users WHERE pubkey = ?").bind(cur.pubkey).first();
        if (isEnvelope) {
          return json(updated);
        } else {
          return json({ success: true, user: updated });
        }
      }

      if (path === "/api/community/profile" && method === "GET") {
        const id = (url.searchParams.get("id") || "").toLowerCase().trim();
        const u = await env.DB.prepare("SELECT * FROM users WHERE pubkey=? OR username=? OR roblox_id=?").bind(id, id, parseInt(id, 10) || 0).first();
        if (!u) return errorJson("User not found", 404);
        return json({
          pubkey: u.pubkey,
          handle: u.username || `pup_${u.pubkey.slice(0, 8)}`,
          display_name: u.display_name || u.username,
          bio: u.bio || "",
          accent_color: u.accent || "#5fe3ac",
          avatar_url: u.avatar_url || null,
          roblox_id: u.roblox_id ? Number(u.roblox_id) : null,
          roblox_verified: !!u.roblox_verified,
        });
      }

      if (path === "/api/community/users/follow" && method === "POST") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);
        const { target_pubkey } = body;
        if (!target_pubkey) return errorJson("target_pubkey required", 400);
        const target = target_pubkey.toLowerCase();
        if (target === a.user.pubkey) return errorJson("Cannot follow yourself", 400);

        const exists = await env.DB.prepare("SELECT 1 FROM follows WHERE follower_pubkey=? AND following_pubkey=?").bind(a.user.pubkey, target).first();
        if (exists) {
          await env.DB.prepare("DELETE FROM follows WHERE follower_pubkey=? AND following_pubkey=?").bind(a.user.pubkey, target).run();
          return json({ success: true, following: false });
        } else {
          await env.DB.prepare("INSERT INTO follows (follower_pubkey, following_pubkey, created_at) VALUES (?, ?, ?)").bind(a.user.pubkey, target, nowSec()).run();
          await notify(env.DB, target, a.user.pubkey, "follow", "user", a.user.pubkey, `${a.user.display_name} started following you`);
          return json({ success: true, following: true });
        }
      }

      const userMatch = path.match(/^\/api\/community\/users\/([^\/]+)$/);
      if (userMatch && method === "GET") {
        const t = userMatch[1].toLowerCase();
        const a = await authenticate(request, env, rawBody);
        const u = await env.DB.prepare("SELECT * FROM users WHERE pubkey=? OR username=? OR roblox_id=?").bind(t, t, parseInt(t, 10) || 0).first();
        if (!u) return errorJson("User not found", 404);

        const counts = await env.DB.batch([
          env.DB.prepare("SELECT COUNT(*) as c FROM follows WHERE following_pubkey=?").bind(u.pubkey),
          env.DB.prepare("SELECT COUNT(*) as c FROM follows WHERE follower_pubkey=?").bind(u.pubkey),
          env.DB.prepare("SELECT COUNT(*) as c FROM posts WHERE author_pubkey=?").bind(u.pubkey),
        ]);

        let isFollowing = false;
        if (a && a.user) {
          const f = await env.DB.prepare("SELECT 1 FROM follows WHERE follower_pubkey=? AND following_pubkey=?").bind(a.user.pubkey, u.pubkey).first();
          isFollowing = !!f;
        }

        return json({
          success: true,
          user: {
            ...u,
            followers_count: counts[0].results[0].c,
            following_count: counts[1].results[0].c,
            posts_count: counts[2].results[0].c,
            is_following: isFollowing,
          },
        });
      }

      const userPostsMatch = path.match(/^\/api\/community\/users\/([^\/]+)\/posts$/);
      if (userPostsMatch && method === "GET") {
        const t = userPostsMatch[1].toLowerCase();
        const u = await env.DB.prepare("SELECT pubkey FROM users WHERE pubkey=? OR username=?").bind(t, t).first();
        if (!u) return errorJson("User not found", 404);

        const posts = await env.DB.prepare(
          `SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent, u.roblox_id
           FROM posts p JOIN users u ON p.author_pubkey = u.pubkey
           WHERE p.author_pubkey = ? ORDER BY p.created_at DESC LIMIT 30`
        )
          .bind(u.pubkey)
          .all();
        return json({ success: true, posts: posts.results });
      }

      if ((path === "/api/community/feed" || path === "/api/community/posts") && method === "GET") {
        const a = await authenticate(request, env, rawBody, body);
        const tab = url.searchParams.get("tab") || "all";
        const limit = Math.min(parseInt(url.searchParams.get("limit") || "25", 10), 50);
        await env.DB.prepare("DELETE FROM posts WHERE created_at < ?").bind(nowSec() - 3888000).run();

        let query = `SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent, u.roblox_id FROM posts p JOIN users u ON p.author_pubkey = u.pubkey`;
        let params = [];

        if (tab === "following" && a && a.user) {
          query += ` JOIN follows f ON f.following_pubkey = p.author_pubkey WHERE f.follower_pubkey = ? ORDER BY p.created_at DESC LIMIT ?`;
          params = [a.user.pubkey, limit];
        } else if (tab === "trending") {
          query += ` WHERE p.created_at > ? ORDER BY (p.likes_count * 2 + p.reposts_count * 3 + p.replies_count) DESC LIMIT ?`;
          params = [nowSec() - 7 * 86400, limit];
        } else {
          query += ` WHERE p.reply_to_id IS NULL ORDER BY p.is_pinned DESC, p.created_at DESC LIMIT ?`;
          params = [limit];
        }

        const res = await env.DB.prepare(query).bind(...params).all();
        let posts = res.results;

        if (a && a.user && posts.length > 0) {
          const ids = posts.map((p) => p.id);
          const ph = ids.map(() => "?").join(",");
          const myLikes = await env.DB.prepare(`SELECT post_id FROM likes WHERE user_pubkey=? AND post_id IN (${ph})`).bind(a.user.pubkey, ...ids).all();
          const likeSet = new Set(myLikes.results.map((l) => l.post_id));
          posts = posts.map((p) => ({ ...p, liked_by_me: likeSet.has(p.id) }));
        }

        const formattedPosts = posts.map((p) => ({
          id: p.id,
          author_pubkey: p.author_pubkey,
          author_handle: p.username ? `@${p.username}` : `@pup_${p.author_pubkey.slice(0, 8)}`,
          author_display: p.display_name || p.username || `pup_${p.author_pubkey.slice(0, 8)}`,
          author_avatar_url: p.avatar_url || null,
          author_roblox_id: p.roblox_id ? Number(p.roblox_id) : null,
          author_accent: p.accent || "#5fe3ac",
          content: p.content || "",
          attachments: p.attachments_json ? JSON.parse(p.attachments_json) : [],
          reply_to_id: p.reply_to_id || null,
          repost_of_id: p.repost_of_id || null,
          likes_count: Number(p.likes_count || 0),
          reposts_count: Number(p.reposts_count || 0),
          replies_count: Number(p.replies_count || 0),
          created_at: Number(p.created_at || nowSec()),
          is_pinned: !!p.is_pinned,
          liked_by_me: !!p.liked_by_me,
          roblox_verified: !!p.roblox_verified,
        }));

        if (path === "/api/community/posts") {
          return json(formattedPosts);
        } else {
          return json({ success: true, posts: formattedPosts });
        }
      }

      if (path === "/api/community/posts" && method === "POST") {
        const a = await authenticate(request, env, rawBody, body);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const { content = "", attachments = [], reply_to_id = null, repost_of_id = null } = actualBody;
        if (!content.trim() && !repost_of_id && attachments.length === 0) return errorJson("Post cannot be empty", 400);

        const id = genId("post");
        const now = nowSec();

        await env.DB.prepare(
          `INSERT INTO posts (id, author_pubkey, content, attachments_json, reply_to_id, repost_of_id, likes_count, reposts_count, replies_count, created_at, updated_at, is_pinned)
           VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, ?, ?, 0)`
        )
          .bind(id, a.user.pubkey, content.trim(), JSON.stringify(attachments), reply_to_id, repost_of_id, now, now)
          .run();

        if (repost_of_id) {
          await env.DB.prepare("UPDATE posts SET reposts_count = reposts_count + 1 WHERE id = ?").bind(repost_of_id).run();
          const orig = await env.DB.prepare("SELECT author_pubkey FROM posts WHERE id = ?").bind(repost_of_id).first();
          if (orig) await notify(env.DB, orig.author_pubkey, a.user.pubkey, "repost", "post", id, `${a.user.display_name} reposted your post`);
        }

        if (reply_to_id) {
          await env.DB.prepare("UPDATE posts SET replies_count = replies_count + 1 WHERE id = ?").bind(reply_to_id).run();
          const parent = await env.DB.prepare("SELECT author_pubkey FROM posts WHERE id = ?").bind(reply_to_id).first();
          if (parent) await notify(env.DB, parent.author_pubkey, a.user.pubkey, "reply", "post", id, `${a.user.display_name} replied to your post`);
        }

        const created = await env.DB.prepare(
          "SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent, u.roblox_id FROM posts p JOIN users u ON p.author_pubkey = u.pubkey WHERE p.id = ?"
        )
          .bind(id)
          .first();

        const formatted = {
          id: created.id,
          author_pubkey: created.author_pubkey,
          author_handle: created.username ? `@${created.username}` : `@pup_${created.author_pubkey.slice(0, 8)}`,
          author_display: created.display_name || created.username || `pup_${created.author_pubkey.slice(0, 8)}`,
          author_avatar_url: created.avatar_url || null,
          author_roblox_id: created.roblox_id ? Number(created.roblox_id) : null,
          author_accent: created.accent || "#5fe3ac",
          content: created.content || "",
          attachments: created.attachments_json ? JSON.parse(created.attachments_json) : [],
          reply_to_id: created.reply_to_id || null,
          repost_of_id: created.repost_of_id || null,
          likes_count: Number(created.likes_count || 0),
          reposts_count: Number(created.reposts_count || 0),
          replies_count: Number(created.replies_count || 0),
          created_at: Number(created.created_at || now),
          is_pinned: !!created.is_pinned,
          liked_by_me: false,
          roblox_verified: !!created.roblox_verified,
        };

        if (isEnvelope) {
          return json(formatted, 201);
        } else {
          return json({ success: true, post: formatted }, 201);
        }
      }

      const postMatch = path.match(/^\/api\/community\/posts\/([^\/]+)$/);
      if (postMatch) {
        const id = postMatch[1];
        if (method === "GET") {
          const a = await authenticate(request, env, rawBody);
          const post = await env.DB.prepare(
            "SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent FROM posts p JOIN users u ON p.author_pubkey = u.pubkey WHERE p.id = ?"
          )
            .bind(id)
            .first();
          if (!post) return errorJson("Post not found", 404);

          const replies = await env.DB.prepare(
            "SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent FROM posts p JOIN users u ON p.author_pubkey = u.pubkey WHERE p.reply_to_id = ? ORDER BY p.created_at ASC"
          )
            .bind(id)
            .all();

          let liked_by_me = false;
          if (a && a.user) {
            const l = await env.DB.prepare("SELECT 1 FROM likes WHERE post_id=? AND user_pubkey=?").bind(id, a.user.pubkey).first();
            liked_by_me = !!l;
          }

          return json({ success: true, post: { ...post, liked_by_me }, replies: replies.results });
        }

        if (method === "DELETE") {
          const a = await authenticate(request, env, rawBody);
          if (!a || !a.user) return errorJson("Unauthorized", 401);
          const p = await env.DB.prepare("SELECT author_pubkey FROM posts WHERE id=?").bind(id).first();
          if (!p) return errorJson("Post not found", 404);
          if (p.author_pubkey !== a.user.pubkey && !a.user.is_admin) return errorJson("Forbidden", 403);
          await env.DB.prepare("DELETE FROM posts WHERE id=?").bind(id).run();
          return json({ success: true, deleted: id });
        }
      }

      const postLikeMatch = path.match(/^\/api\/community\/posts\/([^\/]+)\/like$/);
      if (postLikeMatch && method === "POST") {
        const id = postLikeMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const post = await env.DB.prepare("SELECT author_pubkey, likes_count FROM posts WHERE id=?").bind(id).first();
        if (!post) return errorJson("Post not found", 404);

        if (post.author_pubkey === a.user.pubkey) return errorJson("Cannot like your own post", 400);

        const exists = await env.DB.prepare("SELECT 1 FROM likes WHERE post_id=? AND user_pubkey=?").bind(id, a.user.pubkey).first();
        if (exists) {
          await env.DB.batch([
            env.DB.prepare("DELETE FROM likes WHERE post_id=? AND user_pubkey=?").bind(id, a.user.pubkey),
            env.DB.prepare("UPDATE posts SET likes_count = MAX(0, likes_count - 1) WHERE id=?").bind(id),
          ]);
          return json({ success: true, liked: false, likes_count: Math.max(0, post.likes_count - 1) });
        } else {
          await env.DB.batch([
            env.DB.prepare("INSERT INTO likes (post_id, user_pubkey, created_at) VALUES (?, ?, ?)").bind(id, a.user.pubkey, nowSec()),
            env.DB.prepare("UPDATE posts SET likes_count = likes_count + 1 WHERE id=?").bind(id),
          ]);
          await notify(env.DB, post.author_pubkey, a.user.pubkey, "like", "post", id, `${a.user.display_name} liked your post`);
          return json({ success: true, liked: true, likes_count: post.likes_count + 1 });
        }
      }

      const postReplyMatch = path.match(/^\/api\/community\/posts\/([^\/]+)\/reply$/);
      if (postReplyMatch && method === "POST") {
        const parentId = postReplyMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const parent = await env.DB.prepare("SELECT author_pubkey FROM posts WHERE id=?").bind(parentId).first();
        if (!parent) return errorJson("Post not found", 404);

        const { content = "", attachments = [] } = body;
        if (!content.trim() && attachments.length === 0) return errorJson("Content required", 400);

        const id = genId("post");
        const now = nowSec();
        await env.DB.batch([
          env.DB.prepare(
            `INSERT INTO posts (id, author_pubkey, content, attachments_json, reply_to_id, likes_count, reposts_count, replies_count, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 0, 0, 0, ?, ?)`
          ).bind(id, a.user.pubkey, content.trim(), JSON.stringify(attachments), parentId, now, now),
          env.DB.prepare("UPDATE posts SET replies_count = replies_count + 1 WHERE id=?").bind(parentId),
        ]);

        await notify(env.DB, parent.author_pubkey, a.user.pubkey, "reply", "post", id, `${a.user.display_name} replied to your post`);
        const created = await env.DB.prepare(
          "SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent FROM posts p JOIN users u ON p.author_pubkey = u.pubkey WHERE p.id = ?"
        )
          .bind(id)
          .first();
        return json({ success: true, reply: created }, 201);
      }

      if (path === "/api/community/dms") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        if (method === "GET") {
          const convs = await env.DB.prepare(
            `SELECT c.*, cm.role AS my_role, cm.last_read_message_id
             FROM conversations c JOIN conversation_members cm ON c.id = cm.conversation_id
             WHERE cm.user_pubkey = ? ORDER BY c.updated_at DESC`
          )
            .bind(a.user.pubkey)
            .all();

          const results = [];
          for (const c of convs.results) {
            const members = await env.DB.prepare(
              `SELECT u.pubkey, u.username, u.display_name, u.avatar_url, u.accent, cm.role
               FROM conversation_members cm JOIN users u ON cm.user_pubkey = u.pubkey WHERE cm.conversation_id = ?`
            )
              .bind(c.id)
              .all();
            const last = await env.DB.prepare("SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at DESC LIMIT 1").bind(c.id).first();
            results.push({ ...c, members: members.results, last_message: last });
          }
          return json({ success: true, conversations: results });
        }

        if (method === "POST") {
          const { recipient_pubkey, is_group = false, name: gname = "", members = [] } = body;
          const now = nowSec();

          if (!is_group) {
            if (!recipient_pubkey) return errorJson("recipient_pubkey required", 400);
            const target = recipient_pubkey.toLowerCase();
            const existing = await env.DB.prepare(
              `SELECT c.id FROM conversations c
               JOIN conversation_members cm1 ON c.id = cm1.conversation_id AND cm1.user_pubkey = ?
               JOIN conversation_members cm2 ON c.id = cm2.conversation_id AND cm2.user_pubkey = ?
               WHERE c.is_group = 0 LIMIT 1`
            )
              .bind(a.user.pubkey, target)
              .first();

            if (existing) return json({ success: true, conversation_id: existing.id, is_existing: true });

            const id = genId("dm");
            await env.DB.batch([
              env.DB.prepare("INSERT INTO conversations (id, is_group, created_by, created_at, updated_at) VALUES (?, 0, ?, ?, ?)").bind(id, a.user.pubkey, now, now),
              env.DB.prepare("INSERT INTO conversation_members (conversation_id, user_pubkey, role, joined_at) VALUES (?, ?, 'owner', ?)").bind(id, a.user.pubkey, now),
              env.DB.prepare("INSERT INTO conversation_members (conversation_id, user_pubkey, role, joined_at) VALUES (?, ?, 'member', ?)").bind(id, target, now),
            ]);
            return json({ success: true, conversation_id: id, is_existing: false }, 201);
          } else {
            const id = genId("grp");
            const allMembers = Array.from(new Set([a.user.pubkey, ...members.map((m) => m.toLowerCase())]));
            const batch = [
              env.DB.prepare("INSERT INTO conversations (id, is_group, name, created_by, created_at, updated_at) VALUES (?, 1, ?, ?, ?, ?)").bind(
                id,
                gname || "Group Chat",
                a.user.pubkey,
                now,
                now
              ),
            ];
            for (const m of allMembers) {
              batch.push(
                env.DB.prepare("INSERT INTO conversation_members (conversation_id, user_pubkey, role, joined_at) VALUES (?, ?, ?, ?)").bind(
                  id,
                  m,
                  m === a.user.pubkey ? "owner" : "member",
                  now
                )
              );
            }
            await env.DB.batch(batch);
            return json({ success: true, conversation_id: id }, 201);
          }
        }
      }

      const dmMsgMatch = path.match(/^\/api\/community\/dms\/([^\/]+)\/messages$/);
      if (dmMsgMatch) {
        const convId = dmMsgMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const isMem = await env.DB.prepare("SELECT 1 FROM conversation_members WHERE conversation_id=? AND user_pubkey=?").bind(convId, a.user.pubkey).first();
        if (!isMem) return errorJson("Forbidden", 403);

        if (method === "GET") {
          const limit = Math.min(parseInt(url.searchParams.get("limit") || "50", 10), 100);
          await env.DB.prepare("DELETE FROM messages WHERE created_at < ?").bind(nowSec() - 3888000).run();
          const msgs = await env.DB.prepare(
            `SELECT m.*, u.username, u.display_name, u.avatar_url, u.accent
             FROM messages m JOIN users u ON m.sender_pubkey = u.pubkey
             WHERE m.conversation_id = ? AND m.is_deleted = 0
             ORDER BY m.created_at DESC LIMIT ?`
          )
            .bind(convId, limit)
            .all();

          const ordered = msgs.results.reverse();
          if (ordered.length > 0) {
            await env.DB.prepare("UPDATE conversation_members SET last_read_message_id = ? WHERE conversation_id=? AND user_pubkey=?")
              .bind(ordered[ordered.length - 1].id, convId, a.user.pubkey)
              .run();
          }
          return json({ success: true, messages: ordered });
        }

        if (method === "POST") {
          const { content = "", attachments = [], reply_to_id = null } = body;
          if (!content.trim() && attachments.length === 0) return errorJson("Content required", 400);

          const id = genId("msg");
          const now = nowSec();
          await env.DB.batch([
            env.DB.prepare(
              `INSERT INTO messages (id, conversation_id, sender_pubkey, content, attachments_json, reply_to_id, is_edited, is_deleted, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, 0, 0, ?, ?)`
            ).bind(id, convId, a.user.pubkey, content.trim(), JSON.stringify(attachments), reply_to_id, now, now),
            env.DB.prepare("UPDATE conversations SET updated_at=? WHERE id=?").bind(now, convId),
          ]);

          const others = await env.DB.prepare("SELECT user_pubkey FROM conversation_members WHERE conversation_id=? AND user_pubkey != ?")
            .bind(convId, a.user.pubkey)
            .all();
          for (const o of others.results) {
            await notify(env.DB, o.user_pubkey, a.user.pubkey, "dm", "conversation", convId, `${a.user.display_name}: ${content.slice(0, 40)}`);
          }

          const msg = await env.DB.prepare(
            "SELECT m.*, u.username, u.display_name, u.avatar_url, u.accent FROM messages m JOIN users u ON m.sender_pubkey = u.pubkey WHERE m.id = ?"
          )
            .bind(id)
            .first();
          return json({ success: true, message: msg }, 201);
        }
      }

      if (path === "/api/community/guilds") {
        const a = await authenticate(request, env, rawBody);
        if (method === "GET") {
          const joined = url.searchParams.get("joined") === "true";
          if (joined && a && a.user) {
            const list = await env.DB.prepare(
              `SELECT g.*, gm.role AS my_role, (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) AS member_count
               FROM guilds g JOIN guild_members gm ON g.id = gm.guild_id WHERE gm.user_pubkey = ? ORDER BY g.updated_at DESC`
            )
              .bind(a.user.pubkey)
              .all();
            return json({ success: true, guilds: list.results });
          } else {
            const list = await env.DB.prepare(
              `SELECT g.*, (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) AS member_count
               FROM guilds g WHERE g.is_private = 0 ORDER BY member_count DESC LIMIT 50`
            ).all();
            return json({ success: true, guilds: list.results });
          }
        }

        if (method === "POST") {
          if (!a || !a.user) return errorJson("Unauthorized", 401);
          const { name: gname, description = "", icon_url = "", banner_url = "", is_private = false } = body;
          if (!gname || !gname.trim()) return errorJson("Guild name required", 400);

          const gid = genId("guild");
          const cid = genId("chan");
          const now = nowSec();

          await env.DB.batch([
            env.DB.prepare(
              `INSERT INTO guilds (id, name, description, icon_url, banner_url, owner_pubkey, is_private, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`
            ).bind(gid, gname.trim(), description.trim(), icon_url, banner_url, a.user.pubkey, is_private ? 1 : 0, now, now),
            env.DB.prepare(
              "INSERT INTO guild_channels (id, guild_id, name, topic, kind, position, is_private, created_at) VALUES (?, ?, 'general', 'General discussion', 'text', 0, 0, ?)"
            ).bind(cid, gid, now),
            env.DB.prepare("INSERT INTO guild_members (guild_id, user_pubkey, role, nickname, joined_at) VALUES (?, ?, 'owner', '', ?)").bind(
              gid,
              a.user.pubkey,
              now
            ),
          ]);

          const g = await env.DB.prepare("SELECT * FROM guilds WHERE id = ?").bind(gid).first();
          const c = await env.DB.prepare("SELECT * FROM guild_channels WHERE id = ?").bind(cid).first();
          return json({ success: true, guild: g, general_channel: c }, 201);
        }
      }

      const guildMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)$/);
      if (guildMatch && method === "GET") {
        const gid = guildMatch[1];
        const a = await authenticate(request, env, rawBody);
        const g = await env.DB.prepare("SELECT * FROM guilds WHERE id = ?").bind(gid).first();
        if (!g) return errorJson("Guild not found", 404);

        const channels = await env.DB.prepare("SELECT * FROM guild_channels WHERE guild_id = ? ORDER BY position ASC, created_at ASC").bind(gid).all();
        const memberCount = (await env.DB.prepare("SELECT COUNT(*) as c FROM guild_members WHERE guild_id = ?").bind(gid).first("c")) || 0;

        let my_role = null;
        if (a && a.user) {
          const m = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id = ? AND user_pubkey = ?").bind(gid, a.user.pubkey).first();
          if (m) my_role = m.role;
        }

        return json({ success: true, guild: { ...g, member_count: memberCount, my_role }, channels: channels.results });
      }

      const guildChanMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)\/channels$/);
      if (guildChanMatch) {
        const gid = guildChanMatch[1];
        if (method === "GET") {
          const chs = await env.DB.prepare("SELECT * FROM guild_channels WHERE guild_id = ? ORDER BY position ASC").bind(gid).all();
          return json({ success: true, channels: chs.results });
        }
        if (method === "POST") {
          const a = await authenticate(request, env, rawBody);
          if (!a || !a.user) return errorJson("Unauthorized", 401);
          const mem = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, a.user.pubkey).first();
          if (!mem || !["owner", "admin", "mod"].includes(mem.role)) return errorJson("Forbidden", 403);

          const { name: cname, topic = "", kind = "text", position = 0, is_private = false } = body;
          if (!cname || !cname.trim()) return errorJson("Channel name required", 400);

          const cid = genId("chan");
          await env.DB.prepare(
            "INSERT INTO guild_channels (id, guild_id, name, topic, kind, position, is_private, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
          )
            .bind(cid, gid, cname.trim().toLowerCase(), topic.trim(), kind, position, is_private ? 1 : 0, nowSec())
            .run();

          const created = await env.DB.prepare("SELECT * FROM guild_channels WHERE id = ?").bind(cid).first();
          return json({ success: true, channel: created }, 201);
        }
      }

      const chanMsgMatch = path.match(/^\/api\/community\/channels\/([^\/]+)\/messages$/);
      if (chanMsgMatch) {
        const cid = chanMsgMatch[1];
        const ch = await env.DB.prepare("SELECT guild_id FROM guild_channels WHERE id = ?").bind(cid).first();
        if (!ch) return errorJson("Channel not found", 404);

        if (method === "GET") {
          const limit = Math.min(parseInt(url.searchParams.get("limit") || "50", 10), 100);
          await env.DB.prepare("DELETE FROM guild_messages WHERE created_at < ?").bind(nowSec() - 3888000).run();
          const msgs = await env.DB.prepare(
            `SELECT gm.*, u.username, u.display_name, u.avatar_url, u.accent, mem.role as member_role
             FROM guild_messages gm JOIN users u ON gm.sender_pubkey = u.pubkey
             LEFT JOIN guild_members mem ON mem.guild_id = gm.guild_id AND mem.user_pubkey = gm.sender_pubkey
             WHERE gm.channel_id = ? AND gm.is_deleted = 0 ORDER BY gm.created_at DESC LIMIT ?`
          )
            .bind(cid, limit)
            .all();
          return json({ success: true, messages: msgs.results.reverse() });
        }

        if (method === "POST") {
          const a = await authenticate(request, env, rawBody);
          if (!a || !a.user) return errorJson("Unauthorized", 401);

          const ban = await env.DB.prepare("SELECT 1 FROM guild_bans WHERE guild_id=? AND user_pubkey=?").bind(ch.guild_id, a.user.pubkey).first();
          if (ban) return errorJson("Banned from this guild", 403);

          let mem = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(ch.guild_id, a.user.pubkey).first();
          if (!mem) {
            const g = await env.DB.prepare("SELECT is_private FROM guilds WHERE id=?").bind(ch.guild_id).first();
            if (g && g.is_private === 0) {
              await env.DB.prepare("INSERT INTO guild_members (guild_id, user_pubkey, role, joined_at) VALUES (?, ?, 'member', ?)")
                .bind(ch.guild_id, a.user.pubkey, nowSec())
                .run();
            } else {
              return errorJson("Must be a guild member to post", 403);
            }
          }

          const { content = "", attachments = [], reply_to_id = null } = body;
          if (!content.trim() && attachments.length === 0) return errorJson("Content required", 400);

          const mid = genId("gmsg");
          const now = nowSec();
          await env.DB.prepare(
            `INSERT INTO guild_messages (id, channel_id, guild_id, sender_pubkey, content, attachments_json, reply_to_id, is_edited, is_deleted, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, 0, ?, ?)`
          )
            .bind(mid, cid, ch.guild_id, a.user.pubkey, content.trim(), JSON.stringify(attachments), reply_to_id, now, now)
            .run();

          const created = await env.DB.prepare(
            "SELECT gm.*, u.username, u.display_name, u.avatar_url, u.accent FROM guild_messages gm JOIN users u ON gm.sender_pubkey = u.pubkey WHERE gm.id = ?"
          )
            .bind(mid)
            .first();
          return json({ success: true, message: created }, 201);
        }
      }

      const guildJoinMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)\/join$/);
      if (guildJoinMatch && method === "POST") {
        const gid = guildJoinMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const ban = await env.DB.prepare("SELECT 1 FROM guild_bans WHERE guild_id=? AND user_pubkey=?").bind(gid, a.user.pubkey).first();
        if (ban) return errorJson("Banned from this guild", 403);

        const g = await env.DB.prepare("SELECT is_private FROM guilds WHERE id=?").bind(gid).first();
        if (!g) return errorJson("Guild not found", 404);

        if (g.is_private === 1) {
          const { invite_code } = body;
          if (!invite_code) return errorJson("Invite code required", 400);
          const inv = await env.DB.prepare("SELECT * FROM guild_invites WHERE code=? AND guild_id=?").bind(invite_code, gid).first();
          if (!inv) return errorJson("Invalid invite", 404);
          if (inv.expires_at > 0 && inv.expires_at < nowSec()) return errorJson("Invite expired", 410);
          if (inv.max_uses > 0 && inv.uses >= inv.max_uses) return errorJson("Invite full", 410);
          await env.DB.prepare("UPDATE guild_invites SET uses = uses + 1 WHERE code=?").bind(invite_code).run();
        }

        await env.DB.prepare(
          "INSERT INTO guild_members (guild_id, user_pubkey, role, nickname, joined_at) VALUES (?, ?, 'member', '', ?) ON CONFLICT(guild_id, user_pubkey) DO NOTHING"
        )
          .bind(gid, a.user.pubkey, nowSec())
          .run();
        return json({ success: true, guild_id: gid });
      }

      const guildInvMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)\/invites$/);
      if (guildInvMatch && method === "POST") {
        const gid = guildInvMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const mem = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, a.user.pubkey).first();
        if (!mem || !["owner", "admin", "mod"].includes(mem.role)) return errorJson("Forbidden", 403);

        const { max_uses = 0, expires_in_seconds = 0 } = body;
        const code = crypto.randomUUID().replace(/-/g, "").slice(0, 10);
        const exp = expires_in_seconds > 0 ? nowSec() + expires_in_seconds : 0;

        await env.DB.prepare(
          "INSERT INTO guild_invites (code, guild_id, created_by, max_uses, uses, expires_at, created_at) VALUES (?, ?, ?, ?, 0, ?, ?)"
        )
          .bind(code, gid, a.user.pubkey, max_uses, exp, nowSec())
          .run();
        return json({ success: true, invite_code: code, expires_at: exp }, 201);
      }

      const guildKickMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)\/kick$/);
      if (guildKickMatch && method === "POST") {
        const gid = guildKickMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const caller = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, a.user.pubkey).first();
        if (!caller || !["owner", "admin", "mod"].includes(caller.role)) return errorJson("Forbidden", 403);

        const { target_pubkey, reason = "" } = body;
        if (!target_pubkey) return errorJson("target_pubkey required", 400);
        const target = target_pubkey.toLowerCase();

        await env.DB.batch([
          env.DB.prepare("DELETE FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, target),
          env.DB.prepare("INSERT INTO moderation_logs (id, guild_id, actor_pubkey, action, target_pubkey, details, created_at) VALUES (?, ?, ?, 'kick', ?, ?, ?)").bind(
            genId("mod"),
            gid,
            a.user.pubkey,
            target,
            reason,
            nowSec()
          ),
        ]);
        return json({ success: true, kicked: target });
      }

      const guildBanMatch = path.match(/^\/api\/community\/guilds\/([^\/]+)\/ban$/);
      if (guildBanMatch && method === "POST") {
        const gid = guildBanMatch[1];
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const caller = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, a.user.pubkey).first();
        if (!caller || !["owner", "admin"].includes(caller.role)) return errorJson("Forbidden", 403);

        const { target_pubkey, reason = "" } = body;
        if (!target_pubkey) return errorJson("target_pubkey required", 400);
        const target = target_pubkey.toLowerCase();
        const now = nowSec();

        await env.DB.batch([
          env.DB.prepare("DELETE FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(gid, target),
          env.DB.prepare(
            "INSERT INTO guild_bans (guild_id, user_pubkey, banned_by, reason, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(guild_id, user_pubkey) DO UPDATE SET reason=excluded.reason"
          ).bind(gid, target, a.user.pubkey, reason, now),
          env.DB.prepare("INSERT INTO moderation_logs (id, guild_id, actor_pubkey, action, target_pubkey, details, created_at) VALUES (?, ?, ?, 'ban', ?, ?, ?)").bind(
            genId("mod"),
            gid,
            a.user.pubkey,
            target,
            reason,
            now
          ),
        ]);
        return json({ success: true, banned: target });
      }

      if (path === "/api/community/moderate" && method === "POST") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);
        const { target_pubkey, action_type, reason, duration_secs, guild_id } = body;
        if (!target_pubkey || !action_type) return errorJson("Missing required fields", 400);

        const target = target_pubkey.toLowerCase();
        const mid = genId("mod");
        const now = nowSec();

        if (guild_id) {
          const caller = await env.DB.prepare("SELECT role FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(guild_id, a.user.pubkey).first();
          if (!caller || !["owner", "admin", "mod"].includes(caller.role)) return errorJson("Forbidden", 403);

          if (action_type === "ban") {
            await env.DB.prepare(
              "INSERT INTO guild_bans (guild_id, user_pubkey, banned_by, reason, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT(guild_id, user_pubkey) DO UPDATE SET reason=excluded.reason"
            ).bind(guild_id, target, a.user.pubkey, reason || "", now).run();
            await env.DB.prepare("DELETE FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(guild_id, target).run();
          } else if (action_type === "kick") {
            await env.DB.prepare("DELETE FROM guild_members WHERE guild_id=? AND user_pubkey=?").bind(guild_id, target).run();
          }
        } else {
          if (!a.user.is_admin) return errorJson("Admin required for global action", 403);
          if (action_type === "ban") {
            await env.KV.put(`banned:${target}`, JSON.stringify({ banned_by: a.user.pubkey, reason, at: now }));
          }
        }

        await env.DB.prepare(
          "INSERT INTO moderation_logs (id, guild_id, actor_pubkey, action, target_pubkey, details, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
        ).bind(mid, guild_id || null, a.user.pubkey, action_type, target, reason || "", now).run();

        return json({
          id: mid,
          target_pubkey: target,
          action_type: action_type,
          moderator_pubkey: a.user.pubkey,
          reason: reason || "",
          created_at: now,
          duration_secs: duration_secs || null,
          guild_id: guild_id || null
        }, 201);
      }

      if (path === "/api/community/reports" && method === "POST") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);
        const { reported_pubkey, target_type, target_id, reason } = body;
        if (!reported_pubkey || !target_type || !target_id || !reason) return errorJson("Missing required fields", 400);

        const rid = genId("rep");
        await env.DB.prepare(
          "INSERT INTO moderation_reports (id, reporter_pubkey, reported_pubkey, target_type, target_id, reason, status, created_at) VALUES (?, ?, ?, ?, ?, ?, 'pending', ?)"
        )
          .bind(rid, a.user.pubkey, reported_pubkey.toLowerCase(), target_type, target_id, reason.trim(), nowSec())
          .run();
        return json({ success: true, report_id: rid }, 201);
      }

      if (path === "/api/community/reports" && method === "GET") {
        const { results } = await env.DB.prepare(
          "SELECT id, reporter_pubkey, target_type, target_id, reason, status, handled_by, created_at FROM moderation_reports ORDER BY created_at DESC LIMIT 50"
        ).all();
        const mapped = (results || []).map(r => ({
          id: r.id,
          reporter_pubkey: r.reporter_pubkey,
          target_type: r.target_type === "guild_message" ? "guild_message" : (r.target_type === "direct_message" ? "direct_message" : (r.target_type === "user" ? "user" : "post")),
          target_id: r.target_id,
          reason: r.reason,
          details: null,
          created_at: Number(r.created_at),
          status: r.status === "resolved" ? "resolved" : (r.status === "dismissed" ? "dismissed" : "pending"),
          resolved_by: r.handled_by || null,
        }));
        return json(mapped);
      }

      if ((path === "/api/community/mod/logs" || path === "/api/community/audit_log" || path === "/api/community/audit-log") && method === "GET") {
        const gid = url.searchParams.get("guild_id");
        let query = "SELECT id, actor_pubkey, action, target_pubkey, details, created_at, guild_id FROM moderation_logs";
        let params = [];
        if (gid) {
          query += " WHERE guild_id=? ORDER BY created_at DESC LIMIT 50";
          params.push(gid);
        } else {
          query += " ORDER BY created_at DESC LIMIT 50";
        }
        const { results } = await env.DB.prepare(query).bind(...params).all();
        const mapped = (results || []).map(l => ({
          id: l.id,
          actor_pubkey: l.actor_pubkey,
          action: l.action,
          target_type: "user",
          target_id: l.target_pubkey || "",
          changes: l.details ? { details: l.details } : {},
          created_at: Number(l.created_at),
          guild_id: l.guild_id || null,
        }));
        return json(mapped);
      }

      if (path === "/api/community/search" && method === "GET") {
        const q = url.searchParams.get("q") || "";
        if (!q.trim() || q.trim().length < 2) return json({ users: [], posts: [], guilds: [] });
        const pat = `%${q.trim()}%`;
        const res = await env.DB.batch([
          env.DB.prepare("SELECT pubkey, username, display_name, avatar_url, bio, accent FROM users WHERE username LIKE ? OR display_name LIKE ? LIMIT 15").bind(pat, pat),
          env.DB.prepare("SELECT p.*, u.username, u.display_name, u.avatar_url, u.accent FROM posts p JOIN users u ON p.author_pubkey = u.pubkey WHERE p.content LIKE ? ORDER BY p.created_at DESC LIMIT 15").bind(pat),
          env.DB.prepare("SELECT id, name, description, icon_url, banner_url, (SELECT COUNT(*) FROM guild_members WHERE guild_id=guilds.id) as member_count FROM guilds WHERE is_private=0 AND name LIKE ? LIMIT 15").bind(pat),
        ]);
        return json({ success: true, users: res[0].results, posts: res[1].results, guilds: res[2].results });
      }

      if (path === "/api/community/notifications" && method === "GET") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const list = await env.DB.prepare(
          `SELECT n.*, u.username as actor_username, u.display_name as actor_display_name, u.avatar_url as actor_avatar_url, u.accent as actor_accent
           FROM notifications n JOIN users u ON n.actor_pubkey = u.pubkey
           WHERE n.recipient_pubkey = ? ORDER BY n.created_at DESC LIMIT 30`
        )
          .bind(a.user.pubkey)
          .all();

        const unread = (await env.DB.prepare("SELECT COUNT(*) as c FROM notifications WHERE recipient_pubkey=? AND is_read=0").bind(a.user.pubkey).first("c")) || 0;
        return json({ success: true, notifications: list.results, unread_count: unread });
      }

      if (path === "/api/community/notifications/read" && method === "POST") {
        const a = await authenticate(request, env, rawBody);
        if (!a || !a.user) return errorJson("Unauthorized", 401);

        const { notification_ids = [] } = body;
        if (notification_ids.length > 0) {
          const ph = notification_ids.map(() => "?").join(",");
          await env.DB.prepare(`UPDATE notifications SET is_read=1 WHERE recipient_pubkey=? AND id IN (${ph})`).bind(a.user.pubkey, ...notification_ids).run();
        } else {
          await env.DB.prepare("UPDATE notifications SET is_read=1 WHERE recipient_pubkey=?").bind(a.user.pubkey).run();
        }
        return json({ success: true });
      }

      if (path === "/api/community/verify-status" && method === "POST") {
        const { pubkey, roblox_id, verified, signature } = body;
        if (!pubkey || !roblox_id || !signature) return errorJson("Missing required fields", 400);

        const msgBytes = new TextEncoder().encode(`puppy-verify-status\0${pubkey}\0${roblox_id}\0${verified ? "1" : "0"}`);
        const isValid = await verifyEd25519(signature.pubkey, msgBytes, signature.sig);
        if (!isValid) return errorJson("Invalid signature", 401);

        const user = await env.DB.prepare("SELECT pubkey FROM users WHERE pubkey = ?").bind(pubkey.toLowerCase()).first();
        if (!user) return errorJson("User not found", 404);

        await env.DB.prepare("UPDATE users SET roblox_verified = ?, roblox_id = ? WHERE pubkey = ?").bind(verified ? 1 : 0, roblox_id, pubkey.toLowerCase()).run();
        return json({ success: true });
      }

      return errorJson(`Not found: ${method} ${path}`, 404);
    } catch (e) {
      return errorJson(`Server error: ${e.message}`, 500);
    }
  },
};
