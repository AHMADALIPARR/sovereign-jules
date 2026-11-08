#!/usr/bin/env python3
"""Stands in for the WebLLM page: polls /infer/next like the page
does and answers with a FILE block. The daemon cannot tell this
from a real browser running WebGPU inference — which is the
point: this verifies the interop channel, not the model."""
import json, sys, time, urllib.request

BASE = sys.argv[1]
CONTENT = "FILE: hello.txt\n<<<\nhello\nsovereign marker\n>>>\n"

for _ in range(120):
    try:
        with urllib.request.urlopen(BASE + "/infer/next", timeout=5) as r:
            if r.status == 200:
                job = json.loads(r.read())
                body = json.dumps({"id": job["id"], "content": CONTENT}).encode()
                req = urllib.request.Request(BASE + "/infer/result", data=body,
                                             headers={"Content-Type": "application/json"})
                urllib.request.urlopen(req, timeout=5).read()
                print("fake browser answered job", job["id"])
                sys.exit(0)
    except Exception:
        pass
    time.sleep(0.5)
print("fake browser: no job arrived")
sys.exit(1)
