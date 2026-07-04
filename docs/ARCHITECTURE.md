# PHPantom Architecture

This document explains how PHPantom resolves PHP symbols — classes, interfaces, traits, enums, and functions — across files and from the PHP standard library.

## Overview

PHPantom is a language server for PHP projects. It provides completion, go-to-definition, go-to-implementation, find references, hover, signature help, and diagnostics. It works by:

1. **Parsing** PHP files into lightweight `ClassInfo` / `FunctionInfo` structures (not a full AST — just the information needed for IDE features).
2. **Caching** parsed results in an in-memory `uri_classes_index` keyed by file URI.
3. **Building** a precomputed symbol map (`symbol_maps`) during parsing for O(log n) go-to-definition lookups and call-site detection for signature help.
4. **Resolving** symbols on demand through a multi-phase lookup chain.
5. **Merging** inherited members (from parent classes, traits, interfaces, and mixins) at resolution time.

### Multi-file scanning

Most features (completion, go-to-definition, hover, signature help, diagnostics) use maps or known file names and never walk directories. Only two features do multi-file scanning:

- **Go-to-implementation** scans for concrete classes that implement an interface or extend an abstract class (see `find_implementors`). Walks PSR-4 source directories only.
- **Find References** scans for all occurrences of a symbol across the project (see `ensure_workspace_indexed`). Walks the entire workspace root.

Both features follow the same principle for vendor code: the `fqn_uri_index` (populated from Composer's classmap and the self-scanner) is the source of truth. Vendor directories are never walked. User PSR-4 roots from `composer.json` are walked because user files change between `dump-autoload` runs.

The two walkers differ in scope because GTI only needs class declarations (which live in PSR-4 roots), while Find References needs any usage of a symbol, which could be in a standalone script, config file, or `index.php` at the project root.

## Design Philosophy

PHPantom is built in layers. Each layer is independently useful and independently testable.

- **Layer 0: Stubs.** Embedded PHP standard library types. Available immediately, no project context needed.
- **Layer 1: Single file.** Parse the open file, extract classes/functions/symbols. Completion, hover, and go-to-definition work within the file with no cross-file resolution at all.
- **Layer 2: On-demand resolution.** When a symbol references a class in another file, resolve it through the `fqn_uri_index` or PSR-4 and parse that file. Only the files actually needed are touched.
- **Layer 3: FQN-to-URI index.** A name-to-URI index covering the whole project. Enables class name completion and O(1) cross-file lookup. Built from Composer's classmap or self-generated via a fast byte-level scan.
- **Layer 4: Full index (opt-in).** Background-parse every file in the fqn_uri_index. Enables workspace symbols, fast find-references, and rich completion item detail.

Each layer builds on the one below it. A bug in the FQN index doesn't break single-file completion. A slow full index doesn't block on-demand resolution. New features can be developed and tested against the lower layers without waiting for a full project scan. This is also why PHPantom starts fast: Layer 0-2 are ready in milliseconds, Layer 3 takes seconds, and Layer 4 (when enabled) fills in over the following minute.

## Module Layout

```
src/
├── lib.rs                  # Backend struct, state, constructors
├── main.rs                 # Entry point (stdin/stdout LSP transport)
├── server.rs               # LSP protocol handlers (initialize, didOpen, completion, …)
├── types.rs                # Data structures (ClassInfo, MethodInfo, SubjectExpr, ResolvedCallableTarget, …)
├── composer.rs             # composer.json / PSR-4 autoload parsing
├── stubs.rs                # Embedded phpstorm-stubs (build-time generated index)
├── resolution.rs           # Multi-phase class/function lookup and name resolution
├── inheritance.rs          # Base class inheritance merging (traits, parent chain)
├── symbol_map/
│   ├── mod.rs              # Data structures (SymbolSpan, SymbolKind, VarDefSite, CallSite, SymbolMap) and impl
│   ├── docblock.rs         # Docblock symbol extraction (type span emission, @template/@method tag scanning, navigability filter)
│   └── extraction.rs       # AST walk that builds a SymbolMap (extract_symbol_map and all extract_from_* helpers)
├── virtual_members/
│   ├── mod.rs              # VirtualMemberProvider trait, VirtualMembers struct, merge logic
│   ├── laravel.rs          # LaravelModelProvider (relationships, scopes, casts, accessors)
│   └── phpdoc.rs           # PHPDocProvider (@method, @property, @property-read, @property-write, @mixin)
├── subject_extraction.rs   # Shared helpers for extracting subjects before ->, ?->, ::
├── util.rs                 # Position conversion, class lookup, logging, directory walkers (collect_php_files, collect_php_files_gitignore)
├── parser/
│   ├── mod.rs              # Top-level parse entry points (parse_php, parse_functions, …)
│   ├── classes.rs          # Class, interface, trait, enum, and anonymous class extraction
│   ├── functions.rs        # Standalone function and define() constant extraction
│   ├── use_statements.rs   # use statement and namespace extraction
│   └── ast_update.rs       # update_ast orchestrator and name resolution helpers
├── docblock/
│   ├── mod.rs              # Re-exports from submodules
│   ├── tags.rs             # PHPDoc tag extraction (@return, @var, @mixin, @deprecated, …)
│   ├── templates.rs        # Template/generics/type-alias tags (@template, @extends, …)
│   ├── virtual_members.rs  # Virtual member tags (@property, @method)
│   ├── conditional.rs      # PHPStan conditional return type parsing
│   └── types.rs            # Type cleaning utilities (clean_type, strip_nullable, …)
├── completion/
│   ├── mod.rs              # Submodule declarations + backward-compatible re-exports
│   ├── handler.rs          # Top-level completion request orchestration
│   ├── target.rs           # Extract what the user is completing (subject + access kind)
│   ├── resolver.rs         # Resolve subject → ClassInfo (type resolution engine), shared resolve_callable_target
│   ├── call_resolution.rs  # Call expression and callable target resolution
│   ├── builder.rs          # Build LSP CompletionItems from resolved ClassInfo
│   ├── array_shape.rs      # Array shape key completion and raw variable type resolution
│   ├── named_args.rs       # Named argument completion inside function/method call parens
│   ├── use_edit.rs         # Use-statement insertion helpers
│   ├── variable/           # Variable resolution
│   │   ├── resolution.rs       # Variable type resolution via assignment scanning
│   │   ├── completion.rs       # Variable name completions and scope collection
│   │   ├── rhs_resolution.rs   # Right-hand-side expression resolution
│   │   ├── class_string_resolution.rs  # Class-string variable resolution ($cls = User::class)
│   │   ├── raw_type_inference.rs   # Raw type inference (array shapes, array functions, generators)
│   │   ├── foreach_resolution.rs   # Foreach value/key and array destructuring type resolution
│   │   └── closure_resolution.rs   # Closure and arrow-function parameter resolution
│   ├── types/              # Type resolution
│   │   ├── resolution.rs       # Type-hint string → ClassInfo mapping (unions, generics, aliases)
│   │   ├── narrowing.rs        # instanceof / assert / custom type guard narrowing
│   │   └── conditional.rs      # PHPStan conditional return type resolution at call sites
│   ├── context/            # Context-specific completion
│   │   ├── catch_completion.rs # Smart exception type completion inside catch() clauses
│   │   ├── class_completion.rs # Class name completions (class, interface, trait, enum)
│   │   ├── constant_completion.rs  # Global constant name completions
│   │   ├── function_completion.rs  # Standalone function name completions
│   │   ├── namespace_completion.rs # Namespace declaration completions
│   │   └── type_hint_completion.rs # Type completion in parameter lists, return types, properties
│   ├── phpdoc/             # PHPDoc completion
│   │   ├── mod.rs              # PHPDoc tag completion inside /** … */ blocks
│   │   └── context.rs          # PHPDoc context detection and symbol info extraction
│   └── source/             # Source analysis
│       ├── comment_position.rs # Comment and docblock position detection
│       ├── helpers.rs          # Source-text scanning helpers (closure/callable return types, new-expression parsing)
│       └── throws_analysis.rs  # Shared throw-statement scanning and @throws tag lookup
├── signature_help.rs       # Signature help: parameter hints inside function/method call parens
├── hover/
│   └── mod.rs              # Hover handler: symbol-map dispatch, type/signature/docblock formatting
├── definition/
│   ├── mod.rs              # Submodule declarations
│   ├── resolve.rs          # Core go-to-definition: symbol-map dispatch + text-based fallback
│   ├── member.rs           # Member-access resolution (->method, ::$prop, ::CONST) with stored offsets
│   ├── variable/
│   │   ├── mod.rs          # VarDefSearchResult enum, Backend methods, tests
│   │   ├── var_definition.rs # AST walk finding variable definition sites
│   │   └── type_hint.rs    # AST walk extracting type hints at definition sites
│   └── implementation.rs   # Go-to-implementation (interface/abstract → concrete classes)
├── references/
│   ├── mod.rs              # Find References handler: same-file and cross-file symbol scanning
│   └── tests.rs            # Unit tests for find-references
├── highlight/
│   └── mod.rs              # Document highlighting: same-file symbol occurrence highlighting
├── diagnostics/
│   ├── mod.rs              # Diagnostic collection and publishing (skips vendor files)
│   ├── deprecated.rs       # @deprecated usage diagnostics (strikethrough)
│   └── unused_imports.rs   # Unused use-statement dimming
build.rs                    # Parses PhpStormStubsMap.php, generates stub index
stubs/                      # Composer vendor dir for jetbrains/phpstorm-stubs
tests/
├── common/mod.rs           # Shared test helpers and minimal PHP stubs
├── completion_*.rs         # Completion integration tests (by feature area)
├── definition_*.rs         # Go-to-definition integration tests
├── hover.rs                # Hover integration tests
├── signature_help.rs       # Signature help integration tests
├── implementation.rs       # Go-to-implementation integration tests
├── document_highlight.rs   # Document highlighting integration tests
├── docblock_*.rs           # Docblock parsing and type tests
├── parser.rs               # PHP parser / AST extraction tests
├── composer.rs             # Composer integration tests
└── …
```

## External Crates

PHPantom uses three crates from the [Mago](https://github.com/carthage-software/mago)
project for PHP parsing and type representation:

| Crate              | Purpose                                                |
| ------------------ | ------------------------------------------------------ |
| `mago-docblock`    | Structured PHPDoc parsing (replaces manual scanning)   |
| `mago-names`       | Use-statement and namespace resolution                 |
| `mago-type-syntax` | PHPStan/Psalm type expression parsing into `PhpType`   |

`PhpType` (`src/php_type.rs`) is an owned enum that wraps the borrowed
`mago_type_syntax::ast::Type` AST. Every type-carrying field in the
data model (`type_hint`, `return_type`, `native_type_hint`,
`native_return_type`, `asserted_type`, `template_param_bounds` values,
generics type arguments, `ResolvedType::type_string`) uses `PhpType`.
The substitution engine in `inheritance.rs` operates on
`HashMap<String, PhpType>` so that template parameter replacement
never re-parses type strings.

Several other Mago crates were evaluated and ruled out:

| Crate              | Reason                                                                  |
| ------------------ | ----------------------------------------------------------------------- |
| `mago-codex`       | Replaces `ClassInfo` model with one that cannot carry `LaravelMetadata`. |
| `mago-semantics`   | 12K false positives on Laravel; no way to inject our type context.       |
| `mago-linter`      | Same problem; `Integration::Laravel` is surface-level only.              |
| `mago-fingerprint` | Requires `mago-names` for limited value; `signature_eq` already works.   |

All Mago crates are pinned to the same release. When upgrading, update
all Mago crates in a single commit and run the test suite.

### Docblock string helpers kept by design

The following functions in `type_strings.rs` operate on raw docblock
text (tokenizing tag descriptions, separating type tokens from
parameter names) rather than on structured type fields, and have no
`PhpType` equivalent:

- `split_type_token` — extracts a type token from a tag description
- `clean_type` — strips `?`, leading `\`, trailing punctuation
- `split_union_depth0` — splits raw union text at depth 0
- `split_generic_args` — splits raw generic arguments at depth 0
- `PHPDOC_TYPE_KEYWORDS` — completion candidate list for PHPDoc types

## Go-to-Definition Architecture

The definition subsystem uses a **three-tier** approach, from fastest to slowest:

### Tier 1: Precomputed Symbol Map (primary)

During `update_ast`, every navigable symbol occurrence in a file is recorded as a `SymbolSpan` in a sorted vec stored in `symbol_maps` (keyed by file URI). Each span records the byte range and a `SymbolKind` variant:

| `SymbolKind`        | What it captures                                                                                                                |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `ClassReference`    | Class/interface/trait/enum names in type contexts (`new Foo`, `extends`, `implements`, type hints, catch, docblock types)       |
| `ClassDeclaration`  | Class name at its declaration site (cursor already at definition)                                                               |
| `MemberAccess`      | `->`, `?->`, `::` member names with the subject text and static/method-call flags                                               |
| `Variable`          | `$variable` tokens (both usage and definition sites)                                                                            |
| `FunctionCall`      | Standalone function call names                                                                                                  |
| `SelfStaticParent`  | `self`, `static`, `parent` keywords in navigable contexts                                                                       |
| `ConstantReference` | Constant names (`define()` name, class constant access, standalone constant reference)                                          |
| `MemberDeclaration` | Method, property, or constant name at its declaration site (not navigable for go-to-definition, but needed for find-references) |

When a go-to-definition request arrives, `resolve_definition` converts the cursor position to a byte offset and does a binary search on the symbol map. If a `SymbolSpan` is found, it dispatches directly to the appropriate resolution path — no text scanning needed. If the offset falls in a gap (whitespace, string interior, comment interior, etc.), the request is instantly rejected.

Docblock type references (`@param`, `@return`, `@var`, `@template`, `@method`, etc.) are extracted by a dedicated string scanner during the AST walk, since docblocks are trivia in the `mago_syntax` AST and produce no expression/statement nodes. These use the same `ClassReference` kind and resolution path as code type hints.

The symbol map also stores:

- **Variable definition sites** (`var_defs`): records every assignment, parameter, foreach binding, catch variable, static/global declaration, and destructuring site with its byte offset, scope boundary, and `effective_from` offset (for assignments, this is the end of the statement so the RHS sees the previous definition). Go-to-definition for `$var` finds the most recent definition before the cursor within the enclosing scope via binary search.
- **Scope boundaries** (`scopes`): function, method, closure, and arrow function body ranges. Used by `find_enclosing_scope` to determine which scope the cursor is in.
- **Template parameter definitions** (`template_defs`): `@template` tag locations so that template parameter names (e.g. `TKey`, `TModel`) that appear in docblock types can be resolved to their declaration site.

### Tier 2: Stored Byte Offsets (cross-file jumps)

`MethodInfo`, `PropertyInfo`, `ConstantInfo`, and `FunctionInfo` each carry a `name_offset: u32` field recording the byte offset of the member's name token in the source file. `ClassInfo` carries `keyword_offset: u32` for the `class`/`interface`/`trait`/`enum` keyword. These are populated during parsing.

When a cross-file jump lands on a class or member, `find_member_position` and `find_definition_position` convert the stored offset directly to an LSP `Position` via `offset_to_position` — no line-by-line text scanning needed.

A value of `0` means "not available" (stubs parsed before offsets were stored, synthetic/virtual members). In that case, the system falls back to Tier 3.

### Tier 3: Docblock Tag Scanning (virtual members only)

The only remaining line-by-line text scanner is in `find_member_position` (`definition/member/file_lookup.rs`). When a member has no stored AST byte offset (`name_offset` is `None` or `0`), the function scans docblock comments for `@property` / `@property-read` / `@property-write` and `@method` tags that declare the virtual member. This is inherent to virtual members: they exist only as docblock annotations and have no AST node to store an offset for.

All cursor context detection (word extraction, member access detection, type-hint resolution) now relies exclusively on the precomputed `SymbolMap` and AST walks. The former text-based helpers `extract_word_at_position`, `extract_member_access_context`, and `resolve_type_hint_at_variable_text` have been removed.

### Variable Definition Resolution

Variable go-to-definition (`$var` → jump to definition) uses two layers:

1. **Symbol map** (`var_defs`): the primary path. Finds the most recent `VarDefSite` before the cursor within the enclosing scope. When the cursor is physically on a definition token (parameter, foreach binding, catch variable), it returns `None` so the caller can fall through to type-hint resolution.
2. **AST-based search** (`resolve_variable_definition_ast`): parses the file and walks the enclosing scope to find the definition site. Handles destructuring (`[$a, $b] = ...`, `list($a, $b) = ...`) and nested scopes correctly. Used as a fallback when the symbol map doesn't have a match. Returns `None` when the AST parse fails.

## Go-to-Type-Definition Architecture

"Go to Type Definition" (`textDocument/typeDefinition`) jumps from a symbol to the class declaration of its **resolved type**, rather than to the definition site. For example, if `$user` is typed as `User`, go-to-definition jumps to the `$user = ...` assignment, while go-to-type-definition jumps to `class User { … }`.

The handler (`definition/type_definition.rs`) reuses the same symbol-map lookup as go-to-definition and the same type-resolution pipelines as hover and completion:

1. **Symbol map lookup.** Convert the cursor position to a byte offset and binary-search the symbol map (same as go-to-definition, including the offset-1 retry for end-of-token cursors).

2. **Type resolution by symbol kind:**

   | `SymbolKind`                                                 | Resolution path                                                                                                                                                                                                  |
   | ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
   | `Variable`                                                   | `resolve_variable_type` (type-string path) with fallback to `resolve_variable_types` (ClassInfo path). `$this` resolves to the enclosing class.                                                           |
   | `MemberAccess`                                               | `resolve_target_classes` finds the subject's class, then the method's `return_type` or the property's `type_hint` is extracted. `self`/`static`/`$this` in return types are replaced with the owning class name. |
   | `SelfStaticParent`                                           | `self`/`static` resolve to the enclosing class; `parent` resolves to the parent class.                                                                                                                           |
   | `ClassReference`                                             | The type is the class itself.                                                                                                                                                                                    |
   | `FunctionCall`                                               | The function's `return_type` is extracted.                                                                                                                                                                       |
   | `ClassDeclaration`, `MemberDeclaration`, `ConstantReference` | No type definition target; returns `None`.                                                                                                                                                                       |

3. **Type string to class names.** `extract_class_names_from_type_string` splits union types at depth-0 `|` separators, strips `?` (nullable), generic parameters (`<…>`), array shapes (`{…}`), and trailing `[]`. Scalar types (`int`, `string`, `array`, `void`, `mixed`, `bool`, `float`, `null`, `false`, `true`, `never`, `callable`, `iterable`, `resource`, `object`) are excluded since they have no user-navigable declaration.

4. **Class name to location.** Each surviving class name is resolved via `resolve_class_reference` (the same function go-to-definition uses for class references), which tries same-file AST lookup, cross-file `fqn_uri_index` + `uri_classes_index`, PSR-4, and template parameter fallback. Duplicate locations are deduplicated.

For union types, the handler returns multiple locations (one per non-scalar class in the union), which editors display as a peek list.

## Signature Help Architecture

Signature help shows parameter hints when the cursor is inside the parentheses
of a function or method call. Detection uses a **two-tier** strategy:

### Tier 1: Precomputed Call Sites (primary)

During `update_ast`, every call expression in the file is recorded as a
`CallSite` in the `SymbolMap`. Each entry stores:

| Field             | Purpose                                                  |
| ----------------- | -------------------------------------------------------- |
| `args_start`      | Byte offset immediately after the opening `(`            |
| `args_end`        | Byte offset of the closing `)`                           |
| `call_expression` | The call target in `resolve_callable` format (see below) |
| `comma_offsets`   | Byte offsets of each top-level comma separator           |

Call sites are emitted for all five AST call kinds: `Call::Function`,
`Call::Method`, `Call::NullSafeMethod`, `Call::StaticMethod`, and
`Expression::Instantiation`. The `call_expression` string is built by
`expr_to_subject_text`, which recursively converts the AST expression into
the text format the resolver expects:

- `"functionName"` for standalone function calls
- `"$subject->method"` for instance and null-safe method calls
- `"ClassName::method"` for static method calls
- `"new ClassName"` for constructor calls

When a signature help request arrives, `detect_call_site_from_map` converts
the LSP position to a byte offset and searches the `call_sites` vec in
reverse for the innermost entry whose argument range contains the cursor.
The active parameter index is computed by counting how many precomputed
comma offsets fall before the cursor. This handles nesting, strings
containing commas/parens, and arbitrary chain depth correctly because the
offsets come from the parser's token stream.

### Tier 2: Text-Based Fallback

When the symbol map has no matching call site (typically because the parser
could not recover the call node from incomplete code, e.g. an unclosed `(`
while the user is typing), the text-based scanner
`detect_call_site_text_fallback` fires. It walks backward from the cursor
to find an unmatched `(`, extracts the call expression with simple
character-level scanning, and counts top-level commas. This path handles
simple calls reliably but cannot resolve property chains, method return
chains, or expressions containing balanced parentheses.

If the text fallback also fails to resolve the callable (e.g. because the
class context is missing from the broken AST), a content-patching strategy
inserts `);` at the cursor position and re-parses the file to recover
class context for resolution.

### Call Expression Resolution

`resolve_callable` dispatches on the call expression format:

1. **`new ClassName`** — loads the class via `class_loader`, finds
   `__construct`. Returns an empty parameter list when no constructor
   is defined.
2. **`$subject->method`** — splits at the last `->`, resolves the subject
   via `resolve_target_classes` (which handles `$this`, variables,
   property chains, call return chains, array access, etc.), then looks
   up the method on the resolved class.
3. **`ClassName::method`** — resolves `self`/`static`/`parent` keywords
   and bare class names via `class_loader`, with a fallback to
   `resolve_target_classes` for class-string variables
   (e.g. `$cls = Pen::class; $cls::make()`).
4. **`functionName`** — resolves via `resolve_function_name` (use map,
   namespace, stubs). Falls back to first-class callable resolution:
   if the expression is a `$variable`, scans backward for
   `$var = target(...)` and recursively resolves the underlying target.

## Symbol Resolution: `find_or_load_class`

When the LSP needs to resolve a class name (e.g. during completion on `Iterator::` or when following a type hint), it calls `find_or_load_class`. This method tries four phases in order, returning as soon as one succeeds:

```
find_or_load_class("Iterator")
│
├── Phase 0: fqn_class_index (O(1) hash hit)
│   Direct lookup by FQN. This is where cached results from
│   previous phases are found on subsequent lookups.
│   ↓ miss
│
├── Phase 1: fqn_uri_index (FQN → file URI)
│   Direct hash lookup in the unified FQN-to-URI index, populated
│   at init time from Composer's classmap, the self-scanner, and
│   phar archives. Covers all classes (including vendor) that don't
│   need PSR-4 path computation.
│   ↓ miss
│
├── Phase 2: PSR-4 resolution
│   Uses PSR-4 mappings from composer.json to locate the file on disk.
│   These mappings only cover user code (vendor PSR-4 is not loaded).
│   Example: "App\Models\User" → workspace/src/Models/User.php
│   Reads, parses, resolves names, caches in uri_classes_index.
│   ↓ miss
│
├── Phase 3: Embedded PHP stubs
│   Looks up the class name in the compiled-in stub index
│   (from phpstorm-stubs). Parses the stub PHP source, caches
│   in uri_classes_index under a phpantom-stub:// URI.
│   ↓ miss
│
└── None
```

### Caching

Every phase that successfully parses a file caches the result in `uri_classes_index`, `fqn_class_index`, and `fqn_uri_index`. This means:

- Phase 1 (fqn_uri_index) files are parsed once, then found via Phase 0.
- Phase 2 (PSR-4) files are parsed once, then found via Phase 0.
- Phase 3 (stubs) are parsed once, then found via Phase 0.
- Files opened in the editor are parsed on `didOpen`/`didChange` and always in Phase 0.

### Index Write Sites

**`update_ast_inner`** (`didOpen`/`didChange`): writes all three class
maps plus `file_imports`, `file_namespaces`, `resolved_names`,
`symbol_maps`, `parse_errors`.

**`parse_and_cache_content_versioned`** (lazy loading): writes only
`uri_classes_index`, `fqn_class_index`, `fqn_uri_index`. Editor-only
maps (`file_imports`, `file_namespaces`, etc.) are skipped for lazily
loaded files to reduce memory usage.

**`clear_file_maps`** (`didClose`): removes `uri_classes_index`,
`symbol_maps`, `file_imports`, `resolved_names`, `file_namespaces`.
Keeps `fqn_class_index` and `fqn_uri_index` so cross-file resolution
survives file close.

**Init functions**: populate `fqn_uri_index` from Composer classmap,
Drupal scans, PSR-0, phar archives, or full workspace scans. All
paths are converted to URI strings at init time.

### Lifecycle Invariants

1. Every class in `uri_classes_index` has a corresponding entry in
   `fqn_class_index`. Both write sites maintain this.
2. Every FQN in `fqn_class_index` has a corresponding entry in
   `fqn_uri_index`. Violating this breaks GTD after `didClose`.
3. `fqn_uri_index` entries are never removed by `didClose`. Once a
   class URI is registered it persists for the session (file renames
   trigger `didClose` + `didOpen` with the new URI).

## Embedded PHP Stubs

PHPantom bundles the [JetBrains phpstorm-stubs](https://github.com/JetBrains/phpstorm-stubs) directly into the binary. This provides type information for ~1,450 built-in classes/interfaces, ~5,000 built-in functions, and ~2,000 built-in constants without requiring any external files at runtime.

### Build-Time Processing

The `build.rs` script:

1. If `stubs/jetbrains/phpstorm-stubs/` does not exist, fetches the latest release tarball from the JetBrains/phpstorm-stubs GitHub repository and extracts it. This makes `cargo install` and `cargo build` work without any prior setup.
2. Reads `stubs/jetbrains/phpstorm-stubs/PhpStormStubsMap.php` — a generated index mapping symbol names to file paths.
3. Emits `stub_map_generated.rs` containing:
   - `STUB_FILES`: an array of `include_str!(...)` calls embedding every referenced PHP file (~502 files, ~8.5MB of source).
   - `STUB_CLASS_MAP`: maps class/interface/trait names → index into `STUB_FILES`.
   - `STUB_FUNCTION_MAP`: maps function names → index into `STUB_FILES`.
   - `STUB_CONSTANT_MAP`: maps constant names → index into `STUB_FILES`.

The stubs are cached in the `stubs/` directory (gitignored). To update to the latest version, delete `stubs/` and rebuild.

### Runtime Lookup — Classes

At `Backend` construction, `stubs.rs` converts the static arrays into `HashMap`s for O(1) lookup. When `find_or_load_class` reaches Phase 3:

1. Look up the class short name in `stub_index` → get the PHP source string.
2. Parse it with the same parser used for user code.
3. Run name resolution (for parent classes, trait uses, etc.).
4. Cache in `uri_classes_index` under `phpantom-stub://ClassName`.
5. Return the `ClassInfo`.

Because stubs are cached after first access, repeated lookups (e.g. every enum needing `UnitEnum`) hit Phase 1 and skip parsing entirely.

### Runtime Lookup — Functions (`find_or_load_function`)

Built-in PHP functions (e.g. `array_map`, `date_create`, `str_contains`) are resolved through `find_or_load_function`, which mirrors the multi-phase pattern used for classes:

```
find_or_load_function(["str_contains", "App\\str_contains"])
│
├── Phase 1: global_functions (user code + cached stubs)
│   Checks all candidate names against the global_functions map.
│   Keys are always FQNs: "array_map" for global functions,
│   "Illuminate\Support\enum_value" for namespaced ones.
│   No short-name fallback entries are stored.
│   ↓ miss
│
├── Phase 1.5: autoload_function_index (byte-level scan)
│   Checks candidate names against autoload_function_index
│   (function FQN → file path on disk).  Populated by the
│   find_symbols byte-level scan for both Composer projects
│   (autoload_files.php) and non-Composer projects (workspace
│   full-scan).
│   When found:
│     1. Reads the file from disk.
│     2. Calls update_ast to get full FunctionInfo + ClassInfo.
│     3. Results are cached in global_functions (so future
│        lookups hit Phase 1).
│     4. Returns the matching FunctionInfo.
│   ↓ miss
│
├── Phase 1.75: Last-resort autoload file parse
│   The byte-level scanner misses functions inside conditional
│   blocks (e.g. `if (! function_exists(...))` guards).  As a
│   safety net, lazily parses each known autoload file path
│   (stored in autoload_file_paths) via update_ast until the
│   function is found.  Skips files already in uri_classes_index.  Each
│   file is parsed at most once; subsequent lookups hit Phase 1.
│   ↓ miss
│
├── Phase 2: Embedded PHP stubs
│   Looks up each candidate name in stub_function_index.
│   When found:
│     1. Parses the entire stub file (extracting all FunctionInfo).
│     2. Caches ALL functions from that file into global_functions
│        under phpantom-stub-fn:// URIs (FQN keys only).
│     3. Also caches any classes defined in the same stub file into
│        uri_classes_index (so return type references can be resolved).
│     4. Returns the matching FunctionInfo.
│   ↓ miss
│
└── None
```

The `function_loader` closures in both `server.rs` (completion) and `definition/resolve.rs` (go-to-definition) build a list of candidate names — the bare name, the use-map resolved name, and the namespace-qualified name — then delegate to `find_or_load_function`. Because `global_functions` is keyed by FQN only, bare calls like `enum_value()` in namespace `App` succeed via the namespace-qualified candidate `App\enum_value`. This means built-in function return types are available for:

- **Variable type resolution**: `$dt = date_create();` → `$dt` is `DateTime`
- **Call chain completion**: `date_create()->` offers `format()`, `modify()`, etc.
- **Nested call resolution**: `simplexml_load_string(...)->xpath(...)` works

User-defined functions in `global_functions` always take precedence over stubs because Phase 1 is checked first — stubs use `entry().or_insert()` to avoid overwriting existing entries.

#### Function Completion: Namespace Awareness

`build_function_completions` handles two modes:

- **`use function` context** (`for_use_import = true`): The insert text is the FQN (e.g. `Illuminate\Support\enum_value`) so the resulting statement reads `use function Illuminate\Support\enum_value;`. The label is the FQN for namespaced functions and the signature for global ones.

- **Inline context** (`for_use_import = false`): The insert text is a snippet using the short name (e.g. `enum_value($0)`). For namespaced functions, an `additional_text_edits` entry inserts `use function FQN;` at the alphabetically correct position, mirroring how class auto-import works. The detail shows the namespace (e.g. `function (Illuminate\Support)`). Functions in the same namespace as the current file do not receive an auto-import.

Deduplication uses the map key (FQN), so two functions with the same short name in different namespaces both appear as separate completion items.

### Stub Patches

The embedded phpstorm-stubs sometimes lack `@template` annotations or have overly broad return types (e.g. `mixed`) for functions whose return type actually depends on an argument. PHPStan solves this with dynamic return type extensions written in PHP; PHPantom solves it by patching the parsed `FunctionInfo` or `ClassInfo` at load time.

#### Function patches

`stub_patches.rs` provides `apply_function_stub_patches(func)`, called from `find_or_load_function` in Phase 2 after parsing each function from stub source and before caching it in `global_functions`. The function dispatches to per-function patch functions based on the function name. Only functions with known deficiencies are patched; all others pass through unchanged.

Current patches:

1. **`array_reduce`** — phpstorm-stubs declare `mixed` return type. The actual return type is the type of the initial value (3rd argument). The patch adds `@template TReturn` bound to `$initial` and sets the return type to `TReturn`, matching PHPStan's `stubs/arrayFunctions.stub`.

This is analogous to the [Laravel Class Patches](#laravel-class-patches) system but for built-in PHP functions rather than framework classes. When phpstorm-stubs gains proper annotations for a patched function, the corresponding patch can be deleted.

**When to add a patch here vs. hardcoded logic in `rhs_resolution.rs`:** if the correct behaviour can be expressed with `@template` / `@return` annotations (i.e. PHPStan's own stubs already have the fix), it belongs in `stub_patches.rs`. If the behaviour requires inspecting call-site argument *values* at resolution time (e.g. `array_map`'s callback return type, or `array_filter` preserving the input array's element type), it must stay as hardcoded logic in `rhs_resolution.rs` / `raw_type_inference.rs`. Those functions are tracked in `ARRAY_PRESERVING_FUNCS` and `ARRAY_ELEMENT_FUNCS` in `completion/variable/mod.rs`, with the full inventory in `docs/todo/completion.md` C1.

#### Class patches

Class stub patches work the same way but are applied by `apply_class_stub_patches(class)` in `parse_and_cache_content_versioned` for URIs starting with `phpantom-stub://` or `phpantom-stub-fn://`. Current class patches add `@template` parameters and `@implements` generics to SPL collection classes (`ArrayIterator`, `ArrayObject`, `SplDoublyLinkedList`, `SplQueue`, `SplStack`, `SplPriorityQueue`, `SplFixedArray`, `SplObjectStorage`, `WeakMap`) that are missing them in phpstorm-stubs. These patches enable generic type substitution through the interface chain (e.g. `ArrayIterator<int, Rule>::current()` resolves to `Rule` via `Iterator::current()` returning `TValue`).

### Runtime Lookup — Constants

The `stub_constant_index` (`HashMap<&'static str, &'static str>`) is built at construction time from `STUB_CONSTANT_MAP`, mapping constant names like `PHP_EOL`, `PHP_INT_MAX`, `SORT_ASC` to their stub file source. It is consulted by the constant-value loader (`Backend::constant_loader`) during variable type resolution: when a variable is assigned from a global constant (e.g. `$eol = PHP_EOL`), the loader extracts the constant's initializer value from the stub source and the type is inferred from the literal (`string`, `int`, `float`, `bool`, `null`, or `array`). The same lookup chain also serves hover, completion of standalone constant names, and go-to-definition.

### Version-Aware Filtering

phpstorm-stubs annotate functions, methods, and parameters with `#[PhpStormStubsElementAvailable]` attributes to indicate which PHP versions they apply to. For example, `array_map` has two variants of its second parameter: `array $array` (from PHP 8.0) and an untyped `$arrays` (PHP 5.3 to 7.4). Without filtering, both appear in signatures.

PHPantom detects the target PHP version during `initialized`:

1. Read `config.platform.php` from `composer.json` (explicit platform override).
2. Fall back to `require.php` from `composer.json` (e.g. `"^8.4"` → 8.4).
3. Default to PHP 8.5 when neither is available.

The detected version is stored on the `Backend` as a `PhpVersion(major, minor)`.

When parsing stubs (both class stubs via `find_or_load_class` and function stubs via `find_or_load_function`), the PHP version is passed through `DocblockCtx.php_version` to the extraction functions. Four filtering points apply:

- **Function-level:** `extract_functions_from_statements` checks the function's `#[PhpStormStubsElementAvailable]` attribute. If the version range excludes the target, the entire function is skipped. This handles duplicate function definitions (e.g. `array_combine` has separate signatures for PHP ≤7.4 and ≥8.0).
- **Method-level:** `extract_class_like_members` applies the same check to methods. For example, `SplFixedArray::__serialize` (from PHP 8.2) is excluded when targeting PHP 8.1.
- **Parameter-level:** `extract_parameters` filters individual parameters. For example, `array_map`'s untyped `$arrays` parameter (PHP 5.3–7.4) is excluded when targeting PHP 8.0+, leaving only the typed `array $array`.
- **Constant-level:** `set_php_version` calls `is_stub_constant_removed` to evict constants whose `@removed X.Y` tag indicates they were removed at or before the target version (e.g. `MCRYPT_ENCRYPT` with `@removed 7.2`).

The attribute supports named arguments (`from: '8.0'`, `to: '7.4'`) and a positional argument (`'8.1'` treated as `from`). Both bounds are inclusive. A missing bound means unbounded in that direction.

User code is never filtered. The `php_version` field in `DocblockCtx` is `None` for files parsed via `update_ast`, so the filtering logic is a no-op for non-stub code.

### Graceful Degradation

If the stubs can't be fetched (e.g. no network access during the build), `build.rs` generates empty arrays and the build succeeds. The LSP just won't know about built-in PHP symbols.

## Inheritance Resolution

When building completion items or resolving definitions, PHPantom merges members from the full inheritance chain. Resolution proceeds in two phases:

1. **Base resolution** (`resolve_class_with_inheritance` in `inheritance.rs`): merges own members, trait members, and parent chain members with generic type substitution.

2. **Virtual member providers** (`resolve_class_fully` in `virtual_members/mod.rs`): queries registered providers in priority order and merges their contributions. Virtual members never overwrite real declared members or contributions from higher-priority providers.

All completion and go-to-definition call sites use `resolve_class_fully`, which is the primary entry point for full resolution.

### Base Resolution

```
ClassInfo (own members)
│
├── 1. Merge used traits (via `use TraitName;`)
│   └── Recursively follows trait composition and parent_class chains
│
└── 2. Walk the extends chain (parent_class)
    └── For each parent: merge its traits, then its public/protected members
```

### Virtual Member Providers

After base resolution, `resolve_class_fully` applies registered virtual member providers. These are implementations of the `VirtualMemberProvider` trait (defined in `src/virtual_members/mod.rs`) that synthesize methods and properties not present in the PHP source code.

Each provider implements two methods:

- `applies_to(class, class_loader) -> bool`: cheap pre-check to skip providers early.
- `provide(class, class_loader) -> VirtualMembers`: produce the virtual methods and properties.

Providers receive the base-resolved class (own + traits + parents) but without other providers' contributions. This prevents circular loading.

```
resolve_class_fully(class)
│
├── 1. resolve_class_with_inheritance(class)
│   └── Returns base-resolved ClassInfo
│
├── 2. For each provider (in priority order):
│   └── if applies_to(class): merge provide(class) into result
│       └── Skips members that already exist (no overwrites)
│
├── 3. Merge members from implemented interfaces
│
└── 4. apply_laravel_patches(class, fqn)
    └── Post-resolution fixups for Laravel classes (see below)
```

Provider priority order (highest first):

1. **Framework provider** (e.g. Laravel): richest type info
2. **PHPDoc provider**: `@method`, `@property`, `@property-read`, `@property-write`, `@mixin`

Two providers are currently registered in `default_providers()`:

- **`LaravelModelProvider`** (`virtual_members/laravel.rs`): synthesizes virtual members for classes extending `Illuminate\Database\Eloquent\Model`. Produces relationship properties (methods returning `HasMany`, `HasOne`, `BelongsTo`, etc. generate a virtual property typed from the relationship's generic parameters), scope methods (both the `scopeActive` naming convention and the `#[Scope]` attribute from Laravel 11+ are supported; either style becomes `active()` as both static and instance), Builder-as-static forwarding (`User::where()->get()` resolves end-to-end), accessors (legacy `getXAttribute()` and modern `Attribute` casts), and cast properties (`$casts` array or `casts()` method entries are mapped to PHP types like `datetime` to `\Carbon\Carbon`, `boolean` to `bool`, custom cast classes to their `get()` return type). Highest priority among virtual member providers. Scope methods are also injected onto `Builder<Model>` instances via a post-generic-substitution hook in `type_hint_to_classes_depth` (see "Scope Methods on Builder Instances" below).
- **`PHPDocProvider`** (`virtual_members/phpdoc.rs`): parses `@method`, `@property`, `@property-read`, `@property-write`, and `@mixin` tags from the class-level docblock stored in `ClassInfo.class_docblock`. Explicit `@method` / `@property` tags are not parsed eagerly during AST extraction; instead, the raw docblock string is preserved and parsed lazily when `provide` is called. For `@mixin` tags, the provider loads the referenced classes and merges their public members. Within the provider, explicit tags take precedence over mixin members. Recurses into mixin-of-mixin chains up to `MAX_MIXIN_DEPTH`.

### Laravel Class Patches

After virtual members and interface members are merged, `resolve_class_fully_inner` calls `apply_laravel_patches(class, fqn)` from `virtual_members/laravel/patches.rs`. Unlike virtual member providers (which *add* new members), patches *modify* existing members' type information to fix framework-specific type inaccuracies that break chain resolution.

The patch system is centralized in a single module with one entry point. Dispatching is based on the fully-qualified class name and trait usage. Current patches:

1. **`Eloquent\Builder::__call` / `__callStatic` return type.** Laravel's `Builder::__call()` is declared as returning `mixed`, but in practice always returns `$this` (scope dispatch, macro dispatch, Query\Builder forwarding). The patch overrides the return type to `static` so that method chains through unknown calls preserve the Builder type.

2. **`Conditionable::when()` / `unless()` return type.** The trait declares `@return $this|TWhenReturnType` but the unresolved method-level template parameter `TWhenReturnType` prevents `is_self_like_type` from recognizing the return as self-referential, breaking method chain resolution on Builder and Collection. The patch replaces the return type with `$this`. Applied to the `Conditionable` trait itself, to `Eloquent\Builder` (which uses the trait), and to any class whose `used_traits` includes `Conditionable`.

3. **Bare `Builder` return types on scope methods.** Handled separately in `scopes.rs` (`is_bare_builder_type`) because it runs at scope-injection time (post-generic-substitution), not during `resolve_class_fully_inner`. Documented in the patch module as part of the inventory but not dispatched from it.

### Precedence Rules

- **Class own members** always win.
- **Trait members** override inherited members but not own members.
- **Parent members** fill in anything not already present.
- **Virtual members** from providers sit below all real declared members (own, trait, parent). Higher-priority providers shadow lower-priority ones. Within the PHPDoc provider, `@method` / `@property` tags beat `@mixin` members.
- **Private members** are never inherited from parents (but trait private members are copied, matching PHP semantics).

### Scope Methods on Builder Instances

Scope methods (both `scopeX` convention and `#[Scope]` attribute) are synthesized on the Model class by `LaravelModelProvider`, but they also need to be available on `Builder<Model>` instances (e.g. `Brand::where('id', $id)->isActive()`). This cannot be handled by a virtual member provider alone because providers run before generic argument substitution, so the provider would not know the concrete model type.

Instead, scope injection happens in three places:

1. **Post-generic-substitution hook** (`completion/resolver.rs`, inside `type_hint_to_classes_depth`): after `resolve_class_fully` + `apply_generic_args` produces a `Builder<User>` class, the resolver detects that the result is an Eloquent Builder with a concrete model generic argument. It calls `build_scope_methods_for_builder(model_name, class_loader)` which loads the model, fully resolves it, extracts scope methods, and returns them as instance methods with `static` in return types substituted to the concrete model name. These are merged onto the Builder's method list, giving `$q->active()` after `$q = User::where(...)`.

2. **Scope body Builder enrichment** (`completion/variable_resolution.rs`, `enrich_builder_type_in_scope`): inside a scope method body, the `$query` parameter is typically typed as `Builder` without generic arguments. The enrichment function detects when the enclosing method is a scope (either the `scopeX` naming convention or the `#[Scope]` attribute) on a class that extends Eloquent Model, and the parameter type is `Builder` without generics. It rewrites the type to `Builder<EnclosingModel>`, which then flows through the generic-args path and triggers the post-substitution hook above. This makes `$query->otherScope()` resolve inside scope bodies.

3. **Go-to-definition fallback** (`definition/member.rs`, `find_scope_on_builder_model`): when go-to-definition resolves a member on a Builder class and the normal lookup chain (own members, traits, parents, mixins, builder forwarding) fails, this fallback checks whether the member is a scope method injected from the model. It confirms the resolved candidate (with injected scopes) has the method, extracts the model name from the scope method's return type generic argument, loads the model, and finds the declaration through the model's inheritance chain. For `scopeX`-style scopes it looks for `scopeXxx`; for `#[Scope]`-attributed methods it falls back to the original method name. This bridges the gap between completion (which works on the merged ClassInfo) and GTD (which traces back to the declaring source file).

### Interface Inheritance in Traits/Used Interfaces

The `merge_traits_into` function also walks the `parent_class` chain of each trait/interface it loads. This is critical for enums: a backed enum's `used_traits` contains `BackedEnum`, and `BackedEnum extends UnitEnum`. The parent chain walk ensures `UnitEnum`'s members (`cases()`, `$name`) are merged alongside `BackedEnum`'s own members (`from()`, `tryFrom()`, `$value`).

## Implicit Enum Interfaces

PHP enums implicitly implement `UnitEnum` (for unit enums) or `BackedEnum` (for backed enums). The parser detects this and adds the appropriate interface to the enum's `used_traits`:

```
enum Color { ... }           → used_traits: ["\UnitEnum"]
enum Status: int { ... }     → used_traits: ["\BackedEnum"]
```

The leading backslash marks the name as fully-qualified so that namespace resolution doesn't incorrectly prefix it (e.g. an enum in `namespace App\Enums` won't resolve to `App\Enums\UnitEnum`).

At resolution time, `merge_traits_into` loads the `UnitEnum` or `BackedEnum` stub from the embedded phpstorm-stubs, and the interface inheritance chain provides all the standard enum methods and properties.

## Composer Integration

### PSR-4 Autoloading

`composer.rs` parses:

- `composer.json` → `autoload.psr-4` and `autoload-dev.psr-4` mappings

PSR-4 mappings come exclusively from the project's own `composer.json`. Vendor PSR-4 (`vendor/composer/autoload_psr4.php`) is not loaded. The `fqn_uri_index` is the sole source of truth for vendor code.

**Design principle:** vendor PSR-4 mappings are not loaded separately. The `fqn_uri_index` (populated from the Composer classmap and augmented by the self-scanner, see below) is the sole source of truth for vendor code. User PSR-4 roots are walked by Go-to-implementation (Phase 5) and Find References because user files change between `dump-autoload` runs.

### Self-Generated Classmap

When Composer's `autoload_classmap.php` is missing or incomplete, PHPantom builds a classmap itself using a fast byte-level PHP scanner (`classmap_scanner.rs`). The scanner is a single-pass state machine that extracts `namespace\ClassName` pairs without a full AST parse, handling comments, strings, heredocs, and property accesses (`$node->class`). `memchr` provides SIMD-accelerated keyword pre-screening so files without class-like keywords are rejected immediately. Inside matched files, `memchr` also accelerates skipping of line comments (jump to newline), block comments (jump to `*`), single-quoted strings (jump to `'` or `\`), double-quoted strings (jump to `"` or `\`), and heredocs/nowdocs (jump to newline) instead of scanning byte-by-byte.

Scanning is parallelised using a two-phase approach: directory walks collect file paths first (single-threaded, via the `ignore` crate), then files are read and scanned in parallel batches using `std::thread::scope`. Three helpers cover the scan modes: `scan_files_parallel_classes` (plain classmap), `scan_files_parallel_psr4` (PSR-4 with FQN filtering), and `scan_files_parallel_full` (classes + functions + constants). The thread count is capped at `std::thread::available_parallelism()`. Small batches (≤ 4 files) skip threading overhead and run sequentially.

The indexing strategy is configurable via `[indexing] strategy` in `.phpantom.toml`:

- **`"composer"`** (default) — merged classmap + self-scan. Load Composer's classmap (if it exists) as a skip set, then self-scan all PSR-4 and vendor directories for anything the classmap missed. Whatever the classmap already covers is a free performance win; whatever it's missing, we find ourselves. No completeness heuristic needed.
- **`"self"`** — always self-scan, ignoring Composer's classmap entirely. Equivalent to the merged approach with an empty skip set.
- **`"full"`** — same as `"self"` for now; reserved for future background indexing.
- **`"none"`** — no proactive scanning; uses Composer's classmap if present but never self-scans to fill gaps.

The merged pipeline works in three steps: (1) load `autoload_classmap.php` into a `HashMap<String, PathBuf>`, (2) collect the classmap's file paths into a `HashSet<PathBuf>` skip set, (3) self-scan all PSR-4 and vendor directories, skipping files already in the skip set. The result is a merged index: classmap entries for everything Composer already knew about, plus self-scanned entries for everything it missed. When the classmap is complete (the common case), the self-scanner walks directories but skips every file, finishing almost instantly. When the classmap is empty or absent, it falls back to a full self-scan. When the classmap is partial (e.g. vendor classes only), vendor files are skipped and only user code is scanned. Every state of the classmap helps.

When self-scanning with a `composer.json` present, the scanner reads `autoload.psr-4`, `autoload-dev.psr-4`, `autoload.classmap`, and `autoload-dev.classmap` to determine which directories to walk. PSR-4 directories are filtered: only classes whose FQN matches the namespace prefix plus the relative file path are included. Vendor packages are discovered from `vendor/composer/installed.json` (both Composer 1 and 2 formats); the JSON packages array is borrowed rather than cloned to avoid allocating a copy of the entire vendor manifest. All directory walkers (full-scan, PSR-4 scanner, vendor package scanner, and go-to-implementation file collector) use the `ignore` crate for gitignore-aware traversal. Hidden directories are skipped automatically, and `.gitignore` rules are respected at every level. When no `composer.json` exists at all, the scanner falls back to walking all `.php` files under the workspace root.

The scan results are converted to URI strings and inserted into `fqn_uri_index`. Everything downstream (resolution, diagnostics, go-to-definition) uses the unified index.

**Redundant I/O elimination:** `init_single_project` parses `composer.json` once and passes the pre-parsed `serde_json::Value` to `build_self_scan_composer`. Previously each function re-read and re-parsed the file independently.

**Vendor dir detection:** the `config.vendor-dir` setting is read from `composer.json` once during `initialized` (via `parse_composer_json`, which returns both the PSR-4 mappings and the vendor dir name). The absolute vendor directory path is cached on `Backend.vendor_dir_paths` and a `file://` URI prefix is stored in `Backend.vendor_uri_prefixes` for fast vendor-file detection at runtime. Both fields are `Vec`s to support monorepo workspaces with multiple subprojects (see below). For single-project workspaces, each collection has exactly one entry.

### Autoload Files

`vendor/composer/autoload_files.php` lists files containing global function definitions and `define()` constants. During `initialized()`, these files are scanned with the lightweight `find_symbols` byte-level pass (not a full AST parse). This populates `autoload_function_index` (function FQN → file path), `autoload_constant_index` (constant name → file path), and `fqn_uri_index` (class FQN → file URI). Full parsing is deferred to the moment a symbol is first accessed via `find_or_load_function` (Phase 1.5), `resolve_constant_definition` (Phase 1.5), or `find_or_load_class` (through `fqn_uri_index`).

The byte-level scanner only discovers top-level declarations. Functions wrapped in `if (! function_exists(...))` guards (common in Laravel helpers) are at brace depth > 0 and are missed. As a safety net, all visited autoload file paths are stored in `autoload_file_paths`. When a function or constant is not found in any index or stubs, `find_or_load_function` and `resolve_constant_definition` lazily parse each known autoload file via `update_ast` as a last resort (Phase 1.75). Each file is parsed at most once.

### Non-Composer Function and Constant Discovery

In projects without `composer.json`, there is no `autoload_files.php` to consult. Instead, the full-scan (`find_symbols`) runs on all workspace files during initialization, populating three indices in a single pass: `fqn_uri_index` (classes), `autoload_function_index` (functions), and `autoload_constant_index` (constants). When a function or constant is first accessed, `find_or_load_function` or `resolve_constant_definition` lazily calls `update_ast` on the file, caching the result for subsequent lookups.

### Monorepo / Multi-Composer-Root Support

When the workspace root has no `composer.json` but contains subdirectories that are independent Composer projects, PHPantom discovers and processes each subproject automatically. This is a best-effort mitigation for workspaces where the editor opens a monorepo root as a single folder. The monorepo path only activates when there is no root `composer.json`.

**Discovery.** `discover_subproject_roots` in `composer.rs` walks the workspace using the `ignore` crate (`WalkBuilder`), respecting `.gitignore` at every level. Any directory containing a `composer.json` is recorded as a subproject root. Nested subprojects (a `composer.json` inside an already-accepted root) are filtered out. Each discovered `composer.json` is read to extract the `config.vendor-dir` setting.

**Per-subproject processing.** For each discovered `(subproject_root, vendor_dir)`, the same Composer pipeline runs: PSR-4 mappings (with base paths resolved to absolute paths so `resolve_class_path` works regardless of workspace root), classmap parsing, vendor package scanning from `installed.json`, and autoload file indexing. Results are merged into the shared backend state with first-subproject-wins semantics for duplicate FQNs.

**Loose file discovery.** After all subprojects are processed, the gitignore-aware full-scan walker (`scan_workspace_fallback_full`) runs on the workspace root with a skip set containing all subproject root paths. This picks up PHP files outside any subproject tree (e.g. top-level scripts, shared utilities) without double-scanning subproject content.

**Vendor tracking.** Each subproject's vendor directory is registered in both `vendor_uri_prefixes` (for URI-level vendor detection in diagnostics, find references, and rename) and `vendor_dir_paths` (for filesystem-level skip logic in go-to-implementation and workspace indexing).

**Trade-offs.** Conflicting class versions across subprojects result in first-wins resolution. A single PHP version is used for the entire workspace. Per-subproject `.phpantom.toml` is not supported. Multi-root LSP workspaces (separate `workspaceFolders`) are a separate feature.

### Function Resolution Priority

When resolving a standalone function call (e.g. `app()`, `date_create()`), the lookup order is:

1. **User code** (`global_functions` from opened/changed files and lazily parsed autoload index hits)
2. **Autoload function index** (`autoload_function_index`, populated by `find_symbols` for both Composer and non-Composer projects, triggers lazy `update_ast` on first access)
3. **Known autoload files** (`autoload_file_paths`, last-resort lazy parse for functions missed by the byte-level scanner, e.g. those inside `function_exists()` guards)
4. **Embedded stubs** (`stub_function_index` from phpstorm-stubs, parsed lazily)

This ensures that user-defined overrides or polyfills always win over stubs.

## Go-to-Implementation: `find_implementors`

When the user invokes go-to-implementation on an interface or abstract class, PHPantom scans for concrete classes that implement or extend it. When invoked on a method definition in a concrete class, it performs a reverse jump to the interface or abstract method that declares the prototype. The forward scan runs five phases, each progressively wider:

```
find_implementors("Cacheable", "App\\Contracts\\Cacheable")
│
├── Phase 1: uri_classes_index (already-parsed classes)
│   Iterates every ClassInfo in every file already in memory.
│   Checks interfaces list and parent_class chain against the target.
│   ↓ continue
│
├── Phase 2: fqn_uri_index (FQN → URI entries not yet covered)
│   Loads classes via class_loader for entries not seen in Phase 1.
│   ↓ continue
│
├── Phase 3: fqn_uri_index files (string pre-filter → parse)
│   Collects unique file paths from fqn_uri_index.
│   Skips files already in uri_classes_index.
│   Reads each file's raw source and checks contains(target_short).
│   Only matching files are parsed via parse_and_cache_file.
│   Every class in a parsed file is checked (not just the looked-up FQN).
│   ↓ continue
│
├── Phase 4: embedded stubs (string pre-filter → lazy parse)
│   Checks each stub's static source string for contains(target_short).
│   Matching stubs are loaded via class_loader (parsed and cached).
│   ↓ continue
│
├── Phase 5: PSR-4 directory walk (user code only)
│   Recursively collects all .php files under every PSR-4 root.
│   Skips files already covered by fqn_uri_index (Phase 3) or uri_classes_index.
│   Reads raw source, applies the same string pre-filter.
│   Matching files are parsed via parse_and_cache_file.
│   Discovers classes missing from fqn_uri_index.
│   ↓ done
│
└── Vec<ClassInfo> (concrete implementors only)
```

### Phase 5 Scope: User Code Only (by design)

Phase 5 walks PSR-4 roots from `composer.json` (`autoload` and `autoload-dev`). Since PSR-4 mappings are sourced exclusively from the project's own `composer.json` (vendor PSR-4 is not loaded), Phase 5 inherently only discovers classes in the user's own source directories (e.g. `src/`, `app/`, `tests/`). Vendor dependencies are fully covered by fqn_uri_index (Phase 3).

Phase 5 exists to catch newly-created or not-yet-indexed user classes that are missing from fqn_uri_index.

Note: `collect_php_files` still receives the vendor dir name because a fallback mapping like `"" => "."` resolves to the workspace root, where the walk must skip the vendor directory (and hidden directories like `.git`).

### String Pre-Filter

Phases 3–5 avoid expensive parsing by first reading the raw file content and checking whether it contains the target class's short name. A file that doesn't mention `"Cacheable"` anywhere in its source can't possibly implement the `Cacheable` interface, so it's skipped without parsing. This keeps the scan fast even for large projects with thousands of files.

### Caching

`parse_and_cache_file` follows the same pattern as `find_or_load_class`: it parses the PHP file, resolves parent/interface names via `resolve_parent_class_names`, and stores the results in `uri_classes_index`, `fqn_class_index`, and `fqn_uri_index`. This means files discovered during a go-to-implementation scan are immediately available for subsequent completion, definition, and implementation lookups without re-parsing.

### Member-Level Implementation

When the cursor is on a method call (e.g. `$repo->find()`), `resolve_member_implementations` first resolves the subject to candidate classes. If any candidate is an interface or abstract class, `find_implementors` is called and each implementor is checked for the specific method. Only classes that directly define (override) the method are returned — inherited-but-not-overridden methods are excluded.

### Reverse Jump: Concrete Method → Prototype Declaration

When go-to-implementation is invoked on a `MemberDeclaration` symbol (a method name at its definition site), `resolve_reverse_implementation` checks whether the enclosing class implements any interface or extends an abstract class that declares a method with the same name. If found, it returns the location of the prototype method in the interface or abstract class. This covers:

- Direct interfaces (`class Foo implements Bar`)
- Interfaces inherited from parent classes (`class Foo extends Base` where `Base implements Bar`)
- Interface-extends chains (`interface A extends B`)
- Abstract parent methods (`abstract class Base { abstract function foo(); }`)

For interface and abstract class method declarations, the same handler works in the forward direction: it calls `resolve_interface_member_implementations` to find all concrete classes that define the method.

### FQN-Based Comparison

All comparisons in `class_implements_or_extends` and deduplication in `find_implementors` use fully-qualified names (built from `ClassInfo.name` + `ClassInfo.file_namespace`). The short-name fallback is only used when the target has no namespace information, preventing false positives when two interfaces in different namespaces share the same short name (e.g. `App\Logger` vs `Vendor\Logger`).

## Find References: `ensure_workspace_indexed`

When the user invokes "Find All References", PHPantom scans all user files for occurrences of the symbol. Vendor files are excluded (matching PhpStorm's behaviour).

### Indexing

Before scanning, `ensure_workspace_indexed` ensures all user files have symbol maps:

1. **Phase 1: fqn_uri_index files (user only)** — files already known from `update_ast` calls. Vendor and stub URIs are skipped.
2. **Phase 2: `.gitignore`-aware workspace walk** — uses the `ignore` crate's `WalkBuilder` to recursively discover PHP files under the workspace root, respecting `.gitignore` rules (including nested and global gitignore files). This automatically skips generated/cached directories like `storage/framework/views/` (Laravel blade cache), `var/cache/` (Symfony), and `node_modules/`. The vendor directory is always skipped regardless of `.gitignore` content. Hidden directories are skipped by default.

Both phases parse files in parallel using `std::thread::scope`. The work is split into chunks (one per CPU core) and each thread reads a file from disk and calls `update_ast`, which acquires write locks briefly to store results while the expensive parsing step runs without any locks held. Batches of 2 or fewer files skip threading overhead.

Parsed files stay cached in `uri_classes_index`, `symbol_maps`, `file_imports`, and `file_namespaces` after the scan completes. There is no post-scan eviction; keeping the entries means subsequent operations (a second find-references call, go-to-definition on a cross-file symbol) benefit from the work already done.

### Cross-file scanning

The `user_file_symbol_maps()` helper snapshots all symbol maps whose URI does not fall under the vendor directory or the internal stub scheme. With `Arc<SymbolMap>`, the snapshot is a vector of cheap reference-count increments rather than deep clones. Four scanners use this snapshot:

- `find_class_references` — matches `ClassReference` spans by resolved FQN
- `find_member_references` — matches `MemberAccess` and `MemberDeclaration` spans by member name, filtered by class hierarchy
- `find_function_references` — matches `FunctionCall` spans by resolved FQN
- `find_constant_references` — matches `ConstantReference` spans by name

Variable references (`$this`, local variables) are scoped to the enclosing function/class in the current file only, and do not use the cross-file scan.

### Class-aware member filtering

When the user triggers "Find References" on a method, property, or constant, the handler resolves the target class and builds a **hierarchy set** containing all related class FQNs. Only references whose subject resolves to a class in this set are included. This prevents `$user->save()` from matching `$order->save()` when `User` and `Order` are unrelated.

The hierarchy set is built in two passes:

1. **Ancestors** — walk the parent chain, interfaces, traits, and mixins upward from the target class, collecting every FQN encountered.
2. **Descendants** — scan all classes in `uri_classes_index` and `fqn_uri_index` for classes that extend, implement, or use anything already in the set. This repeats until no new FQNs are added (transitive closure), bounded by `MAX_INHERITANCE_DEPTH`.

For each candidate `MemberAccess` span, the subject text is resolved to class FQNs using a lightweight path:

- `self` / `static` / `$this` → enclosing class FQN via `find_class_at_offset`
- `parent` → enclosing class's parent FQN
- Bare class name (static access) → resolved via the file's use-map and namespace
- `$variable` → full variable type resolution via `resolve_variable_types`

When the subject resolves to one or more FQNs, the reference is included only if at least one FQN is in the hierarchy set. When resolution fails entirely (untyped variable, complex expression), the reference is included conservatively to avoid false negatives.

For `MemberDeclaration` spans, the enclosing class at the span offset is checked against the hierarchy set. Property declaration sites (which use `Variable` spans for GTD compatibility) are filtered by checking the declaring class's FQN.

## Union Type Completion (by design)

When a variable can hold one of several types (from match arms, ternary
branches, null-coalescing, or conditional return types), the completion
list shows the **union** of all members across all possible types, not
just the intersection of shared members.

This is a deliberate choice that matches PHPStorm and Intelephense
behaviour. The rationale:

1. **The developer may not have isolated branches yet.** When working
   through a match or ternary, the code often starts with a shared
   variable before the developer splits behaviour per type. Hiding
   branch-specific members would block progress during that phase.
2. **Missing completions are worse than extra completions.** Restricting
   to the intersection would hide useful members whenever a variable has
   more than one possible type.
3. **Type safety belongs in diagnostics.** Calling a method that only
   exists on one branch is a potential bug, but the right place to flag
   it is a diagnostic/static-analysis pass, not the completion list.

This is distinct from narrowing via early return, `unset()`, or
`instanceof` guards. Those reflect deliberate developer intent to
eliminate types, so the narrowed-out members are correctly hidden. Union
completion is about types the developer has _not yet_ separated.

Members that are only available on a subset of the union already show the
originating class in the `detail` field (e.g. "Class: AdminUser"), which
gives the developer a visual hint. A future enhancement could sort
intersection members above branch-only members or add an explicit marker.

## Context-Aware Class Name Completion

When the user types a class name outside of a member-access chain (`->`, `::`), the LSP needs to decide which symbols to offer. PHP reuses many of the same keywords in positions that accept very different kinds of symbols. `new` only makes sense with concrete classes. `implements` only accepts interfaces. `use` inside a class body only accepts traits. Offering the wrong kind of symbol is noisy at best and confusing at worst. This section explains how the LSP detects the context, selects which symbols to show, and handles the many edge cases that arise from incomplete information.

### Context Detection

`detect_class_name_context()` in `class_completion.rs` walks backward from the cursor through whitespace and comma-separated identifier lists to find the keyword that governs the completion position. It recognises twelve contexts:

| Context                | Trigger                                | What is shown                                      |
| ---------------------- | -------------------------------------- | -------------------------------------------------- |
| `Any`                  | Bare identifier (e.g. `$x = DateT\|`)  | All class-likes, constants, and functions          |
| `New`                  | `new \|`                               | Concrete (non-abstract) classes only               |
| `ExtendsClass`         | `class A extends \|`                   | Non-final classes (abstract is OK)                 |
| `ExtendsInterface`     | `interface B extends \|`               | Interfaces only                                    |
| `Implements`           | `class C implements \|`                | Interfaces only                                    |
| `TraitUse`             | `class D { use \|`                     | Traits only                                        |
| `Instanceof`           | `$x instanceof \|`                     | Classes, interfaces, enums (not traits)            |
| `TypeHint`             | `function f(\|)`, `): \|`, `public \|` | Classes, interfaces, enums (not traits)            |
| `UseImport`            | `use \|` (top level)                   | All class-likes + `function`/`const` keyword hints |
| `UseFunction`          | `use function \|`                      | Functions only                                     |
| `UseConst`             | `use const \|`                         | Constants only                                     |
| `NamespaceDeclaration` | `namespace \|`                         | Namespace names only                               |

`TypeHint` is not detected by `detect_class_name_context` (which only handles keyword-triggered contexts). Instead, the type-hint completion handler (`complete_type_hint`) and the PHPDoc type completion handler (`complete_docblock_type_or_variable`) pass `ClassNameContext::TypeHint` directly to `build_class_name_completions`. This excludes traits from parameter types, return types, property types, and PHPDoc `@param`/`@return`/`@var` type positions. `@throws` uses a separate Throwable-filtered path instead.

Comma-separated lists are handled by walking past `Identifier,` sequences so that `implements Foo, Bar, \|` still resolves to `Implements`. Multi-line declarations work the same way because the backward walk skips all whitespace including newlines.

`TraitUse` vs `UseImport` is distinguished by brace depth: `use` at brace depth >= 1 is inside a class body (trait use), while brace depth 0 is a top-level import.

### Handler Routing

`try_class_constant_function_completion()` in `handler.rs` is the entry point. It extracts the partial identifier, detects the context, and branches:

- **`UseFunction`** and **`UseConst`** short-circuit to dedicated builders (`build_function_completions`, `build_constant_completions`). These bypass class-name logic entirely. Items from the current file are filtered out (importing a function from the file you are editing is pointless). A semicolon is appended to the insert text. For `UseFunction`, namespaced functions use their FQN as insert text so the resulting statement reads `use function Illuminate\Support\enum_value;`.

- **`NamespaceDeclaration`** short-circuits to `build_namespace_completions`, which produces namespace-path items from PSR-4 prefixes and known class FQNs.

- **`UseImport`** calls `build_class_name_completions` with an **empty use-map** (the file's own use-map contains the half-typed `use` line, which the parser interprets as a real import and would appear as a bogus completion item). The results are post-processed: classes defined in the current file are removed, a semicolon is appended, and `function`/`const` keyword hints are injected when the partial matches.

- **All other class-only contexts** (`New`, `ExtendsClass`, `ExtendsInterface`, `Implements`, `TraitUse`, `Instanceof`) call `build_class_name_completions` normally. Constants and functions are suppressed.

- **`Any`** calls all three builders (classes, constants, functions) and merges the results.

### Class Name Sources and Priority

`build_class_name_completions` collects candidates from five sources. Deduplication is by FQN (`seen_fqns` set). All candidates are ranked by a five-dimension `sort_text` key:

```
{match_quality}{source_tier}{affinity}{demote}{gap}_{short_name_lower}
```

**Match quality** (highest weight). Compares the typed prefix against the short name: `a` = exact match, `b` = starts-with (or empty prefix), `c` = substring only. An exact match always ranks above every prefix match, regardless of source or affinity.

**Source tier**. Three tiers based on how the class relates to the current file:

| Tier | Source                                         | Meaning                                     |
| ---- | ---------------------------------------------- | ------------------------------------------- |
| `0`  | Use-imported classes                           | Developer explicitly imported this name     |
| `1`  | Same-namespace classes (from `uri_classes_index`)        | PHP auto-resolves without a `use` statement |
| `2`  | Everything else (fqn_uri_index, stubs) | Requires a `use` import                     |

**Import affinity**. A four-digit inverted score derived from the file's namespace declaration and existing `use` imports. Namespaces that appear frequently in the file's imports score higher, pushing related classes above unrelated ones within the same tier. The score sums occurrence counts across all ancestor prefixes of the candidate's namespace.

**Heuristic demotion**. `0` = normal, `1` = demoted by naming-convention heuristics (see below). Demotion sits after affinity but before gap, so a demoted exact match still ranks above every non-demoted prefix match (match quality is higher weight), and a non-demoted name always beats a demoted name within the same affinity group regardless of name length.

**Name-length gap**. A three-digit distance between the short name length and the typed prefix length (`short_name.len() - prefix.len()`). Smaller gaps sort first. This is the lowest-weight dimension, breaking ties within the same affinity and demotion group. The effect: typing `"Pro"` ranks `Product` (gap 4) above `ProductFilterTerm` (gap 14) when both live in the same namespace. This smooths the visual transition as a prefix match narrows toward an exact match. Without it, there is a visible "snap" when a candidate suddenly jumps to the top upon becoming an exact match.

The result set is capped at 300 items. When truncated, `is_incomplete` is set to `true` so the editor re-requests completions as the user types more characters. Items are sorted by `sort_text` before truncation so higher-priority items survive.

### Kind Filtering

Each context defines which class-like kinds are valid through `matches_kind_flags(kind, is_abstract, is_final)`. For example, `TraitUse` only matches `ClassLikeKind::Trait`, and `New` only matches non-abstract classes. This filter is applied at every source, but the amount of information available varies by source, which creates a layered filtering strategy.

**Loaded classes** (in `uri_classes_index`). The full `ClassInfo` is available, so the filter is exact. An interface will never appear in `TraitUse` completions if it has been parsed.

**Unloaded stubs** (in `stub_index` but not yet parsed into `uri_classes_index`). The raw PHP source is available. `detect_stub_class_kind` scans it for the declaration keyword (`class`, `interface`, `trait`, `enum`) and modifiers (`abstract`, `final`, `readonly`). This is fast (string search, no tree-sitter parse) and gives accurate kind information for filtering.

**Unloaded project classes** (in `fqn_uri_index` but not parsed). No kind information is available. These are allowed through the filter with benefit of the doubt. A naming-convention heuristic (`likely_mismatch`) demotes but does not remove suspicious names (e.g. `FooInterface` is demoted in `ExtendsClass` context).

### Use-Map Validation

The file's `use` map is the most problematic source because it can contain entries that are not class-likes at all:

- **Namespace aliases** (`use App\Models as M;` puts `M -> App\Models` in the map, but `App\Models` is a namespace, not a class).
- **Non-existent imports** (the user may have typed `use Vendor\Something;` for a class the LSP has never seen).
- **Half-typed lines** (the parser interprets `use c` as importing `"c"`).

Three filters run on use-map entries, in order:

1. **`is_likely_namespace_not_class`**. Checks all four class sources (uri_classes_index, fqn_uri_index, stubs). If the FQN is found in any of them, it is a real class and passes. If not found, the function looks for positive namespace evidence: is the FQN declared as a namespace in `file_namespaces`, or do known classes exist under it as a prefix (e.g. `Cassandra\Exception\AlreadyExistsException` proves `Cassandra\Exception` is a namespace)? If positive evidence is found, the entry is rejected. If there is no evidence either way, the entry passes (benefit of the doubt).

2. **`matches_context_or_unloaded`**. If the class is loaded in `uri_classes_index`, applies exact kind filtering. If not loaded but present in `stub_index`, scans the stub source for the declaration keyword. If truly unknown, allows through. This catches stub interfaces appearing in `TraitUse` and similar mismatches.

3. **`is_known_class_like`** (narrow contexts only). For contexts that expect a very specific kind (`TraitUse`, `Implements`, `ExtendsInterface`), an additional check rejects use-map entries whose FQN cannot be found in any class source at all. The reasoning: if we are looking specifically for traits and we have never seen this FQN anywhere, it is almost certainly not a trait. This is deliberately not applied to broader contexts like `New` or `Instanceof`, where an imported-but-not-yet-indexed class should still appear.

The half-typed-line problem is solved at a higher level: the `UseImport` handler passes an empty use-map to `build_class_name_completions`, so the parser's misinterpretation never enters the candidate set.

### UseImport Special Handling

The `UseImport` context (`use |` at top level) has several differences from other contexts:

- **FQN insertion**. The insert text is always the full qualified name, not a short name. Writing `use User;` is invalid PHP even if `User` is in the current namespace. The `is_fqn_prefix` flag is forced to `true` and `effective_namespace` is set to `None` to prevent namespace-relative simplification.

- **No auto-import text edits**. In other contexts, selecting a class from a different namespace generates an `additional_text_edits` entry that inserts a `use` statement. In `UseImport` context this would be circular (you are already writing the `use` statement), so no text edit is generated.

- **Semicolons**. All class items get a `;` appended to the insert text so accepting a completion produces `use App\Models\User;`.

- **Keyword hints**. When the partial matches, `function` and `const` keyword items are injected with a trailing space (not a semicolon) so the user can continue typing the function or constant name.

- **Current-file exclusion**. Classes defined in the cursor file are filtered out. The same applies to `UseFunction` (functions from the current file) and `UseConst` (constants from the current file).

### Unloaded Stub Scanning

Many stubs are never parsed into `uri_classes_index` during a session. Rather than parse thousands of stub files eagerly, the LSP scans the raw PHP source on demand. `detect_stub_class_kind` finds the short name in the source, checks that it is preceded by a declaration keyword (`class`, `interface`, `trait`, `enum`), and extracts `abstract`/`final` modifiers. This gives accurate filtering without the cost of a full parse.

The scan handles PHP 8.2 `readonly` classes (`final readonly class Foo`) by stripping `readonly` before checking for `abstract`/`final`. Multi-class stub files (e.g. V8Js which declares both `V8Js` and `V8JsException`) are handled by searching for each name independently.

### Naming-Convention Heuristics

When a class is not loaded and not a stub (typically a fqn_uri_index entry whose file has not been opened), the LSP has no kind information. Rather than guess, it uses naming conventions to adjust sort order via the demotion flag in `sort_text`:

- Names ending in `Interface` or starting with `I[A-Z]` are demoted in `ExtendsClass` and `New`.
- Names starting with `Abstract` or `Base[A-Z]` are demoted in `Implements`, `ExtendsInterface`, `TraitUse`, and `New`.

Demotion sets the fourth dimension of the sort key to `1`, pushing the item below non-demoted items within the same match quality, tier, and affinity group. The item is never removed, because naming conventions are not reliable enough to exclude candidates entirely. A demoted exact match still ranks above every non-demoted prefix match, since match quality is the primary dimension.

## Memory Overhead

The `symbol_maps` store is keyed by file URI, matching `uri_classes_index`. A typical PHP file has 100–400 navigable symbol tokens. Each `SymbolSpan` is ~50–100 bytes (two `u32` fields plus the `SymbolKind` enum with a `String` or two), totalling ~20–40 KB per open file. Variable definition sites add ~1–3 KB. For comparison, `open_files` already stores the full file content (often 50–200 KB per file), so the symbol map is a small fraction of existing memory use.

Files that are not open (vendor files loaded via PSR-4 on demand) do not get a symbol map — they use the stored byte offsets from Tier 2 (which live on `ClassInfo` / `MethodInfo` / etc. in `uri_classes_index`).

## Diagnostic Philosophy

PHPantom earns trust through two promises:

1. **Provide suggestions for everything the developer expects.** Completion, hover, go-to-definition, and every other LSP feature should work on every symbol in the project with no dead spots.
2. **Stay silent on everything the developer knows works.** Every diagnostic PHPantom reports must be something that is genuinely wrong, not something that is merely imprecise or unconventional. If the code runs without errors, PHPantom should not complain about it.

Most PHP developers have learned to ignore IDE errors. They have seen tools produce walls of false positives from bugs in the inference engine, mistakes in stubs, or overly strict rules applied to code that works perfectly well in production. The result is that developers treat red squiggles as noise and test in production to see if a problem is real. PHPantom exists to break that cycle. When a diagnostic appears, it must mean something. A single false positive costs more trust than ten missed true positives, because it teaches the developer to stop reading.

### Type Coverage Diagnostics

The core diagnostic categories assert **type coverage**: can the LSP resolve every class, member, and function call in the project?

| Diagnostic | What it asserts |
|---|---|
| Syntax errors | The file parses. |
| Unused imports | The use-map is clean. |
| Unknown classes | Every class reference resolves to a real class. |
| Unknown members | Every member access resolves to a declared member. |
| Unknown functions | Every function call resolves to a declared function. |
| Argument count | Call sites match the target's parameter count. |
| Argument type mismatch | Every argument's type is definitely compatible with the parameter's declared type. |
| Implementation errors | Concrete classes satisfy their interface/abstract contracts. |
| Undefined variables | Every variable read has a prior definition in scope. |
| Deprecated usage | The developer is aware of deprecation. |

A project that passes all of these has 100% type coverage from the LSP's perspective: every symbol is resolvable, every completion trigger produces results, and every hover shows a type.

This also serves as a deliberate division of labour with PHPStan. PHPStan only complains about missing types at levels 6, 9, and 10. PHPantom fills those gaps cheaply and immediately so PHPStan can focus on logic errors rather than fighting incomplete type information. PHPantom rejects structural problems fast (unknown class, unknown member, wrong argument count, unimplemented interface method), and PHPStan hunts for semantic bugs with full type context available.

### Type Compatibility Diagnostics: Yes / No / Maybe

Argument type mismatch diagnostics (`type_error.argument`) go beyond coverage into type-compatibility territory. Here the risk of false positives is highest, because PHP is a dynamically typed language with pervasive implicit coercion, and real-world codebases are full of patterns that are technically imprecise but functionally correct.

Every type check is classified into one of three buckets:

- **Yes** (definitely compatible). `Cat` passed to `Animal` where `Cat extends Animal`. No diagnostic.
- **No** (definitely incompatible). `array` passed to `int`. Diagnostic reported.
- **Maybe** (might work, might not). `?Carbon` passed to `Carbon`, bare `array` passed to `array<string>`, `HtmlString` passed to `string`. No diagnostic.

**Only "no" produces a diagnostic.** The "maybe" bucket is deliberately wide. If there is any reasonable interpretation under which the code could work at runtime, PHPantom stays silent. Examples of "maybe":

- **Nullable to non-nullable** (`?Carbon` to `Carbon`). The developer may have null-checked before the call. We cannot see the guard clause from the call site alone.
- **Bare array to typed array** (`array` to `array<string>`). A bare `array` is untyped. It might contain strings. We cannot prove otherwise.
- **Stringable objects to string** (`HtmlString` to `string`). PHP calls `__toString()` at runtime. We follow PHP's runtime behaviour.
- **Int to string** (without `strict_types`). PHP coerces integers to strings in many contexts. When `declare(strict_types=1)` is present in the calling file, this coercion is flagged as a type error.
- **`list<X>` and `array<int, X>`**. Used interchangeably in practice. array<int, X> might have sequential-keys in actuality.
- **Interface to concrete subtype** (`CarbonInterface` to `Carbon`). The interface is broader, but in practice the value is almost always the expected concrete type.

This conservatism is the foundation for trust. Once developers learn that PHPantom's red squiggles are never false alarms, they start paying attention to them. At that point, a stricter mode can be offered as an opt-in for teams that want more help. But the default must be: if we are not certain, we stay silent.

## Analyze Command

The `analyze` subcommand (`phpantom_lsp analyze --project-root <DIR>`) runs all native diagnostics across a project in headless batch mode. It reuses the same `Backend` initialization pipeline as the LSP server, so results match exactly what a user sees in their editor.

The command exists so users can measure and maintain type coverage without opening files one by one. A clean run (exit code 0) means the LSP can resolve every symbol in the project. Any reported diagnostic is a spot where completion, hover, or go-to-definition will have a gap.

The batch pipeline runs in two parallel phases:

1. **Parse phase.** Read every user file (discovered via PSR-4 source directories) and call `update_ast` to populate `fqn_class_index`, `uri_classes_index`, `symbol_maps`, `file_imports`, `file_namespaces`, and `fqn_uri_index`. After this phase, all user classes are in `fqn_class_index` and cross-file references resolve via O(1) hash lookup.

2. **Diagnose phase.** Run all seven diagnostic collectors on every file in parallel (`std::thread::scope` with one chunk per CPU core). Because Phase 1 already indexed all user classes, the diagnostic phase never triggers lazy `parse_and_cache_file` for other user files, eliminating write-lock contention.

Output uses PHPStan's table format (with progress bar and coloured output) so the results are familiar to teams already using PHPStan. Severity filtering (`--severity error|warning|all`) and path scoping let users focus on specific areas.

## Diagnostic Worker Architecture

Diagnostics run in three independent background `tokio::spawn` tasks so they never block completion, hover, or signature help:

1. **Native diagnostic worker** — collects fast (syntax errors, unused imports, deprecated usage) and slow (unknown classes/members/functions, argument count, implementation errors) diagnostics. Debounces at 500 ms.
2. **PHPStan worker** — runs PHPStan in editor mode (`--tmp-file` / `--instead-of`). Debounces at 2 000 ms.
3. **PHPCS worker** — runs PHP_CodeSniffer via `phpcs --report=json` with stdin piping. Debounces at 2 000 ms.

Each worker is created during `initialized` via `clone_for_diagnostic_worker`, which builds a shallow clone of the `Backend`. All `Arc`-wrapped fields (maps, caches, the notify/pending slots) are shared by `Arc::clone`, so every worker sees all mutations the main `Backend` makes. The PHPStan and PHPCS workers each have their own notify handle, pending-URI slot, and diagnostic cache so they run independently of each other and of the native diagnostic worker.

Non-`Arc` fields are snapshotted at spawn time: `php_version`, `vendor_uri_prefixes`, `vendor_dir_paths`, and `config`. These fields are only written during `initialized` (before the workers are spawned) and never change afterwards. If a future feature adds hot-reloading of `.phpantom.toml` or runtime PHP version changes, the workers would need to be notified or re-cloned. This invariant ("init-time fields are write-once") should be verified before adding any post-init mutation to these fields.

## Forward Walker

The forward walker (`src/completion/variable/forward_walk.rs`) walks
method bodies top-to-bottom, building a scope of variable types at
each statement boundary. It is shared by diagnostics, completion,
hover, go-to-definition, and signature help.

## Name Resolution

PHP class names go through resolution at parse time (`resolve_parent_class_names`):

1. **Fully-qualified** (`\Foo\Bar`) → strip leading `\`, use as-is.
2. **In use map** (`Bar` with `use Foo\Bar;`) → expand to `Foo\Bar`.
3. **Qualified** (`Sub\Bar` with `use Foo\Sub;`) → expand first segment.
4. **Unqualified, not in use map** → prepend current namespace.
5. **No namespace** → keep as-is.

This runs on `parent_class`, `used_traits`, and `mixins` for every `ClassInfo` extracted from a file.
