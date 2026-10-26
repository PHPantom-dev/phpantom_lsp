# Running PHPantom in the browser (WebAssembly)

PHPantom compiles to WebAssembly, so the same type engine that powers the
native language server can run inside a web editor with no server round-trip.
This is how the [PHPStan playground](https://phpstan.org/try) gets completion,
hover, go-to-definition, symbol highlighting and rename.

## Prebuilt module

Every release has a `phpantom_lsp-wasm32-wasip1.tar.gz` asset containing the
`phpantom_lsp.wasm` module, built and smoke-tested by CI, so a host can pin a
released version instead of building its own. Build it yourself if you want to
run against unreleased changes.

## Build

```bash
rustup target add wasm32-wasip1

cargo rustc --lib --crate-type cdylib \
    --profile wasm-release --target wasm32-wasip1
```

The module lands in `target/wasm32-wasip1/wasm-release/phpantom_lsp.wasm`. It is
around 15 MiB raw and 3 MiB gzipped, most of which is the embedded
phpstorm-stubs; compressing with Brotli instead gets it appreciably smaller
still.

Two things about that command are deliberate:

- **`--crate-type cdylib` on the command line** rather than a `crate-type`
  entry in `Cargo.toml`. Declaring `cdylib` in the manifest would make every
  native `cargo build` and `cargo test` link an extra shared library that
  nothing uses.
- **`--profile wasm-release`** rather than `--profile release`. It inherits
  from `release` but optimizes for size (`opt-level = "s"`), which is the right
  trade in a browser.

## Target choice: WASI, not `wasm32-unknown-unknown`

The build targets `wasm32-wasip1`, so the standard library's filesystem, clock
and path APIs are all present, since `file://` URIs and paths are used in dozens of places, also WASI
`Url::to_file_path` and friends work unchanged. On `wasm32-unknown-unknown` the
`url` crate omits those methods entirely, and every call site needs shimming.

## Host interface

The module is a WASI **reactor**: instantiate it once, call `_initialize`, then
push LSP messages through it for the lifetime of the editor session. It exports
four functions:

| Export | Purpose |
| --- | --- |
| `lsp_alloc(len) -> ptr` | Allocate `len` bytes of linear memory for the host to write a request into. |
| `lsp_handle(ptr, len) -> ptr` | Handle one LSP JSON-RPC message. Returns the response buffer, or null for a notification. |
| `lsp_response_len() -> len` | Length of the buffer the last `lsp_handle` returned. |
| `lsp_dealloc(ptr, len)` | Free a buffer obtained from `lsp_alloc` or `lsp_handle`. |

The response length is a separate call so `lsp_handle` can return a plain `u32`
pointer; packing pointer and length into a `u64` would surface as a `BigInt` in
JavaScript.

Messages are ordinary LSP JSON-RPC, which means a browser LSP client such as
[`@codemirror/lsp-client`](https://www.npmjs.com/package/@codemirror/lsp-client)
can be pointed at it through a thin transport. `initialize` advertises what the
dispatcher in `src/lsp_dispatch.rs` actually routes: completion (with
`completionItem/resolve`), hover, definition, document highlight, rename and
signature help, with full-text document sync.

In the browser, supply the WASI imports with a shim such as
[`@bjorn3/browser_wasi_shim`](https://www.npmjs.com/package/@bjorn3/browser_wasi_shim).
Nothing in the LSP path needs a real filesystem: the stubs are embedded in the
module and open documents are held in memory.

A host loop looks like this:

```js
const payload = new TextEncoder().encode(JSON.stringify(message));
const inPtr = exports.lsp_alloc(payload.length);
new Uint8Array(exports.memory.buffer, inPtr, payload.length).set(payload);

const outPtr = exports.lsp_handle(inPtr, payload.length);
exports.lsp_dealloc(inPtr, payload.length);

if (outPtr !== 0) {
    const len = exports.lsp_response_len();
    // Copy before the next wasm call: growing linear memory detaches this view.
    const bytes = new Uint8Array(exports.memory.buffer, outPtr, len).slice();
    exports.lsp_dealloc(outPtr, len);
    response = JSON.parse(new TextDecoder().decode(bytes));
}
```

## Verifying a build

`scripts/wasm-smoke-test.mjs` drives the module under Node's built-in WASI the
same way a browser host would, and asserts that completion, hover, definition,
highlight and signature help all return real results:

```bash
node scripts/wasm-smoke-test.mjs
```

It takes the path to the `.wasm` as an optional argument and defaults to the
`wasm-release` build above.

## What the wasm build leaves out

The wasm module is the *per-file* language-server path: parse a buffer, index
it, answer requests about it. Whole-project features are compiled out or simply
absent, because they depend on things wasm does not have:

- **The stdio and TCP transports.** The host calls `lsp_handle` directly, so
  `tower-lsp-server`'s transport (and the `tokio` `net` feature, which pulls in `mio`)
  is not built. `tokio` is declared per-target in `Cargo.toml` for this reason.
- **Parallel project indexing.** The `analyze` and `fix` batch paths spawn OS
  worker threads. That is project indexing, not what an editor needs per
  keystroke.

Diagnostics are not routed by the dispatcher either. `Backend::collect_slow_diagnostics` is synchronous and does work
in wasm, so wiring `textDocument/publishDiagnostics` is a matter of adding a
route in `src/lsp_dispatch.rs` if a host wants it.
