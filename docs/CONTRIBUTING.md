# Contributing to PHPantom

Thanks for your interest in contributing!

## Getting Started

1. Fork and clone the repository
2. Follow the [build instructions](BUILDING.md) to get a working development environment
3. Read [ARCHITECTURE.md](ARCHITECTURE.md) for an overview of how the codebase is structured

## Before Submitting a PR

All CI checks must pass with zero warnings and zero failures:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l
php -d zend.assertions=1 examples/php/scaffolding/assertions.php
php -l examples/laravel/app/Demo.php
find examples/symfony/src examples/symfony/config -name '*.php' -print0 | xargs -0 -n1 php -l
php examples/laravel/assertions.php
php examples/symfony/assertions.php
phpantom_lsp analyze --project-root examples/laravel --no-colour
phpantom_lsp analyze --project-root examples/symfony --no-colour
```

Note that clippy runs twice, once for library code and once including test code. The `php -l` checks keep the PHP and framework playgrounds valid. The `php -d zend.assertions=1` run executes `assertions.php`'s `runDemoAssertions()` to verify that `scaffolding/scaffolding.php`'s stubs actually return what their docblocks claim. Each framework playground has an `assertions.php` of its own that checks the demo against the real framework: the Laravel one boots Eloquent on an in-memory database, and the Symfony one loads the routes, services, translations, templates, forms, and validation mappings with Symfony's own loaders. The framework `assertions.php` and `phpantom_lsp analyze` runs need `composer install` in each playground first; the `analyze` runs check the playgrounds for diagnostic regressions. `examples/symfony/` must report `[OK] No errors`. In `examples/laravel/`, `app/Demo.php` carries three deliberate mistakes: `Artisan::call('does:not-exist')` demonstrates `invalid_laravel_command`, and one `view('welcome', …)` call both leaves out a variable the template declares and passes a misspelled key, demonstrating `missing_view_variable` and `unused_view_variable`. So the analyze run must report exactly `[ERROR] Found 3 errors` on those two lines, not `[OK] No errors`; any other count, or an error on a different line, is a regression.

## Code Style

- Run `cargo fmt` before committing
- Fix clippy warnings rather than suppressing them. Avoid `#[allow(clippy::...)]` unless truly necessary.
- Add `///` doc comments to all public functions and struct fields

## Testing

CI runs the test suite with coverage across six parallel Nextest shards.
The `Test` check passes only after every shard succeeds and their coverage
reports are uploaded together to Codecov. To reproduce one shard locally
(replace `1` with the shard number):

```bash
cargo llvm-cov nextest --partition hash:1/6 --lcov --output-path lcov.info
```

PR latency benchmarks build once, then distribute the discovered benchmark
cases across four runners. Each runner measures its cases sequentially with
Criterion's usual sampling settings. The `Benchmark (PR comparison)` check
compares the combined results only after every shard passes and every case
is accounted for. To reproduce a shard locally from the repository root:

```bash
cargo bench --bench completion --bench references --bench laravel_completion \
  --bench custom_builder --bench symfony_completion \
  --no-run --message-format=json > /tmp/benchmark-build.jsonl
python3 benches/ci_benchmarks.py prepare --build-dir /tmp/phpantom-benchmarks \
  --shards 4 < /tmp/benchmark-build.jsonl
python3 benches/ci_benchmarks.py run --build-dir /tmp/phpantom-benchmarks \
  --shard 1 --output /tmp/shard-1.json
```

CI uses static musl binaries; these commands use your local Cargo target.
Run the sharding helper's tests with
`python3 -m unittest discover -s benches -p 'test_ci_benchmarks.py'`.

Latency and memory benchmark comparisons appear in the job summaries.
A regression over 30% fails the comparison check. Alerts also leave a PR
comment for same-repository PRs, except Dependabot runs; fork and Dependabot
PRs use the summaries so they do not need permission to post a review.

- Integration tests go in `tests/integration/`, one file per feature area (`completion_*.rs`, `definition_*.rs`, `code_action_*.rs`, ...)
- Use `create_test_backend()` from `tests/integration/common/mod.rs` for same-file tests
- Use `create_psr4_workspace()` for cross-file / PSR-4 tests
- A helper (a request wrapper, a label extractor, a position finder) that three or more test files would define belongs in `tests/integration/common/mod.rs`, not copied into each file; check there before writing one
- Inline `#[cfg(test)]` modules in `src/` are for unit tests of private helpers; a test that drives a `Backend` through a public handler goes in `tests/integration/`
- Test the happy path, edge cases, and interactions with existing features
- When adding a feature, update the matching demo file under `examples/php/` with working examples (and verify with `find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l`). For Laravel-specific features, also update `examples/laravel/app/Demo.php` (and verify with `php -l examples/laravel/app/Demo.php`). For Symfony-specific features, update the matching file under `examples/symfony/` and add a "Try:" line to its `README.md`. In both, add a check to the playground's `assertions.php` for every new name or claim the demo makes. Put other framework-specific examples in the matching `examples/<framework>/` project and lint its PHP files.

See [BUILDING.md](BUILDING.md) for more on running tests and manual LSP testing.

## Changelog

Update [CHANGELOG.md](CHANGELOG.md) when your PR adds, changes, or fixes something a user would notice. Add entries under `## [Unreleased]` in the appropriate subsection (`### Added`, `### Fixed`, `### Changed`, or `### Removed`). Write for end users, not developers: describe what changed in the editor, not which internal modules were touched. Contributions users never see, such as CI or tooling work, go under `### Internal` so they are still credited.

Keep entries short and easy to scan:

- Start with a bolded one-line summary, then one or two plain sentences (three at most for a large feature). For a feature, say what the user gets and how to use it. For a fix, say what went wrong from the user's point of view and what happens now.
- Leave out how the code worked before or why the bug happened. That belongs in the commit message.
- Keep the details a user acts on: config keys, CLI flags, and a short code example when it makes the change concrete. One headline number is enough for a performance entry.
- If a section groups its entries under `####` headings (Laravel, Blade templates, Type inference, Diagnostics, Performance and memory, and so on), put your entry under the matching one. Related fixes can share one entry.
- End with `Contributed by @username` if you are not the maintainer. Don't reference the issue a change fixes.

Example:

```markdown
- **`analyze` takes more than one path.** `phpantom_lsp analyze app/ lib/Helper.php tests/` checks everything named, so a pre-commit hook or CI step can pass only the changed files.
```

## Documentation

The documentation site is built with [Zensical](https://zensical.org/).
The only dependency you need is [uv](https://docs.astral.sh/uv/getting-started/installation/).

Preview the docs locally with live reload:

```bash
uv run zensical serve
```

Then open `http://127.0.0.1:8000` in your browser. Changes to files in
`docs/` and `zensical.toml` are reflected immediately. If port 8000 is
already in use, pick a different one:

```bash
uv run zensical serve -a 127.0.0.1:8200
```

Build the docs for offline use:

```bash
uv run zensical build
```

The output is written to `site/` (gitignored). `uv` handles creating
a virtual environment and installing all Python dependencies
automatically on first run.

## Reporting Issues

Open an issue on GitHub with:

- What you expected to happen
- What actually happened
- Steps to reproduce (a minimal PHP snippet is ideal)
