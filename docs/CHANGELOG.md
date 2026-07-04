# Changelog

All notable changes to PHPantom will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`update` command.** A new `phpantom_lsp update` subcommand downloads the latest release from GitHub and replaces the current binary. Supports `--check` (dry run, exit code 1 if update available) and `--no-confirm` (for CI). Handles `.tar.gz` (Unix) and `.zip` (Windows) archives across all 6 supported platforms. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/194.
- **`array_map` respects scalar callback return types.** `array_map(fn(Item $item): string => $item->id, $items)` now correctly infers `list<string>` instead of `list<Item>`. Previously, scalar return types like `string`, `int`, `bool` on the callback were ignored and the input array's element type was used instead. Fixes #147. (contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/195)
- **`array_map` infers callback return type from body.** When the callback has no explicit return type hint, the LSP now infers the return type by resolving the body expression. For example, `array_map(fn($item) => $item->id, $items)` where `$items` is `list<Item>` now correctly produces `list<string>` (from `Item::$id`'s type) instead of `list<Item>`. Works for both arrow functions and closures. (contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/195)
- **Static methods complete on instance access.** Member completion after `->` now offers a class's static methods alongside its instance methods, since PHP lets you call a static method through an instance (`$obj->make()`). Static properties remain excluded, as they are only reachable via `::`. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/174.
- **Array-callable navigation.** Method-name strings in array callables — `[Controller::class, 'method']` and `[$object, 'method']` — now resolve like a real member reference. This makes go-to-definition, find-references, and rename work on Laravel controller actions such as `Route::get('/', [IndexPageController::class, 'indexPage'])`.
- **Array-callable method completion.** Typing inside the method-name string of an array callable (`[Controller::class, '|']`) now offers method name completions from the resolved class, including inherited and trait methods. Works with `Class::class` constants, `$this`, and typed variables. (thanks @calebdw)
- **Convert arrow function to closure.** A new `refactor.rewrite` code action converts arrow functions to anonymous closures (`fn($x) => $x * 2` to `function($x) { return $x * 2; }`). Variables from the outer scope are automatically captured via a `use()` clause. Preserves `static` and return type hints. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/191.
- **`@phpstan-sealed` tag support.** The `@phpstan-sealed FooClass|BarClass` PHPDoc tag is now recognized. Class names in the tag are treated as type references, preventing false "unused import" diagnostics. Docblock completion also offers the tag. (contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/190)
- **Magic methods complete when implemented.** Magic methods declared on a class (`__invoke`, `__toString`, `__call`, and the rest) are now offered in member completion, so explicit calls like `$x->__invoke()` autocomplete and support go-to-definition. They are sorted below the regular methods so they never appear at the top of the list.
- **Staleness detection and auto-refresh.** The class index, function index, and constant index now stay fresh automatically. When PHP files are created or deleted outside the editor (e.g. `git checkout`, code generation), the indices update without a restart, and edits made outside the editor are reflected the next time the file is used. When `composer.json` or `composer.lock` changes (e.g. after `composer install`), vendor packages are rescanned automatically.
- **`#[ArrayShape]` attribute support.** Functions and methods annotated with `#[ArrayShape(["key" => "type", ...])]` (used by ~84 phpstorm-stubs entries) now produce array shape key completions, hover type info, and correct type resolution. Affects commonly used functions like `parse_url`, `stat`, `pathinfo`, `gc_status`, `getimagesize`, and `session_get_cookie_params`.
- **Convert to arrow function.** A new `refactor.rewrite` code action converts single-expression closures to arrow functions (`function($x) { return $x * 2; }` to `fn($x) => $x * 2`). The action is only offered when the conversion is safe: single return statement, no by-reference `use` captures, no `void`/`never` return type, and PHP >= 7.4.
- **Convert switch to match.** A new `refactor.rewrite` code action converts `switch` statements to `match` expressions when all arms are single-expression returns or assignments to the same variable. Handles fall-through cases (merged with commas), trailing `break` removal, and `throw` arms. Requires PHP >= 8.0.
- **Extract interface.** A new `refactor.extract` code action generates an interface from a concrete class. All public method signatures (excluding the constructor) are extracted into a new `{ClassName}Interface.php` file in the same directory, and the class is updated with `implements {ClassName}Interface`. Class-level and method-level `@template` tags are preserved when referenced by extracted methods.
- **`@template` on `@method` tags.** Virtual methods declared via `@method` PHPDoc tags can now define their own template parameters using the `<T of Bound>` syntax (e.g. `@method TVal get<TVal of mixed>(TVal $default)`). Template inference at call sites works the same as for real methods.
- **Laravel custom Eloquent builder support.** Models using the `#[UseEloquentBuilder]` attribute now have their custom builder's methods forwarded as static methods on the model. `query()`, `newQuery()`, and `newModelQuery()` return the custom builder type with correct generic model substitution. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/118.
- **Eloquent relation and column string completion.** Typing inside string arguments to `with()`, `load()`, `whereHas()`, and other Eloquent methods that accept relation names now offers relationship method names as completions, with dot-notation traversal for nested relations. Similarly, `where()`, `orderBy()`, `select()`, `pluck()`, and other column-accepting methods offer model column names (from `$casts`, `$fillable`, `@property` tags, timestamps, etc.).
- **`model-property<T>` pseudo-type recognition.** The Larastan `model-property<Model>` type no longer triggers "unknown class" diagnostics. It is treated as a string subtype.
- **`compact()` strings are linked to local variables.** A string argument to `compact('user')` is now treated as a reference to the matching local variable. Renaming the variable updates the string (and renaming from the string updates the variable and its other uses), find-references includes the string, and go-to-definition on the string jumps to the variable's assignment. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/159.

### Changed

- **Class lookups no longer fall back to a linear scan.** The O(n) fallback scan over all parsed files was dead code since the O(1) hash index is always populated first. Removing it eliminates a potential micro-stutter during initial indexing.

### Fixed

- **Literal type matching in argument diagnostics.** String, integer, and float literal arguments now match PHPDoc literal-union parameter types. For example, `orderBy('id', 'desc')` no longer produces a bogus error when the parameter is typed as `'asc'|'desc'`. Conversely, passing a provably wrong literal (e.g. `'invalid'` to `'asc'|'desc'`, or `'hello'` to `numeric-string`) is now correctly flagged. Fixes #180. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/191.
- **Type Hierarchy works in more clients.** The `textDocument/prepareTypeHierarchy` capability was registered without registration options, so some clients (notably Zed) did not reliably expose the Type Hierarchy action. The dynamic registration now carries proper `TypeHierarchyRegistrationOptions` with a PHP document selector, so those clients recognise that the feature applies to PHP files. Contributed by @sidux in https://github.com/PHPantom-dev/phpantom_lsp/pull/179.
- **Extract method generates correct code for more selections.** A variable that the selection reads before it first assigns (for example a parameter the extracted code both consults and updates) is now passed in as an argument as well as returned, instead of being left undefined inside the new method. And an early `return` whose value references a variable defined inside the selection is now kept inside the extracted method and propagated to the caller, instead of being copied to the call site where that variable does not exist.
- **The editor stays responsive during fast typing in large files.** Editors send a burst of requests on every keystroke (completion, a documentation lookup for each suggestion, diagnostics, code lens, semantic highlighting, and more). The server processed only a few at a time, so during continuous typing the burst backed up until the server stopped answering anything at all, including the completion the user was waiting on, and it only recovered after a restart. Now the burst is processed concurrently and every expensive request runs off the main loop: diagnostics (which re-analyze the whole file on each edit) compute in the background instead of on the request that asked for them, so a diagnostic pull returns immediately and never blocks the threads that deliver completion and hover, and repeated whole-file requests (semantic highlighting, code lens, the document outline, folding, document links) are collapsed so a fast typist's superseded requests no longer pile up and monopolize the CPU. Completion and other requests keep coming back while you type.
- **Typing in a large file no longer pegs the CPU and stalls completion.** Semantic highlighting recomputed every token's position by rescanning the file from the beginning, so a large file took many seconds at full CPU to highlight. Editors request highlighting on every keystroke, so this ran continuously while typing and starved completion, hover, and other requests until they appeared to hang. Highlighting a large file is now effectively instant, and the same speedup applies to the document outline and code folding, which used the same per-position rescan.
- **The first use of a global helper function no longer stalls.** Functions defined in Composer "files" autoload entries and guarded by `if (! function_exists(...))` (such as Laravel's `app()`, `session()`, and `route()`) were parsed on demand the first time one was used, which meant the first completion, hover, or go-to-definition involving such a helper blocked while the server parsed every autoload file in turn. These files are now parsed up front during indexing, so the first lookup is instant.
- **Framework global helpers loaded outside Composer autoload are now indexed.** Some frameworks ship their global function aliases in a `*_global.php` file that sits beside an autoloaded helper file but is pulled in by the framework's own bootstrap rather than Composer's `files` autoload, so it never appears in the autoload manifest. CakePHP is the canonical case: helpers like `__()`, `h()`, and `env()` live in such a sibling and were reported as unknown functions on every call. These sibling helper files are now indexed too, so the globals resolve. Contributed by @dereuromark in https://github.com/AJenbo/phpantom_lsp/pull/175.
- **Classes defined inside conditional blocks are now fully resolved.** A class declared inside an `if`/`else` version guard (the Doctrine `ServiceEntityRepository` pattern, where a base class is defined differently per ORM version) was previously discovered by name only, so its parent and `@extends` generics were dropped. Such classes now carry their full inheritance, so member completion, hover, go-to-definition, and generic type resolution work both on them and inside their own methods. When the same class name appears in more than one branch, the first declaration wins. Contributed by @MrSrsen in https://github.com/PHPantom-dev/phpantom_lsp/pull/154.
- **Editing a base class stays responsive in large projects.** Changing a class that many others extend used to invalidate the resolved-class cache by rescanning every cached class on each edit, which briefly stalled large projects with deep class hierarchies. Invalidation now touches only the classes that actually depend on the edited one.
- **Completion latency stays flat during sustained fast typing.** Concurrent requests resolving the same classes contended on a single lock guarding the resolved-class cache, so completion latency crept upward for as long as a typing burst continued. The cache now allows parallel reads, so the many lookups in flight at once no longer serialize behind one another, and when a request first loads a vendor class the work to record it in the shared index is prepared before the index is locked, so other requests no longer wait on it.
- **The server no longer freezes and stops responding.** Editors cancel in-flight requests constantly (every cursor move supersedes the previous hover and highlight), and a burst of cancellations, such as when the editor regains focus after being in the background, could wedge the server so that it went completely silent and had to be restarted. Cancelled requests are now handled cleanly.
- **Returning to a backgrounded editor stays responsive.** When an editor regains focus it re-reports every file in the workspace as changed in one large batch. Processing that batch could stall the server while it re-read thousands of files from disk. The batch is now handled off the main loop and skips files that were never loaded, so the editor stays responsive.
- **Inherited members no longer briefly flagged as unknown after opening a project.** A method or property inherited from a vendor base class (for example the base methods of a framework controller) could be reported as an unknown member right after a file opened, even though hover resolved it correctly, and the error went away when the file was closed and reopened. Such members now resolve as soon as indexing finishes.
- **Named arguments are matched to parameters by name.** Calls that pass arguments by name (`f(c: 3)`) are now bound to the parameters they actually target instead of by their position in the call. Conditional return types resolve correctly when the deciding argument is passed by name out of order, a "missing required argument" error is now reported when a named argument fills an optional parameter but leaves a required one unsupplied, and pass-by-reference type inference seeds the right variable.
- **Argument-count false positives.** Extra arguments to a class with no constructor are no longer flagged (PHP accepts them), and namespaced calls to overloaded built-ins written with a leading backslash (`\mt_rand()`) are no longer measured against the wrong minimum. Immediately invoking the callable returned by a function or method (`makeHandler($a, $b)($request)`) now checks the inner call's arguments against the returned callable's own signature instead of the outer call's, fixing both false argument-count errors and wrong inlay hint parameter names on the invocation. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/191.
- **`@var` annotations no longer leak between functions.** A `/** @var T $x */` annotation in one function used to suppress "undefined variable" warnings for that name everywhere in the file; it is now scoped to the function it appears in.
- **Type resolution through chained and untyped access.** Null-safe call chains such as `$a->b?->c()` resolve through the full receiver. Array access on a value of unknown type resolves to `mixed`, so `$x = $arr['key'] ?? 5` no longer produces spurious type errors. `foreach` element types resolve through interfaces that reach a known iterable several hops away. Nested array-shape narrowing (`$a["x"]["y"]`) no longer targets the wrong key.
- **`self` references inside class-level attributes resolve.** A `self::`, `static::`, or `parent::` reference inside an attribute attached to a class (for example `#[Route(name: self::ROUTE)]`) is now resolved against the class it decorates, so the referenced constant or member is no longer reported as unresolvable.
- **`@method` tags override inherited methods of the same name.** A `@method` annotation on a class now takes precedence over a method inherited from a more distant ancestor. The common repository pattern, where a base repository declares `@method Entity|null findOneBy(...)` while its vendor parent returns a generic `object`, now resolves to the concrete entity type, so members accessed on the result are no longer flagged as unverifiable.
- **`??=` keeps the resolved type.** After `$x ??= new Foo()`, the variable resolves to `Foo` (or the union of its existing non-null type and the assigned value), so property and method access on `$x` is no longer reported as unresolvable.
- **Generics with fewer arguments than parameters.** `@extends Collection<User>` against `Collection<TKey, TValue>` now binds `User` to the value parameter, so inherited element types resolve correctly.
- **Nullable generic return types resolve through inheritance.** A method whose native return hint is nullable (`object|null`) and whose docblock returns a template (`@return ?T`) now resolves to the bound type, so a repository's `find()` returns `Entity|null` instead of the bare `object|null`. Contributed by @MrSrsen in https://github.com/PHPantom-dev/phpantom_lsp/pull/152.
- **Conditional `is null` return types** resolve consistently regardless of how the call site is parsed, and an explicitly passed `null` now selects the null branch.
- **Go-to-definition, rename, and highlight accuracy.** References in `@see` tags to qualified names like `App\Foo::bar()` now land on the correct location, and renaming a property selects the whole `$name` instead of `$nam`.
- **`@phpstan-require-extends` and `@phpstan-require-implements` navigation.** Class and interface names in these trait constraint tags now support go-to-definition and hover, and an import used only by such a tag is no longer flagged as unused. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/172.
- **Renaming variables captured by nested closures and arrow functions.** Renaming or finding references to a variable used inside deeply nested arrow functions (`fn () => fn () => $var`) or closures with `use ($var)` now updates every occurrence, whether the rename is triggered on the declaration or from deep inside the nesting. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/145.
- **Variables inside dynamic property accesses are tracked.** A variable used as a dynamic property selector (`$message->{$attribute}`) now counts as a use, so it is no longer wrongly reported as unused, find-references includes it, and renaming the variable updates the selector along with its other occurrences. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/174.
- **Member rename stays scoped to the declaration it targets.** Renaming a method or property no longer touches same-named members on unrelated classes. A private method rename updates only that method and its real usages, calls on a receiver whose type cannot be resolved are left alone, renaming one implementation of an interface no longer renames sibling implementations, and renaming a child override stays on the child branch. Renaming a parent or interface declaration still updates the inherited overrides and implemented usages. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/160.
- **Find references on a constructor lists every call site.** Finding references to a `__construct` declaration now reports the `new ClassName(...)` instantiations, `#[ClassName(...)]` attribute usages, and explicit delegation calls written as `parent::__construct()`, `self::__construct()`, or `Class::__construct()`, including for subclasses that inherit the constructor (and excluding subclasses that override it). Attribute classes that are never written as `new` are now found. Contributed by @RemcoSmitsDev in https://github.com/AJenbo/phpantom_lsp/pull/155.
- **Positions on lines with multibyte characters.** Signature help, go-to-definition on virtual properties, named-argument completion, unused-import removal, and the `@phpstan-ignore` quickfix placed cursors and edits at the wrong column on lines containing multibyte characters; they now use the correct UTF-16 columns. Type strings containing `*` wildcards or variance annotations are also no longer mangled.
- **Unused-import hint location.** When two imports share a name prefix (`use App\Foo;` and `use App\FooBar;`), the "unused import" dimming now lands on the correct statement.
- **Document outline ranges.** Methods, properties, constants, and functions in the outline and breadcrumbs now report a range covering the whole declaration, with the name nested inside, as editors expect for folding and breadcrumb extent.
- **Stale vendor symbols after `composer update`.** Functions and constants removed from the vendor tree are now purged from the indexes, so completion and go-to-definition stop offering symbols that no longer exist.
- **Type hierarchy** locates the class name even when the `class` keyword and the name are on separate lines.
- **Edits on Windows (CRLF) files land correctly.** Rename, remove-unused-import, and the PHPStan return-type quickfix computed line offsets assuming single-byte line endings, so on files with `\r\n` terminators the edits drifted one byte per preceding line and could corrupt the file. Offsets now account for the real terminator.
- **Malformed `@method` tags no longer crash requests.** A docblock with a degenerate `@method` signature (such as `@method >()`) could panic completion, hover, and go-to-definition. Such tags are now parsed gracefully and simply produce no virtual method.
- **Code lens navigation.** Code lenses now work in Zed, Neovim, Emacs, and other editors. Previously the click command used a VS Code-specific API that other editors ignored.
- **`@mixin` with union types.** `@mixin Foo|Bar` now correctly exposes members from all classes in the union. Previously only single-class mixins were recognized.
- **`throw new` completion no longer offers non-instantiable types.** Interfaces, abstract classes, traits, and enums are now filtered out, matching the behavior of `new` completion. The `throw new` path also now filters to Throwable descendants only.
- **Unified class name completion architecture.** `throw new` and `catch()` completion now use the same `build_class_name_completions` pipeline as `new`, `extends`, `implements`, etc. `throw new` uses a `ThrowNew` context (instantiable + Throwable) and `catch()`/`@throws` uses a `Catch` context (class or interface + Throwable). This gives both contexts the same affinity scoring, FQN shortening via use-map, namespace segment drill-down, deprecation flags, and consistent filtering. The separate `build_catch_class_name_completions` function has been removed.
- **Consolidated class completion passes.** The previous 5-pass architecture (use-map, same-namespace, fqn_uri_index, fqn_uri_index duplicate, stub_index) has been simplified to 2 passes (fqn_uri_index + stub_index) with an inline `classify` closure that determines tier (`'0'` use-imported, `'1'` same/sub-namespace, `'2'` everything else) per candidate. The redundant pass 4 (identical to pass 3) is eliminated, and tier assignment is now based on proximity checks rather than which data source produced the item.
- **Analysis deadlock.** Lazily-parsed vendor files acquired two internal locks in the opposite order from the editor's file-change handler, causing a deadlock when both ran concurrently.
- **External tool diagnostics on large files.** PHPStan, Mago, and PHPCS diagnostics no longer time out on files that produce a large report. Their output is now read while the tool is still running, so a report bigger than the operating system's pipe buffer can no longer stall the tool and force a timeout.
- **Promote to constructor property.** Promoting a parameter whose property is declared together with others on one line (`private int $a, $b;`) no longer deletes the sibling properties. The action is now offered only when the property is declared on its own.
- **`get_defined_vars()` counts as using every variable in scope.** A function or method that calls `get_defined_vars()` (for example to build a debug dump) no longer reports its local variables as unused, since the call reads all of them. Variables local to a nested closure or arrow function are still checked. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/158.

### Changed

- **Improved LSP responsiveness.** File parsing (`update_ast`) and diagnostics now run in background tasks, preventing interactive requests (completion, hover) from being blocked by full-file parses during typing. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/118.
- **Member completion caching.** Unfiltered member lists are cached per-target to speed up subsequent completions during keyword entry. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/118.
- **Laravel startup performance.** Common Laravel builder classes are warmed in the background at startup to eliminate the first-access penalty on Eloquent completions. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/118.
- **Faster code actions.** Requesting code actions (the lightbulb menu) now parses the file once and shares the result across every refactoring, instead of re-parsing it for each one.

## [0.8.0] - 2026-05-14

### Added

- **Blade template support.** Completion, hover, go-to-definition, diagnostics, semantic tokens, and inlay hints work inside `.blade.php` files. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/100.
- **Blade keyword highlighting.** Blade directives, echo delimiters, PHP keywords, cast types, comments, and PHPDoc tags inside `.blade.php` files now receive semantic tokens for proper syntax coloring.
- **Blade view directive navigation.** Go-to-definition works on view names inside Blade directives (`@include`, `@extends`, `@includeIf`, `@includeWhen`, `@includeUnless`, `@includeFirst`, `@component`, `@each`), jumping to the referenced template file.
- **Replace FQCN with import.** A refactoring code action on any fully-qualified class name (`\Foo\Bar`) inserts a `use` statement and replaces all occurrences of the same FQCN throughout the file with the short name. Detects existing imports and short-name conflicts. A separate "Replace all FQCNs with imports" action appears when the file contains multiple distinct FQCNs, replacing all of them at once (skipping those with import conflicts).
- **Broader type narrowing.** `instanceof`, type-guard functions, `in_array()` strict mode, `assert()`, `@phpstan-assert-if-true`/`-if-false`, and compound `&&`/`||` conditions now narrow types in if/else branches, guard clauses, while-loop bodies, ternary expressions, and `match(true)` arms.
- **Argument type mismatch diagnostics.** Flags function and method calls where an argument's resolved type is incompatible with the declared parameter type.
- **Invalid class-like kind diagnostics.** Flags class-like names used in positions where their kind is guaranteed to fail at runtime: `new` on abstract classes, interfaces, traits, or enums; `extends` on a final class, interface, or trait; `implements` with a non-interface; trait `use` with a non-trait; `instanceof` with a trait; `catch` with a non-Throwable type; and traits in type-hint positions.
- **Unused variable diagnostics.** Variables assigned but never read are flagged with hint severity and rendered as dimmed text. Variables named `$_` or prefixed with `$_` are exempt.
- **Mago diagnostic proxy.** Mago lint and analyze diagnostics are surfaced as LSP diagnostics with quick-fix code actions. Configurable under `[mago]` in `.phpantom.toml`.
- **Laravel Pint formatting.** Projects with `laravel/pint` in `require-dev` automatically use Pint for formatting via stdin. Configurable under `[formatting]` in `.phpantom.toml` with `pint = "path"` or `pint = ""` to disable.
- **PHPCS diagnostic proxy.** PHP_CodeSniffer violations are surfaced as LSP diagnostics with severity mapping. Configurable under `[phpcs]` in `.phpantom.toml`.
- **Return type inference from method bodies.** Methods without a declared return type or `@return` docblock now have their return type inferred from `return` statements, improving completion, hover, and diagnostics for untyped code.
- **Closure and arrow function parameter inference.** Untyped closure parameters are inferred from the enclosing call's callable signature, including through method chains that return `static`. Generic type substitution flows through to inferred parameters.
- **Closure and arrow function inlay hints.** When a closure or arrow function is passed to a callable-typed parameter, inlay hints show inferred parameter types and the return type derived from the enclosing callable signature.
- **Generics.** `@mixin` tags referencing a template parameter now resolve through the template bound. `new $var()` where `$var` is `class-string<T>` resolves to `T`. SPL collection classes now carry `@template` parameters so iteration methods resolve to concrete type arguments.
- **Namespace renaming.** Renaming a namespace segment updates all declarations, use statements, and fully-qualified references across the workspace. When a PSR-4 autoload mapping exists, the corresponding directory is moved automatically.
- **Linked editing ranges.** Place the cursor on a variable and all occurrences within its scope enter linked editing mode, updating every occurrence as you type.
- **Import all missing classes.** A bulk code action that imports every unresolved class name in the file at once. Ambiguous names are left for manual resolution.
- **Context-aware import candidate filtering.** Import class actions now filter candidates by syntactic context (only interfaces after `implements`, only traits after `use`, etc.).
- **Convert to instance variable.** A code action that promotes a local variable inside a method to a class property, rewriting all references to `$this->prop` (or `self::$prop` in static methods).
- **Laravel view, route, and translation key navigation.** Go to Definition works for Blade view names (`view('...')`), route names (`route('...')`), and translation keys (`__('...')`, `trans(...)`, `Lang::get(...)`). Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/101.
- **Laravel config and env key navigation.** Go to Definition and Find All References work for config keys and env variables (`config('app.name')`, `env('APP_KEY')`). Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/93.
- **Untyped property type inference from constructor.** Properties without type declarations are resolved by inspecting the constructor body for assignments and promoted parameter defaults. Contributed by @lucasacoutinho in https://github.com/AJenbo/phpantom_lsp/pull/81.
- **Binary expression type inference.** Hover and variable resolution now show result types for all binary operators (`int + int` → `int`, `int + float` → `float`, `int / int` → `int|float`). Compound assignments update the variable's type accordingly.
- **Nested array shape inference from multi-level key assignments.** Assignments like `$b['a']['b'] = 'x'` now produce a nested array shape type (`array{a: array{b: string}}`), enabling array key completion for incrementally built arrays.
- **Loop type propagation.** Variables assigned late in loop bodies are now visible from the start on subsequent iterations.
- **`global` keyword variable resolution.** Variables imported with `global $var` now resolve to their top-level type, enabling completion, hover, and go-to-definition.
- **`array_reduce`, `array_sum`, and `array_product` return type inference.** `array_reduce()` resolves to the type of its initial value argument. `array_sum()` and `array_product()` resolve to `int|float`.
- **Machine-readable CLI output.** Both `analyze` and `fix` accept a `--format` flag with `table`, `github`, and `json` options. When `GITHUB_ACTIONS` is set, table output automatically includes GitHub annotations.
- **Magic property diagnostics.** New `report-magic-properties` option under `[diagnostics]` in `.phpantom.toml`. When enabled, classes with `__get` that also have virtual properties (from `@property` docblock tags, Laravel Eloquent column inference, or other providers) will flag unknown property access instead of silently allowing it.
- **Inline diagnostic suppression.** `// @phpantom-ignore code` on the same line or the line above suppresses the specified diagnostic. Multiple codes can be comma-separated. A bare `// @phpantom-ignore` suppresses all diagnostics on the target line.
- **Find references and rename for PHPDoc virtual members.** `@property`, `@property-read`, `@property-write`, and `@method` declarations in docblocks are now included in find-references and rename results alongside their runtime usages, including when the subject has a nullable or union type (e.g. `Foo|null` from `->first()`). Contributed by @AbyssWaIker in https://github.com/AJenbo/phpantom_lsp/pull/115.

### Changed

- **Find References performance and freshness.** Project-wide Find References now avoids more unnecessary file work while still returning references through aliased class and function imports, and it refreshes newly added workspace PHP files on later searches. Contributed by @MingJen in https://github.com/AJenbo/phpantom_lsp/pull/116.
- **Incremental text sync.** The server now uses incremental document sync, receiving only changed ranges from the editor instead of the full file content on every keystroke.
- **LSP responsiveness.** Hover, go-to-definition, signature help, code actions, rename, and other handlers now run on background threads. Slow requests no longer block other requests or cancellations.
- **Faster analysis.** Analysis time cut significantly on large projects.
- **Reduced redundant file parsing.** Concurrent threads resolving the same vendor class no longer parse the file in parallel; the second thread waits for the first to finish.
- **Unified first-class callable resolution.** First-class callable return type inference (`$fn = $obj->method(...)`) now uses the shared call return type pipeline, improving accuracy for chained calls and generic substitutions.
- **Editing responsiveness.** Classes evicted from the cache after a file edit are now eagerly re-populated in dependency order.
- **Diagnostic delivery model.** Editors that support pull diagnostics now get diagnostics on first file open without waiting for a debounce timer. Updates from external tools no longer re-run the entire native diagnostic pipeline.
- **Virtual member resolution.** Mixins and virtual accessors are now resolved completely on every class, eliminating cases where they were missing after edits.
- **Diagnostic code identifiers.** All diagnostic codes now use a consistent `snake_case` noun-phrase scheme: `unknown_variable`, `type_mismatch_argument`, `argument_count_mismatch`, `deprecated_usage`, `missing_implementation`. Users with editor filters matching on these codes will need to update them.
- **Lower memory usage for lazily-loaded files.** Vendor and stub files no longer store per-file import tables and namespace maps after parsing, and go-to-implementation uses a dedicated reverse-inheritance index instead of scanning all parsed files.
- **Lower memory usage for variable type tracking.**
- **Faster variable name completion.** Variable name suggestions now use the precomputed symbol map instead of re-parsing the file. Foreach iteration variables correctly persist after the loop (matching PHP semantics), `@var` docblock variable names are included, and `unset()` removes variables from suggestions.
- **Faster go-to-definition for variables.** Variable definition lookup no longer re-parses the file as a fallback; the precomputed symbol map handles all cases.
- **Updated embedded phpstorm-stubs.**

### Fixed

- **`throw new` completion missing vendor classes.** Classes whose Throwable ancestry could not be immediately verified (e.g. vendor classes not yet parsed) were silently excluded from `throw new` and `catch` completion, even though later heuristic-based sections should have included them.
- **Stale mixin members after editing.** Mixin class resolution (e.g. `@mixin Builder`) is now invalidated when any file changes, so newly added or removed methods on mixin targets appear immediately without restarting the server.
- **Version-gated stub constants now filtered.** Constants with `@removed` tags (e.g. `MCRYPT_ENCRYPT`, removed in PHP 7.2) are now excluded from completion and resolution when the project targets a newer PHP version. Previously only classes and functions were filtered.
- **Go-to-definition.** Fixed a potential deadlock when navigating to a vendor class that hadn't been parsed yet.
- **LSP no longer freezes under heavy editor activity.** Server-to-client requests (diagnostic refresh, progress token creation) could deadlock the service loop when the editor was simultaneously sending bursts of open/close/hover messages. All server-to-client requests are now either fire-and-forget or time-bounded, long-running handlers are cancellation-safe, and the process exits cleanly if the service loop ever terminates unexpectedly.
- **Rename class preserves `self`, `static`, and `parent` keywords.** Renaming a class no longer replaces occurrences of `self::`, `static::`, or `parent::` with the new class name.
- **Rename propagates into closures and arrow functions.** Renaming a variable now follows explicit `use ($var)` captures into closure bodies and implicit captures into arrow function bodies, instead of leaving those occurrences unchanged.
- **Spurious function auto-imports.** Import statements like `use function is_array;` were misidentified as function declarations, polluting the completion list with phantom entries that inserted incorrect imports.
- **Duplicate `use function` insertion.** Accepting a function completion no longer inserts a `use function` statement when the exact import already exists in the file.
- **Function import conflict handling.** When a different function with the same short name is already imported, completing a namespaced function now inserts the fully-qualified name instead of the ambiguous short name.
- **False-positive unused variable diagnostics.** Variables passed to `compact()`, by-reference out-parameters (e.g. `preg_match($p, $s, $matches)`), and variables used only via `global` are no longer incorrectly flagged.
- **False-positive type mismatch diagnostics.** Bare `array` return values passed to typed array parameters, properties narrowed via `instanceof`, type alias parameters, and use-map shadowing no longer trigger incorrect type errors.
- **Functions inside `if (!function_exists(...))` guards.** Function bodies nested inside conditional blocks no longer produce false-positive unresolved-member-access errors.
- **Standalone `@var` completion.** Variables typed only via a standalone `/** @var Type $var */` docblock now resolve for member completion and go-to-definition.
- **`@var` docblocks with additional tags.** Extra tags like `@psalm-suppress` in the same docblock no longer corrupt the type string.
- **Foreach `@var` annotations for key and value variables.** Multi-line docblocks with multiple `@var` tags before a `foreach` now correctly override both key and value types.
- **Foreach element type from untyped arrays.** Variables in a `foreach` over bare `array` now resolve to `mixed` instead of empty.
- **Foreach narrowing with break in else.** The variable state from break paths is now included in the post-loop type.
- **Foreach target type after non-empty literal array.** The pre-loop sentinel value no longer survives as a possible post-loop type.
- **Foreach over `::class` literal arrays resolves static access.** `$className::CONST` and `$className::method()` no longer produce unresolved-member diagnostics.
- **Hover on reassigned variable shows post-assignment type.** Hovering on the left-hand side of a reassignment now shows the type produced by the assignment.
- **Multi-namespace class resolution.** Short class names now resolve against the correct namespace for the current scope.
- **Multi-namespace variable isolation.** Variable resolution now only considers the namespace block containing the cursor.
- **Multi-namespace function return type resolution.** Function return types are now resolved against the function's own namespace.
- **Multi-namespace static call class resolution.** `ClassName::method()` now resolves against the correct namespace block.
- **Short class name resolution in type hints.** The resolver now prefers the class in the same namespace as the owning type before falling back to first-match.
- **Class loader global fallback.** Unqualified class names in namespaced code now fall back to global scope lookup when the namespace-qualified name doesn't exist.
- **Template inference through stub interfaces.** `@template-implements` on stub-loaded interfaces now correctly propagates substituted return types to child methods.
- **Generic method return types from `@var` annotations.** Method calls on variables annotated with a generic type now correctly substitute class-level template parameters into the return type.
- **Template union inference from multiple arguments.** When multiple arguments bind to the same `@template T`, the resolved type is now the union of all inferred types instead of only the first.
- **Template param inference from type bounds.** Nested template params are now inferred from concrete generic arguments when a template parameter has a generic bound.
- **Method-level `@template` with `key-of` bound.** Passing a string literal to a method with `@template K as key-of<TData>` now resolves the return type to the specific array shape value type.
- **`__get` magic method template resolution.** Property access on a class whose `__get` uses `key-of<T>` bounds now infers the concrete type from the property name.
- **Magic `__get` property access.** Accessing undefined properties on objects with a `__get` method now resolves to the method's declared return type.
- **Magic `__call` method return type.** Calling undefined methods on objects with a `__call` method now resolves to `__call`'s declared return type.
- **SoapClient arbitrary methods.** Calling any method on `SoapClient` no longer produces false-positive "unknown member" diagnostics.
- **Literal `true`/`false` preserved in template inference.** Passing `true` or `false` to a generic constructor now keeps the precise type instead of widening to `bool`.
- **`@psalm-method` overrides `@method`.** The vendor-prefixed tag now takes priority when both are present.
- **`@psalm-param`/`@phpstan-param` priority over `@param`.** `@phpstan-param` takes precedence over `@psalm-param`, which takes precedence over `@param`, matching PHPStan and Psalm behaviour.
- **`@psalm-if-this-is` template inference.** Method-level template parameters are now inferred by matching the receiver's concrete type against the annotation's type pattern.
- **`self::class` and `static::class` in template arguments.** Passing these to a `class-string<T>` parameter now correctly resolves T to the enclosing class.
- **`static` return type through first-class callables.** `self::method(...)()` and similar patterns now preserve `static` in the return type.
- **Interface method return type inheritance.** Template-substituted return types from interfaces are now propagated to overriding methods without a return type.
- **Property `self`/`static` type resolution.** Properties with `@var self|null` or `static` now resolve to the owning class name in hover.
- **Trait `self` return type resolution through inheritance.** Trait methods with return type `self` now resolve to the declaring class, not the calling subclass.
- **Conditional return type resolution for scalar arguments.** `$param is string` conditions in `@return` annotations now resolve correctly for literal values.
- **SPL iterator generic type propagation.** Decorator iterators like `CachingIterator` and `LimitIterator` now propagate the wrapped iterator's generic type parameters.
- **`ArrayIterator` constructor generic inference.** `new ArrayIterator($typedArray)` now infers key and value types from the array argument.
- **`range()` return type inference.** `range()` now returns `list<string>` for string arguments and `list<int|float>` otherwise, instead of bare `array`.
- **`(object)` cast type inference.** Casting now resolves to an object shape matching the operand's structure instead of bare `stdClass`.
- **ArrayAccess array-access assignment.** `$obj[$key] = $val` on `ArrayAccess` objects no longer overwrites the variable's generic type with an array type.
- **Static method calls on class-string unions.** `$variable::method()` where `$variable` holds a union of class-strings now resolves through all possible classes.
- **Array shape keys with special characters.** Keys containing backslashes or newlines are now properly quoted and escaped in type display.
- **Implement methods: no invalid generic return type hints.** The "Implement missing methods" code action no longer emits generic docblock syntax as a native PHP return type hint.
- **Composer `files` autoload packages now indexed.** Vendor packages using `"autoload": {"files": [...]}` now have their classes discovered correctly.
- **Classmap collision resolution.** When two files declare the same class name, the file matching PSR-4 naming convention is now preferred.
- **Eloquent `$dates` and `where{Property}` go-to-definition.** Go-to-definition now works for properties backed by the `$dates` array and dynamic `where{Property}()` methods.
- **Type hierarchy registration.** Dynamic registration is now gated on client capability, preventing errors in unsupported editors.
- **False-positive diagnostics on startup.** Files opened while the project was still indexing could produce spurious "class not found" errors. Diagnostics are now deferred until initialization completes.
- **Analyzer and LSP no longer hang on files with deeply nested loops.**
- **Infinite loop on array key reassignment patterns.** Files containing `$arr['key'] = f($arr['key'])` no longer hang the analyzer.
- **Chained calls with complex arguments resolve the correct return type.** Calling `redirect($string . $var)->with(...)` now resolves to `RedirectResponse` as expected. Complex argument expressions (concatenation, method calls, etc.) were previously serialized as empty, causing conditional return types to take the wrong branch.
- **Stack overflow on large codebases and large files.** The `analyze` command no longer crashes with stack overflows on large files.
- **Non-deterministic diagnostic counts eliminated.** Projects with heavy use of generics no longer see false positives that vary between runs.
- **Pull-diagnostic reliability.** Editors that support pull diagnostics no longer show duplicate or stale diagnostics.
- **Hover scales linearly on large files.** Hover requests no longer take O(n²) time on files with many method calls.
- **`analyze` and `fix` commands run at consistent speed regardless of invocation style.**
- **Type narrowing.** Comprehensive fixes: `is_*()` guards correctly narrow multi-member unions; `instanceof` on `mixed` or `object` narrows to the checked type; `=== null` and `== null` narrow correctly; `assert()` narrowing persists through subsequent branches; `isset()`/`empty()` strip `null` from nullable types; property access expressions are narrowed through conditionals; array shape keys are narrowed through guard clauses; OR'd `instanceof` checks resolve to the union of all branches; post-loop narrowing applies the loop condition's inverse; branch merging preserves nullable information correctly.
- **Generics.** Constructor generic inference works through inherited constructors with correct remapping through multi-level `@extends` chains. Function-level templates are inferred from arguments extending wrapper classes. Class-level template parameters are preserved through chained method calls. Template parameters fall back to their declared bound when subclasses omit annotations. Method calls on unions of generic types resolve to the union of each branch's return type. `key-of<T>`, `value-of<T>`, and indexed access types evaluate to concrete types after template substitution. Array literal arguments infer key and value types separately.
- **Mixin resolution.** Static method calls on instances with `@mixin` now resolve through the mixin. `@method` and `@property` tags on mixin classes are propagated to the consumer. `$this` return types on mixin methods resolve to the consumer class.
- **`@method` tag resolution.** Colon return type syntax, parenthesised return types, and the ambiguous single-`static` pattern are now parsed correctly. Template parameters in `@method` return types are substituted through `@extends` and `@implements` annotations.
- **First-class callable invocation return types.** Immediately invoking a first-class callable (`Foo::method(...)()`) now resolves to the underlying function's return type.
- **Chained instantiation preserves constructor-inferred generics.** Expressions like `(new Box(new Product()))->get()` now propagate template arguments to subsequent method calls.
- **`@return numeric` pseudo-type.** Functions annotated with `@return numeric` now resolve correctly instead of falling back to `string`.
- **`parent::__construct()` with `@extends` generics.** No longer produces false-positive type errors for substituted parameter types.
- **Array access on bare `array` and `mixed` types.** Accessing a key on plain `array` now resolves to `mixed` instead of an empty type.
- **Vendor functions and constants.** Functions and constants defined in vendor packages are now indexed at startup, eliminating false-positive diagnostics.
- **Use-imported classes no longer shadowed by global-namespace stubs.** Fixes Laravel Facade static method resolution.
- **Same-name class in a different namespace no longer shadows inherited members.**
- **Short-name collisions eliminated project-wide.** Two unrelated classes sharing a short name are no longer treated as identical.
- **Transitive interface inheritance.** A class implementing an interface that extends another interface is now correctly recognized as a subtype of the parent interface.
- **Conditional return types.** Methods with conditional return types now check whether the argument class implements the bound interface, and class names in conditional annotations are resolved through the defining file's use statements.
- **Promoted properties.** Inline `/** @var */` annotations on promoted constructor properties now resolve inside the constructor body.
- **Backed enums.** Accessing `->value` resolves to the specific backing type. `@implements` generics on enums are resolved correctly.
- **Class constants.** Inherited constants accessed via `self::CONST` or `ChildClass::CONST` resolve through multi-level inheritance.
- **Hover / type display.** `T[]` displays as `array<T>`, `mixed[]` as `array`. PHPDoc type aliases are normalized. Methods returning `parent` resolve to the actual parent class name.
- **Chain assignments.** `$a = $b = new Foo()` resolves all variables in the chain.
- **Destructuring.** Array destructuring (`[$a, $b] = $expr`, `list()`, keyed shapes, nested patterns) and foreach destructuring now resolve types correctly.
- **Variable type resolution.** Short class names from `@var`, `@param`, and `new ClassName()` are resolved to FQN before entering the type pipeline.
- **Closure inlay hints.** Template parameters in callable signatures are substituted with concrete types inferred from sibling arguments.
- **Laravel scopes.** Public methods with the `#[Scope]` attribute are no longer treated as scopes.
- **Static methods.** `$this` no longer resolves inside static methods.
- **Hover cache invalidation.** Editing a cross-file class's docblock now immediately reflects updated content on hover.
- **Foreach type resolution.** Nested generic array access, static property iterables, type alias expansion, and by-reference bindings all resolve element types correctly. Loop prescan no longer leaks types into the same-statement RHS.
- **Completion in loops and branches.** Array shape keys added inside `if` blocks, variables assigned later in loop bodies, and variables on the RHS of reassignments all resolve correctly.
- **Scope leakage after closures in chained method calls.** Variables from the enclosing method are no longer invisible after a closure argument.
- **Docblock `@param` annotations no longer leak across sibling methods or closures.**
- **`class-string<T>` parameter completion.** Parameters typed as `class-string<T>` resolve to the bound class for member access.
- **Inherited parameter types propagate to child methods.**
- **False positive type error for closures passed to callable parameters.** `\Closure` is now recognised as a subtype of `callable`.
- **Union-typed method calls no longer lose resolution on second occurrence.**
- **Fluent method chains in namespaced classes.** Methods returning `static` or `self` resolve correctly across namespaces.
- **False-positive undefined variable diagnostics.** By-reference parameters, nested array access assignments, and `$this`-prefixed variable names no longer produce false positives.
- **Auto-import formatting.** Missing blank line before first import and bulk "remove unused imports" in braced namespaces are fixed.
- **Exception types in `catch` clauses matched correctly across namespaces.**
- **Nested `match(true)` expressions no longer produce incorrect diagnostics.**
- **Lowercase built-in class names recognized as subtypes of `object`.**
- **False "class not found" for global-namespace classes loaded via Composer's `files` autoloading.**
- **False-positive type errors on generic class methods.** Template parameters are now substituted into method parameter types before checking argument compatibility.

## [0.7.0] - 2026-04-08

### Added

- **`@psalm-return`, `@psalm-param`, and `@psalm-var` tag support.** Psalm-prefixed docblock tags are now recognized alongside their PHPStan equivalents for return types, parameter types, variable types, conditional return types, template parameter bindings, and semantic token highlighting.
- **Refactoring code actions.** Extract function, extract method, extract variable, extract constant, inline variable, promote constructor parameter, generate constructor (traditional and promoted), generate getter/setter, and generate property hooks (PHP 8.4+). Deferred computation ensures the lightbulb menu appears instantly; edit generation only runs when the user picks an action.
- **PHPStan quickfixes.** Automated fixes for a wide range of PHPStan diagnostics: update or remove mismatched `@return`/`@param`/`@var` tags, remove unused return type union members, fix unsafe `new static()` (add `@phpstan-consistent-constructor`, `final` class, or `final` constructor), add or remove `#[Override]`, add `#[\ReturnTypeWillChange]`, fix void return mismatches, add inferred iterable return types, remove unreachable statements, remove always-true `assert()` calls, fix overriding member visibility, fix vendor-prefixed class names, and simplify ternary expressions to `??` or `?->`. All quickfixes eagerly clear their diagnostic on apply.
- **`fix` CLI subcommand.** `phpantom_lsp fix` applies automated code fixes across a project. Specify rules with `--rule` (multiple allowed) or omit to run all preferred fixers. `--dry-run` reports what would change without writing files. The first shipped rule, `unused_import`, removes unused `use` statements project-wide, collapsing blank lines left behind by removals (contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/54). Supports path filtering and single-file mode.
- **Keyword completions.** Context-aware PHP keyword suggestions filtered by scope (e.g. `return` only inside functions, `break` only inside loops, member keywords inside class bodies, enum backing types after `enum Name:`). Contributed by @ryangjchandler in https://github.com/AJenbo/phpantom_lsp/pull/43.
- **Attribute completion.** Typing inside `#[…]` offers only classes decorated with `#[\Attribute]`, filtered by the target declaration kind.
- **Eloquent model enhancements.** Timestamp properties (`created_at`, `updated_at`) are automatically typed as `Carbon` with support for `$timestamps = false` and custom column constants. Legacy `$dates` arrays produce typed virtual properties. `$appends` entries produce virtual properties. `where{PropertyName}()` dynamic methods are synthesized from all known columns (including `@property` annotations) on both the model and the Builder. `whereHas`/`whereDoesntHave` closure parameters resolve to `Builder<RelatedModel>` by traversing relationship methods, with dot-notation chain support. `Conditionable::when()`/`unless()` chains preserve type information.
- **Type-guard narrowing.** `is_array()`, `is_string()`, `is_int()`, `is_float()`, `is_bool()`, `is_object()`, `is_numeric()`, and `is_callable()` narrow union types inside `if`/`else`/`elseif` bodies and after guard clauses, preserving generic element types through narrowing.
- **Array value type tracking.** Arrays built incrementally with variable keys inside loops now carry element types through `foreach` iteration, bracket access, and null-coalescing. Foreach over generic arrays with non-class element types (array shapes, scalars) now preserves the full element type.
- **Inherited docblock type propagation.** When a child class overrides a method without providing its own `@return` or `@param` docblock, the ancestor's richer types flow through automatically. Applies to return types, parameter types (matched by position), property type hints, and descriptions.
- **Bidirectional template inference from closures.** Templates appearing in callable parameter signatures are now inferred from both the closure's return type and its parameter types. Positional matching is supported, and return-type bindings take priority when the same template appears in both positions.
- **Drupal project support.** Drupal projects are detected via `composer.json`. Drupal-specific directories and PHP extensions (`.module`, `.install`, `.theme`, `.profile`, `.inc`, `.engine`) are recognized and indexed. Contributed by @syntlyx in https://github.com/AJenbo/phpantom_lsp/pull/52.
- **Completion and signature help for `new self`, `new static`, and `new parent`.** Constructor parameter snippets and signature help inside the parentheses. Contributed by @RemcoSmitsDev in https://github.com/AJenbo/phpantom_lsp/pull/51.
- **Hover on parameter variables at their definition site.** Hovering on a function or method parameter now shows its resolved type, using the `@param` docblock type when it is richer than the native hint. Contributed by @RemcoSmitsDev in https://github.com/AJenbo/phpantom_lsp/pull/68.
- **Array element type extraction from property generics.** Bracket access on properties annotated with generic array or collection types (e.g. `$this->cache[$key]->`) now resolves the element type correctly through nested chains, string-literal keys, and method chains after the bracket.
- **`@phpstan-assert-if-true $this` narrowing.** Instance methods annotated with `@phpstan-assert-if-true` or `@phpstan-assert-if-false` targeting `$this` now narrow the receiver variable in the corresponding branch. Contributed by @syntlyx in https://github.com/AJenbo/phpantom_lsp/pull/52.
- **Namespace completion from file path.** When creating a new PHP file, typing `namespace ` suggests the correct namespace inferred from the file's location and the project's PSR-4 autoload mappings. The most specific mapping is preselected so you can accept it with a single keypress. When multiple PSR-4 roots match the same directory, all candidates appear ranked by specificity (longest match first).
- **Standalone `@var` docblock for untyped closure parameters.** When a closure parameter lacks a type hint and no assignment follows, a `@var` block above the usage is now picked up as the variable's type.
- **`--stdio` CLI flag.** Accepted (and ignored) for compatibility with LSP client wrappers that pass `--stdio` by default. Contributed by @markkimsal in https://github.com/AJenbo/phpantom_lsp/pull/67.
- **`--tcp` CLI flag.** `phpantom_lsp --tcp 9257` starts the server listening on a TCP port instead of stdin/stdout. Useful for debugging or connecting from IDE plugins that prefer a network transport over spawning a child process. Accepts a full address (`127.0.0.1:9257`) or just a port number. The server accepts one connection and exits when the client disconnects.
- **Zed extension setup instructions.** Contributed by @daronspence in https://github.com/AJenbo/phpantom_lsp/pull/47.
- **SETUP.md improvements.** Contributed by @mattsches in https://github.com/AJenbo/phpantom_lsp/pull/61.
- **Method-level template parameters resolve inside method bodies.** `@template T of Builder` with `@param T $query` now resolves `$query` to the template bound inside the method body, providing completions from the bound class.
- **Undefined variable diagnostic.** Variable reads that have no prior definition (assignment, parameter, foreach binding, catch variable, `global`, `static`, `use()` clause, or destructuring) in the same scope are flagged as errors. Writes must appear before the read in source order, catching use-before-assign bugs, while assignments inside branches (if/else, switch, try/catch) still count to avoid false positives. Suppressed for superglobals, `isset()`/`empty()` guards, `compact()` references, `extract()` calls, variable variables (`$$`), `@` error suppression, and `@var` annotations. Static property accesses (`self::$prop`, `static::$prop`, `parent::$prop`) are excluded. Variables passed to by-reference parameters are recognized as definitions: 40+ built-in PHP functions are covered (regex, cURL, OpenSSL, sockets, DNS, etc.), and user-defined functions, static methods, and constructors with `&$param` parameters are detected automatically from their signatures. Scoping is tracked through arbitrary nesting of closures, arrow functions, and catch blocks. Top-level code outside functions is skipped.
- **By-reference parameter type inference for method, static, and constructor calls.** When a variable is passed to a by-reference parameter with a type hint (e.g. `function foo(Baz &$bar)`), the variable acquires that type after the call. Previously this only worked for standalone function calls. Now it also works for `$this->method()`, static method calls, and constructor calls.

### Changed

- **Fewer false-positive diagnostics.** Variable resolution now produces the same result across completions, hover, and diagnostics, eliminating cases where diagnostics disagreed about a variable's type.
- **`@phpstan-ignore` is never the preferred quickfix.** The "Ignore PHPStan error" code action is explicitly non-preferred, so editor keyboard shortcuts no longer accidentally apply it when another fix is available.
- **Generate PHPDoc infers `@return` from the function body.** Typing `/**` above a function that returns `array` now produces a specific element type (e.g. `@return list<string>`) instead of `@return array<mixed>`.
- **Faster startup.** Stub loading during initialization is significantly faster.
- **More accurate generics resolution.** Type substitution and resolution for complex nested generic types is more correct, particularly for unions, intersections, array shapes, and deeply nested generic arguments.
- **More accurate type predicates.** `NULL`, `Null`, and case variants of `null` are now handled consistently throughout type checking, matching PHP's case-insensitive treatment of type keywords.
- **Go-to-definition at declaration sites returns the symbol's own location.** Class, member, and variable declaration names now return their own location instead of nothing, so editors that detect "definition == cursor" can automatically fall back to Find References. Contributed by @lucasacoutinho in https://github.com/AJenbo/phpantom_lsp/pull/76.

### Fixed

- **Completion no longer triggers on the `<?php` open tag.** Typing `<?php` and pressing enter no longer applies a spurious function suggestion like `php_ini_loaded_file()`.
- **Case-insensitive `parent` handling in chained static calls.** `resolve_lhs_to_class` now handles `parent::method(...)` in chained callable expressions and uses case-insensitive matching for `self`/`static` in the same context.
- **Intersection types preserved through resolution.** Variables and parameters with intersection types (e.g. `Countable&Serializable`) now display correctly in hover, extract-function parameter hints, and generated docblocks. Previously intersection types were flattened to unions (`Countable|Serializable`).
- **Return types now carry class info through the resolution pipeline.** Method and function return types that name a class (e.g. `Collection<User>`) now populate the resolved class info eagerly, so downstream consumers (hover, narrowing, completion) no longer need a second resolution pass.
- **Generic parameters preserved on resolved types.** Catch clause variables, pass-by-reference parameters, closure parameters, and constructor calls now thread the original type hint (including generic parameters) through the resolution pipeline instead of discarding it.
- **Type-guard narrowing no longer drops class info on unions.** Narrowing a union like `Foobar|string|int` with `is_string()`/`is_int()` in elseif chains now correctly preserves class info for the remaining class member.
- **False-positive undefined-variable diagnostic on static property access.** `self::$prop`, `static::$prop`, and `ClassName::$prop` no longer trigger an undefined variable warning. Dynamic forms (`self::$$prop`, `self::${expr}`) still correctly flag undefined variables used in the expression. Contributed by @lucasacoutinho in https://github.com/AJenbo/phpantom_lsp/pull/75.
- **Case-insensitive `self`, `static`, and `parent` resolution.** `SELF::method()`, `Static::create()`, `PARENT::foo()`, and other non-lowercase spellings now resolve correctly. Previously only the exact lowercase forms were recognized.
- **Property type resolution in call arguments.** When a method argument is `$this->prop` and the property has a generic, nullable, or union type, the full type structure is now preserved. Previously only the base class name was extracted, discarding generics and union components.
- **Update docblock enrichment comparison.** The "Update docblock" code action now uses structural type comparison instead of string equality when deciding whether a `@param` type needs enrichment. Types that are semantically equivalent but formatted differently (e.g. `\App\User` vs `App\User`) no longer trigger spurious updates. Body-based `@return` enrichment now correctly detects when an existing `@return` tag already has type structure, instead of always proposing a replacement.
- **`@phpstan-assert` and `@psalm-assert` tags with generic types.** Assertions like `@phpstan-assert Collection<int, User> $param` now parse the full generic type instead of truncating at the first space inside angle brackets.
- **`parent::method()` resolution in inline arguments.** Passing `parent::method()` as an argument to a function now resolves the return type correctly, matching the existing handling for `self::` and `static::`.
- **Laravel Eloquent Builder and Collection type resolution.** Generic and nullable types on Eloquent models (e.g. `Collection<int, User>`, `?User`) now resolve correctly when used for Builder scope injection, custom collection swapping, and relationship chain inference. Previously these types were stringified with their generic parameters or nullable prefix, causing lookups to fail silently.
- **Docblock generation no longer panics on lines with multibyte characters.** Files containing non-ASCII characters (e.g. accented letters) could cause the `/**` docblock trigger to crash or produce misaligned edits due to a mismatch between UTF-16 column offsets and byte offsets.
- **Conditional return types showing `mixed` in hover.** When a method with a conditional return type (e.g. `@phpstan-return ($type is class-string<T> ? T : mixed)`) resolved to a concrete class, hover still displayed the method's declared return type (`mixed`) instead of the resolved class. Affects methods like Symfony's `SerializerInterface::deserialize()`.
- **Method-level `@throws` types now resolve short names to FQN.** Exception types in `@throws` tags on class methods are now fully qualified using the file's `use` imports, matching the behaviour already in place for standalone functions. Cross-file throws propagation and the "Update docblock" code action produce correct results when the exception class is imported via a `use` statement.
- **Missing diagnostics and import actions in files without a namespace.** When a namespaced class (e.g. `Carbon\Carbon`) had already been parsed, using its short name (`Carbon`) in a file without a `namespace` declaration incorrectly resolved against the namespaced class. This suppressed both the "class not found" diagnostic and the "Import" code action. Bare-name lookups now only match classes that are themselves in the global namespace.
- **Find-references false positives for global classes.** Searching for references to a global-scope class (e.g. `Helper` with no namespace) could include references to unrelated namespaced classes with the same short name (e.g. `App\Helper`). Short-name fallback matching now only applies when the resolved name is unqualified.
- **Fluent chains only flag the first broken link.** In a chain where the first method does not exist, only that method is flagged instead of every subsequent call receiving its own warning.
- **Null narrowing from `!== null` checks.** Null-initialized variables guarded by `$var !== null`, `!is_null()`, or bare truthy checks now have `null` narrowed away inside the then-body and in subsequent `&&` operands. Works in chained conditions, ternary expressions, and return statements.
- **Variables assigned inside `if`/`while` conditions now resolve in the body.** `if ($admin = AdminUser::first())` and `while ($row = nextRow())` register the assignment so the variable has a type inside the branch or loop body.
- **Loop-body assignments not visible inside the same loop iteration.** When a variable is initialized as `null` and reassigned later in a loop body, the assigned type is now visible at every point inside the loop. Combined with null narrowing, variables correctly resolve to the assigned class type.
- **`@var` docblock annotations no longer leak across class and method boundaries.** A `@var` annotation for a same-named variable in a different class no longer bleeds into the current scope.
- **Inline `@var` cast no longer overrides the variable type on the RHS of the same assignment.** `/** @var array<string, mixed> */ $data = $data->toArray()` no longer resolves the RHS `$data` using the cast type.
- **Foreach over union types containing arrays now resolves the element type.** A parameter typed `User|array<User>` iterated with `foreach` now correctly yields `User` as the loop variable type. Previously the element type extraction did not look inside union members, producing no completions.
- **`@param` docblock overrides ignored when the native type hint resolves.** When a parameter has both a native type hint and a more specific `@param` override, the docblock type now takes effect. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/55.
- **Variable reassignment inside `try`/`catch`/`finally` blocks now tracked.** Subsequent accesses within the same block resolve against the reassigned type instead of the original.
- **Self-referential variable reassignments in nested loops no longer produce false "type could not be resolved" diagnostics.** Recursive resolution that hits the depth limit no longer poisons the cache for later lookups.
- **`instanceof` narrowing with unresolvable target class.** When the target class cannot be loaded, the variable's type is treated as unknown instead of keeping the un-narrowed type, eliminating false positives for members on the narrowed subclass.
- **`stdClass` and `object` types no longer produce false-positive diagnostics.** Variables typed as `object` or `stdClass` now permit arbitrary property access. `is_object()` correctly narrows `mixed` to `object` and compound `&&` conditions propagate the narrowing.
- **Docblock type refinement no longer matches class names containing type keywords.** A class named `PointOfInterest` would incorrectly be treated as an `int` refinement because the refinement check used substring matching. Refinement compatibility now uses structural type predicates.
- **`class-string<T>` static method dispatch.** Calling static methods on a `class-string<Foo>` variable now resolves return types correctly, including `static` substitution to the bound class.
- **`self`/`static`/`$this` in cross-file method return types now resolve correctly.** When a method on a cross-file class returns a type referencing `self` (e.g. `@return HasMany<self, $this>`), the owning class was looked up by short name through the consuming file's import table, which failed when the consuming file did not import that class. The owning class is now looked up by its fully-qualified name.
- **`in_array` guard clause no longer wipes out variable type.** When the haystack's element type matches the variable's type, the narrowing system no longer excludes the type entirely.
- **Method chains through `__call` no longer lose the return type.** When `__call` returns `$this`, `static`, or `self`, the chain type is preserved through dynamic method calls.
- **Scope methods on Eloquent Builder no longer produce false-positive diagnostics.** Bare `Builder` return types on scope methods are automatically wrapped as `Builder<ConcreteModel>` to preserve the chain.
- **Scope methods missing from completion on relationship results.** Scope methods from related models now appear in completions, not just hover.
- **Closure and variable hover now preserves generic arguments.** Closure parameters inferred from callable signatures, variables assigned from chained methods returning `static`/`$this`/`self`, and hovering on the `$` sign of a variable at its assignment site all now show the correct generic type.
- **Callable parameter inference preserves generic arguments from the receiver.** A closure typed as `fn(Builder $q)` inside a `Builder<Product>` chain now infers `$q` as `Builder<Product>`, so model-specific scope methods resolve correctly.
- **`@see` tags in floating docblocks now support go-to-definition.** Docblock comments not directly attached to a class, function, or statement (e.g. inline `/** @see SupervisorOptions::$balanceCooldown */` inside array literals or after expressions) are now parsed for symbol references. Previously these were silently ignored, particularly in files without a namespace.
- **Nullable `static` return types on inherited methods.** Methods returning `?static` or `static|null` now correctly resolve to the calling subclass across files.
- **Template binding with nested generics.** Parameter types like `Wrapper<Collection<T>, V>` no longer break during template binding.
- **Single generic argument on collections bound to the wrong template parameter.** `Collection<User>` now binds to the value parameter instead of the key parameter when key-like template parameters precede value parameters.
- **Nullable return types losing `|null` after template substitution.** `@return TValue|null` now preserves `|null` through substitution, so calls like `::first()` correctly show the nullable type.
- **`@mixin` referencing a template parameter now resolves.** A class with `@template T` and `@mixin T` now pulls in methods from the concrete type passed via generic arguments.
- **`@property` and `@method` tags losing nullable types.** Tags like `@property int|null $foo` no longer have `|null` stripped.
- **Callable types inside unions displayed ambiguously.** `(Closure(int): string)|Foo` is now parenthesized correctly in hover and completions.
- **Hover and go-to-definition on attributes.** Attributes on properties, class constants, parameters, and enum cases are now navigable.
- **Function-level `@template` with generic wrapper parameters.** Template substitution at call sites now correctly handles `array`, `iterable`, and `list` as wrapper names.
- **Closure parameter inference from function-level `@template` bindings.** Functions like `array_any` and `array_all` now infer concrete types for untyped closure arguments from the array parameter's element type.
- **Property chain arguments in template substitution.** Expressions like `$this->items` passed to templated functions now resolve their type for template binding.
- **Variadic parameter element type lost in `foreach`.** Iterating over a variadic parameter now resolves the loop variable to the element type.
- **Anonymous class variables now resolve their type.** `$model = new class extends Foo { ... }` followed by `$model->method()` now resolves through the anonymous class's inherited members.
- **Namespaced functions imported via `use function` no longer flagged as unknown.** Functions defined in one file and imported via `use function` in another now resolve correctly.
- **`parent::method()` return type resolution in variable analysis.** Calling `parent::method()` and assigning the result now correctly resolves the parent method's return type.
- **Closure parameter inference inside `switch` cases and `if` conditions.** Closure parameters that should be inferred from the enclosing callable context now resolve correctly when the closure appears inside a switch case or if-condition.
- **Generic arguments propagated through transitive `@extends` chains.** When a class extends a parent that itself extends a generic grandparent, generic arguments now flow through the full chain.
- **Stack overflow when a foreach value variable shadows the iterator receiver.** Patterns like `foreach ($category->getBranch() as $category)` no longer cause infinite recursion.
- **PHPStan `*` wildcard in generic type arguments.** Type strings like `Relation<TRelatedModel, *, *>` now parse correctly.
- **Types with `covariant` or `contravariant` variance annotations in generic args now parse correctly.** Annotations like `BelongsTo<Category, covariant $this>` no longer cause the entire type to become unresolvable.
- **Diagnostics now work for vendor files open in the editor.** Projects using `--prefer-source` or monorepo setups no longer have diagnostics suppressed in vendor files.
- **PHPStan diagnostics no longer hidden by unrelated native diagnostics on the same line.** Deduplication now only suppresses a full-line diagnostic when the precise diagnostic on the same line reports a related issue.
- **Nullable boolean properties now use `is` prefix for getters.** Properties typed `?bool` or `?boolean` now generate `isFoo()` instead of `getFoo()` when using the "Generate getter" code action.
- **Aliased namespace imports used in attributes no longer flagged as unused.** `use Symfony\Component\Validator\Constraints as Assert;` with `#[Assert\Uuid(...)]` no longer produces a false "Unused import" diagnostic.
- **`DB::select()` return type.** `DB::select()` and related methods now return `array<int, stdClass>` instead of bare `array`, and `DB::selectOne()` returns `?stdClass`.
- **Redis `Connection` method resolution.** Redis commands on `Illuminate\Redis\Connections\Connection` now resolve through the phpredis stubs.
- **Array shape tracking from keyed assignments inside conditional branches.** Shape types built incrementally with variable keys inside loops with if/else branching are now preserved through foreach iteration.
- **Deprecated class in `implements` renders with strikethrough.** Deprecated classes referenced in `implements` clauses are correctly tagged.
- **Interleaved array access and property chains no longer produce false positives.** Expressions like `$results[$i]->activities[$id]->extras` where array subscript and property access alternate were incorrectly parsed, causing the intermediate property chain to be dropped. This led to "Property not found on class" false positives when the element type was resolved but the subsequent property lookup was skipped.
- **FQN `\assert()` now narrows types.** Writing `\assert($var instanceof Foo)` with a leading backslash was not recognized as an instanceof narrowing, causing false-positive "property not found" diagnostics after the assertion.
- **Generic template substitution producing invalid types.** When a template parameter was the base of a generic type (e.g. `T<int>` where `T` maps to `Collection<string>`), the substitution produced malformed types like `Collection<string><int>`. The replacement's base name is now used correctly, yielding `Collection<int>`.

## [0.6.0] - 2026-03-26

### Added

- **Semantic Tokens.** Type-aware syntax highlighting that goes beyond what a TextMate grammar can achieve. Classes, interfaces, enums, traits, methods, properties, parameters, variables, functions, constants, and template parameters all get distinct token types. Modifiers convey declaration sites, static access, readonly, deprecated, and abstract status.
- **PHPStan diagnostics.** PHPStan errors appear inline as you edit. Auto-detects `vendor/bin/phpstan` or `$PATH`. Runs in the background without blocking native diagnostics. Configurable via `[phpstan]` in `.phpantom.toml` (`command`, `memory-limit`, `timeout`). "Ignore PHPStan error" and "Remove unnecessary @phpstan-ignore" code actions manage inline ignore comments.
- **Formatting.** Built-in PHP formatting (PER-CS 2.0 style). Formatting works out of the box without any external tools. Projects that depend on php-cs-fixer or PHP_CodeSniffer in their `composer.json` `require-dev` automatically use those tools instead (both can run in sequence). Per-tool command overrides and disable switches in `[formatting]` in `.phpantom.toml`.
- **Inlay hints.** Parameter name and by-reference indicators appear at call sites. Hints are suppressed when the argument already makes the parameter obvious: variable names matching the parameter, property accesses with a matching trailing identifier, string literals whose content matches, well-known single-parameter functions like `count` and `strlen`, and spread arguments. Named arguments never receive a redundant hint.
- **PHPDoc block generation.** Typing `/**` above any declaration generates a docblock skeleton. Tags are only emitted when the native type hint needs enrichment. Properties and constants always get `@var`. Class-likes with templated parents or interfaces get `@extends`/`@implements` tags. Uncaught exceptions get `@throws` with auto-import. Works both via completion and on-type formatting.
- **Syntax error diagnostic.** Parse errors from the Mago parser now appear as Error-severity diagnostics instantly as you type.
- **Implementation error diagnostic.** Concrete classes that fail to implement all required methods from their interfaces or abstract parents are now flagged with an Error-severity diagnostic on the class name. The existing "Implement missing methods" quick-fix appears inline alongside the error.
- **Argument count diagnostic.** Flags function and method calls that pass too few arguments. The "too many arguments" check is off by default (PHP silently ignores extra arguments) and can be enabled with `extra-arguments = true` in the `[diagnostics]` section of `.phpantom.toml`.
- **Completion item documentation.** Selecting a completion item in the popup now shows rich documentation including the full typed signature, description, deprecation notice, and parameter details. Previously only the class name was shown.
- **Method commit characters.** Typing `(` while a method completion is highlighted auto-accepts it and begins the argument list.
- **Document Symbols.** The outline sidebar and breadcrumbs now show classes, interfaces, traits, enums, methods, properties, constants, and standalone functions with correct nesting, icons, visibility detail, and deprecation tags.
- **Workspace Symbols.** "Go to Symbol in Workspace" (Ctrl+T / Cmd+T) searches across all indexed files including vendor classes. Results include namespace context and deprecation markers, sorted by relevance.
- **Type Hierarchy.** "Show Type Hierarchy" on any class, interface, trait, or enum reveals its supertypes and subtypes with full up-and-down navigation through the inheritance tree, including cross-file resolution and transitive relationships.
- **Code Lens.** Clickable annotations above methods that override a parent class method or implement an interface method. Clicking navigates to the prototype declaration.
- **Update docblock.** Code action on a function or method whose existing docblock is out of sync with its signature. Adds missing `@param` tags, removes stale ones, reorders to match the signature, fixes contradicted types, and removes redundant `@return void`. Refinement types and unrelated tags are preserved. Only triggers on the signature or the preceding docblock, not inside the function body.
- **Change visibility.** Code action on any method, property, constant, or promoted constructor parameter offers to change its visibility (`public`, `protected`, `private`). Only triggers on the declaration signature, not inside the body.
- **`@throws` code actions.** Quick-fixes for adding missing and removing unnecessary `@throws` tags, triggered by PHPStan diagnostics. Adding inserts the tag and a `use` import when needed. Removing cleans up orphaned blank lines and deletes the entire docblock when it would be empty. The diagnostic disappears on the next keystroke without waiting for the next PHPStan run.
- **File rename on class rename.** Renaming a class whose file follows PSR-4 naming now also renames the file to match. The file is only renamed when it contains a single class-like declaration and the editor supports file rename operations.
- **Folding Ranges.** AST-aware code folding for class bodies, method/function bodies, closures, arrays, argument/parameter lists, control flow blocks, doc comments, and consecutive single-line comment groups.
- **Selection Ranges.** Smart select / expand selection returns AST-aware nested ranges from innermost to outermost.
- **Document Links.** `require`/`include` paths are now Ctrl+Clickable. Path resolution supports string literals, `__DIR__` concatenation, `dirname(__DIR__)`, `dirname(__FILE__)`, and nested `dirname` with levels.
- **Analyze command.** `phpantom_lsp analyze` scans a Composer project and reports PHPantom's own diagnostics in a PHPStan-like table format. Useful for measuring type coverage across an entire codebase without opening files one by one. Accepts an optional path argument to limit the scan to a single file or directory. Output includes diagnostic identifiers and supports `--severity` filtering and `--no-colour` for CI.
- **Null-coalesce (`??`) type refinement.** When the left-hand side of `??` is provably non-nullable (e.g. `new Foo()`, `clone $x`, a literal), the right-hand side is recognized as dead code and the result resolves to the LHS type only. When the LHS is nullable (e.g. a `?Foo` return type), `null` is stripped from the LHS and the result is the union of the non-null LHS with the RHS.
- **`@mixin` generic substitution.** When a class declares `@mixin Foo<T>`, the generic arguments are now preserved and substituted into the mixin's members, including through multi-level inheritance chains.
- **PHPDoc `@var` completion.** Inline `@var` above variable assignments sorts first and pre-fills the inferred type when available. Template parameters from `@template` enrich `@param`, `@return`, and `@var` type hints.
- **`@see` and `@link` improvements.** `@see` references in docblocks now work with go-to-definition (class, member, and function forms). Hover popups show all `@link` and `@see` URLs as clickable links. Deprecation diagnostics include `@see` targets when the `@deprecated` docblock references them.
- **Progress indicators.** Go to Implementation and Find References now show a progress indicator in the editor while scanning.
- **Phar archive class resolution.** Classes inside `.phar` archives (e.g. PHPStan's `phpstan.phar`) are now discovered and indexed automatically. No PHP runtime needed. Only uncompressed phars are supported (the format used by PHPStan and most other phar-distributed tools).
- **PSR-0 autoload support.** Packages that use the legacy PSR-0 autoloading standard are now discovered automatically.
- **Global config.** Settings from a global `.phpantom.toml` in the user's config directory (typically `~/.config/phpantom_lsp/.phpantom.toml`) are now loaded as defaults. Project-level configs take precedence. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/39.
- **Config schema.** A JSON schema for `.phpantom.toml` is now bundled, enabling autocompletion and validation in editors that support TOML schemas. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/38.

### Changed

- **Pull diagnostics.** Diagnostics are now delivered via the LSP 3.17 pull model when the editor supports it. The editor requests diagnostics only for visible files, and cross-file invalidation no longer recomputes every open tab. Clients without pull support fall back to the previous push model automatically.
- **Hover type accuracy.** Hover now resolves variable types through the same pipeline as completion, so all narrowing features (instanceof, assert, custom type guards, in_array) apply. When the cursor is inside a specific if/else branch, hover shows only the type visible in that branch. Complex expressions like null-coalesce chains, array shapes, empty arrays, and unresolved symbols all display correctly.
- **Version-aware stub types.** Built-in function signatures that changed across PHP versions (e.g. `int|false` in 7.x becoming `int` in 8.0) now show the correct type for your project's PHP version. This eliminates false-positive diagnostics and incorrect completions from stale type annotations.
- **Completion labels.** Method and function completion items now show only parameter names in the label (e.g. `setName($name)`) with the return type displayed inline (e.g. `: User`). Properties and constants show just the type hint. The previous `Class: ClassName` detail line has been removed; class context is available in the documentation panel when the item is highlighted.
- **Completion sort order.** Member completion items are now sorted by kind (constants, then properties, then methods) before alphabetical order within each group. Union-type completions apply the same kind-based ordering within both the intersection and branch-only tiers.
- **Class name completion ranking.** Completions now rank by match quality first (exact match, then starts-with, then substring), so typing `Order` puts `Order` above `OrderLine` above `CheckOrderFlowJob` regardless of where the class comes from. Within each match quality group, use-imported and same-namespace classes appear first, followed by everything else sorted by namespace affinity (classes from heavily-imported namespaces rank higher).
- **Use-import completion.** Same-namespace classes no longer appear in `use` statement completions (PHP auto-resolves them without an import). Classes that are already imported are filtered out. Namespace affinity still ranks the remaining candidates.
- **Deprecation tags.** Completion items use the modern `tags: [DEPRECATED]` field instead of the legacy `deprecated` boolean. Both convey the same strikethrough rendering in editors.
- **Import class code action ordering.** The "Import Class" code action now sorts candidates by namespace affinity (derived from existing imports) instead of alphabetically, so the most likely namespace appears first.
- **Cross-file resolution.** Completion, hover, and go-to-definition no longer fail when one reference uses a leading backslash and another does not.
- **Embedded stubs track upstream master.** The bundled phpstorm-stubs are now pulled from the `master` branch instead of the latest GitHub release, matching what PHPStan does. This brings in upstream fixes and new PHP version annotations weeks or months before a formal release.

### Fixed

- **CLI analyze performance.** Single-file analysis is up to 5.8× faster. Full-project analysis of ~2 500 files is up to 10× faster.
- **Diagnostic performance on large files.** Unknown-member diagnostics on files with many member accesses are up to 7× faster.
- **Position encoding.** All LSP position conversions now correctly count UTF-16 code units, matching the LSP specification. Files containing emoji or supplementary Unicode characters no longer produce incorrect positions.
- **Rename and find references for parameters.** Renaming a parameter in a function, method, or closure now correctly updates all usages in the body and the `@param` tag in the docblock. Previously, parameters were scoped incorrectly because they sit physically before the opening `{` of the body, causing rename and find references to miss body usages when triggered from the parameter (and vice versa). Document highlight is also fixed.
- **Rename updates imports.** Renaming a class now updates `use` statement FQNs, preserves explicit aliases, and introduces an alias when the new name collides with an existing import.
- **False-positive diagnostics for `$this` inside traits.** Accessing host-class members via `$this->`, `self::`, `static::`, or `parent::` inside a trait method no longer produces "not found" warnings, including chain expressions and accesses inside closures or arrow functions nested within trait methods.
- **False-positive diagnostics for same-named variables in different methods.** Diagnostic resolution is now scoped to the enclosing function/method/closure body, so two methods using a variable like `$order` resolve it independently.
- **False positive on namespaced constants.** Standalone namespaced constant references (e.g. `\PHPStan\PHP_VERSION_ID`) no longer produce a spurious "Class not found" diagnostic. Previously the symbol map classified them as class references instead of constant references.
- **Diagnostic deduplication.** Multiple diagnostics on the same span or line are no longer collapsed into one. If PHPStan reports five issues on a line, all five are shown. When PHPantom and PHPStan both flag the same issue, the more precise native diagnostic wins.
- **Diagnostics.** Enums that implement interfaces are now checked for missing methods. Scalar member access errors detect method-return chains where an intermediate call returns a scalar type. By-reference `@param` annotations no longer produce a false "unknown class" diagnostic.
- **Removed PHP symbols in stubs.** Functions, methods, and classes annotated with `@removed X.Y` in phpstorm-stubs are now filtered out when the target PHP version is at or above the removal version. Previously symbols like `mysql_tablename` (removed in PHP 7.0) and `each` (removed in PHP 8.0) appeared in completions and resolved without warnings.
- **Hover on union member access.** Hovering over a method, property, or constant on a union type (e.g. `$ambiguous->turnOff()` where `$ambiguous` is `Lamp|Faucet`) now shows hover information from all branches that declare the member, separated by a horizontal rule. Previously only the first matching branch was shown. When both branches inherit the member from the same declaring class, the hover is deduplicated to a single entry.
- **Hover on inherited members.** Hovering over an inherited method, property, or constant now shows the declaring class in the code block (e.g. `class Model { public static function find(...) }`) instead of the class it was accessed on. Previously `User::find()` would incorrectly show `class User` even though `find()` is declared on `Model`.
- **Constant type inference.** Variables assigned from global constants (`$a = MY_CONST`) or class constants without type hints (`$b = Config::TIMEOUT`) now resolve to the type implied by the constant's initializer value. Integer, float, string, bool, null, and array literals are all recognised. Typed class constants (`public const string NAME = '...'`) continue to use their declared type hint.
- **Variable type after reassignment.** When a method parameter is reassigned mid-body (e.g. `$file = $result->getFile()`), subsequent member accesses now resolve against the new type instead of the original parameter type.
- **Variable assignments inside foreach loops.** Variables conditionally reassigned inside a `foreach` body are now visible after the loop.
- **Variable-to-variable type propagation.** Assignments like `$found = $pen` now resolve `$found` to the type of `$pen`. This also eliminates false-positive diagnostics when the initial assignment was `$found = null` and a later reassignment provided the real type.
- **Variable type inside self-referencing assignment RHS.** In `$request = new Foo(arg: $request->uuid)`, the `$request` reference inside the constructor arguments now correctly resolves to the original type instead of the type being assigned.
- **Variable resolution inside anonymous classes.** Variables inside anonymous class methods (e.g. closure parameters in `return new class extends Migration { ... }`) now resolve correctly. Previously, anonymous class bodies were invisible to the variable resolution pipeline because they appear as expressions inside statements rather than top-level class declarations.
- **Closure and arrow function variable scope.** Variable name completion now correctly respects PHP scoping rules for anonymous functions and arrow functions. Parameters and `use`-captured variables are visible inside closures. Arrow function parameters are visible inside the arrow body while the enclosing scope's variables remain accessible.
- **Function return type resolution across files.** Standalone functions that declare return types using short names from their own `use` imports now resolve correctly in consuming files. Function parameter types and `@throws` types are also resolved.
- **Native type override compatibility.** A docblock type only overrides a native type hint when it is a compatible refinement (e.g. `class-string<Foo>` can refine `string`, but `array<int>` no longer incorrectly overrides `string`).
- **PHPStan pseudo-type recognition.** Types like `non-positive-int`, `non-negative-int`, `non-zero-int`, `lowercase-string`, `truthy-string`, `callable-object`, and many other PHPStan pseudo-types are now recognized across the entire pipeline.
- **Nullable and generic types in class lookup.** Variables typed as `?ClassName` or `Collection<Item>` now resolve correctly across all code paths.
- **Generic substitution through transitive interface chains.** When a class implements an interface that itself extends another generic interface, template parameters are now substituted at each level instead of propagating raw template parameter names.
- **Generic shape substitution.** Template parameters inside array shapes (`array{data: T}`) and object shapes (`object{name: T}`) are now correctly substituted when inherited through `@extends`.
- **Type narrowing with same-named classes from different namespaces.** instanceof narrowing now correctly distinguishes classes that share a short name but live in different namespaces (e.g. `Contracts\Provider` vs `Concrete\Provider`).
- **Guard clause narrowing across instanceof branches.** After `if ($x instanceof Y) { return; }`, subsequent `instanceof` checks on the same variable no longer incorrectly resolve to `Y`.
- **`instanceof self/static/parent` narrowing.** Type narrowing with `instanceof self`, `instanceof static`, and `instanceof parent` now works correctly in all contexts (assert, if-blocks, guard clauses, compound conditions).
- **Type narrowing inside `return` statements.** `instanceof` checks in `&&` chains and ternary conditions now narrow the variable type when the expression is the operand of a `return` statement.
- **Inline array access on method returns.** Expressions like `$c->items()[0]->getLabel()` now resolve the element type correctly for both completion and diagnostics.
- **Array shape bracket access.** Variables assigned from string-key bracket access on array shapes (`$name = $data['name']`) now resolve to the correct value type. Chained access (`$first = $result['items'][0]`) walks through shape keys and generic element types in sequence.
- **Ternary and null-coalesce member access.** Accessing a member on a ternary or null-coalesce expression (e.g. `($a ?: $b)->property`, `($x ?? $y)->method()`) now resolves correctly for hover, go-to-definition, and diagnostics.
- **Null-safe method chain resolution.** Null-safe method calls (`$obj?->method()`) now resolve the return type correctly for variable type inference, including cross-file chains.
- **Clone expressions.** `(clone $var)->` now resolves to the same type as `$var`, providing correct completion, hover, and diagnostics.
- **`self::/static::/parent::` in member access chains.** Expressions like `self::Active->value` inside an enum method now resolve correctly. Previously, `self`, `static`, and `parent` were only recognized as bare subjects, not when followed by `::MemberName` in a chain.
- **Inherited methods missing through deep stub chains.** Methods are now found on classes that inherit through multi-level chains where intermediate classes live in stubs.
- **Interface constants through multi-extends chains.** Constants defined on parent interfaces are now found when an interface extends multiple other interfaces.
- **Double parentheses when completing calls.** Completing a function, constructor, or static method name when parentheses already follow the cursor (e.g. `array_m|()`, `new Gadge|()`, `throw new Excepti|()`) no longer inserts a second pair of parentheses. Previously only `->` and `::` method calls were handled.
- **Namespace alias completion.** Typing a class name through a namespace alias (e.g. `OA\Re` with `use OpenApi\Attributes as OA`) now correctly suggests classes under the aliased namespace.
- **Catch clause completion.** Throwable interfaces and abstract exception classes now appear in catch clause completions.
- **Type-hint and PHPDoc completion.** Traits are now excluded from completions in parameter types, return types, property types, and PHPDoc type tags. `@throws` continues to use Throwable-filtered completion.
- **Trait alias go-to-definition.** Clicking a trait alias (e.g. `$this->__foo()` from `use Foo { foo as __foo; }`) now jumps to the trait method instead of the class's own same-named method.
- **Self-referential array key assignments no longer crash.** Patterns like `$numbers['price'] = $numbers['price']->add(...)` no longer cause a stack overflow during hover or completion.
- **Eloquent `morphedByMany` relationships.** The inverse side of polymorphic many-to-many relationships is now recognised. Virtual properties and `_count` properties are synthesized for models using this relationship type.
- **Virtual property merging.** Native type hints are now considered when determining virtual property specificity, preventing properties with native PHP type declarations from being incorrectly overridden by less specific virtual properties.

## [0.5.0] - 2026-03-12

### Added

- **Diagnostics.** Unknown classes, unknown members, and unknown functions are flagged with appropriate severity. An opt-in unresolved member access diagnostic is available via `.phpantom.toml`.
- **Find References.** Locate every usage of a symbol across the project. Supports classes, methods, properties, constants, functions, and variables. Variable references are scoped to the enclosing function or closure. Member references are scoped to the class hierarchy, so unrelated classes sharing a method name are excluded.
- **Rename.** Rename variables, classes, methods, properties, functions, and constants across the workspace. Variable renames are scoped to their enclosing function or closure.
- **Deprecation support.** `@deprecated` tags and `#[Deprecated]` attributes surface in hover, completion strikethrough, and diagnostics. A quick-fix code action rewrites deprecated calls when a `replacement` template is available.
- **Document highlighting.** Placing the cursor on a symbol highlights all occurrences in the current file. Variables are scoped to their enclosing function or closure with write vs. read distinction.
- **Implement missing methods.** Code action that generates method stubs when a class is missing required interface or abstract method implementations.
- **Project configuration.** `.phpantom.toml` for per-project settings: PHP version override, diagnostic toggles, and indexing strategy. Run `phpantom --init` to generate a default config.
- **Reverse go-to-implementation.** Go-to-implementation on a concrete method jumps to the interface or abstract class that declares the prototype, and vice versa.
- **Go to Type Definition.** Jump from a variable, property, method call, or function call to the class declaration of its resolved type. Union types produce multiple locations.
- **Self-generated classmap.** PHPantom works without `composer dump-autoload -o`. Missing or incomplete classmaps are supplemented by scanning autoload directories. Non-Composer projects are supported by scanning all PHP files.
- **Monorepo support.** Discovers subdirectories that are independent Composer projects and processes each through the full pipeline.
- **`@implements` generic resolution.** `@implements Interface<ConcreteType>` substitutes template parameters on the interface's methods and properties. Foreach iteration on generic iterable interfaces resolves value and key types.
- **Interface template inheritance.** Implementing classes inherit `@template` parameters, bindings, conditional return types, and type assertions from their interfaces.
- **Function-level `@template` with generic return types.** Functions that use `@template` parameters inside generic return types now resolve concrete types from call-site arguments.
- **Generic `@phpstan-assert` with `class-string<T>`.** Assertion methods that accept a `class-string<T>` parameter resolve the narrowed type from the call-site argument.
- **Property-level narrowing.** `if ($this->prop instanceof Foo)` narrows `$this->prop` in then/else bodies and after guard clauses.
- **Inline `&&` short-circuit narrowing.** The right-hand side of `&&` now sees the narrowed type from the left-hand side.
- **Compound negated guard clause narrowing.** `if (!$x instanceof A && !$x instanceof B) { return; }` narrows `$x` to `A|B` in the surviving code.
- **Closure variable scope isolation.** Variables outside a closure are no longer offered as completions unless captured via `use()`.
- **Pipe operator (PHP 8.5).** `$input |> trim(...) |> createDate(...)` resolves through the chain.
- **AST-based array type inference.** Array shape keys, element access, spread elements, and push-style assignments all resolve through an AST walker.
- **`new $classStringVar` and `$classStringVar::method()`.** Class-string variables resolve for `new` and static member access.
- **Invoked closure and arrow function return types.** `(fn(): Foo => ...)()` and `(function(): Bar { ... })()` resolve to their return type.
- **Docblock navigation.** Go-to-definition and hover work on class names inside callable types, array/object shape value types, and object shape properties.
- **GTD from parameter and property variables.** Clicking a parameter or property at its definition site jumps to the type hint class.
- **PHP version-aware stubs.** Detects the target PHP version from `composer.json` and filters built-in stub signatures accordingly.
- **`@param-closure-this`.** `$this` inside a closure resolves to the type declared by `@param-closure-this` on the receiving parameter.
- **Non-Composer function and constant discovery.** Cross-file function completion, go-to-definition, and constant resolution for projects without `composer.json`.
- **Indexing progress indicator.** The editor shows a progress bar during workspace initialization, including per-subproject progress in monorepos.
- **Pass-by-reference parameter type inference.** After calling a function with a typed `&$var` parameter, the variable acquires that type.
- **`iterator_to_array()` element type.** Resolves the element type from the iterator's generic annotation.
- **Enum case properties.** `$case->name` and `$case->value` resolve on enum case variables.
- **Inline `@var` on promoted constructor properties.** Overrides the native type hint, matching existing `@param` support.
- **`--version` and `--help` CLI flags.** Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/7.

### Changed

- **Resolution engine rewritten on AST.** Variable type inference, call return types, and go-to-definition all run through the AST walker for better accuracy.
- **Hover redesigned.** Short names with `namespace` line, actual default values, `@link` URLs, precise token highlighting, constructor signatures on `new`, `@template` details, enum case listing, trait member listing, origin indicators, and deprecated explanations.
- **Signature help enriched.** Compact parameter list with native types, per-parameter `@param` descriptions, default values, and attribute parenthesis support.
- **Faster resolution and lower memory usage.**
- **Parallel workspace indexing.** File parsing, PSR-4 scanning, and vendor scanning run across all CPU cores. `.gitignore` rules are respected.
- **Two-phase diagnostic publishing.** Cheap diagnostics (unused imports, deprecation) publish immediately; expensive diagnostics (unknown classes/members/functions) arrive in a second pass.
- **Merged classmap + self-scan pipeline.** Composer classmaps and self-scanning work together instead of being mutually exclusive. Stale classmaps are supplemented automatically.
- **Automatic stub fetching.** The build script downloads phpstorm-stubs automatically when missing. Composer is no longer needed to build PHPantom. Contributed by @calebdw in https://github.com/AJenbo/phpantom_lsp/pull/16.
- **Feature comparison table corrected.** Phactor capabilities updated in the README. Contributed by @dantleech in https://github.com/AJenbo/phpantom_lsp/pull/10.

### Fixed

- **Cross-file inheritance from global-scope classes imported via `use`.**
- **Inherited `@method` and `@property` tags across files.**
- **Diagnostics refresh across open files when a class signature changes.**
- **Variable types resolve through ternary, elvis, null-coalesce, and match assignments.**
- **`instanceof` narrowing no longer widens specific types.**
- **Elseif chain narrowing and sequential assert narrowing.**
- **`@phpstan-type` aliases in foreach, `list()`, and key types.**
- **False-positive unknown-class warnings on PHPStan type syntax.**
- **Go-to-implementation no longer produces false positives across namespaces.**
- **`__invoke()` return type resolution.** Works with chaining, foreach, and parenthesized invocations.
- **Enum `from()` and `tryFrom()` chaining.**
- **`static`/`self`/`$this` in method return types used as iterable expressions.**
- **Mixed `->` then `::` accessor chains.**
- **Inline `(new Foo)->method()` chaining.**
- **`?->` null-safe chain resolution.**
- **Array function resolution for `array_pop`, `array_filter`, `array_values`, `end`, `array_map`.**
- **Inline `@var` annotations no longer leak across scopes.**
- **Literal string conditional return types.**
- **Class constant and enum case assignment resolution.**
- **Go-to-definition on trait `as` alias and `insteadof` declarations.**
- **Inline array-element function calls resolve correctly in diagnostics.** `end($obj->items)->method()` no longer produces a false diagnostic.
- **Double-negated `instanceof` narrowing.**
- **Self-referential array key assignments no longer crash.**

## [0.4.0] - 2026-03-01

### Added

- **Signature help.** Parameter hints in function/method calls with active parameter highlighting.
- **Hover.** Type, signature, and docblock in a Markdown popup for all symbol kinds.
- **Closure and callable inference.** Untyped closure parameters inferred from the callable signature. First-class callable syntax resolves return types.
- **Laravel Eloquent.** Relationships, scopes, Builder forwarding, factories, custom collections, casts, accessors, mutators, `$attributes`, and `$visible`.
- **Type narrowing.** `in_array()` with strict mode, early return guards, `instanceof` in ternaries and with interfaces.
- **Anonymous class support.** `$this->` resolves inside anonymous classes with full inheritance support.
- **Context-aware completions.** `extends`, `implements`, `use` inside class body, union member sorting, namespace segments, string literal suppression.
- **Additional resolution.** Multi-line chains, nested array keys, generator yield types, conditional return types with template substitution, switch/unset variable tracking.
- **Transitive interface go-to-implementation.**

### Fixed

- Visibility filtering, scope isolation, static call chains, `static` return type, trait resolution, mixin fluent chains, go-to-definition accuracy, import handling, UTF-8 boundaries, and parenthesized RHS expressions.

## [0.3.0] - 2026-02-21

### Added

- **Go-to-implementation.** Interface/abstract class to all concrete implementations.
- **Method-level `@template`.** Infers `T` from the call-site argument.
- **`@phpstan-type` / `@psalm-type` aliases** and `@phpstan-import-type`.
- **Array function type preservation.** `array_filter`, `array_map`, `array_pop`, `current`, etc.
- **Early return narrowing.** Guard clauses narrow types for subsequent code.
- **Callable variable invocation.** `$fn()->` resolves return types.
- **Additional resolution.** Spread operators, trait `insteadof`/`as`, chained assignments, destructuring, foreach on function returns, type hint completion, try-catch suggestions.

### Fixed

- PHPDoc type parsing and internal stability fixes.

## [0.2.0] - 2026-02-18

### Added

- **Generics.** Class-level `@template` with `@extends` substitution. Method-level `class-string<T>`. Generic trait substitution.
- **Array shapes and object shapes.** Key completion from literals, incremental assignments, destructuring, element access.
- **Foreach type resolution.** Generic iterables, array shapes, `Collection<User>`, `Generator<int, Item>`, `IteratorAggregate`.
- **Expression type inference.** Ternary, null-coalescing, and match expressions.
- **Additional completions.** Named arguments, variable name suggestions, standalone functions, `define()` constants, PHPDoc tags, deprecated members, promoted property types, property chaining, `require_once` discovery, go-to type definition.

### Fixed

- `@mixin` context for return types, global class imports, namespace resolution, and aliased class go-to-definition.

## [0.1.0] - 2026-02-16

Initial release.

### Added

- **Completion.** Methods, properties, and constants via `->`, `?->`, and `::` with visibility filtering.
- **Type resolution.** Inheritance merging, `self`/`static`/`parent`, union types, nullsafe chains.
- **PHPDoc support.** `@return`, `@property`, `@method`, `@mixin`, conditional return types, inline `@var`.
- **Type narrowing.** `instanceof`, `is_a()`, `@phpstan-assert`.
- **Enum support.** Case completion and `UnitEnum`/`BackedEnum` interface members.
- **Go-to-definition.** Classes, methods, properties, constants, functions, `new` expressions, variables.
- **Class name completion with auto-import.**
- **PSR-4 lazy loading and Composer classmap support.**
- **Embedded phpstorm-stubs.**
- **Zed editor extension.**

[Unreleased]: https://github.com/AJenbo/phpantom_lsp/compare/0.8.0...HEAD
[0.8.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.7.0...0.8.0
[0.7.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.5.0...0.6.0
[0.5.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.3.0...0.4.0
[0.3.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.2.0...0.3.0
[0.2.0]: https://github.com/AJenbo/phpantom_lsp/compare/0.1.0...0.2.0
[0.1.0]: https://github.com/AJenbo/phpantom_lsp/commits/0.1.0
