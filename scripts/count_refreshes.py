#!/usr/bin/env python3
"""Count `workspace/diagnostic/refresh` requests during a startup session.

Drives the server over stdio the way a pull-diagnostics editor does: opens
one file, answers every refresh with a workspace pull, and reports how many
refreshes arrived.  Used to check that a source run which reproduces a
file's diagnostics no longer invalidates the client's whole result set.

    python3 scripts/count_refreshes.py <project-root> <file.php> [seconds]
"""

import json
import subprocess
import sys
import threading
import time
from pathlib import Path

root = Path(sys.argv[1]).resolve()
target = Path(sys.argv[2]).resolve()
duration = float(sys.argv[3]) if len(sys.argv) > 3 else 25.0

proc = subprocess.Popen(
    ["target/debug/phpantom_lsp"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL,
)

lock = threading.Lock()
counts = {"refresh": 0, "ws_pull": 0, "doc_pull": 0, "full_items": 0}
next_id = [100]


def send(msg):
    body = json.dumps(msg).encode()
    with lock:
        proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        proc.stdin.flush()


def request(method, params):
    next_id[0] += 1
    send({"jsonrpc": "2.0", "id": next_id[0], "method": method, "params": params})


def read_message():
    length = None
    while True:
        line = proc.stdout.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        if line.lower().startswith(b"content-length:"):
            length = int(line.split(b":")[1])
    return json.loads(proc.stdout.read(length))


def pump():
    while True:
        msg = read_message()
        if msg is None:
            return
        method = msg.get("method")
        if method == "workspace/diagnostic/refresh":
            counts["refresh"] += 1
            send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
            # A refresh invalidates every result, so a real editor re-pulls.
            counts["ws_pull"] += 1
            request("workspace/diagnostic",
                    {"identifier": "phpantom", "previousResultIds": []})
            counts["doc_pull"] += 1
            request("textDocument/diagnostic",
                    {"textDocument": {"uri": target.as_uri()},
                     "identifier": "phpantom"})
        elif method in ("window/workDoneProgress/create",
                        "client/registerCapability"):
            send({"jsonrpc": "2.0", "id": msg["id"], "result": None})
        elif "result" in msg and isinstance(msg["result"], dict):
            for item in msg["result"].get("items", []):
                if isinstance(item, dict) and item.get("kind") == "full":
                    counts["full_items"] += 1


threading.Thread(target=pump, daemon=True).start()

request("initialize", {
    "processId": None,
    "rootUri": root.as_uri(),
    "capabilities": {
        "window": {"workDoneProgress": True},
        "workspace": {"diagnostics": {"refreshSupport": True}},
        "textDocument": {"diagnostic": {"dynamicRegistration": False,
                                        "relatedDocumentSupport": False}},
    },
})
time.sleep(1.0)
send({"jsonrpc": "2.0", "method": "initialized", "params": {}})
send({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
    "textDocument": {"uri": target.as_uri(), "languageId": "php",
                     "version": 1, "text": target.read_text()},
}})
request("workspace/diagnostic",
        {"identifier": "phpantom", "previousResultIds": []})

time.sleep(duration)
print(json.dumps(counts))
proc.kill()
