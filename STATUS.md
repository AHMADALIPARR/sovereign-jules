# STATUS

## Verified in this workspace (test/run-tests.sh, 18/18)

- Blueprint step 1: a ping delivery returns HTTP 200.
- Blueprint step 2: a trigger comment prints the extracted repo,
  branch, and prompt to stdout; a comment without the trigger
  starts nothing.
- Blueprint step 3: a triggered task clones the repository into
  its ephemeral sandbox directory.
- Blueprint step 4, local form: a stub model over the real HTTP
  path returns a file change; the change is applied, the test
  gate passes, branch sovereign/<task-id> is pushed to a local
  bare remote with the change on it, and the PR payload is
  written in the checklist format. The GitHub PR call itself runs
  only with a token present (see Limits).
- Tool discovery runs mid-pipeline and seals its candidate count
  into the ledger.
- Webhook HMAC: valid signature 200, forged signature 401.
- The run ledger chain verifies; the dashboard serves the Jules
  CSS with task cards rendered from the ledger.

- The WebGPU backend's full loop also completes 18/18 with the
  browser channel played by a stand-in client (the daemon cannot
  distinguish it from the real page): job claimed from
  /infer/next, result posted back, pipeline completed.
- The edge worker is verified under Node with a captured fetch:
  valid GitHub signature forwards a scrubbed body (installation
  and sender objects dropped) re-signed for the daemon; a forged
  signature gets 401 at the edge and is never forwarded.

## Limits

- WebGPU inference itself — a real model generating in a real
  browser — first runs on a GPU machine (bbqbaddie, RTX 3080, or
  the Jetson). This VM has no GPU. What is verified here is the
  complete interop channel the page speaks.
- WebLLM runs MLC-format models. Sizing is physics: a dense 27B
  at q4 needs roughly 16 GB of VRAM, past a 3080's 10/12 GB; the
  7–14B q4 class fits comfortably, and low-bit (e.g. ternary)
  27B-class weights fit where dense ones do not. The model id is
  one config line (webllm_model).
- The live GitHub loop (App installation, push to github.com,
  PR creation through the REST API) is code-complete but not
  exercised here: it needs the owner's GitHub App, his token in
  the environment, and his edge route (webhook -> local network).
  It runs first on his machines (Jetson / bbqbaddie), where the
  27B model also lives. The model call in this workspace was a
  stub over the real HTTP path.
- The model writes full files (FILE blocks), not unified diffs:
  local models produce files more reliably than patches. Diff
  application is a format addition, not a redesign.
- The sandbox is an isolated working directory per task. On the
  owner's fleet the same seam takes a Docker container or a
  Firecracker microVM per task; the pipeline does not change.
- The JSON reading is targeted field extraction for GitHub
  payloads and OpenAI-shaped replies, not a general parser.
