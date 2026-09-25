#!/usr/bin/env python3
"""One-off probe of the Devsense PHP LS Blade support. Not part of the repo."""
import json
import os
import queue
import subprocess
import sys
import threading
import time

BIN = "/home/ajenbo/.local/share/zed/extensions/work/php/node_modules/devsense-php-ls-linux-x64/dist/devsense.php.ls"
ROOT = "/home/ajenbo/code/phpantom_lsp/examples/laravel"
ROOT_URI = "file://" + ROOT

proc = subprocess.Popen(
    [BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    stderr=subprocess.DEVNULL, cwd=ROOT,
)

messages = queue.Queue()
diagnostics = {}


def reader():
    buf = proc.stdout
    while True:
        headers = {}
        line = buf.readline()
        if not line:
            return
        while line and line.strip():
            k, _, v = line.decode().partition(":")
            headers[k.strip().lower()] = v.strip()
            line = buf.readline()
        length = int(headers.get("content-length", 0))
        if not length:
            continue
        body = buf.read(length)
        try:
            msg = json.loads(body)
        except Exception:
            continue
        if msg.get("method") == "textDocument/publishDiagnostics":
            p = msg["params"]
            diagnostics[p["uri"]] = p["diagnostics"]
        messages.put(msg)


threading.Thread(target=reader, daemon=True).start()

_next_id = [0]


def send(method, params, is_request=True):
    msg = {"jsonrpc": "2.0", "method": method, "params": params}
    if is_request:
        _next_id[0] += 1
        msg["id"] = _next_id[0]
    body = json.dumps(msg).encode()
    proc.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
    proc.stdin.flush()
    return msg.get("id")


def wait_response(rid, timeout=30):
    deadline = time.time() + timeout
    stash = []
    result = None
    while time.time() < deadline:
        try:
            msg = messages.get(timeout=0.2)
        except queue.Empty:
            continue
        if msg.get("id") == rid and ("result" in msg or "error" in msg):
            result = msg
            break
        # answer server->client requests so it doesn't stall
        if "id" in msg and "method" in msg:
            body = json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": None}).encode()
            proc.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
            proc.stdin.flush()
        stash.append(msg)
    for m in stash:
        messages.put(m)
    return result


def drain(seconds):
    deadline = time.time() + seconds
    while time.time() < deadline:
        try:
            msg = messages.get(timeout=0.2)
        except queue.Empty:
            continue
        if "id" in msg and "method" in msg:
            body = json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": None}).encode()
            proc.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
            proc.stdin.flush()


rid = send("initialize", {
    "processId": os.getpid(),
    "rootUri": ROOT_URI,
    "workspaceFolders": [{"uri": ROOT_URI, "name": "laravel"}],
    "capabilities": {
        "textDocument": {
            "hover": {"contentFormat": ["markdown", "plaintext"]},
            "completion": {"completionItem": {"snippetSupport": True}},
            "definition": {},
            "publishDiagnostics": {},
        },
        "workspace": {"workspaceFolders": True, "configuration": False},
    },
    "initializationOptions": {},
})
resp = wait_response(rid, 30)
if resp is None:
    print("FATAL: no initialize response")
    sys.exit(1)
send("initialized", {}, is_request=False)
print("initialized ok; waiting for indexing...")
drain(25)


def open_doc(rel, text=None):
    path = os.path.join(ROOT, rel)
    if text is None:
        text = open(path).read()
    uri = "file://" + path
    send("textDocument/didOpen", {"textDocument": {
        "uri": uri, "languageId": "blade" if rel.endswith(".blade.php") else "php",
        "version": 1, "text": text,
    }}, is_request=False)
    return uri, text


def pos_of(text, needle, inner_offset, occurrence=1):
    idx = -1
    for _ in range(occurrence):
        idx = text.index(needle, idx + 1)
    idx += inner_offset
    line = text.count("\n", 0, idx)
    col = idx - (text.rfind("\n", 0, idx) + 1)
    return {"line": line, "character": col}


def show_hover(uri, pos, label):
    rid = send("textDocument/hover", {"textDocument": {"uri": uri}, "position": pos})
    r = wait_response(rid)
    out = None
    if r and r.get("result"):
        c = r["result"].get("contents")
        if isinstance(c, dict):
            out = c.get("value")
        elif isinstance(c, list):
            out = " | ".join(x.get("value", str(x)) if isinstance(x, dict) else str(x) for x in c)
        else:
            out = str(c)
    print(f"\n== HOVER {label} @{pos['line']+1}:{pos['character']+1}")
    print((out or "  (no hover)")[:600])


def show_completion(uri, pos, label, filt=None, trigger=None):
    params = {"textDocument": {"uri": uri}, "position": pos}
    if trigger:
        params["context"] = {"triggerKind": 2, "triggerCharacter": trigger}
    else:
        params["context"] = {"triggerKind": 1}
    rid = send("textDocument/completion", params)
    r = wait_response(rid)
    items = []
    if r and r.get("result"):
        res = r["result"]
        items = res.get("items", res) if isinstance(res, (dict, list)) else []
    labels = [i.get("label", "?") for i in items]
    if filt:
        labels = [l for l in labels if filt(l)]
    print(f"\n== COMPLETION {label} @{pos['line']+1}:{pos['character']+1}: {len(items)} items")
    print("  " + ", ".join(labels[:25]) if labels else "  (none matching)")


def show_definition(uri, pos, label):
    rid = send("textDocument/definition", {"textDocument": {"uri": uri}, "position": pos})
    r = wait_response(rid)
    locs = r.get("result") if r else None
    print(f"\n== DEFINITION {label} @{pos['line']+1}:{pos['character']+1}")
    if not locs:
        print("  (no definition)")
        return
    if isinstance(locs, dict):
        locs = [locs]
    for loc in locs[:5]:
        u = loc.get("uri") or loc.get("targetUri")
        rng = loc.get("range") or loc.get("targetRange")
        print(f"  -> {u.replace(ROOT_URI, '')} : line {rng['start']['line']+1}")


# ---- welcome.blade.php with probe lines appended -------------------------
extra = "\n<x-\n@include('\n@\n{{ $definitelyUndefinedVar }}\n{{ $user-> }}\n"
orig = open(os.path.join(ROOT, "resources/views/welcome.blade.php")).read()
welcome_text = orig + extra
welcome_uri, _ = open_doc("resources/views/welcome.blade.php", welcome_text)
alert_uri, alert_text = open_doc("resources/views/components/alert.blade.php")
bakery_uri, bakery_text = open_doc("resources/views/bakeries/index.blade.php")
drain(15)

t = welcome_text
show_completion(welcome_uri, pos_of(t, "\n<x-\n", 4), "after '<x-' (component-ish only)",
                filt=lambda l: "::" not in l)
show_definition(welcome_uri, pos_of(t, "<x-post-summary", 5), "tag <x-post-summary> (class-backed)")
show_definition(welcome_uri, pos_of(t, "<x-card :bakery", 4), "tag <x-card> (index component)")
show_completion(welcome_uri, pos_of(t, "<x-post-summary :post=", 17), "attributes inside <x-post-summary ",
                filt=lambda l: len(l) < 20)

# section-name completion in the child template
admin_path = "resources/views/admin/users/index.blade.php"
admin_orig = open(os.path.join(ROOT, admin_path)).read()
admin_uri, admin_text = open_doc(admin_path)
show_completion(admin_uri, pos_of(admin_text, "@section('content')", 10), "@section('| name")
show_definition(admin_uri, pos_of(admin_text, "@section('content')", 12), "@section('content') -> @yield?")

# does it analyse in-memory text? inject an undefined var + bogus member mid-file
mutated = alert_text.replace(
    "{{ $slot }}", "{{ $slot }} {{ $definitelyUndefinedXyz }} {{ $slot->noSuchMethodXyz() }}")
send("textDocument/didChange", {
    "textDocument": {"uri": alert_uri, "version": 2},
    "contentChanges": [{"text": mutated}],
}, is_request=False)

drain(10)
print("\n== DIAGNOSTICS (published)")
for uri, diags in diagnostics.items():
    short = uri.replace(ROOT_URI, "")
    if not diags:
        continue
    print(f"  {short}: {len(diags)}")
    for d in diags[:8]:
        rng = d["range"]["start"]
        print(f"    [{d.get('severity')}] {rng['line']+1}:{rng['character']+1} {d.get('message','')[:110]}")
if not any(diagnostics.values()):
    print("  (none published on any file)")

proc.terminate()
