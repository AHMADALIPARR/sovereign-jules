// main.rs — Sovereign Jules: a Jules-style asynchronous coding
// orchestrator, sovereign edition. Zero external crates: std only.
//
// Shape (the Jules architecture, rebuilt on owned ground):
//   GitHub App webhook -> this daemon -> ephemeral sandbox clone ->
//   tool discovery (the Tool Discovery Engine, not context stuffing)
//   -> local/API model -> file changes -> test gate -> branch + PR.
// Every stage seals into a hash-chained run ledger; the dashboard
// renders the ledger. A stage that fails stops the line: no test
// pass, no push. Evidence or silence.
//
// Modes:
//   sovereign-jules <config>            run the daemon
//   sovereign-jules --verify-ledger <config>
//   sovereign-jules --dashboard <config>   print the dashboard HTML

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================ SHA-256 ============================
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let bitlen = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 { w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]); }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K256[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let mj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(mj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1); d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e); h[5] = h[5].wrapping_add(f); h[6] = h[6].wrapping_add(g); h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for i in 0..8 { out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{:02x}", b)).collect() }
fn sha256_hex(s: &str) -> String { hex(&sha256(s.as_bytes())) }
fn hmac_sha256_hex(key: &str, msg: &[u8]) -> String {
    let mut k = if key.len() > 64 { sha256(key.as_bytes()).to_vec() } else { key.as_bytes().to_vec() };
    k.resize(64, 0);
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut inner = ipad; inner.extend_from_slice(msg);
    let ih = sha256(&inner);
    let mut outer = opad; outer.extend_from_slice(&ih);
    hex(&sha256(&outer))
}

// ============================ JSON (targeted) ============================
// The daemon reads a handful of fields from GitHub payloads and one
// field from the model reply. These helpers extract exactly those,
// with string unescaping; they are not a general JSON parser.
fn json_unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' { out.push(c); continue; }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('u') => {
                let h: String = chars.by_ref().take(4).collect();
                if let Ok(n) = u32::from_str_radix(&h, 16) { if let Some(c) = char::from_u32(n) { out.push(c); } }
            }
            Some(other) => { out.push('\\'); out.push(other); }
            None => out.push('\\'),
        }
    }
    out
}
/// Slice the object that follows "key": in the text (balanced braces).
fn json_object<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{}\":", key);
    let start = text.find(&pat)? + pat.len();
    let rest = &text[start..];
    let open = rest.find('{')?;
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for i in open..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if esc { esc = false; } else if c == '\\' { esc = true; } else if c == '"' { in_str = false; }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => { depth -= 1; if depth == 0 { return Some(&rest[open..=i]); } }
            _ => {}
        }
    }
    None
}
fn json_str(text: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let mut from = 0;
    while let Some(pos) = text[from..].find(&pat) {
        let after = &text[from + pos + pat.len()..];
        let after = after.trim_start();
        if let Some(stripped) = after.strip_prefix('"') {
            let mut raw = String::new();
            let mut esc = false;
            for c in stripped.chars() {
                if esc { raw.push('\\'); raw.push(c); esc = false; }
                else if c == '\\' { esc = true; }
                else if c == '"' { return Some(json_unescape(&raw)); }
                else { raw.push(c); }
            }
            return None;
        }
        if after.starts_with("null") { return None; }
        from += pos + pat.len();
    }
    None
}
fn json_num(text: &str, key: &str) -> Option<i64> {
    let pat = format!("\"{}\":", key);
    let pos = text.find(&pat)?;
    let after = text[pos + pat.len()..].trim_start();
    let num: String = after.chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    num.parse().ok()
}
fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ============================ config ============================
#[derive(Clone)]
struct Config {
    port: u16,
    bot_name: String,
    workdir: String,
    ledger_path: String,
    webhook_secret: String,
    model_endpoint: String,
    model_name: String,
    model_backend: String,
    webllm_model: String,
    infer_timeout_secs: u64,
    discover_bin: String,
    discover_registry: String,
    test_cmd: String,
    git_name: String,
    git_email: String,
    gh_token_env: String,
}
impl Config {
    fn load(path: &str) -> Config {
        let mut m: HashMap<String, String> = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                if let Some((k, v)) = line.split_once('=') { m.insert(k.trim().to_string(), v.trim().to_string()); }
            }
        }
        let g = |k: &str, d: &str| m.get(k).cloned().unwrap_or_else(|| d.to_string());
        Config {
            port: g("port", "8787").parse().unwrap_or(8787),
            bot_name: g("bot_name", "sovereign-bot"),
            workdir: g("workdir", "runs"),
            ledger_path: g("ledger_path", "ledger.log"),
            webhook_secret: g("webhook_secret", ""),
            model_endpoint: g("model_endpoint", "http://127.0.0.1:11434/v1/chat/completions"),
            model_name: g("model_name", "nemotron-super"),
            model_backend: g("model_backend", "http"),
            webllm_model: g("webllm_model", "Qwen2.5-Coder-7B-Instruct-q4f16_1-MLC"),
            infer_timeout_secs: g("infer_timeout_secs", "900").parse().unwrap_or(900),
            discover_bin: g("discover_bin", ""),
            discover_registry: g("discover_registry", ""),
            test_cmd: g("test_cmd", ""),
            git_name: g("git_name", "Sovereign Jules"),
            git_email: g("git_email", "bot@localhost"),
            gh_token_env: g("gh_token_env", "GH_TOKEN"),
        }
    }
}

// ============================ ledger ============================
struct Ledger { path: String, seq: u64, tip: String }
impl Ledger {
    fn open(path: &str) -> Ledger {
        let mut seq = 0u64;
        let mut tip = "GENESIS".to_string();
        if let Ok(text) = std::fs::read_to_string(path) {
            for line in text.lines() {
                if line.is_empty() { continue; }
                seq += 1;
                if let Some(h) = line.rsplit('\t').next() { tip = h.to_string(); }
            }
        }
        Ledger { path: path.to_string(), seq, tip }
    }
    fn append(&mut self, task: &str, stage: &str, detail: &str) -> String {
        self.seq += 1;
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let clean = |s: &str| s.replace('\t', " ").replace('\n', " ");
        let body = format!("{}\t{}\t{}\t{}\t{}", self.seq, ts, clean(task), clean(stage), clean(detail));
        let hash = sha256_hex(&format!("{}|{}", self.tip, body));
        let line = format!("{}\t{}\n", body, hash);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
        }
        self.tip = hash.clone();
        hash
    }
}
fn verify_ledger(path: &str) -> bool {
    let text = match std::fs::read_to_string(path) { Ok(t) => t, Err(_) => return false };
    let mut tip = "GENESIS".to_string();
    for line in text.lines() {
        if line.is_empty() { continue; }
        let hash = match line.rsplit('\t').next() { Some(h) => h, None => return false };
        let body = &line[..line.len() - hash.len() - 1];
        if sha256_hex(&format!("{}|{}", tip, body)) != hash { return false; }
        tip = hash.to_string();
    }
    true
}

// ============================ shell + http ============================
fn run_cmd(dir: &str, prog: &str, args: &[&str]) -> (bool, String) {
    let out = Command::new(prog).args(args).current_dir(dir).output();
    match out {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            (o.status.success(), s)
        }
        Err(e) => (false, format!("spawn failed: {}", e)),
    }
}
fn tail(s: &str, n: usize) -> String {
    if s.len() <= n { s.to_string() } else { s[s.len() - n..].to_string() }
}
/// Minimal HTTP/1.1 POST for http:// endpoints (the local model).
/// https:// endpoints go through curl — std has no TLS, and the
/// daemon does not pretend otherwise.
fn http_post(url: &str, body: &str, auth: Option<&str>) -> Result<String, String> {
    if url.starts_with("https://") {
        let mut cmd = Command::new("curl");
        cmd.args(["-s", "-X", "POST", url, "-H", "Content-Type: application/json", "--data-binary", "@-"]);
        if let Some(a) = auth { cmd.args(["-H", &format!("Authorization: Bearer {}", a)]); }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        child.stdin.as_mut().unwrap().write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        return Ok(String::from_utf8_lossy(&out.stdout).to_string());
    }
    let rest = url.strip_prefix("http://").ok_or("bad url")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let mut stream = TcpStream::connect(hostport).map_err(|e| e.to_string())?;
    let mut req = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, hostport, body.len()
    );
    if let Some(a) = auth { req.push_str(&format!("Authorization: Bearer {}\r\n", a)); }
    req.push_str("\r\n");
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    let body_start = resp.find("\r\n\r\n").map(|i| i + 4).unwrap_or(0);
    Ok(resp[body_start..].to_string())
}

// ============================ the pipeline ============================
struct Task {
    id: String,
    repo_full: String,
    clone_url: String,
    default_branch: String,
    issue_number: i64,
    issue_title: String,
    commenter: String,
    prompt: String,
}
struct Job {
    system: String,
    user: String,
    claimed: bool,
    slot: Arc<(Mutex<Option<Result<String, String>>>, std::sync::Condvar)>,
}
type Jobs = Arc<Mutex<std::collections::HashMap<String, Arc<Job>>>>;
struct Ctx { cfg: Config, ledger: Arc<Mutex<Ledger>>, jobs: Jobs }

/// One inference call, two backends. "http" posts to the configured
/// OpenAI-shaped endpoint. "webllm" enqueues a job for the WebGPU
/// page (webllm/infer.html, served at GET /infer) and waits: the
/// model runs in a headless Chromium, on the GPU, via WebLLM —
/// no Python, no CUDA, no external API.
fn infer(ctx: &Ctx, task_id: &str, system: &str, user: &str) -> Result<String, String> {
    let cfg = &ctx.cfg;
    if cfg.model_backend == "webllm" {
        let job_id = format!("{}-job", task_id);
        let slot = Arc::new((Mutex::new(None), std::sync::Condvar::new()));
        {
            let mut jobs = ctx.jobs.lock().unwrap();
            jobs.insert(job_id.clone(), Arc::new(Job {
                system: system.to_string(), user: user.to_string(),
                claimed: false, slot: slot.clone(),
            }));
        }
        let (lock, cvar) = &*slot;
        let mut guard = lock.lock().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(cfg.infer_timeout_secs);
        loop {
            if let Some(r) = guard.take() {
                ctx.jobs.lock().unwrap().remove(&job_id);
                return r;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                ctx.jobs.lock().unwrap().remove(&job_id);
                return Err(format!("webllm: no result within {}s (is the inference page open?)", cfg.infer_timeout_secs));
            }
            let (g, _) = cvar.wait_timeout(guard, deadline - now).unwrap();
            guard = g;
        }
    }
    let payload = format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}],\"temperature\":0}}",
        json_escape(&cfg.model_name), json_escape(system), json_escape(user)
    );
    let reply = http_post(&cfg.model_endpoint, &payload, None)?;
    json_object(&reply, "message").and_then(|m| json_str(m, "content")).or_else(|| json_str(&reply, "content"))
        .ok_or_else(|| "model reply carried no message content".to_string())
}
fn seal(ctx: &Ctx, task: &str, stage: &str, detail: &str) {
    if let Ok(mut l) = ctx.ledger.lock() { l.append(task, stage, detail); }
    println!("[{}] {} — {}", task, stage, detail);
}

/// The model's reply format is FILE blocks:
///   FILE: relative/path
///   <<<
///   full new content
///   >>>
/// Full contents, not diffs: a local model writes files more
/// reliably than it writes patches (see STATUS).
fn parse_file_blocks(reply: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = reply.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if let Some(path) = lines[i].trim().strip_prefix("FILE:") {
            let path = path.trim().to_string();
            if i + 1 < lines.len() && lines[i + 1].trim() == "<<<" {
                let mut content = String::new();
                let mut j = i + 2;
                while j < lines.len() && lines[j].trim() != ">>>" {
                    content.push_str(lines[j]);
                    content.push('\n');
                    j += 1;
                }
                if j < lines.len() { out.push((path, content)); i = j; }
            }
        }
        i += 1;
    }
    out
}
fn safe_path(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|seg| seg == "..")
}

fn run_task(ctx: &Ctx, task: &Task) {
    let cfg = &ctx.cfg;
    seal(ctx, &task.id, "received", &format!("{} #{} from {}: {}", task.repo_full, task.issue_number, task.commenter, task.prompt));
    let run_dir = format!("{}/{}", cfg.workdir, task.id);
    let clone_dir = format!("{}/repo", run_dir);
    let _ = std::fs::create_dir_all(&run_dir);

    // --- sandbox: ephemeral clone ---
    let (ok, log) = run_cmd(".", "git", &["clone", "--quiet", &task.clone_url, &clone_dir]);
    if !ok { seal(ctx, &task.id, "failed", &format!("clone: {}", tail(&log, 300))); return; }
    seal(ctx, &task.id, "cloned", "repository cloned into the ephemeral sandbox");

    // --- tool discovery: the registry answers, context does not carry ---
    let mut tools_note = String::from("tool discovery not configured");
    let mut tools_block = String::new();
    if !cfg.discover_bin.is_empty() {
        let (ok, out) = run_cmd(".", &cfg.discover_bin, &["--registry", &cfg.discover_registry, "match", &task.prompt]);
        if ok {
            tools_block = out.clone();
            let n = out.lines().filter(|l| l.contains("score=")).count();
            tools_note = format!("{} candidate tools discovered for this task", n);
        } else {
            tools_note = "tool discovery engine unavailable — proceeding without it".to_string();
        }
    }
    seal(ctx, &task.id, "tools_discovered", &tools_note);

    // --- context: file list + the task + the discovered tools ---
    let (_, files) = run_cmd(&clone_dir, "git", &["ls-files"]);
    let files: String = files.lines().take(100).collect::<Vec<_>>().join("\n");
    let system = "You are a coding agent working in a sandbox clone. Reply with ONLY file blocks in this exact format, one per file you change:\nFILE: relative/path\n<<<\n<full new file content>\n>>>\nNo prose outside the blocks.";
    let user = format!(
        "Task (from {} on {} #{} — {}):\n{}\n\nRepository files:\n{}\n\nTools discovered for this task (name, evidence tier):\n{}",
        task.commenter, task.repo_full, task.issue_number, task.issue_title, task.prompt, files, tools_block
    );
    let content = match infer(ctx, &task.id, &system, &user) {
        Ok(c) => c,
        Err(e) => { seal(ctx, &task.id, "failed", &format!("model call: {}", e)); return; }
    };
    seal(ctx, &task.id, "model_replied", &format!("reply sha256 {}", sha256_hex(&content)));

    // --- apply ---
    let blocks = parse_file_blocks(&content);
    if blocks.is_empty() { seal(ctx, &task.id, "failed", "model reply contained no FILE blocks"); return; }
    let mut applied = 0;
    for (path, content) in &blocks {
        if !safe_path(path) { seal(ctx, &task.id, "failed", &format!("unsafe path refused: {}", path)); return; }
        let full = format!("{}/{}", clone_dir, path);
        if let Some(parent) = full.rfind('/') { let _ = std::fs::create_dir_all(&full[..parent]); }
        if std::fs::write(&full, content).is_err() { seal(ctx, &task.id, "failed", &format!("write failed: {}", path)); return; }
        applied += 1;
    }
    seal(ctx, &task.id, "applied", &format!("{} file(s) written", applied));

    // --- test gate: no pass, no push ---
    let mut test_line = "no test command configured — changes unverified".to_string();
    let mut test_out = String::new();
    if !cfg.test_cmd.is_empty() {
        let (ok, out) = run_cmd(&clone_dir, "sh", &["-c", &cfg.test_cmd]);
        test_out = tail(&out, 1500);
        if !ok { seal(ctx, &task.id, "failed", &format!("test gate: `{}` FAILED", cfg.test_cmd)); return; }
        test_line = format!("test gate: `{}` — PASS", cfg.test_cmd);
    }
    seal(ctx, &task.id, "tested", &test_line);

    // --- branch, commit, push ---
    let branch = format!("sovereign/{}", task.id);
    let _ = run_cmd(&clone_dir, "git", &["config", "user.name", &cfg.git_name]);
    let _ = run_cmd(&clone_dir, "git", &["config", "user.email", &cfg.git_email]);
    let (ok, log) = run_cmd(&clone_dir, "git", &["checkout", "-b", &branch]);
    if !ok { seal(ctx, &task.id, "failed", &format!("branch: {}", tail(&log, 200))); return; }
    let _ = run_cmd(&clone_dir, "git", &["add", "-A"]);
    let (ok, log) = run_cmd(&clone_dir, "git", &["commit", "-m", &format!("{} ({})", task.issue_title, task.id)]);
    if !ok { seal(ctx, &task.id, "failed", &format!("commit: {}", tail(&log, 200))); return; }

    let token = std::env::var(&cfg.gh_token_env).unwrap_or_default();
    let is_http = task.clone_url.starts_with("https://");
    let push_url = if is_http && !token.is_empty() {
        task.clone_url.replacen("https://", &format!("https://x-access-token:{}@", token), 1)
    } else {
        task.clone_url.clone()
    };
    let (ok, log) = run_cmd(&clone_dir, "git", &["push", &push_url, &branch]);
    if !ok {
        // Never let the token reach a log line.
        let clean = if token.is_empty() { log } else { log.replace(&token, "***") };
        seal(ctx, &task.id, "failed", &format!("push: {}", tail(&clean, 200)));
        return;
    }

    // --- PR: the bot comment is standard markdown, checklist + fences ---
    let checklist = format!(
        "## Sovereign Jules — {}\n\n- [x] Task received from @{}\n- [x] Repository cloned into an ephemeral sandbox\n- [x] {}\n- [x] Changes applied ({} file(s))\n- [x] {}\n\nTerminal output:\n```\n{}\n```\n",
        task.issue_title, task.commenter, tools_note, applied, test_line,
        if test_out.is_empty() { "(no test command configured)".to_string() } else { test_out }
    );
    if is_http && !token.is_empty() {
        let pr_json = format!(
            "{{\"title\":\"{}\",\"head\":\"{}\",\"base\":\"{}\",\"body\":\"{}\"}}",
            json_escape(&task.issue_title), json_escape(&branch), json_escape(&task.default_branch), json_escape(&checklist)
        );
        let api = format!("https://api.github.com/repos/{}/pulls", task.repo_full);
        match http_post(&api, &pr_json, Some(&token)) {
            Ok(r) => {
                let url = json_str(&r, "html_url").unwrap_or_else(|| "(url not in reply)".to_string());
                seal(ctx, &task.id, "completed", &format!("PR opened: {}", url));
            }
            Err(e) => seal(ctx, &task.id, "failed", &format!("PR create: {}", e)),
        }
    } else {
        // Dry run: the branch is pushed (local remotes) or prepared;
        // the PR payload is written for inspection, never faked.
        let _ = std::fs::write(format!("{}/pr-body.md", run_dir), &checklist);
        seal(ctx, &task.id, "completed", "branch pushed; PR dry-run (no GitHub token) — payload in pr-body.md");
    }
}

// ============================ dashboard ============================
// The Jules dashboard look, recreated with the owner's supplied
// zero-dependency CSS (verbatim) over the run ledger.
const JULES_CSS: &str = r#"
:root {
  --jules-bg: #0e1114;
  --jules-surface: #1a1d21;
  --jules-border: #2d3136;
  --jules-primary: #8ab4f8; /* Google Labs Blue */
  --jules-text-main: #e8eaed;
  --jules-text-muted: #9aa0a6;
  --font-mono: 'JetBrains Mono', 'Fira Code', monospace;
}

body {
  background-color: var(--jules-bg);
  color: var(--jules-text-main);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  margin: 0;
  display: grid;
  grid-template-columns: 260px 1fr;
  height: 100vh;
}

.sidebar {
  border-right: 1px solid var(--jules-border);
  padding: 24px;
  background-color: var(--jules-surface);
}

.task-card {
  background-color: var(--jules-surface);
  border: 1px solid var(--jules-border);
  border-radius: 8px;
  padding: 20px;
  margin-bottom: 16px;
  transition: border-color 0.2s;
}

.task-card:hover {
  border-color: var(--jules-primary);
}

.bot-comment {
  font-family: var(--font-mono);
  font-size: 0.85rem;
  color: var(--jules-text-muted);
  background: #131518;
  padding: 12px;
  border-left: 3px solid var(--jules-primary);
  border-radius: 4px;
}
"#;
fn render_dashboard(ledger_path: &str) -> String {
    let mut tasks: Vec<(String, Vec<(String, String)>)> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(ledger_path) {
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 6 { continue; }
            let (task, stage, detail) = (f[2], f[3], f[4]);
            match tasks.iter_mut().find(|(t, _)| t == task) {
                Some((_, ev)) => ev.push((stage.to_string(), detail.to_string())),
                None => tasks.push((task.to_string(), vec![(stage.to_string(), detail.to_string())])),
            }
        }
    }
    tasks.reverse();
    let mut cards = String::new();
    for (task, events) in &tasks {
        let status = events.last().map(|(s, _)| s.as_str()).unwrap_or("");
        cards.push_str(&format!("<div class=\"task-card\"><h3>{}</h3><p style=\"color:var(--jules-primary)\">{}</p>", html_escape(task), html_escape(status)));
        for (stage, detail) in events {
            cards.push_str(&format!("<div class=\"bot-comment\"><strong>{}</strong> — {}</div>", html_escape(stage), html_escape(detail)));
        }
        cards.push_str("</div>");
    }
    if cards.is_empty() { cards = "<div class=\"task-card\">No tasks yet. Mention the bot on an issue or PR.</div>".to_string(); }
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Sovereign Jules</title><style>{}</style></head><body><div class=\"sidebar\"><h2>Sovereign Jules</h2><p style=\"color:var(--jules-text-muted)\">Asynchronous coding, on owned ground. Every stage sealed in the run ledger.</p></div><main style=\"padding:24px;overflow:auto\">{}</main></body></html>",
        JULES_CSS, cards
    )
}

// ============================ webhook server ============================
fn handle_webhook(ctx: &Ctx, headers: &HashMap<String, String>, body: &str) -> (u16, String) {
    let cfg = &ctx.cfg;
    if !cfg.webhook_secret.is_empty() {
        let sig = headers.get("x-hub-signature-256").cloned().unwrap_or_default();
        let want = format!("sha256={}", hmac_sha256_hex(&cfg.webhook_secret, body.as_bytes()));
        if sig != want { return (401, "{\"error\":\"bad signature\"}".to_string()); }
    }
    let event = headers.get("x-github-event").cloned().unwrap_or_default();
    match event.as_str() {
        "ping" => (200, "{\"ok\":true,\"zen\":\"sovereign\"}".to_string()),
        "issue_comment" => {
            let action = json_str(body, "action").unwrap_or_default();
            if action != "created" { return (200, "{\"ignored\":\"action\"}".to_string()); }
            let comment = json_object(body, "comment").unwrap_or("");
            let text = json_str(comment, "body").unwrap_or_default();
            let trigger = format!("@{}", cfg.bot_name);
            if !text.contains(&trigger) { return (200, "{\"ignored\":\"no trigger\"}".to_string()); }
            let repo = json_object(body, "repository").unwrap_or("");
            let issue = json_object(body, "issue").unwrap_or("");
            let user = json_object(comment, "user").unwrap_or("");
            let prompt = text.replace(&trigger, "").trim().to_string();
            let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let task = Task {
                id: format!("t{}-{}", ts, ts % 997),
                repo_full: json_str(repo, "full_name").unwrap_or_default(),
                clone_url: json_str(repo, "clone_url").unwrap_or_default(),
                default_branch: json_str(repo, "default_branch").unwrap_or_else(|| "main".to_string()),
                issue_number: json_num(issue, "number").unwrap_or(0),
                issue_title: json_str(issue, "title").unwrap_or_else(|| "task".to_string()),
                commenter: json_str(user, "login").unwrap_or_default(),
                prompt,
            };
            println!("task {} extracted: repo={} branch={} prompt={:?}", task.id, task.repo_full, task.default_branch, task.prompt);
            let tid = task.id.clone();
            let ctx2 = Ctx { cfg: ctx.cfg.clone(), ledger: ctx.ledger.clone(), jobs: ctx.jobs.clone() };
            std::thread::spawn(move || run_task(&ctx2, &task));
            (202, format!("{{\"task\":\"{}\"}}", tid))
        }
        "push" => {
            let repo = json_object(body, "repository").unwrap_or("");
            println!("push event: {} {}", json_str(repo, "full_name").unwrap_or_default(), json_str(body, "ref").unwrap_or_default());
            (200, "{\"ok\":true}".to_string())
        }
        _ => (200, "{\"ignored\":\"event\"}".to_string()),
    }
}
fn serve(ctx: Ctx) {
    let listener = TcpListener::bind(format!("127.0.0.1:{}", ctx.cfg.port)).expect("bind failed");
    println!("sovereign-jules listening on 127.0.0.1:{} — bot @{}", ctx.cfg.port, ctx.cfg.bot_name);
    for stream in listener.incoming() {
        let mut stream = match stream { Ok(s) => s, Err(_) => continue };
        let ctx2 = Ctx { cfg: ctx.cfg.clone(), ledger: ctx.ledger.clone(), jobs: ctx.jobs.clone() };
        std::thread::spawn(move || {
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            let mut header_end = None;
            let mut content_length = 0usize;
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        raw.extend_from_slice(&buf[..n]);
                        if header_end.is_none() {
                            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                                header_end = Some(pos + 4);
                                let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                                for line in head.lines() {
                                    let l = line.to_lowercase();
                                    if l.starts_with("content-length:") { content_length = l[15..].trim().parse().unwrap_or(0); }
                                }
                            }
                        }
                        if let Some(he) = header_end { if raw.len() >= he + content_length { break; } }
                    }
                    Err(_) => break,
                }
            }
            let text = String::from_utf8_lossy(&raw).to_string();
            let mut parts = text.splitn(2, "\r\n\r\n");
            let head = parts.next().unwrap_or("");
            let body = parts.next().unwrap_or("").to_string();
            let mut lines = head.lines();
            let request_line = lines.next().unwrap_or("");
            let mut headers: HashMap<String, String> = HashMap::new();
            for line in lines {
                if let Some((k, v)) = line.split_once(':') { headers.insert(k.trim().to_lowercase(), v.trim().to_string()); }
            }
            let (code, payload, ctype) = if request_line.starts_with("GET /infer/next") {
                let mut found = None;
                {
                    let mut jobs = ctx2.jobs.lock().unwrap();
                    let key = jobs.iter().find(|(_, j)| !j.claimed).map(|(k, _)| k.clone());
                    if let Some(k) = key {
                        let j = jobs.get(&k).unwrap();
                        // claim it: rebuild the entry marked claimed
                        let nj = Arc::new(Job { system: j.system.clone(), user: j.user.clone(), claimed: true, slot: j.slot.clone() });
                        found = Some(format!("{{\"id\":\"{}\",\"system\":\"{}\",\"user\":\"{}\"}}",
                            json_escape(&k), json_escape(&nj.system), json_escape(&nj.user)));
                        jobs.insert(k, nj);
                    }
                }
                match found {
                    Some(p) => (200, p, "application/json"),
                    None => (204, String::new(), "application/json"),
                }
            } else if request_line.starts_with("POST /infer/result") {
                let id = json_str(&body, "id").unwrap_or_default();
                let job = ctx2.jobs.lock().unwrap().get(&id).cloned();
                match job {
                    Some(j) => {
                        let result = match json_str(&body, "error") {
                            Some(e) => Err(format!("webllm page error: {}", e)),
                            None => Ok(json_str(&body, "content").unwrap_or_default()),
                        };
                        let (lock, cvar) = &*j.slot;
                        *lock.lock().unwrap() = Some(result);
                        cvar.notify_all();
                        (200, "{\"ok\":true}".to_string(), "application/json")
                    }
                    None => (404, "{\"error\":\"unknown job\"}".to_string(), "application/json"),
                }
            } else if request_line.starts_with("GET /infer ") || request_line.starts_with("GET /infer HTTP") {
                let page = include_str!("../webllm/infer.html").replace("__WEBLLM_MODEL__", &ctx2.cfg.webllm_model);
                (200, page, "text/html")
            } else if request_line.starts_with("GET /health") {
                (200, "ok".to_string(), "text/plain")
            } else if request_line.starts_with("GET / ") || request_line.starts_with("GET / HTTP") {
                (200, render_dashboard(&ctx2.cfg.ledger_path), "text/html")
            } else if request_line.starts_with("POST /webhook") {
                let (c, p) = handle_webhook(&ctx2, &headers, &body);
                (c, p, "application/json")
            } else {
                (404, "not found".to_string(), "text/plain")
            };
            let resp = format!("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                code, if code == 200 || code == 202 { "OK" } else { "ERR" }, ctype, payload.len(), payload);
            let _ = stream.write_all(resp.as_bytes());
        });
    }
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = if args.len() > 1 && args[1].starts_with("--") { args[1].clone() } else { "".to_string() };
    let cfg_path = if mode.is_empty() { args.get(1) } else { args.get(2) }.cloned().unwrap_or_else(|| "sovereign-jules.conf".to_string());
    let cfg = Config::load(&cfg_path);
    if mode == "--verify-ledger" {
        let ok = verify_ledger(&cfg.ledger_path);
        println!("ledger {} — {}", cfg.ledger_path, if ok { "VALID" } else { "REFUSED" });
        std::process::exit(if ok { 0 } else { 4 });
    }
    if mode == "--dashboard" {
        println!("{}", render_dashboard(&cfg.ledger_path));
        return;
    }
    let ledger = Arc::new(Mutex::new(Ledger::open(&cfg.ledger_path)));
    let jobs: Jobs = Arc::new(Mutex::new(std::collections::HashMap::new()));
    serve(Ctx { cfg, ledger, jobs });
}
