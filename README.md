# Sovereign Jules

A Jules-style asynchronous coding orchestrator on owned ground.
Google's Jules is proprietary and cloud-hosted; its architecture
is not. This is that architecture rebuilt sovereign: a GitHub
webhook listener, an ephemeral sandbox per task, tool discovery
instead of context stuffing, a local or API model, a test gate,
and a pull request — every stage sealed in a hash-chained run
ledger, watched from a zero-dependency dashboard in the Jules
dark-mode aesthetic.

## Ingress — the edge route

GitHub does not talk to the daemon directly. Deliveries land on
`edge/worker.js`, a Cloudflare Worker that **verifies** GitHub's
HMAC at the edge, **scrubs** the payload down to the fields the
daemon reads (installation metadata, sender objects, and the rest
of the tail are dropped at the edge, not at the house), and
**forwards** the scrubbed body to the daemon re-signed with a
daemon secret, so the daemon's own HMAC check verifies exactly
what it receives. See `edge/wrangler.toml.example`.

## The cognitive engine — WebGPU

Two model backends, one seam (`model_backend`):

- `http` — an OpenAI-shaped endpoint (local Ollama, an API).
- `webllm` — a headless Chromium page served by the daemon at
  `GET /infer` loads **WebLLM**, which runs the model through
  **WebGPU** directly in VRAM: no Python, no CUDA, no external
  API. The daemon hands the page jobs (`/infer/next`,
  `/infer/result`) carrying the repository context and the task
  text; the page returns the completion. The pipeline downstream
  — apply, test gate, push, PR — is identical either way.

## The loop

1. **Ingestion.** A GitHub App (Contents, Pull Requests, Issues:
   read/write; `issue_comment` and `push` events) points its
   webhook at the daemon. With `webhook_secret` set, deliveries
   must carry a valid `X-Hub-Signature-256` HMAC or they are
   refused with 401.
2. **Trigger.** A comment mentioning `@<bot_name>` starts a task:
   the daemon extracts the repository, branch, and prompt.
3. **Sandbox.** The repository is cloned into an ephemeral work
   directory for this task alone.
4. **Discovery.** The task prompt is matched against the Tool
   Discovery Engine's sealed registry; the model receives the
   candidate tools for this task — not a preloaded catalogue.
5. **Model.** An OpenAI-shaped endpoint (local Ollama by default;
   the owner's 27B on the Jetson, or an API) returns full-file
   changes in `FILE:` blocks, which are applied to the clone.
   Unsafe paths are refused.
6. **Test gate.** The configured test command runs in the sandbox.
   Failure stops the line: no push, and the ledger says so.
7. **Pull request.** On a pass, the daemon pushes branch
   `sovereign/<task-id>` and opens a PR whose body is the
   standard checklist markdown (`- [x]`) with the terminal output
   in a fenced block. Without a GitHub token the run is a dry
   run: the payload is written to the run directory, never faked.

## The dashboard

`GET /` renders the run ledger as task cards in the Jules Labs
dark aesthetic — raw CSS, no framework, no scripts. The ledger
itself is append-only and hash-chained; `--verify-ledger`
recomputes it.

## Configuration

See `sovereign-jules.conf.example` (key = value): port, bot name,
workdir, model endpoint and name, discovery engine paths, test
command, git identity, webhook secret, and the environment
variable that holds the GitHub token. Tokens come from the
environment only; they are never written to disk or to the ledger.

## Build and test

```sh
cargo build --release
./test/run-tests.sh
```

Zero external crates: the daemon is Rust standard library only.
