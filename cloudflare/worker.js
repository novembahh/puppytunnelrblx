export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    const method = request.method;

    const corsHeaders = {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Methods": "GET, HEAD, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Authorization",
      "Access-Control-Max-Age": "86400",
    };

    if (method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    function json(data, status = 200, headers = {}) {
      return new Response(JSON.stringify(data, null, 2), {
        status,
        headers: Object.assign({
          "Content-Type": "application/json; charset=utf-8",
        }, corsHeaders, headers),
      });
    }

    function compareSemver(a, b) {
      if (!a && !b) return 0;
      if (!a) return -1;
      if (!b) return 1;
      if (a === b) return 0;
      const [vA, preA] = a.split('-');
      const [vB, preB] = b.split('-');
      const partsA = vA.split('.').map(x => parseInt(x, 10) || 0);
      const partsB = vB.split('.').map(x => parseInt(x, 10) || 0);
      for (let i = 0; i < Math.max(partsA.length, partsB.length); i++) {
        const numA = partsA[i] || 0;
        const numB = partsB[i] || 0;
        if (numA > numB) return 1;
        if (numA < numB) return -1;
      }
      if (!preA && preB) return 1;
      if (preA && !preB) return -1;
      if (preA && preB) {
        return preA.localeCompare(preB, undefined, { numeric: true, sensitivity: 'base' });
      }
      return 0;
    }

    async function loadManifest(channel) {
      const ch = (channel || "stable").toLowerCase();
      if (!["stable", "beta", "nightly"].includes(ch)) return null;
      if (env && env.KV) {
        try {
          const val = await env.KV.get("updates:manifest:" + ch);
          if (val) return JSON.parse(val);
        } catch (e) {
          console.error("KV read error:", e);
        }
      }
      return null;
    }

    async function loadHistory() {
      if (env && env.KV) {
        try {
          const val = await env.KV.get("updates:history");
          if (val) return JSON.parse(val);
        } catch (e) {
          console.error("KV history read error:", e);
        }
      }
      return [];
    }

    if (path === "" || path === "/") {
      return json({
        service: "PuppyTunnel Update Service",
        status: "healthy",
        domain: "puppytunnel.vip",
        endpoints: {
          check: "/api/updates/check?channel=stable|beta|nightly&version=<curr>&os=windows|linux|macos&arch=x86_64|aarch64",
          manifest: "/api/updates/manifest?channel=stable|beta|nightly",
          history: "/api/updates/history",
          download: "/api/updates/download/:channel/:filename",
          health: "/health"
        },
        channels: ["stable", "beta", "nightly"]
      });
    }

    if (path === "/health") {
      return json({
        status: "ok",
        service: "puppytunnel-updates",
        kv_bound: !!(env && env.KV),
        timestamp: new Date().toISOString()
      });
    }

    if (path === "/api/updates/check") {
      if (method !== "GET" && method !== "HEAD") return json({ error: "Method not allowed" }, 405);
      const channel = (url.searchParams.get("channel") || "stable").toLowerCase();
      const currVersion = url.searchParams.get("version") || "";
      const os = (url.searchParams.get("os") || "windows").toLowerCase();
      const arch = (url.searchParams.get("arch") || "x86_64").toLowerCase();

      const manifest = await loadManifest(channel);
      if (!manifest) {
        return json({ error: "Invalid channel '" + channel + "'. Supported: stable, beta, nightly" }, 400);
      }

      const latestVersion = manifest.version;
      const isNewer = !currVersion || compareSemver(latestVersion, currVersion) > 0;
      const platformKey = os + "-" + arch;
      const assetList = Array.isArray(manifest.assets) ? manifest.assets : (manifest.assets ? Object.values(manifest.assets) : []);
      const asset = (manifest.assets_map && manifest.assets_map[platformKey])
        || assetList.find(a => (a.os === os && a.arch === arch) || a.platform === platformKey)
        || assetList.find(a => a.platform === "windows-x86_64" || (a.os === "windows" && a.arch === "x86_64"))
        || assetList[0];

      if (isNewer) {
        return json({
          update_available: true,
          channel: manifest.channel,
          current_version: currVersion || "0.0.0",
          latest_version: manifest.version,
          release_date: manifest.release_date,
          changelog: manifest.changelog,
          pubkey: manifest.pubkey,
          signature: manifest.signature,
          download_url: asset ? asset.url : null,
          filename: asset ? asset.filename : null,
          sha256: asset ? asset.sha256 : null,
          ed25519_signature: asset ? asset.signature : null,
          size_bytes: asset ? asset.size_bytes : null,
          min_os_version: manifest.min_os_version,
          mandatory: false
        });
      } else {
        return json({
          update_available: false,
          channel: manifest.channel,
          current_version: currVersion,
          latest_version: manifest.version,
          release_date: manifest.release_date
        });
      }
    }

    if (path === "/api/updates/manifest") {
      if (method !== "GET" && method !== "HEAD") return json({ error: "Method not allowed" }, 405);
      const chParam = url.searchParams.get("channel");
      if (chParam) {
        const manifest = await loadManifest(chParam);
        if (!manifest) {
          return json({ error: "Invalid channel '" + chParam + "'. Supported: stable, beta, nightly" }, 400);
        }
        return json(manifest);
      }

      const stable = await loadManifest("stable");
      const beta = await loadManifest("beta");
      const nightly = await loadManifest("nightly");
      return json({
        channels: {
          stable,
          beta,
          nightly
        }
      });
    }

    if (path === "/api/updates/history") {
      if (method !== "GET" && method !== "HEAD") return json({ error: "Method not allowed" }, 405);
      const history = await loadHistory();
      return json(history);
    }

    const dlMatch = path.match(/^\/api\/updates\/download\/([^/]+)\/([^/]+)$/);
    if (dlMatch) {
      const channelParam = dlMatch[1];
      const filename = dlMatch[2];
      const manifest = await loadManifest(channelParam);
      if (!manifest) {
        return json({ error: "Channel not found: " + channelParam }, 404);
      }
      const assetList = Array.isArray(manifest.assets) ? manifest.assets : (manifest.assets ? Object.values(manifest.assets) : []);
      const asset = assetList.find(a => a.filename === filename || a.name === filename);
      if (!asset) {
        return json({ error: "File not found: " + filename }, 404);
      }

      const binaryPayload = "PuppyTunnel Binary Payload " + manifest.version + " " + asset.os + " " + asset.arch + " " + manifest.channel;
      const encoded = new TextEncoder().encode(binaryPayload);
      return new Response(encoded, {
        status: 200,
        headers: Object.assign({
          "Content-Type": "application/octet-stream",
          "Content-Disposition": 'attachment; filename="' + filename + '"',
          "Content-Length": encoded.byteLength.toString(),
          "X-Checksum-Sha256": asset.sha256,
          "X-Signature-Ed25519": asset.signature,
          "ETag": '"' + asset.sha256 + '"',
          "Cache-Control": "public, max-age=86400",
        }, corsHeaders)
      });
    }

    return json({ error: "Not Found", path }, 404);
  }
};
