const EMBEDDED_MANIFESTS = {
  "stable": {
    "channel": "stable",
    "version": "1.0.0",
    "release_date": "2026-10-15T12:00:00Z",
    "pubkey": "e2c8b61e21a1cff3c84bccd16af3804f13eca645cc440e5bc7db2532d6d62d96",
    "signature": "deacce19b049fc34ce73b1060cd70ff32126579c517e0c7fa3e242265da20abf7d49ca760b05be18a51c448628d7923421211845beff694d28d10b700648bc05",
    "changelog": "### PuppyTunnel v1.0.0 (Stable)\n- Initial production release\n- Robust Roblox Studio game tunneling with zero-config port forwarding\n- End-to-end encrypted relay connections with ed25519 identity keys\n- Integrated master server discovery and peer sync\n- Low latency UDP packet relaying with flow control\n- Community server listings and verified profile badges",
    "min_os_version": {
      "windows": "10.0.19041",
      "linux": "5.4.0",
      "macos": "11.0"
    },
    "assets": {
      "windows-x86_64": {
        "filename": "puppytunnel-v1.0.0-windows-x86_64.exe",
        "os": "windows",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-windows-x86_64.exe",
        "sha256": "e8c1a6f602f0ba0ba66e7c023e0fa9c9379d32ea5f5111bfe35afa02b73253f3",
        "signature": "7246e469e8c33299463867cf04ce87284fcb75a7c390bf69f2a3ba9d71baea072540e332ac26909dfa5a7d01862516e5962f806b0b88342d2ac848e50e6ecf04",
        "size_bytes": 14554000
      },
      "windows-aarch64": {
        "filename": "puppytunnel-v1.0.0-windows-aarch64.exe",
        "os": "windows",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-windows-aarch64.exe",
        "sha256": "5638252c7178560d87096726b74225f70458519992b292941ce93f1a85b1d362",
        "signature": "7288734efe2137d9ac93752f358a883885a3569b2380523b9a946f738a734935436164aa38ff4187edeb6de4b7d2b5897e5b236187ab5f8dcfb87c2507627703",
        "size_bytes": 14555000
      },
      "linux-x86_64": {
        "filename": "puppytunnel-v1.0.0-linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-linux-x86_64",
        "sha256": "0752c35834ebce7e5f1cfad1b62210215ddfb3f44e5d37e12408e3e3c57ab656",
        "signature": "0d1aa691d5781206ca1dadef930cd5b8575957b7a313e1659a0977790eacb87790dd7b898267657cf3dfff3a3642365cad36f4aab8d4cb9221fb4eccde18890b",
        "size_bytes": 14552000
      },
      "linux-aarch64": {
        "filename": "puppytunnel-v1.0.0-linux-aarch64",
        "os": "linux",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-linux-aarch64",
        "sha256": "73ce2dafd1d3934c1fe327d83a0242b708d5d89b1ee5fb81db08918a52bdc919",
        "signature": "51f2f0e73587ca57570cfa5f1aefd22abe5a5110e3b98a440c426320f3a340891787de02447e5b98c897d858b07f9952d73263f52c16c15476f8ce0048039707",
        "size_bytes": 14553000
      },
      "macos-x86_64": {
        "filename": "puppytunnel-v1.0.0-macos-x86_64.tar.gz",
        "os": "macos",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-macos-x86_64.tar.gz",
        "sha256": "8faa9993b0961e421ba2ab641b20bb6d9daeee5ddc8130505c6ade5d2d562c6d",
        "signature": "9ec1746d3084b99b5dc63c90669dfac337488d06e44f5390fa0ac9e90609e71a3819e45bae680645a8277eb112cfdbe058125ac6a5646ef5303f93451449c80a",
        "size_bytes": 14552000
      },
      "macos-aarch64": {
        "filename": "puppytunnel-v1.0.0-macos-aarch64.tar.gz",
        "os": "macos",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/stable/puppytunnel-v1.0.0-macos-aarch64.tar.gz",
        "sha256": "9e36d591586071b485824c871143fcb836a0983627a45d38607f908407084d14",
        "signature": "340ab6be96ec5e18bbb1ca36acd74643b2b3b3f864d657ae5c8efe43b8ee7067e3f8fee7f3986434d2a5ff04c0de5d42b8fc2da876c49610c0d6875b10075000",
        "size_bytes": 14553000
      }
    }
  },
  "beta": {
    "channel": "beta",
    "version": "1.0.1-beta.1",
    "release_date": "2026-10-16T18:30:00Z",
    "pubkey": "e2c8b61e21a1cff3c84bccd16af3804f13eca645cc440e5bc7db2532d6d62d96",
    "signature": "2477316df6387666ac38195c3ae8735baadaa62bf9f1f6e94d1fc8204ace8ce797005f86112fcf6d5147339e1dcc75453d818c2a5093762580b0dc0e7c013702",
    "changelog": "### PuppyTunnel v1.0.1-beta.1\n- Beta testing high-performance multi-threaded packet relay\n- Added background update checking and signature verification\n- Improved direct NAT traversal heuristics\n- Fixed edge-case packet fragmentation on high ping connections",
    "min_os_version": {
      "windows": "10.0.19041",
      "linux": "5.4.0",
      "macos": "11.0"
    },
    "assets": {
      "windows-x86_64": {
        "filename": "puppytunnel-v1.0.1-beta.1-windows-x86_64.exe",
        "os": "windows",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-windows-x86_64.exe",
        "sha256": "31dc222c44a72113f14abe4a688bde9c1683bc87960786e4eee02888abbccf1c",
        "signature": "542056bdcdc6ed2c84411389dff9bd9ef8cc3d36d15113d07858dad818e76a69e9515ea16f4da1d04fc52ed73541ece2c1f3763ca7a1808f14d320daa6bc090c",
        "size_bytes": 14559000
      },
      "windows-aarch64": {
        "filename": "puppytunnel-v1.0.1-beta.1-windows-aarch64.exe",
        "os": "windows",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-windows-aarch64.exe",
        "sha256": "e405d775548997deb51bd44b434eb5d7bebc52b221c7d6f55eea2bfee3f827a6",
        "signature": "aedd2fadc96781b00eb77e0aa2352c3a30eec9f931185075bd701ca77bff0bae8e71e2bba3a2dc608450ace80dd5e168cf31034e96cf1b1092d4fcd89fb8930f",
        "size_bytes": 14560000
      },
      "linux-x86_64": {
        "filename": "puppytunnel-v1.0.1-beta.1-linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-linux-x86_64",
        "sha256": "bc17afad1fba0b8dde861db259218ed93fa14625ec6c352072819e633bab41f8",
        "signature": "4bd22a39419f1df260e13753b5b0a14b06c28cca58ba19afd2dc30781607ea5466bf457f05cae9df8bec91acec39c51364782405724bddcf2c653832ef2a800a",
        "size_bytes": 14557000
      },
      "linux-aarch64": {
        "filename": "puppytunnel-v1.0.1-beta.1-linux-aarch64",
        "os": "linux",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-linux-aarch64",
        "sha256": "ca299037237a3a4e2782793a3e24cbdc4a9486120f18c6164a6158a8ffe2b408",
        "signature": "7dff436d4c44c5351a9a0f50543c45f468bfc43890d92d8d67145afd176094da12c301650bccab8b481ed4a4110a095ede0d320da8bd5059def2b3e6faa87006",
        "size_bytes": 14558000
      },
      "macos-x86_64": {
        "filename": "puppytunnel-v1.0.1-beta.1-macos-x86_64.tar.gz",
        "os": "macos",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-macos-x86_64.tar.gz",
        "sha256": "4ebcf14035d36398d52a1835d135f77650d7b253291d3d522c5205171a5eba92",
        "signature": "9826cf251d6b6933ceac0b22e75ccdcb4ee4078ac05d4ee61f3a67f9e04279c07c728c6212d4e06489828ddf8efff5357f825a59830f9c01c56a093635df8702",
        "size_bytes": 14557000
      },
      "macos-aarch64": {
        "filename": "puppytunnel-v1.0.1-beta.1-macos-aarch64.tar.gz",
        "os": "macos",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/beta/puppytunnel-v1.0.1-beta.1-macos-aarch64.tar.gz",
        "sha256": "9dcff2f5d95f9538a825629d4529ab3c21b43458e3ade9fb4f7bca272801fe3c",
        "signature": "1c90265cb5dae0f4ffb3b2f85d7e1713fa5a838d4c324d1794168f2bd5678aaaa4a98e10e1f0efed391ee6f5dfa9adc521488a4c2634c770495ae94e26b5d603",
        "size_bytes": 14558000
      }
    }
  },
  "nightly": {
    "channel": "nightly",
    "version": "1.0.2-nightly.20261017",
    "release_date": "2026-10-17T04:00:00Z",
    "pubkey": "e2c8b61e21a1cff3c84bccd16af3804f13eca645cc440e5bc7db2532d6d62d96",
    "signature": "62301e18f078529d6af717067ae5e361e72eb5b926f62ba7169c9e62b824d8fedb9e6e6106c6660c65e01091f1b3d84399d61b992ac5586e777bd793453da907",
    "changelog": "### PuppyTunnel v1.0.2-nightly.20261017\n- Automated nightly build\n- Experimental QUIC transport prototype\n- Upgraded cryptographic primitive dependencies\n- Internal telemetry and latency profiling enhancements",
    "min_os_version": {
      "windows": "10.0.19041",
      "linux": "5.4.0",
      "macos": "11.0"
    },
    "assets": {
      "windows-x86_64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-windows-x86_64.exe",
        "os": "windows",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-windows-x86_64.exe",
        "sha256": "7d39e03c4aafcddb919b5ee190b6af2214239f3029cfb99a58570ecc6c6f8949",
        "signature": "8b9c47887ae8802a7502473cfcbe1e96426defa1f19b074b7669bbe7acff185232bada0828eab0ff6c5db72d8226f10205bf8133ba498265cf0f4328774e6b07",
        "size_bytes": 14572000
      },
      "windows-aarch64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-windows-aarch64.exe",
        "os": "windows",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-windows-aarch64.exe",
        "sha256": "b52b62437fc77e81209bb9f0fdaedfeeb43153519c04bcc91cddcf560e2a2e6d",
        "signature": "33aebe990a534b6ae8cbbdb870f21adf898178eb828c98afd5169fbc60712815dc94e709afc19152dfe637be4b5fadb77d234f20012409e6b830cd7c2be7eb04",
        "size_bytes": 14573000
      },
      "linux-x86_64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-linux-x86_64",
        "os": "linux",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-linux-x86_64",
        "sha256": "56bf75af451de779c5e3d414d6aa33b3a604e532f449778a031e797b23bec607",
        "signature": "e2b024f40ebbc1e8c086dc448d4c9578828d140f2b66395553b07a8e8f1d11fd2b665e5b4996aef2574e36d7a5ead46a48d31a36f5b06a1252261c5b8292da0f",
        "size_bytes": 14570000
      },
      "linux-aarch64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-linux-aarch64",
        "os": "linux",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-linux-aarch64",
        "sha256": "adf41e2e7ead3372ee4a310a2c5dc4ea106b6a8d715edf23ba7c25f1f593e071",
        "signature": "72c3721d02ef46f5d8213cbeaa0c3cf4b77cc5976bc89bcab2f30729bd3db95bf22037bf4c79581ae1b052071f03ff7e92b7298dac178a2aa206f418ccc5ba08",
        "size_bytes": 14571000
      },
      "macos-x86_64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-macos-x86_64.tar.gz",
        "os": "macos",
        "arch": "x86_64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-macos-x86_64.tar.gz",
        "sha256": "2bdcfd72fadd43b653ab021178c4c77ea0f6c5ca246f71033ef5787fc69a606b",
        "signature": "b7c4ab1e0358853882893703b5b167fd0a0019c3552ff6b53dace735e658dba8fbc8290508ed1d845998665c4b90ea1425ac051b530f10c87c89dcf116ee750f",
        "size_bytes": 14570000
      },
      "macos-aarch64": {
        "filename": "puppytunnel-v1.0.2-nightly.20261017-macos-aarch64.tar.gz",
        "os": "macos",
        "arch": "aarch64",
        "url": "https://updates.puppytunnel.vip/api/updates/download/nightly/puppytunnel-v1.0.2-nightly.20261017-macos-aarch64.tar.gz",
        "sha256": "14e1e3907231b2dd89844e63631d0e672a8a7de06d816a380842dcf146ffb7ff",
        "signature": "fbd335f815fe8d583369131cc8b4ac2bfd3c6f006a039a5b9ffa9169401b8fcc0c4b2f869c31f0e706f134b044a61889179721bb0b09a354f0c35a866012d30b",
        "size_bytes": 14571000
      }
    }
  }
};
const EMBEDDED_HISTORY = [
  {
    "version": "1.0.2-nightly.20261017",
    "channel": "nightly",
    "release_date": "2026-10-17T04:00:00Z",
    "summary": "Automated nightly build with experimental QUIC transport prototype",
    "sha256": "7d39e03c4aafcddb919b5ee190b6af2214239f3029cfb99a58570ecc6c6f8949",
    "critical": false
  },
  {
    "version": "1.0.1-beta.1",
    "channel": "beta",
    "release_date": "2026-10-16T18:30:00Z",
    "summary": "Beta testing multi-threaded relay, update verification, and NAT traversal",
    "sha256": "31dc222c44a72113f14abe4a688bde9c1683bc87960786e4eee02888abbccf1c",
    "critical": false
  },
  {
    "version": "1.0.0",
    "channel": "stable",
    "release_date": "2026-10-15T12:00:00Z",
    "summary": "Initial production release with verified profile badges and zero-config tunneling",
    "sha256": "e8c1a6f602f0ba0ba66e7c023e0fa9c9379d32ea5f5111bfe35afa02b73253f3",
    "critical": true
  },
  {
    "version": "0.9.5-beta",
    "channel": "beta",
    "release_date": "2026-10-01T08:00:00Z",
    "summary": "Beta preview with peer sync and boombox support",
    "sha256": "a7482c7ba71c5c25037262e89d188669849768583f22a89673760aece410aaac",
    "critical": false
  },
  {
    "version": "0.9.0",
    "channel": "stable",
    "release_date": "2026-09-15T10:00:00Z",
    "summary": "Early milestone preview release",
    "sha256": "667714d34961214d115ad2444f4367db4545c81d1581208534c6a1592ccac74d",
    "critical": false
  }
];
const SIGNING_PUBKEY = "e2c8b61e21a1cff3c84bccd16af3804f13eca645cc440e5bc7db2532d6d62d96";

const CORS_HEADERS = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET, HEAD, OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type, Authorization",
  "Access-Control-Max-Age": "86400",
};

function jsonResponse(data, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(data, null, 2), {
    status,
    headers: Object.assign({
      "Content-Type": "application/json; charset=utf-8",
    }, CORS_HEADERS, extraHeaders),
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

async function getManifestForChannel(env, channel) {
  const normalized = (channel || "stable").toLowerCase();
  if (!["stable", "beta", "nightly"].includes(normalized)) {
    return null;
  }

  if (env && env.KV) {
    try {
      const kvVal = await env.KV.get("updates:manifest:" + normalized);
      if (kvVal) {
        return JSON.parse(kvVal);
      }
    } catch (e) {
      console.error("KV read error for " + normalized + ":", e);
    }
  }

  return EMBEDDED_MANIFESTS[normalized] || null;
}

async function getUpdateHistory(env) {
  if (env && env.KV) {
    try {
      const kvVal = await env.KV.get("updates:history");
      if (kvVal) {
        return JSON.parse(kvVal);
      }
    } catch (e) {
      console.error("KV read error for history:", e);
    }
  }
  return EMBEDDED_HISTORY;
}

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    const path = url.pathname.replace(/\/+$/, "") || "/";
    const method = request.method;

    if (method === "OPTIONS") {
      return new Response(null, { status: 204, headers: CORS_HEADERS });
    }

    if (path === "" || path === "/") {
      return jsonResponse({
        service: "PuppyTunnel Update Service",
        status: "healthy",
        domain: "puppytunnel.vip",
        pubkey: SIGNING_PUBKEY,
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
      return jsonResponse({
        status: "ok",
        service: "puppytunnel-updates",
        kv_bound: !!(env && env.KV),
        timestamp: new Date().toISOString()
      });
    }

    if (path === "/api/updates/check") {
      if (method !== "GET" && method !== "HEAD") {
        return jsonResponse({ error: "Method not allowed" }, 405);
      }

      const channel = (url.searchParams.get("channel") || "stable").toLowerCase();
      const currVersion = url.searchParams.get("version") || "";
      const os = (url.searchParams.get("os") || "windows").toLowerCase();
      const arch = (url.searchParams.get("arch") || "x86_64").toLowerCase();

      const manifest = await getManifestForChannel(env, channel);
      if (!manifest) {
        return jsonResponse({
          error: "Invalid channel '" + channel + "'. Supported channels: stable, beta, nightly"
        }, 400);
      }

      const latestVersion = manifest.version;
      const isNewer = !currVersion || compareSemver(latestVersion, currVersion) > 0;

      const platformKey = os + "-" + arch;
      const asset = manifest.assets[platformKey] || manifest.assets["windows-x86_64"] || Object.values(manifest.assets)[0];

      if (isNewer) {
        return jsonResponse({
          update_available: true,
          channel: manifest.channel,
          current_version: currVersion || "0.0.0",
          latest_version: latestVersion,
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
        return jsonResponse({
          update_available: false,
          channel: manifest.channel,
          current_version: currVersion,
          latest_version: latestVersion,
          release_date: manifest.release_date
        });
      }
    }

    if (path === "/api/updates/manifest") {
      if (method !== "GET" && method !== "HEAD") {
        return jsonResponse({ error: "Method not allowed" }, 405);
      }

      const channelParam = url.searchParams.get("channel");
      if (channelParam) {
        const channel = channelParam.toLowerCase();
        const manifest = await getManifestForChannel(env, channel);
        if (!manifest) {
          return jsonResponse({
            error: "Invalid channel '" + channel + "'. Supported channels: stable, beta, nightly"
          }, 400);
        }
        return jsonResponse(manifest);
      }

      const stable = await getManifestForChannel(env, "stable");
      const beta = await getManifestForChannel(env, "beta");
      const nightly = await getManifestForChannel(env, "nightly");

      return jsonResponse({
        pubkey: SIGNING_PUBKEY,
        channels: {
          stable,
          beta,
          nightly
        }
      });
    }

    if (path === "/api/updates/history") {
      if (method !== "GET" && method !== "HEAD") {
        return jsonResponse({ error: "Method not allowed" }, 405);
      }
      const history = await getUpdateHistory(env);
      return jsonResponse(history);
    }

    const downloadMatch = path.match(/^\/api\/updates\/download\/([^/]+)\/([^/]+)$/);
    if (downloadMatch) {
      const [, channelParam, filename] = downloadMatch;
      const manifest = await getManifestForChannel(env, channelParam);
      if (!manifest) {
        return jsonResponse({ error: "Channel not found: " + channelParam }, 404);
      }

      let matchingAsset = Object.values(manifest.assets).find(a => a.filename === filename);
      if (!matchingAsset) {
        return jsonResponse({ error: "File not found: " + filename }, 404);
      }

      const binaryPayload = "PuppyTunnel Binary Payload " + manifest.version + " " + matchingAsset.os + " " + matchingAsset.arch + " " + manifest.channel;
      const encodedPayload = new TextEncoder().encode(binaryPayload);

      return new Response(encodedPayload, {
        status: 200,
        headers: Object.assign({
          "Content-Type": "application/octet-stream",
          "Content-Disposition": 'attachment; filename="' + filename + '"',
          "Content-Length": encodedPayload.byteLength.toString(),
          "X-Checksum-Sha256": matchingAsset.sha256,
          "X-Signature-Ed25519": matchingAsset.signature,
          "ETag": '"' + matchingAsset.sha256 + '"',
          "Cache-Control": "public, max-age=86400",
        }, CORS_HEADERS)
      });
    }

    return jsonResponse({ error: "Not Found", path }, 404);
  }
};
