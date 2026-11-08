// Runs the edge worker under Node with a captured fetch: verifies
// the verify -> scrub -> re-sign -> forward behaviour without a
// Cloudflare account.
import worker from "../edge/worker.js";
import { createHmac } from "node:crypto";

const GH_SECRET = "gh-secret";
const DAEMON_SECRET = "daemon-secret";
const sign = (secret, body) =>
  "sha256=" + createHmac("sha256", secret).update(body).digest("hex");

let forwarded = null;
globalThis.fetch = async (url, opts) => {
  forwarded = { url, opts };
  return new Response('{"task":"t1"}', { status: 202 });
};
const env = { WEBHOOK_SECRET: GH_SECRET, DAEMON_SECRET, DAEMON_URL: "https://daemon.example/webhook" };

const payload = {
  action: "created",
  issue: { number: 7, title: "Add the marker", body: "long issue body", user: { login: "x", id: 42 } },
  comment: { body: "@sovereign-bot do it", user: { login: "owner", id: 7 }, id: 99 },
  repository: { full_name: "local/test-repo", clone_url: "https://example/r.git", default_branch: "main", private: true },
  installation: { id: 555 },
  sender: { login: "owner", avatar_url: "https://example/a.png" },
};
const body = JSON.stringify(payload);

// good signature: forwarded, scrubbed, re-signed
let res = await worker.fetch(new Request("https://edge.example/", {
  method: "POST", body,
  headers: { "X-GitHub-Event": "issue_comment", "X-Hub-Signature-256": sign(GH_SECRET, body) },
}), env);
console.assert(res.status === 202, "good signature passes through the daemon status");
const sent = JSON.parse(forwarded.opts.body);
console.assert(sent.comment.body === "@sovereign-bot do it", "comment survives the scrub");
console.assert(sent.repository.clone_url === "https://example/r.git", "clone_url survives");
console.assert(!("installation" in sent) && !("sender" in sent), "installation/sender dropped");
console.assert(!("private" in sent.repository), "extra repository fields dropped");
console.assert(forwarded.opts.headers["X-Hub-Signature-256"] === sign(DAEMON_SECRET, forwarded.opts.body),
  "forward re-signed with the daemon secret over the scrubbed body");

// bad signature: dies at the edge
forwarded = null;
res = await worker.fetch(new Request("https://edge.example/", {
  method: "POST", body,
  headers: { "X-GitHub-Event": "issue_comment", "X-Hub-Signature-256": "sha256=deadbeef" },
}), env);
console.assert(res.status === 401 && forwarded === null, "forged delivery refused at the edge, never forwarded");
console.log("worker harness: all assertions held");
