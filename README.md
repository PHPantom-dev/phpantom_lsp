# PHPantom

<p align="center">
  <img src="docs/assets/spookaphant.svg" alt="Spookaphant" width="316" height="230" />
</p>

[![CI](https://img.shields.io/github/actions/workflow/status/AJenbo/phpantom_lsp/ci.yml?logo=github&label=CI)](https://github.com/AJenbo/phpantom_lsp/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/AJenbo/phpantom_lsp/graph/badge.svg?token=UH5RFA3AR9)](https://codecov.io/gh/AJenbo/phpantom_lsp)
[![crates](https://img.shields.io/crates/v/phpantom_lsp?logo=rust)](https://crates.io/crates/phpantom_lsp)

A fast, lightweight PHP language server written in Rust. Ready in seconds, uses a fraction of the RAM other language servers need, and stays responsive throughout. No indexing phase, no waiting.

> [!NOTE]
> PHPantom is in active development. The core editing features are solid and used daily on production codebases.

## Features

PHPantom focuses on deep type intelligence. Here's how it compares:

|                                 | PHPantom | Intelephense | PHP Tools  | Phpactor    | PHPStorm    |
| ------------------------------- | -------- | ------------ | ---------- | ----------- | ----------- |
| Common LSP features<sup>1</sup> | ✅       | ✅           | ✅         | ✅          | ✅          |
| Workspace symbols               | 🚧       | ✅           | ✅         | ✅          | ✅          |
| Semantic tokens                 | ✅       | ❌           | ✅         | ❌          | ✅          |
| Linked editing                  | ✅       | ❌           | ✅         | ❌          | ✅          |
| Extras<sup>2</sup>              | ✅       | 💰           | 🚧         | 🚧          | ✅          |
| **Diagnostics**                 |          |              |            |             |             |
| PHPStan integration             | ✅       | ❌           | ❌         | 🚧          | 🚧          |
| Undefined variable              | ✅       | 💰           | ✅         | ✅          | ✅          |
| Type errors                     | ✅       | 💰           | ✅         | 🚧          | ✅          |
| Unused variable                 | ✅       | ❌           | ✅         | ❌          | ✅          |
| **Type Intelligence**           |          |              |            |             |             |
| Generics / `@template`          | ✅       | 🚧           | ✅         | 🚧          | ✅          |
| `@mixin` completion             | ✅       | 💰           | ✅         | ✅          | 🚧          |
| Array / object shapes           | ✅       | ✅           | ✅         | 🚧          | 🚧          |
| PHPStan types                   | ✅       | ❌           | 🚧         | 🚧          | 🚧          |
| Conditional return types        | ✅       | ❌           | ✅         | 🚧          | ❌          |
| Closure parameter inference     | ✅       | 🚧           | 🚧         | 🚧          | ❌          |
| Laravel                         | ✅       | ❌           | 🚧         | ❌          | 🧩          |
| **Refactoring**                 |          |              |            |             |             |
| Rename                          | ✅       | 💰           | 💰         | ✅          | ✅          |
| Common refactorings<sup>3</sup> | ✅       | ❌           | 💰         | ✅          | ✅          |
| Extract constant                | ✅       | ❌           | ❌         | ✅          | ✅          |
| Promote constructor parameter   | ✅       | ❌           | ❌         | ❌          | ✅          |
| Simplify expressions            | ✅       | ❌           | 💰         | ❌          | ✅          |
| **Performance**                 |          |              |            |             |             |
| Time to ready                   | < 1 s    | 1 min 25 s   | 3 min 17 s | 15 min 39 s | 19 min 38 s |
| RAM usage                       | 59 MB    | 520 MB       | 3.9 GB     | 498 MB      | 2.0 GB      |
| Disk cache                      | 0        | 45 MB        | 0          | 4.1 GB      | 551 MB      |

<sub>
🚧 = partial support. 🧩 = requires plugin. 💰 = paid tier.<br>
<sup>1</sup> Completion, hover, signature help, go-to-definition, find references, diagnostics, document symbols.<br>
<sup>2</sup> Auto-import, go-to implementation / type-definition, smart select, folding ranges, formatting, code lens, inlay hints, type hierarchy, document links.<br>
<sup>3</sup> Implement interface methods, extract method/function, extract/inline variable, generate constructor, generate getter/setter.<br>
Performance measured on a production codebase: 21K PHP files, 1.5M lines of code (vendor + application).
</sub>

> **Want to verify?** Open [`examples/demo.php`](examples/demo.php) in your editor and trigger completion at the marked locations. It exercises every type intelligence feature in the table, including edge cases where tools diverge. For Laravel specifically, open [`examples/laravel/`](examples/laravel/) — a standalone project with real Eloquent models, config, routes, views, and translations that exercises Eloquent property resolution, query builder chaining, scopes, custom collections, and go-to-definition for config keys, route names, and translation strings.

## Context-Aware Intelligence

- **Smart PHPDoc completion.** `@throws` detects uncaught exceptions in the method body, `@param` pre-fills from the signature, and tags are filtered to context and never suggested twice.
- **Array shape inference.** Literal arrays offer key completion with no annotation. Nested shapes, spreads, and array functions like `array_map` preserve element types.
- **Closure parameter inference.** `$users->map(fn($u) => $u->name)` infers `$u` as `User` from the collection's generic context.
- **Conditional return types.** PHPStan-style conditional `@return` types resolve to the concrete branch at each call site.
- **Type aliases and shapes.** `@phpstan-type`, `@phpstan-import-type`, and `object{...}` shapes all resolve through to completions.
- **Laravel.** Eloquent relationships, scopes, accessors, casts, and Builder chains resolve end-to-end. Blade templates get completion, hover, go-to-definition, and diagnostics through virtual PHP preprocessing. No Larastan, no ide-helper, no database access required.
- **Everything else you'd expect.** Generics, type narrowing, named arguments, destructuring, first-class callables, anonymous classes, `@deprecated` detection, and namespace segment drilling.

## Project Awareness

PHPantom understands Composer projects out of the box, but works without setup on non-Composer projects too:

- **Autoloader-accurate results.** Completions and go-to-definition only surface classes that Composer's autoloader can actually load, avoiding false positives from internal, inaccessible, or duplicate vendor classes. You see exactly what your application can use.
- **PSR-4 autoloading.** Resolves classes across files on demand.
- **Classmap and file autoloading.** `autoload_classmap.php` and `autoload_files.php`.
- **Embedded PHP stubs** from [phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs) bundled in the binary, no runtime downloads needed.
- **Drupal project support.** Detects Drupal projects via `composer.json`, resolves the web root, and indexes Drupal-specific directories and PHP extensions (`.module`, `.install`, `.theme`, etc.) with `.gitignore` bypassed so that Composer-managed core and contrib code is always available.
- **`require_once` discovery.** Functions from required files are available for completion.
- **Go-to-implementation.** Jump from an interface or abstract class to all concrete implementations. Scans open files, classmap, PSR-4 directories, and embedded stubs.

## Documentation

- **[Installation](docs/SETUP.md).** Editor-specific setup for Zed, Neovim, PHPStorm, and others.
- **[CLI Reference](docs/CLI.md).** Batch diagnostics (`analyze`), automated fixes (`fix`), and CI integration.
- **[Building from Source](docs/BUILDING.md).** Build, test, and debug instructions.
- **[Architecture](docs/ARCHITECTURE.md).** Symbol resolution, stub loading, and inheritance merging.
- **[Contributing](docs/CONTRIBUTING.md)**
- **[Changelog](docs/CHANGELOG.md)**
- **Benchmarks.** [Latency](https://ajenbo.github.io/phpantom_lsp/dev/bench/) and [memory usage](https://ajenbo.github.io/phpantom_lsp/dev/memory/) tracked on every commit.
- **[Roadmap](docs/todo.md).** Planned features and domain-specific plans.

## Acknowledgements

PHPantom stands on the shoulders of:

- **[Mago](https://github.com/carthage-software/mago):** the PHP parser that powers all of PHPantom's AST analysis.
- **[PHPStan](https://phpstan.org/) and [Psalm](https://psalm.dev/):** for their pioneering work in PHP static analysis and shaping the type ecosystem. PHPantom builds on PHPStan-informed type experience, with full support for `@phpstan-*` annotations.
- **[JetBrains phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs):** type information for the entire PHP standard library, embedded directly into the binary.
- **[Phpactor](https://github.com/phpactor/phpactor):** the PHP language server whose comprehensive test suite and benchmark fixtures informed PHPantom's own test coverage. Many of PHPantom's type inference fixtures were adapted from Phpactor's reflection tests.

## License

MIT. See [LICENSE](LICENSE).
