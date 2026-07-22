#!/usr/bin/env python3
"""Measure PHPantom LSP resident memory (RSS) on two workloads.

Scenarios
---------
1. **hello_world** – open a single-file PHP project containing one SPL call.
2. **laravel_model** – open the default ``User`` model inside a full
   laravel/laravel checkout (with Composer dependencies installed).

The script communicates with the LSP binary over JSON-RPC / stdio,
waits for indexing to finish, sends a ``textDocument/didOpen``, lets the
server settle, then samples ``VmRSS`` from ``/proc/<pid>/status``.

Output is printed as ``customSmallerIsBetter`` JSON so
``github-action-benchmark`` can ingest it with proper units::

    [
      {"name": "memory_hello_world",  "unit": "MiB", "value": 24.5},
      {"name": "memory_laravel_model", "unit": "MiB", "value": 31.7}
    ]

Usage (CI)::

    python3 benches/memory_usage.py --binary target/release/phpantom_lsp

Usage (local, after ``cargo build --release``)::

    python3 benches/memory_usage.py
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path
from typing import Any

# ── Constants ────────────────────────────────────────────────────────────────

# Pinned laravel/laravel tag so the benchmark is reproducible.
# The composer.lock generated from this tag lives in
# benches/fixtures/laravel/composer.lock and is copied into the
# extracted source before ``composer install`` so dependency versions
# never drift.
LARAVEL_TAG = "v12.12.2"
LARAVEL_TARBALL = (
    "https://github.com/laravel/laravel/archive/refs/tags/"
    f"{LARAVEL_TAG}.tar.gz"
)

# Path to the pinned composer.lock (relative to this script).
LARAVEL_LOCK_FIXTURE = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "fixtures", "laravel", "composer.lock"
)

HELLO_WORLD_PHP = """\
<?php

$items = [1, 2, 3, 4, 5];
$doubled = array_map(fn(int $n): int => $n * 2, $items);
$filtered = array_filter($doubled, fn(int $n): bool => $n > 4);
echo implode(', ', $filtered) . "\\n";
"""

LARAVEL_CONTROLLER_PHP = r"""<?php

namespace App\Http\Controllers;

use App\Models\User;
use Illuminate\Contracts\View\View;

class TestController extends Controller
{
    public function helloWorld(User $user): View
    {
        return view('welcome', ['name' => $user->name]);
    }
}
"""

# Timeout for the LSP to finish indexing (seconds).
INDEX_TIMEOUT = 120

# How long to wait after didOpen for the server to settle (seconds).
SETTLE_TIME = 2

# Number of RSS samples to take and average.
RSS_SAMPLES = 5
RSS_SAMPLE_INTERVAL = 0.5  # seconds between samples

# ── JSON-RPC helpers ─────────────────────────────────────────────────────────


def _encode_message(obj: dict[str, Any]) -> bytes:
    """Encode a JSON-RPC message with Content-Length header."""
    body = json.dumps(obj).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    return header + body


def _read_headers(stream) -> dict[str, str]:
    """Read HTTP-style headers from *stream* until the blank line."""
    headers: dict[str, str] = {}
    while True:
        line = b""
        while not line.endswith(b"\r\n"):
            ch = stream.read(1)
            if not ch:
                raise EOFError("LSP process closed stdout")
            line += ch
        line_str = line.decode("ascii").strip()
        if not line_str:
            break
        key, _, value = line_str.partition(":")
        headers[key.strip().lower()] = value.strip()
    return headers


def _read_message(stream) -> dict[str, Any]:
    """Read one JSON-RPC message from the LSP stdout."""
    headers = _read_headers(stream)
    length = int(headers["content-length"])
    body = b""
    while len(body) < length:
        chunk = stream.read(length - len(body))
        if not chunk:
            raise EOFError("LSP process closed stdout")
        body += chunk
    return json.loads(body)


# ── LspClient ────────────────────────────────────────────────────────────────


class LspClient:
    """Minimal LSP client that talks to a subprocess over stdio.

    A background thread continuously reads messages from the server's
    stdout and dispatches them:

    * **Server-to-client requests** (``window/workDoneProgress/create``,
      ``client/registerCapability``) are answered immediately with an
      empty success response.
    * **Responses** (messages with an ``id`` matching a pending request)
      are routed to a per-request ``queue.Queue`` so the caller's
      ``wait_for_response`` can block on it.
    * **Notifications** (``$/progress``, ``window/logMessage``, etc.)
      are placed on a shared notification queue that ``wait_for_indexing``
      drains.

    This avoids the pitfalls of mixing ``select()`` with Python's
    buffered I/O on pipes.
    """

    # Server methods that are requests (have an ``id``) and expect a
    # response from the client.
    _SERVER_REQUESTS = frozenset({
        "window/workDoneProgress/create",
        "client/registerCapability",
        "workspace/configuration",
    })

    def __init__(self, binary: str, workspace_root: str):
        self._id = 0
        self._root = workspace_root
        self._write_lock = threading.Lock()
        self._pending: dict[int, queue.Queue] = {}
        self._pending_lock = threading.Lock()
        self._notifications: queue.Queue[dict[str, Any]] = queue.Queue()
        self._reader_error: Exception | None = None
        self._stopped = False

        self._proc = subprocess.Popen(
            [binary],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=workspace_root,
        )

        self._reader_thread = threading.Thread(
            target=self._reader_loop, daemon=True
        )
        self._reader_thread.start()

    # ── Background reader ────────────────────────────────────────────

    def _reader_loop(self) -> None:
        """Continuously read messages and dispatch them."""
        try:
            while not self._stopped:
                try:
                    msg = _read_message(self._proc.stdout)
                except EOFError:
                    break

                # Is this a response to one of our requests?
                if "id" in msg and "method" not in msg:
                    rid = msg["id"]
                    with self._pending_lock:
                        q = self._pending.get(rid)
                    if q is not None:
                        q.put(msg)
                    continue

                # Is this a server-to-client request that needs a reply?
                method = msg.get("method", "")
                if method in self._SERVER_REQUESTS and "id" in msg:
                    self._send_raw({
                        "jsonrpc": "2.0",
                        "id": msg["id"],
                        "result": None,
                    })
                    # Also forward as a notification so callers can
                    # observe it (e.g. progress token creation).
                    self._notifications.put(msg)
                    continue

                # Everything else is a notification.
                self._notifications.put(msg)
        except Exception as exc:
            self._reader_error = exc

    # ── Transport ────────────────────────────────────────────────────

    def _send_raw(self, obj: dict[str, Any]) -> None:
        with self._write_lock:
            self._proc.stdin.write(_encode_message(obj))
            self._proc.stdin.flush()

    @property
    def pid(self) -> int:
        return self._proc.pid

    def _next_id(self) -> int:
        self._id += 1
        return self._id

    def send_request(self, method: str, params: dict[str, Any]) -> int:
        """Send a JSON-RPC request. Returns the request id."""
        rid = self._next_id()
        q: queue.Queue[dict[str, Any]] = queue.Queue()
        with self._pending_lock:
            self._pending[rid] = q
        self._send_raw({
            "jsonrpc": "2.0",
            "id": rid,
            "method": method,
            "params": params,
        })
        return rid

    def send_notification(self, method: str, params: dict[str, Any]) -> None:
        """Send a JSON-RPC notification (no id, no response expected)."""
        self._send_raw({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        })

    def wait_for_response(
        self, request_id: int, timeout: float = 30
    ) -> dict[str, Any]:
        """Block until the response for *request_id* arrives."""
        with self._pending_lock:
            q = self._pending.get(request_id)
        if q is None:
            raise ValueError(f"Unknown request id {request_id}")
        try:
            msg = q.get(timeout=timeout)
        except queue.Empty:
            raise TimeoutError(
                f"No response for request {request_id} within {timeout}s"
            ) from None
        finally:
            with self._pending_lock:
                self._pending.pop(request_id, None)
        return msg

    # ── High-level protocol ──────────────────────────────────────────

    def initialize(self) -> None:
        """Send initialize + initialized."""
        root_uri = Path(self._root).as_uri()
        rid = self.send_request("initialize", {
            "processId": os.getpid(),
            "rootUri": root_uri,
            "capabilities": {
                "window": {"workDoneProgress": True},
            },
            "initializationOptions": {},
        })
        self.wait_for_response(rid, timeout=30)
        self.send_notification("initialized", {})

    def did_open(self, uri: str, text: str) -> None:
        """Send textDocument/didOpen."""
        self.send_notification("textDocument/didOpen", {
            "textDocument": {
                "uri": uri,
                "languageId": "php",
                "version": 1,
                "text": text,
            }
        })

    def hover(self, uri: str, line: int, character: int, timeout: float = 30) -> dict[str, Any]:
        """Send textDocument/hover and wait for the response.

        This forces the server to resolve types at the given position,
        triggering lazy loading of stubs, inheritance chains, etc.
        """
        rid = self.send_request("textDocument/hover", {
            "textDocument": {"uri": uri},
            "position": {"line": line, "character": character},
        })
        return self.wait_for_response(rid, timeout=timeout)

    def wait_for_indexing(self, timeout: float = INDEX_TIMEOUT) -> None:
        """Block until PHPantom finishes indexing.

        PHPantom sends ``$/progress`` with ``kind: "end"`` when indexing
        finishes.  We also accept a quiet period as a fallback for
        workspaces where indexing is trivial and no progress tokens are
        emitted.
        """
        deadline = time.monotonic() + timeout
        active_tokens: set[str] = set()
        saw_any_progress = False

        while time.monotonic() < deadline:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            try:
                msg = self._notifications.get(timeout=min(remaining, 2.0))
            except queue.Empty:
                # Silence. If we never saw progress, treat it as "done"
                # (trivial workspace). If all tokens ended, also done.
                if not saw_any_progress or not active_tokens:
                    return
                continue

            method = msg.get("method", "")

            if method == "$/progress":
                saw_any_progress = True
                token = str(msg["params"].get("token", ""))
                value = msg["params"].get("value", {})
                kind = value.get("kind", "")
                if kind == "begin":
                    active_tokens.add(token)
                elif kind == "end":
                    active_tokens.discard(token)
                    if not active_tokens:
                        return

        raise TimeoutError(f"Indexing did not finish within {timeout}s")

    def drain_notifications(self, seconds: float) -> None:
        """Read and discard notifications for *seconds*."""
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            try:
                self._notifications.get(timeout=min(remaining, 0.5))
            except queue.Empty:
                pass

    def shutdown(self) -> None:
        """Send shutdown + exit."""
        self._stopped = True
        rid = self.send_request("shutdown", {})
        try:
            self.wait_for_response(rid, timeout=10)
        except (TimeoutError, EOFError):
            pass
        try:
            self.send_notification("exit", {})
        except (BrokenPipeError, OSError):
            pass
        try:
            self._proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self._proc.kill()
            self._proc.wait()

    def __del__(self):
        try:
            if hasattr(self, "_proc") and self._proc.poll() is None:
                self._proc.kill()
                self._proc.wait()
        except Exception:
            pass


# ── Memory sampling ──────────────────────────────────────────────────────────


def get_rss_kib(pid: int) -> int | None:
    """Read VmRSS from /proc/<pid>/status. Returns KiB or None."""
    try:
        status = Path(f"/proc/{pid}/status").read_text()
        for line in status.splitlines():
            if line.startswith("VmRSS:"):
                # Format: "VmRSS:    12345 kB"
                parts = line.split()
                return int(parts[1])
    except (FileNotFoundError, ProcessLookupError, IndexError, ValueError):
        return None
    return None


def sample_rss(
    pid: int,
    samples: int = RSS_SAMPLES,
    interval: float = RSS_SAMPLE_INTERVAL,
) -> int:
    """Take multiple RSS samples and return the median (KiB)."""
    values: list[int] = []
    for _ in range(samples):
        rss = get_rss_kib(pid)
        if rss is not None:
            values.append(rss)
        time.sleep(interval)
    if not values:
        raise RuntimeError(f"Could not read RSS for PID {pid}")
    values.sort()
    return values[len(values) // 2]


# ── Scenario runners ─────────────────────────────────────────────────────────


def run_hello_world(binary: str, work_dir: str) -> int:
    """Measure RSS for a trivial single-file PHP project.

    Returns RSS in KiB.
    """
    project_dir = os.path.join(work_dir, "hello_project")
    os.makedirs(project_dir, exist_ok=True)

    php_file = os.path.join(project_dir, "hello.php")
    with open(php_file, "w") as f:
        f.write(HELLO_WORLD_PHP)

    client = LspClient(binary, project_dir)
    try:
        client.initialize()
        client.wait_for_indexing(timeout=30)

        file_uri = Path(php_file).as_uri()
        client.did_open(file_uri, HELLO_WORLD_PHP)

        # Hover over ``array_map`` (line 3, col 11) to force the server
        # to load stubs and resolve types before we measure memory.
        client.hover(file_uri, line=3, character=11)
        client.drain_notifications(SETTLE_TIME)

        rss = sample_rss(client.pid)
        return rss
    finally:
        client.shutdown()


def setup_laravel(work_dir: str) -> str:
    """Download laravel/laravel at the pinned tag and install Composer deps.

    Uses a GitHub tarball (no git required). A pinned ``composer.lock``
    (checked into the repo under ``benches/fixtures/laravel/``) is
    copied into the extracted source *before* ``composer install`` so
    the exact same dependency tree is resolved every time.

    Returns the path to the Laravel project root.
    """
    import tarfile
    import urllib.request

    tarball_path = os.path.join(work_dir, "laravel.tar.gz")

    # Download the tarball.
    print(f"  Downloading laravel/laravel {LARAVEL_TAG}...", file=sys.stderr)
    urllib.request.urlretrieve(LARAVEL_TARBALL, tarball_path)

    # Extract. GitHub tarballs contain a single top-level directory
    # named ``laravel-<tag-without-v>/``.
    print("  Extracting...", file=sys.stderr)
    with tarfile.open(tarball_path, "r:gz") as tar:
        tar.extractall(path=work_dir, filter="data")
    os.remove(tarball_path)

    # Find the extracted directory (e.g. ``laravel-12.12.2``).
    extracted = [
        d for d in os.listdir(work_dir)
        if os.path.isdir(os.path.join(work_dir, d)) and d.startswith("laravel-")
    ]
    if len(extracted) != 1:
        raise RuntimeError(
            f"Expected exactly one laravel-* directory, found: {extracted}"
        )
    laravel_dir = os.path.join(work_dir, extracted[0])

    # Copy our pinned composer.lock so ``composer install`` resolves
    # the exact same versions every run.
    lock_src = LARAVEL_LOCK_FIXTURE
    if not os.path.isfile(lock_src):
        raise FileNotFoundError(
            f"Pinned composer.lock not found at {lock_src}. "
            "Did you forget to check in benches/fixtures/laravel/composer.lock?"
        )
    shutil.copy2(lock_src, os.path.join(laravel_dir, "composer.lock"))

    # Install Composer dependencies from the lock file.
    print("  Running composer install...", file=sys.stderr)
    subprocess.run(
        [
            "composer", "install",
            "--no-interaction",
            "--no-progress",
            "--prefer-dist",
            "--quiet",
        ],
        check=True,
        cwd=laravel_dir,
        timeout=300,
    )

    return laravel_dir


def run_laravel_model(binary: str, work_dir: str) -> int:
    """Measure RSS after indexing laravel/laravel and hovering a model property.

    Opens a controller that uses ``$user->name``, then hovers on
    ``name``.  This forces the server to resolve the full Eloquent
    model hierarchy to discover the virtual property, which is a
    realistic workload that exercises lazy class loading.

    Returns RSS in KiB.
    """
    laravel_dir = setup_laravel(work_dir)

    # Create a controller that references the User model.
    ctrl_dir = os.path.join(laravel_dir, "app", "Http", "Controllers")
    os.makedirs(ctrl_dir, exist_ok=True)
    ctrl_path = os.path.join(ctrl_dir, "TestController.php")
    with open(ctrl_path, "w") as f:
        f.write(LARAVEL_CONTROLLER_PHP)

    client = LspClient(binary, laravel_dir)
    try:
        client.initialize()
        client.wait_for_indexing(timeout=INDEX_TIMEOUT)

        file_uri = Path(ctrl_path).as_uri()
        client.did_open(file_uri, LARAVEL_CONTROLLER_PHP)

        # Hover on ``$user->name`` (line 11, col 50) to force the
        # server to walk the full Eloquent parent chain and resolve
        # the virtual ``name`` property before we measure memory.
        client.hover(file_uri, line=11, character=50)
        client.drain_notifications(SETTLE_TIME)

        rss = sample_rss(client.pid)
        return rss
    finally:
        client.shutdown()


# ── Main ─────────────────────────────────────────────────────────────────────


def find_binary() -> str | None:
    """Try to find the phpantom_lsp binary."""
    # Check common locations.
    candidates = [
        "target/release/phpantom_lsp",
    ]
    for c in candidates:
        if os.path.isfile(c) and os.access(c, os.X_OK):
            return c

    # Check PATH.
    which = shutil.which("phpantom_lsp")
    if which:
        return which

    return None


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Measure PHPantom LSP memory usage (RSS).",
    )
    parser.add_argument(
        "--binary",
        default=None,
        help="Path to the phpantom_lsp binary. Auto-detected if omitted.",
    )
    parser.add_argument(
        "--scenario",
        choices=["hello_world", "laravel_model", "all"],
        default="all",
        help="Which scenario to run (default: all).",
    )
    parser.add_argument(
        "--work-dir",
        default=None,
        help=(
            "Working directory for temporary files. "
            "A temp dir is used if omitted."
        ),
    )

    args = parser.parse_args()

    binary = args.binary or find_binary()
    if not binary:
        print(
            "Error: could not find phpantom_lsp binary. Use --binary.",
            file=sys.stderr,
        )
        sys.exit(1)

    binary = os.path.abspath(binary)
    print(f"Using binary: {binary}", file=sys.stderr)

    # Verify the binary works.
    try:
        result = subprocess.run(
            [binary, "--version"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        version = result.stdout.strip() or result.stderr.strip()
        print(f"Version: {version}", file=sys.stderr)
    except Exception as e:
        print(f"Warning: could not get version: {e}", file=sys.stderr)

    use_temp = args.work_dir is None
    work_dir = args.work_dir or tempfile.mkdtemp(prefix="phpantom_mem_")
    os.makedirs(work_dir, exist_ok=True)

    results: dict[str, int] = {}

    try:
        if args.scenario in ("hello_world", "all"):
            print("Running hello_world scenario...", file=sys.stderr)
            rss = run_hello_world(binary, work_dir)
            results["memory_hello_world"] = rss
            print(
                f"  RSS: {rss} KiB ({rss / 1024:.1f} MiB)", file=sys.stderr
            )

        if args.scenario in ("laravel_model", "all"):
            print("Running laravel_model scenario...", file=sys.stderr)
            rss = run_laravel_model(binary, work_dir)
            results["memory_laravel_model"] = rss
            print(
                f"  RSS: {rss} KiB ({rss / 1024:.1f} MiB)", file=sys.stderr
            )

    finally:
        if use_temp:
            shutil.rmtree(work_dir, ignore_errors=True)

    # Output results as customSmallerIsBetter JSON.
    entries = [
        {"name": name, "unit": "MiB", "value": round(kib / 1024, 1)}
        for name, kib in sorted(results.items())
    ]
    json.dump(entries, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()