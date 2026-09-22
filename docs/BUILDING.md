# Building & Development

## Quick Start

```bash
cargo build --release   # build the binary
```

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)

## Build

The `build.rs` script automatically fetches the latest [JetBrains phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs) from GitHub and embeds them directly into the binary. This gives the LSP full knowledge of built-in PHP classes, functions, and constants with no runtime dependencies.

The stubs are downloaded on first build and cached in `stubs/`. To update to the latest stubs, delete the `stubs/` directory and rebuild.

For details on how symbol resolution and stub loading work, see [ARCHITECTURE.md](ARCHITECTURE.md).

### Optional features

Both are off by default, so an editor build compiles neither.

`offline-stubs` keeps the build script from reaching the network: when
the stubs are missing or stale it embeds an empty stub index and
continues instead of downloading them. Stubs already present in
`stubs/` are still embedded, so a machine that has built once keeps its
standard library. A build without them resolves only the project's own
code, so use it when the build must not make network calls rather than
to save time.

`semantic-export` compiles `phpantom_lsp::semantic_export`, an API for
programs that embed PHPantom as a library rather than talking to it
over LSP. The caller supplies PHP documents as strings, and gets back
declarations, occurrences, calls, byte ranges, and diagnostics as owned
values that outlive the backend they came from. Nothing is read from
disk and no server is started.

```bash
cargo build --features semantic-export,offline-stubs
```

### Matching the released binary

The Linux binaries we publish are static musl builds using mimalloc as
the allocator (see the comment on the `mimalloc` dependency in
`Cargo.toml`). A plain `cargo build --release` on a glibc workstation
already uses the same allocator — mimalloc is enabled for Linux, not
just musl — so day-to-day performance and memory testing on any Linux
dev machine is representative without extra setup.

To build the exact target we ship (e.g. to reproduce a memory or
performance report bit-for-bit), build for musl:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt install musl-tools   # provides musl-gcc; adjust for your distro
CARGO_BUILD_TARGET=x86_64-unknown-linux-musl cargo build --release
```

This produces `target/x86_64-unknown-linux-musl/release/phpantom_lsp`
alongside your normal build (it doesn't touch `target/release/`), so it
doesn't disturb incremental builds of your everyday work.

If you're benchmarking or profiling memory and get numbers that look
off from what's documented in `docs/todo/performance.md`, check which
allocator built the binary before assuming a regression — macOS and
Windows use the system allocator in both CI and locally already, so
they need no special steps.

### Building for the browser

PHPantom also compiles to WebAssembly, which is how the PHPStan playground
runs it client-side. See [wasm.md](wasm.md) for the build command, the host
interface, and what the wasm build leaves out.

## Testing

Run the full test suite:

```bash
cargo test
```

### CI Checks

Before submitting changes, run exactly what CI runs:

```bash
cargo test
cargo clippy -- -D warnings
cargo clippy --tests -- -D warnings
cargo clippy --all-targets --features semantic-export -- -D warnings
cargo test --features semantic-export --lib semantic_export
cargo fmt --check
find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l
php -d zend.assertions=1 examples/php/scaffolding/assertions.php
php -l examples/laravel/app/Demo.php
phpantom_lsp analyze --project-root examples/laravel --no-colour
```

CI additionally builds the WebAssembly target and runs
`scripts/wasm-smoke-test.mjs` against it, which is worth running locally
if your change touches dependencies, `Cargo.toml`, or anything in the
per-file request path. See [wasm.md](wasm.md).

All ten must pass with zero warnings and zero failures, except the
final `analyze` run: `app/Demo.php` carries three deliberate mistakes,
each demonstrating a diagnostic. `Artisan::call('does:not-exist')`
demonstrates `invalid_laravel_command`, and one `view('welcome', …)`
call both leaves out a variable the template declares and passes a
misspelled key, demonstrating `missing_view_variable` and
`unused_view_variable`. So it must report exactly
`[ERROR] Found 3 errors`, on those two lines. Any other count, or an
error on a different line, is a regression.

### Manual LSP Testing

The included `test_lsp.sh` script sends JSON-RPC messages to the server over stdin/stdout, exercising the full LSP protocol flow (initialize, open file, hover, completion, shutdown):

```bash
./test_lsp.sh
```

This is useful for verifying end-to-end behavior outside of an editor.

## Debugging

Enable logging by setting the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug cargo run 2>phpantom.log
```

Logs are written to stderr, so redirect as needed.

For editor setup instructions, see [Editor Setup](editor-setup.md).
