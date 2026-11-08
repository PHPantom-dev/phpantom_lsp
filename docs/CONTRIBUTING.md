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
find examples/yii/src examples/yii/config examples/yii/resources -name '*.php' -print0 | xargs -0 -n1 php -l
find examples/wordpress/wp-content -name '*.php' -print0 | xargs -0 -n1 php -l
find examples/drupal/web/modules/custom examples/drupal/web/themes/custom -type f \( -name '*.php' -o -name '*.module' -o -name '*.theme' \) -print0 | xargs -0 -n1 php -l
php examples/laravel/assertions.php
php examples/symfony/assertions.php
php examples/yii/assertions.php
php examples/wordpress/assertions.php
php examples/drupal/assertions.php
find examples/tempest/app examples/tempest/resources examples/tempest/ui -name '*.php' -print0 | xargs -0 -n1 php -l
php examples/tempest/assertions.php
phpantom_lsp analyze --project-root examples/laravel --no-colour
phpantom_lsp analyze --project-root examples/symfony --no-colour
phpantom_lsp analyze --project-root examples/yii --no-colour
phpantom_lsp analyze --project-root examples/wordpress --no-colour
phpantom_lsp analyze --project-root examples/drupal --no-colour
phpantom_lsp analyze --project-root examples/tempest --no-colour
```

Note that clippy runs twice, once for library code and once including test code. The `php -l` checks keep the PHP and framework playgrounds valid. The `php -d zend.assertions=1` run executes `assertions.php`'s `runDemoAssertions()` to verify that `scaffolding/scaffolding.php`'s stubs actually return what their docblocks claim. Each framework playground has an `assertions.php` of its own that checks the demo against the real framework: the Laravel one boots Eloquent on an in-memory database, the Symfony one loads the routes, services, translations, templates, forms, and validation mappings with Symfony's own loaders, the Yii one builds the configuration and DI container the way Yii's web runner does, the WordPress one installs WordPress into a throwaway SQLite database and loads the demo plugin and theme, the Tempest one renders its views and components with the framework renderer, and the Drupal one reads the demo's services, routes, entity type, hooks, permissions, configuration, theme hooks, render elements, and libraries with Drupal's own loaders and reflection. The framework `assertions.php` and `phpantom_lsp analyze` runs need `composer install` in each playground first; the `analyze` runs check the playgrounds for diagnostic regressions. `examples/symfony/`, `examples/yii/`, `examples/wordpress/`, `examples/drupal/` and `examples/tempest/` must report `[OK] No errors`. In `examples/laravel/`, `app/Demo.php` carries four deliberate mistakes: `Artisan::call('does:not-exist')` demonstrates `invalid_laravel_command`, `->withoutMiddleware('bakery.closed')` demonstrates `invalid_laravel_middleware`, and one `view('welcome', …)` call both leaves out a variable the template declares and passes a misspelled key, demonstrating `missing_blade_variable` and `unused_blade_variable`. So the analyze run must report exactly `[ERROR] Found 4 errors` on those three lines, not `[OK] No errors`; any other count, or an error on a different line, is a regression.

## Code Style

- Run `cargo fmt` before committing
- Fix clippy warnings rather than suppressing them. Avoid `#[allow(clippy::...)]` unless truly necessary.
- Add `///` doc comments to all public functions and struct fields

## Testing

CI builds the instrumented test suite once and runs its Nextest archive across
six parallel shards, without compiling again on those runners.
The `Test` check passes only after every shard succeeds and their coverage
reports are uploaded together to Codecov. To reproduce one shard locally
(replace `1` with the shard number):

```bash
cargo llvm-cov nextest-archive --locked --archive-file coverage-tests.tar.zst
cargo llvm-cov nextest --archive-file coverage-tests.tar.zst \
  --partition hash:1/6 --lcov --output-path lcov.info
```

Windows also builds once, then runs the full suite across four Nextest archive
partitions. The `Test (Windows)` check accounts for every partition's successful
result for the tested revision and retains its existing non-blocking policy.
An exact-commit archive cache also avoids compilation on reruns, keyed by the
compiler, runner image, target, and test profile. Every partition still runs.

The optional semantic-export feature has a separate integration test target,
so its checks do not compile the full default integration suite again:

```bash
cargo test --features semantic-export --lib --test semantic_export semantic_export
```

PR latency benchmarks build the base and PR revisions in parallel with the same
toolchain and settings, then distribute the cases across twelve runners. Each
runner runs both revisions sequentially with Criterion's usual sampling settings
and each revision's own fixtures. One shared assignment
keeps matching cases on the same runner even when a PR adds benchmarks.
The `Benchmark (PR comparison)` check compares the paired results only after
every shard passes and every case is accounted for. New cases are reported
without a baseline; removed cases are listed explicitly. Main-branch
measurements use the same sharding to populate the historical dashboard.
The head build also produces the release server used by the scaling and memory
checks, so those jobs do not rebuild it.

CI keeps every check on each PR, including Windows, coverage, WebAssembly,
and performance comparisons. Superseded PR runs are cancelled. Rust dependency
caches are separated by build type and can be saved in each PR's own cache
scope as well as on `main`; memory checks also cache Composer downloads.
Tests omit debug information to reduce disk usage and linking time, while
release optimization and Criterion sampling stay
unchanged.

Rust and runner families are pinned so updates do not silently replace the
build environment. Benchmark binaries are cached for an exact source revision,
compiler, target, native toolchain, build recipe, and release settings. A matching
base built on `main` is reused by later PRs; a missing match is compiled. Scaling
and memory checks depend only on the head build. Each benchmark runner pairs
the manifests locally, avoiding a separate preparation job.

The manual CI trigger accepts 8, 12, or 16 benchmark workers for controlled
comparisons. The comparison summary lists job queue and execution times and
release artifact cache hits; Cargo timing reports are retained with the build
artifacts. Measure cold runs, warm reruns, source changes, and concurrent PRs
separately. A 5–6 minute result remains a target until measured on GitHub with
the full set of checks.

Memory sampling waits for the opened file's native diagnostic passes before
the settling period, so a slow background pass does not leave a temporary
parse in the memory measurement. Manual profiling runs without a Codecov
token retain the coverage reports without uploading them; PR and push uploads
keep their existing behavior.

To reproduce a paired shard locally from the repository root, replace
`BASE_COMMIT` with the PR's base revision:

```bash
benchmark_root="$PWD"
git worktree add --detach /tmp/phpantom-bench-base BASE_COMMIT
cargo build --locked --release --manifest-path /tmp/phpantom-bench-base/Cargo.toml \
  --bench '*' --message-format=json > /tmp/baseline-build.jsonl
(
  cd /tmp/phpantom-bench-base
  python3 "$benchmark_root/benches/ci_benchmarks.py" prepare \
    --build-dir /tmp/phpantom-baseline --shards 1 < /tmp/baseline-build.jsonl
)
cargo build --locked --release --bench '*' --message-format=json > /tmp/benchmark-build.jsonl
python3 benches/ci_benchmarks.py prepare --build-dir /tmp/phpantom-benchmarks \
  --shards 12 --baseline-dir /tmp/phpantom-baseline < /tmp/benchmark-build.jsonl
python3 benches/ci_benchmarks.py run --build-dir /tmp/phpantom-benchmarks \
  --baseline-source /tmp/phpantom-bench-base --shard 1 --output /tmp/shard-1.json
```

CI uses static musl binaries; these commands use your local Cargo target.
Run the benchmark helpers' tests with
`python3 -m unittest discover -s benches -p 'test_*.py'`.

Latency and memory benchmark comparisons appear in the job summaries.
A regression over 30% fails the comparison check. Latency compares the raw
paired measurements and reports both timings and their ratio. Memory alerts
also leave a PR comment for same-repository PRs, except Dependabot runs;
fork and Dependabot PRs use the summaries.

- Integration tests go in `tests/integration/`, one file per feature area (`completion_*.rs`, `definition_*.rs`, `code_action_*.rs`, ...)
- Use `create_test_backend()` from `tests/integration/common/mod.rs` for same-file tests
- Use `create_psr4_workspace()` for cross-file / PSR-4 tests
- A helper (a request wrapper, a label extractor, a position finder) that three or more test files would define belongs in `tests/integration/common/mod.rs`, not copied into each file; check there before writing one
- Inline `#[cfg(test)]` modules in `src/` are for unit tests of private helpers; a test that drives a `Backend` through a public handler goes in `tests/integration/`
- Test the happy path, edge cases, and interactions with existing features
- When adding a feature, update the matching demo file under `examples/php/` with working examples (and verify with `find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l`). For Laravel-specific features, also update `examples/laravel/app/Demo.php` (and verify with `php -l examples/laravel/app/Demo.php`). For Symfony-specific features, update the matching file under `examples/symfony/` and add a "Try:" line to its `README.md`, and do the same under `examples/yii/` for Yii-specific ones and under `examples/wordpress/` for WordPress-specific ones. In each, add a check to the playground's `assertions.php` for every new name or claim the demo makes. Put other framework-specific examples in the matching `examples/<framework>/` project and lint its PHP files.

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

## Diagnostic messages

A diagnostic's message is the text users read most, in the editor and in CI logs, so every check writes it the same way. Where PHP itself reports the same mistake at runtime, use PHP's wording (`Cannot instantiate interface App\Shape`, `Undefined variable $total`).

- Write one sentence that starts with a capital letter and has no final period. PHP's short form also counts: an adjective, the kind, and the name (`Unused import App\Models\User`, `Unknown middleware 'auth.admin'`). Don't write a label and a colon (`Dead catch: …`), and don't start with a name, a `$`, or an `@`; lead with its kind instead (`Method …`, `Directive @if …`).
- Write a name the way PHP code spells it, without quotes: classes fully qualified and without a leading backslash (`App\Models\User`), functions with `()` (`App\helper()`), and members on their class whenever it is known: `User::save()`, `User::$name`, `User::MAX`. Variables and parameters keep their `$`.
- Call each kind of thing by its name: a constant is a constant, not a member. When two checks report the same fact, they use the same words.
- Put a value written in the code as a string in single quotes, the way PHP writes the literal: route, view, config, and translation keys, array keys, option names (`Unknown view 'emails.welcome'`). A token from a parser goes in single quotes too (`Unexpected token '}'`).
- Write types the way a docblock does, without quotes (`int`, `array<string, User>`, `?string`).
- Report a type mismatch as `… expects X, got Y`, with what was expected first: `Argument 1 ($id) expects int, got string`, `Property App\User::$name expects string, got null`, `Return expects int, got string`. Extra detail goes in parentheses at the end.
- Add a clause that says what to do only when there is one obvious fix, after a semicolon: `Readonly property App\Point::$x is never initialized; assign it in the constructor`.
- Say what is wrong in plain words rather than the vocabulary of the PHP RFCs (`Nothing in the try block throws App\NotFound`, not `Dead catch`). No em-dashes.
- Text quoted from the user's code, such as the reason in an `@deprecated` tag, follows a colon as written, without its final period. Messages from PHPStan, PHPCS, PHPMD, and Mago are passed on as the tool wrote them.

The [Diagnostics Reference](diagnostics.md) lists every identifier; a new check adds its row there.

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
