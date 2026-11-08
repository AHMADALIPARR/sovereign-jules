#!/bin/sh
# Sovereign Jules — end-to-end verification by execution.
# Walks the blueprint's own verification steps against a local
# bare remote and a stub model; no GitHub, no real model, no token.
set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
BIN="$ROOT/target/release/sovereign-jules"
DISCOVER="${DISCOVER:-$HOME/workspace/tool-discovery-engine/discover}"
REGISTRY="${REGISTRY:-$HOME/workspace/tool-discovery-engine/registry}"
W=$(mktemp -d /tmp/sj-test.XXXXXX)
PORT=8891
MPORT=8892
pass=0; fail=0
ok() { if [ "$1" = 0 ]; then echo "  PASS  $2"; pass=$((pass+1)); else echo "  FAIL  $2"; fail=$((fail+1)); fi }
cleanup() { kill $DPID $MPID $WPID $SPID 2>/dev/null; }
trap cleanup EXIT

[ -x "$BIN" ] || { export RUSTUP_HOME=$HOME/workspace/.tools/rustup CARGO_HOME=$HOME/workspace/.tools/cargo PATH=$HOME/workspace/.tools/cargo/bin:$PATH; cargo build --release > /dev/null 2>&1; }

# --- seed repo + bare remote -------------------------------------
git init -q -b main "$W/seed"
printf 'hello\n' > "$W/seed/hello.txt"
printf 'grep -q "sovereign marker" hello.txt\n' > "$W/seed/test.sh"
git -C "$W/seed" add -A
git -C "$W/seed" -c user.name=test -c user.email=test@test commit -q -m seed
git clone -q --bare "$W/seed" "$W/remote.git"

# --- config --------------------------------------------------------
cat > "$W/conf" << EOF
port = $PORT
bot_name = sovereign-bot
workdir = $W/runs
ledger_path = $W/ledger.log
model_endpoint = http://127.0.0.1:$MPORT/v1/chat/completions
model_name = stub
discover_bin = $DISCOVER
discover_registry = $REGISTRY
test_cmd = sh test.sh
git_name = Sovereign Jules (test)
git_email = test@test
EOF

python3 test/stub_model.py $MPORT & MPID=$!
"$BIN" "$W/conf" > "$W/daemon.log" 2>&1 & DPID=$!
i=0; while [ $i -lt 20 ]; do curl -s "http://127.0.0.1:$PORT/health" > /dev/null 2>&1 && break; i=$((i+1)); sleep 0.3; done

# --- step 1: webhook ping returns 200 ------------------------------
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/webhook" -H "X-GitHub-Event: ping" -H 'Content-Type: application/json' --data-binary @fixtures/ping.json)
[ "$code" = 200 ]; ok $? "step 1: ping -> HTTP 200"

# --- a comment without the trigger starts nothing ------------------
sed "s|CLONE_URL_HERE|$W/remote.git|" fixtures/no_trigger.json > "$W/no_trigger.json"
curl -s -X POST "http://127.0.0.1:$PORT/webhook" -H "X-GitHub-Event: issue_comment" --data-binary @"$W/no_trigger.json" > /dev/null
sleep 1
if grep -q "received" "$W/ledger.log" 2>/dev/null; then r=1; else r=0; fi
[ $r = 0 ]; ok $? "no trigger, no task"

# --- step 2+3+4: trigger -> extract -> clone -> model -> test -> push
sed "s|CLONE_URL_HERE|$W/remote.git|" fixtures/issue_comment.json > "$W/issue.json"
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/webhook" -H "X-GitHub-Event: issue_comment" --data-binary @"$W/issue.json")
[ "$code" = 202 ]; ok $? "step 2: trigger comment accepted (202)"
grep -q "extracted: repo=local/test-repo" "$W/daemon.log"; ok $? "step 2: repo, branch and prompt printed to stdout"

i=0; while [ $i -lt 40 ]; do grep -q "completed\|failed" "$W/ledger.log" 2>/dev/null && break; i=$((i+1)); sleep 0.5; done

ls "$W"/runs/*/repo/hello.txt > /dev/null 2>&1; ok $? "step 3: sandbox holds the cloned repository"
grep -q "tools_discovered" "$W/ledger.log"; ok $? "tools discovered via the discovery engine"
grep -q "tested.*PASS" "$W/ledger.log"; ok $? "test gate passed before any push"
grep -q "completed.*PR dry-run" "$W/ledger.log"; ok $? "step 4 (local form): branch pushed, PR payload prepared"
BR=$(git -C "$W/remote.git" branch | grep sovereign | head -1 | tr -d ' *')
[ -n "$BR" ]; ok $? "branch $BR exists on the remote"
git -C "$W/remote.git" show "$BR:hello.txt" | grep -q "sovereign marker"; ok $? "the model's change is on the branch"
ls "$W"/runs/*/pr-body.md > /dev/null 2>&1 && grep -q '\- \[x\] Test gate\|\- \[x\] test gate' "$W"/runs/*/pr-body.md; ok $? "PR body carries the checklist format"
"$BIN" --verify-ledger "$W/conf" | grep -q VALID; ok $? "run ledger chain verifies"

# --- dashboard ------------------------------------------------------
curl -s "http://127.0.0.1:$PORT/" > "$W/dash.html"
grep -q -- "--jules-bg: #0e1114" "$W/dash.html" && grep -q "task-card" "$W/dash.html"; ok $? "dashboard serves the Jules CSS and task cards"

# --- webhook signature gate -----------------------------------------
printf 'port = 8893\nbot_name = sovereign-bot\nworkdir = %s/runs2\nledger_path = %s/ledger2.log\nwebhook_secret = test-secret\n' "$W" "$W" > "$W/conf2"
"$BIN" "$W/conf2" > /dev/null 2>&1 & SPID=$!
sleep 1
SIG=$(python3 -c "
import hmac, hashlib
body = open('fixtures/ping.json','rb').read()
print('sha256=' + hmac.new(b'test-secret', body, hashlib.sha256).hexdigest())")
good=$(curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:8893/webhook -H "X-GitHub-Event: ping" -H "X-Hub-Signature-256: $SIG" --data-binary @fixtures/ping.json)
bad=$(curl -s -o /dev/null -w '%{http_code}' -X POST http://127.0.0.1:8893/webhook -H "X-GitHub-Event: ping" -H "X-Hub-Signature-256: sha256=deadbeef" --data-binary @fixtures/ping.json)
kill $SPID 2>/dev/null
[ "$good" = 200 ] && [ "$bad" = 401 ]; ok $? "webhook HMAC: good signature 200, forged 401"

# --- WebGPU backend: the same loop through the browser channel ------
sed "s/port = $PORT/port = 8894/; s|workdir = $W/runs|workdir = $W/runs-web|; s|ledger_path = $W/ledger.log|ledger_path = $W/ledger-web.log|" "$W/conf" > "$W/conf-web"
printf 'model_backend = webllm\nwebllm_model = TestModel-MLC\ninfer_timeout_secs = 60\n' >> "$W/conf-web"
"$BIN" "$W/conf-web" > "$W/daemon-web.log" 2>&1 & WPID=$!
i=0; while [ $i -lt 20 ]; do curl -s "http://127.0.0.1:8894/health" > /dev/null 2>&1 && break; i=$((i+1)); sleep 0.3; done
curl -s "http://127.0.0.1:8894/infer" | grep -q "TestModel-MLC"; ok $? "inference page served with the configured model id"
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:8894/webhook" -H "X-GitHub-Event: issue_comment" --data-binary @"$W/issue.json")
python3 test/fake_browser.py http://127.0.0.1:8894 > "$W/browser.log" 2>&1
grep -q "answered job" "$W/browser.log"; ok $? "browser channel: job claimed and answered by the page role"
i=0; while [ $i -lt 40 ]; do grep -q "completed" "$W/ledger-web.log" 2>/dev/null && break; i=$((i+1)); sleep 0.5; done
grep -q "completed.*PR dry-run" "$W/ledger-web.log"; ok $? "webllm backend: full pipeline completed via the browser channel"
kill $WPID 2>/dev/null

# --- edge worker ------------------------------------------------------
node test/worker_harness.mjs > /dev/null 2>&1; ok $? "edge worker: verify, scrub, re-sign, forward"

echo "tests: $pass passed, $fail failed"
[ "$fail" = 0 ]
