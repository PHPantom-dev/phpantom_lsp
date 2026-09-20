# Contributing to PHPantom

Thanks for your interest in contributing!

## Getting Started

1. Fork and clone the repository
2. Follow the [build instructions](BUILDING.md) to get a working development environment
3. Read [ARCHITECTURE.md](ARCHITECTURE.md) for an overview of how the codebase is structured

## Before Submitting a PR

All six CI checks must pass with zero warnings and zero failures:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l
php -d zend.assertions=1 examples/php/scaffolding/assertions.php
php -l examples/laravel/app/Demo.php
phpantom_lsp analyze --project-root examples/laravel --no-colour
```

Note that clippy runs twice, once for library code and once including test code. The `php -l` checks ensure the `examples/php/` playground remains valid PHP. The `php -d zend.assertions=1` run executes `assertions.php`'s `runDemoAssertions()` to verify that `scaffolding/scaffolding.php`'s stubs actually return what their docblocks claim. The final `php -l` and `phpantom_lsp analyze` runs check `examples/laravel/` for syntax errors and diagnostic regressions. `app/Demo.php` carries three deliberate mistakes: `Artisan::call('does:not-exist')` demonstrates `invalid_laravel_command`, and one `view('welcome', …)` call both leaves out a variable the template declares and passes a misspelled key, demonstrating `missing_view_variable` and `unused_view_variable`. So the analyze run must report exactly `[ERROR] Found 3 errors` on those two lines, not `[OK] No errors`; any other count, or an error on a different line, is a regression.

## Code Style

- Run `cargo fmt` before committing
- Fix clippy warnings rather than suppressing them. Avoid `#[allow(clippy::...)]` unless truly necessary.
- Add `///` doc comments to all public functions and struct fields

## Testing

- Integration tests go in `tests/integration/`, one file per feature area (`completion_*.rs`, `definition_*.rs`, `code_action_*.rs`, ...)
- Use `create_test_backend()` from `tests/integration/common/mod.rs` for same-file tests
- Use `create_psr4_workspace()` for cross-file / PSR-4 tests
- A helper (a request wrapper, a label extractor, a position finder) that three or more test files would define belongs in `tests/integration/common/mod.rs`, not copied into each file; check there before writing one
- Inline `#[cfg(test)]` modules in `src/` are for unit tests of private helpers; a test that drives a `Backend` through a public handler goes in `tests/integration/`
- Test the happy path, edge cases, and interactions with existing features
- When adding a feature, update the matching demo file under `examples/php/` with working examples (and verify with `find examples/php -name '*.php' -print0 | xargs -0 -n1 php -l`). For Laravel-specific features, also update `examples/laravel/app/Demo.php` (and verify with `php -l examples/laravel/app/Demo.php`).

See [BUILDING.md](BUILDING.md) for more on running tests and manual LSP testing.

## Changelog

Update [CHANGELOG.md](CHANGELOG.md) when your PR adds, changes, or fixes something a user would notice. Add entries under `## [Unreleased]` in the appropriate subsection (`### Added`, `### Fixed`, `### Changed`, or `### Removed`). Write for end users, not developers: describe what changed in the editor, not which internal modules were touched. See the existing entries for the style and level of detail expected.

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
