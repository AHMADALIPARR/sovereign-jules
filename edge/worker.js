// worker.js — the Sovereign Jules edge ingress (Cloudflare Worker).
//
// GitHub fires here, not at the daemon. The worker does three
// things at the edge:
//   1. VERIFY  GitHub's HMAC-SHA256 signature (WEBHOOK_SECRET).
//      A forged delivery dies at the edge and never travels.
//   2. SCRUB   the payload down to the fields the daemon reads —
//      action, ref, repository essentials, issue essentials,
//      comment body + author. Everything else GitHub sends
//      (installation metadata, sender objects, URLs, the long
//      tail) is dropped here, on the edge, not at the house.
//   3. FORWARD the scrubbed body to the daemon, re-signed with
//      DAEMON_SECRET, so the daemon's own HMAC check verifies
//      exactly what it receives.
//
// Secrets (wrangler secret put): WEBHOOK_SECRET, DAEMON_SECRET.
// Var: DAEMON_URL — the edge route to the local daemon
// (e.g. https://jules.example.com/webhook via the owner's tunnel).

async function hmacHex(secret, bodyText) {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw", enc.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]
  );
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(bodyText));
  return [...new Uint8Array(sig)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function scrub(payload) {
  const repo = payload.repository || {};
  const issue = payload.issue || {};
  const comment = payload.comment || {};
  const out = {
    action: payload.action || "",
    ref: payload.ref || "",
    repository: {
      full_name: repo.full_name || "",
      clone_url: repo.clone_url || "",
      default_branch: repo.default_branch || "main",
    },
  };
  if (payload.issue) {
    out.issue = { number: issue.number || 0, title: issue.title || "" };
  }
  if (payload.comment) {
    out.comment = {
      body: comment.body || "",
      user: { login: (comment.user || {}).login || "" },
    };
  }
  return out;
}

export default {
  async fetch(request, env) {
    if (request.method !== "POST") {
      return new Response("method not allowed", { status: 405 });
    }
    const bodyText = await request.text();

    // 1. verify GitHub's signature
    const given = request.headers.get("x-hub-signature-256") || "";
    const expected = "sha256=" + (await hmacHex(env.WEBHOOK_SECRET, bodyText));
    if (given !== expected) {
      return new Response("bad signature", { status: 401 });
    }

    let payload;
    try {
      payload = JSON.parse(bodyText);
    } catch {
      return new Response("bad json", { status: 400 });
    }

    // 2. scrub
    const clean = JSON.stringify(scrub(payload));
    const event = request.headers.get("x-github-event") || "";

    // 3. forward, re-signed for the daemon
    const daemonSig = "sha256=" + (await hmacHex(env.DAEMON_SECRET, clean));
    const res = await fetch(env.DAEMON_URL, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-GitHub-Event": event,
        "X-Hub-Signature-256": daemonSig,
      },
      body: clean,
    });
    return new Response(await res.text(), {
      status: res.status,
      headers: { "Content-Type": "application/json" },
    });
  },
};
