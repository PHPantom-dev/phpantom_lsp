# Changelog

All notable changes to PHPantom will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Format selection.** Formatting a selection reformats only the lines it covers, with whichever formatter the project uses, in PHP files and Blade templates alike.

### Changed

- **Faster analysis of methods that check many properties.** Narrowing a property or a call's result no longer walks the method again from its first line.
- **Faster analysis of loops.** A loop body is no longer walked again once its types have settled.
- **Faster analysis of `switch` lookup tables.** A `switch` that assigns a different literal in each of hundreds of cases, such as a table of country or currency names, now takes well under a second instead of up to half a minute.
- **Faster analysis of long files.** Checking a file of tens of thousands of lines, such as a legacy top-level script, no longer slows down out of proportion to its length. A 22,000-line script is checked in about a second instead of nearly five.
- **Faster go-to-implementation on library classes.** Finding the implementations of a class or interface from a Composer package now reads only the files that could hold one, instead of loading every class the project depends on.

### Fixed

- **A `catch` block sees what the `try` body assigned before anything could throw.** A variable set by an immediately invoked closure, or by any statement that cannot throw, now keeps that value in the `catch` instead of reverting to what it held before the `try`.
- **Convert to instance variable no longer redeclares an inherited property.** The action is not offered when a parent class or trait already declares the property.
- **Imported type aliases resolve in the file that imports them.** A method typed with an alias from `@phpstan-import-type` now returns the full shape when it is called from the same file, not only from other files.
- **An override that names a different class than its ancestor keeps its own.** A `take(Box $b)` that names its own `Box` no longer inherits the ancestor's `@param Box&Countable` for a `Box` in another namespace.
- **A no-op emptiness check no longer breaks a `non-empty-string` argument.** After `if ($h !== '') { ... }`, passing `$h` to a `non-empty-string` parameter is no longer reported, since the variable is still a plain `string`.
- **A value both branches could hold no longer says which branch ran.** When one branch leaves `$x` as `1|2` and the other as `2|3`, a later `if ($x === 2)` no longer narrows variables to what only one of the branches assigned.
- **An override that narrows its return type keeps it.** When an interface method documents `@return Factory<Meta>` and an extending interface redeclares it as `: OrmFactory`, the override's result reads as `OrmFactory`, so returning it from a method declared `: OrmFactory` is no longer reported. This fixes Doctrine's `EntityManagerInterface::getMetadataFactory()`.
- **A failed call with several `@phpstan-assert-if-true` tags narrows none of its arguments.** When `bothDefined($start, $end)` returns false, `$start` and `$end` are no longer both taken to be `null`, since only one of the promises has to have failed.
- **A template next to other types in a parameter union binds only what they leave over.** Passing `string|float|bool|null` values to a `@param array<K, T|string|null>` now binds `T` to `float|bool`, so the call's return type no longer brings back the `null` the function filtered out.
- **A passing strict `in_array()` against `object` elements keeps the needle's classes.** A `Foo|Bar|string` needle checked against a `list<object>` still reads as `Foo` or `Bar` inside the branch.
- **A typed array no longer proves it holds the keys an open array shape requires.** A parameter declared `array<string, int>` or `list<int>` is no longer taken to match an `array{foo: int, ...}` or `list{int, ...}` it says nothing about.
- **Shape keys spelled as class constants are read as the keys they hold.** A docblock shape like `array{Slots::NAME: string, Slots::AGE: int}` whose constants are `0` and `1` is now accepted where a `list` is expected.
- **Blade `{{!!$flag}}`.** A double negation written without a space is an escaped echo again, instead of a raw echo that broke the rest of the template.
- **An assignment is seen by the rest of the expression it sits in.** In `if (($user = find($id)) && $user->isActive())`, the right-hand side now knows what `$user` holds, and `[$i++, $i]` holds the old `$i` in its first item and the incremented one in its second.
- **Narrowing follows the order a condition is evaluated in.** In `if ($x instanceof Foo && ($x = make()))` the body sees what `make()` returned, and a call that changes an object between two checks drops what the first one proved. A check inside one argument no longer narrows the arguments after it.
- **Ternary and `match` arms see what was assigned earlier in the statement.** In `[$x = 1, $x > 0 ? needInt($x) : 0]`, the arm reads the `1` rather than what `$x` held before the statement.
- **Completion after `&&` knows what the left side proved about a property.** In `if ($this->pet instanceof Cat && $this->pet->`, completion offers `Cat`'s methods alone.
- **A call nested in an expression writes through its reference parameters.** `$found = [preg_match('/(\d+)/', $s, $m), …]` now leaves `$m` typed the same as a `preg_match()` call on its own line.
- **`@phpstan-self-out` applies wherever the call sits.** A call whose result is assigned or tested, such as `$ok = $box->replace('x')`, now updates the type of `$box` too, not only a call made on its own line.
- **Variables inside a closure keep their types when another method is chained onto the call.** In `$obj->with(function (string $s) { … })->run()`, hover and completion inside the closure again know the types of its parameters and locals.
- **`@throws` and function imports land in the namespace block you are in.** In a file with several `namespace` blocks, the `use` line added by these completions no longer goes into another block, and in Blade templates it is no longer dropped.
- **Writing to a `global` variable is no longer reported as unused.** The same goes for a write through a reference, such as `$first = &$items[0]; $first = 5;`.
- **phpcbf fixes against the `[phpcs]` standard.** With `standard` set in `.phpantom.toml`, formatting now applies the rules PHPCS reports instead of PHP_CodeSniffer's default standard.
- **A `[phpcs]` standard now selects phpcbf too.** A project that sets `standard` without a ruleset file or `require-dev` entry gets its fixes from phpcbf instead of the built-in formatter.
- **Code actions work in editors that cannot resolve them lazily.** Quick fixes and refactorings such as extract function now apply in clients that do not support `codeAction/resolve`.
- **A class with `__toString()` is accepted as `Stringable`.** Passing one where `Stringable` or `string|Stringable` is expected no longer reports a false type mismatch, whether the method is declared on the class, a parent, or a trait.
- **`->value` and `->name` on an enum read as its cases' values and names.** A `value-of<Enum>` parameter or return, or a declared literal union, no longer reports a false type mismatch when fed an enum's `->value`.
- **A new import inside a braced `namespace { }` block is indented to match the block.** Imports added by completion, code actions and class moves no longer land flush left.
- **Moving a class out of a braced global `namespace { }` block keeps the file valid.** The new namespace is written into the block's `namespace` keyword instead of a separate `namespace` statement above it.
- **Removing two unused members at the end of a group import keeps the statement intact.** `use App\Models\{User, Post, Comment};` no longer loses its closing `};` when "Remove all unused imports" or `fix` drops the last two members.
- **Imports and `namespace` statements go ahead of code written on the `<?php` line.** In a file with no `namespace` whose code starts on the same line as the opening tag, or a `declare` after it, a new import or `namespace` statement is now written right after the tag or the `declare` instead of on the next line, below the code it was meant to cover.
- **Diagnostics appear in editors that do not answer a file-watcher registration.** An editor that never replied to the server's request to watch files, such as CodeLite, showed no diagnostics at all. The request is now only sent to editors that support it, and a missing reply no longer holds anything up.

## [0.11.1] - 2026-10-07

### Added

- **Laravel translations are understood across locales.** JSON and PHP language files under `lang/` and `resources/lang/` share navigation, locale and replacement-key completion, and hover with links to each locale's value. Missing keys offer an insertion quick fix when their PHP group file already exists. Contributed by @shuvroroy.
- **Code that cannot be reached is dimmed.** Statements after a `return`, `throw`, `exit`, `continue`, `break`, or an `if` whose every branch leaves the block are greyed out the way an unused import is. Contributed by @petrovo-as.

### Changed

#### Performance and memory

- **Indexing Blade templates with no DocBlock is now much faster.** Templates without a DocBlock declaring their variables have those variables inferred from the controllers that render them, and that is now much faster. The work is also spread across every core.
- **Faster reference counts and hover in untyped code.** Code lenses, hover and diagnostics are faster in projects where untyped methods call other untyped methods.
- **Code that calls undefined functions is analyzed much faster.** Files full of calls to missing helpers or functions from uninstalled packages no longer take minutes to analyze.
- **Files with many diagnostics are reported much faster.** Large files that produce thousands of diagnostics no longer spend most of their time locating each one.
- **Long scripts are analyzed much faster and in far less memory.** A long file of top-level code, such as a legacy procedural script or a generated configuration file, no longer takes minutes and gigabytes of memory to analyze.
- **Untyped methods in large classes are inferred much faster.** Code that calls methods without a declared return type from a large class, such as a generated one, is no longer slowed down in proportion to the size of the file that declares them.

### Fixed

- **Moving a class is refused when other declarations share its `namespace`.** In the editor or with `phpantom_lsp move`, a class that sits beside other classes, functions, or constants under one `namespace` is no longer moved on its own, which put those in the new namespace while every reference to them kept the old one.
- **Moving a class into the global namespace is refused in a file with several `namespace` statements.** In the editor or with `phpantom_lsp move`, the move no longer deletes the class's `namespace` line, which left the class in another namespace or the file invalid.
- **Moving a class imports what its own `namespace` block used.** In a file with several `namespace` blocks, moving a class, in the editor or with `phpantom_lsp move`, now imports the names its block reached through the old namespace, and no longer imports names another block wrote.
- **Moving a class changes the right `namespace` block.** When several `namespace` blocks in one file each declare a class with the same short name, moving one of them, in the editor or with `phpantom_lsp move`, no longer rewrites another block's `namespace`.
- **A function added by a watched file is found right away.** A function declared in a file created or changed on disk no longer keeps being reported as undefined while a diagnostic pass or hover is running.
- **Global Laravel translation paths are recognised.** Translations registered with `loadTranslationsFrom($path)` now support completion, navigation, hover, and key diagnostics, including when the namespace is explicitly `null`. Analysis also finds translation files when `--project-root` is a relative path. Contributed by @shuvroroy.
- **Alpine and Vue `:attr` bindings in Blade.** A `:name="…"` attribute on a plain HTML tag is no longer parsed as PHP, so it stops producing syntax errors. Only `<x-…>` component tags evaluate bound attributes.
- **laravel-ide-helper files are skipped.** `_ide_helper.php` and `_ide_helper_models.php` are no longer indexed, since PHPantom resolves facades and models natively and their stand-in classes only competed with the real ones. List them with a leading `!` in `[indexing] exclude` to index them anyway.
- **PHP_CodeSniffer only runs on projects that use it.** A `vendor/bin/phpcs` installed by some other package no longer produces coding-standard warnings. PHPCS diagnostics now need `squizlabs/php_codesniffer` in `require-dev`, a PHPCS ruleset file, or a `[phpcs] standard` in `.phpantom.toml`.
- **Import edits and unused-import hints respect each `namespace` block.** In a file with several `namespace` blocks, an import in one block no longer hides the "Import class" action in another, and an import only another block uses is now dimmed as unused. New imports from code actions, completion, PHPStan quick-fixes and class moves go into the block that needs them, and renaming or moving a class updates each block's own import.
- **`analyze` reports the same Blade diagnostics on every run.** In Laravel projects, the diagnostics reported for Blade templates no longer vary between runs, and they now match what the editor shows.
- **Blade only recognizes the directives your Laravel version has.** A directive newer than the installed Laravel is plain text, the way Blade itself treats it. A `"@context"` key in a JSON-LD block no longer produces a cascade of syntax errors on Laravel versions before 11.
- **A container binding removed from a service provider stops resolving.** Deleting the last `$this->app->bind(...)` from your providers no longer leaves the old key resolving to its class until restart.
- **Member actions appear when the selection starts in the indentation.** Getter/setter, property hooks, visibility and other actions on a property or method are now offered with the cursor at column 0, on a whole-line selection, or when the selection begins on the line above.
- **A `match (true)` arm knows the arms above it did not match.** `match (true) { $name === null => 'none', default => strtoupper($name) }` no longer reports `$name` as possibly `null` in the `default` arm. Hover, completion, and the value of the `match` see the same narrowing, wherever `default` is written. Contributed by @phcorp.
- **Imports of non-ASCII class names no longer break a file.** An import such as `use App\Models\Øl;` no longer stops the file's diagnostics and code actions.
- **Files outside the project no longer break indexing.** A PHP file opened from elsewhere, or a file change in another folder of the editor's workspace, no longer stops the server when the configuration changes or drops the other file changes reported with it.
- **Blade `@break` and `@continue` keep their condition.** `@break($done)` and `@continue($skip)` in a loop now only leave the iteration when the condition holds, instead of being read as an unconditional jump.
- **An integer `range()` passed straight into a call is a list of integers.** `array_map(fn (int $i) => …, range(0, $n - 1))` no longer reports that the callback is passed `int|float`. Contributed by @phcorp.
- **Type checks narrow properties the way they narrow variables.** `if (\is_resource($this->stream)) { fclose($this->stream); }` on a `resource|null` property no longer reports "expects resource, got resource|null". Every type-check function now narrows a property, with or without a leading backslash. Contributed by @phcorp.
- **`\extract()` and `\compact()` are recognised with a leading backslash.** Variables they define or read are no longer reported as undefined or unused.
- **A Blade template's first import no longer breaks the view.** When a template's first line opens a block, such as `@if` or a component tag, completing a class adds its `@use` above that line instead of inside the block, where the view would no longer compile. A template that opens with `@php` gets the import as a `use` statement inside that block.
- **Unused `@use` imports in Blade templates are dimmed.** A `@use` directive that nothing in the template uses is now shown as unused, the way an unused PHP import is, and `phpantom_lsp fix` removes it.
- **A `@use` group import wrapped over several lines no longer shifts a Blade template.** Hover, diagnostics and go-to-definition no longer land on the wrong line for the rest of a template that wraps a `@use('App\Models\{...}')` list over several lines.
- **An unsealed array shape is held to the entries it lists.** A parameter or return type such as `array{foo: int, ...}` or `list{string, int, ...}` no longer accepts any non-empty array: an argument that leaves out `foo` or gives an entry the wrong type is reported, and so is an extra entry that does not fit the `...<K, V>` tail.
- **An unsealed array shape passed to a typed array is held to the entries it lists.** A value typed `array{foo: string, ...}` is no longer accepted where `array<string, int>` is expected: the entries it lists, and what its `...` tail holds, have to fit the parameter's key and value types.
- **A shape keyed by a class constant is no longer reported against a typed array.** A docblock type such as `array{Slots::FIRST: string}` passed where `array<int, string>` is expected no longer reports that its key is a string.
- **Type checks narrow in any letter case.** `Is_String($x)` and `IS_RESOURCE($this->stream)` now narrow exactly as `is_string()` and `is_resource()` do, since PHP function names are case-insensitive.
- **Comparing an array to an array literal narrows it.** After `if ($arr === [null]) { return; }`, an `array{?string}` is known to hold a `string`, so passing it where a `list{string}` is expected is no longer reported. Inside the `if`, the array is the literal's `array{null}`.
- **`match` arms narrow the way the equivalent `if` does.** An arm with several conditions sees what any one of them proves, not what all of them would together. An `&&` in a `match (true)` arm condition narrows the operands after it, and a `match` passed straight into a call narrows its arms the same as one assigned to a variable first.
- **A ternary or `match` to the right of `&&` or `||` knows what the left side proved.** `$x !== null && ($x->ready ? process($x) : null)` no longer reports `$x` as possibly `null` inside the ternary.
- **Hovering a Blade component tag's name or a directive's keyword shows nothing unrelated.** On `<x-panel :author="$post->author">` or `@if($cond)`, hovering the tag name or `@if` no longer describes code the template never wrote or the expression after it.
- **Hovering a raw Blade echo's `{!!` or `!!}` describes the echo.** On `{!!$html!!}`, hovering either delimiter no longer shows the hover of `$html`, and go-to-definition on it no longer jumps to the variable. The hover now says the output is not escaped.
- **Config keys a package sets from its service provider are known.** `auth('sanctum')` is no longer reported as an unknown guard, and neither is any other key a package's provider sets with `config([...])` or `Config::set()` instead of shipping it in a config file.
- **`in_array()` only narrows by what the list proves.** When `in_array($x, $list, true)` is false for a `list<string>` or `list<User>`, `$x` can still be a string or a `User`, since the list may not hold that one, and when it is true for a list of `mixed` values, `$x` keeps its classes. A literal list such as `[null, '']` narrows both ways, now also when it is stored in a variable first.
- **A raw Blade echo written inside literal braces is read as a raw echo.** In `{{!!$html!!}}`, the braces around the echo are plain text, so hovering one no longer shows `e()` or leads to its declaration, and the echo's own `{!!` and `!!}` are described and coloured as a raw echo's.
- **An import added to a `namespace` written on one line goes inside it.** A `use` from a code action, completion, or class move is no longer written after the closing `}` of `namespace B { class Foo {} }`, outside the block, where PHP refuses the file. A `namespace` whose `{` is on the next line no longer gets the `use` between its name and the brace.

## [0.11.0] - 2026-10-05

### Added

#### Command line

- **`phpantom_lsp format`.** Formats every PHP file and Blade template in a project using the same formatter the editor runs on save: the project's Laravel Pint, php-cs-fixer, or PHP_CodeSniffer if it uses one, and the built-in formatter otherwise. `--check` lists unformatted files and exits non-zero without changing anything, so CI can enforce formatting. Paths can be named to limit the run, and `--format github` and `--format json` are supported.
- **`phpantom_lsp move`.** Moves a class or a whole namespace and updates declarations, imports, references, and PSR-4 paths across the project. Both sides can be class names or PSR-4 paths, and `--dry-run` checks a move without writing anything. Moves that would overwrite an existing class, or that target a Composer-installed class, are refused. Mentions the move cannot rewrite (in Blade templates, YAML, baselines, or path strings) are listed with file and line. Supports `table`, `json`, and `github` output. Contributed by @calebdw.
- **`analyze` accepts multiple paths.** `phpantom_lsp analyze app/ lib/Helper.php tests/` checks everything named, so a pre-commit hook or CI step can pass only the changed files.
- **`phpantom_lsp init` asks what to set.** On an interactive terminal, `init` (and `init --global`) asks about the most commonly changed settings and writes only the answers that differ from the defaults. `--yes` or piped input skips the questions.

#### Editing and navigation

- **Call hierarchy.** See which functions and methods call a given one, and what it calls in turn. Call sites are grouped by the function containing them, and callees resolve across files, including the constructor a `new` runs. Contributed by @sidux.
- **Reference CodeLens.** A clickable reference count above classes, functions, methods, properties, and constants opens the Find References list. It replaces the read-only count at the end of the line. Counts are computed in the background, update only what an edit could have changed, and keep their line while loading so the file does not jump around. Contributed by @sidux.
- **Implementation CodeLens.** Interfaces, abstract classes, and their methods show a clickable implementation count. Contributed by @sidux.
- **Convert qualified names to imports.** A code action adds the matching `use`, `use function`, or `use const` and shortens every matching name in the file, adding an alias if the short name is already taken. A second action imports everything from a whole namespace at once. Contributed by @calebdw.
- **Navigate to PHP classes from YAML and XML.** Ctrl+Click a fully-qualified class name or `Class::member` in any YAML or XML file to open it. These also count in Find References and CodeLens. Contributed by @sidux.
- **PHPUnit data providers and dependencies are navigable.** Ctrl+Click the method named in `#[DataProvider]`, `#[DataProviderExternal]`, `#[Depends]`, `@dataProvider`, or `@depends` to jump to it. Renaming the method updates these references, and a name that matches no method is reported.

#### Diagnostics

- **PHPMD support.** Projects using PHP Mess Detector 3 see its findings in the editor on save, or across the project when workspace diagnostics are on. The project's PHPMD config and `vendor/bin/phpmd` are found automatically, and each finding links to the rule's documentation. Configure or disable it with `[phpmd]` in `.phpantom.toml`.
- **Nullable argument mismatches can be warnings.** With `[diagnostics] downgrade-nullable-argument-mismatch = true`, an argument that only fails its parameter type because it might be `null` is reported as a warning instead of an error. Off by default. Contributed by @iz-ahmad.

#### Blade templates

- **Blade formatting.** Formatting a `.blade.php` file fixes its indentation, following directives, HTML and component tags, and attribute lists, without changing line contents. `<script>`, `<style>`, `<pre>`, `<textarea>`, `@php`, `@verbatim`, and `blade-formatter-disable` regions keep their layout, and templates where indentation is part of the output (Envoy, Markdown mail) are left alone. Projects that format Blade with Pint keep using Pint. Set `blade-php = true` under `[formatting]` to also format the PHP inside templates and normalise Blade spacing (`@if($a&&$b)` becomes `@if ($a && $b)`).
- **Document outline.** The outline, breadcrumbs, and go-to-symbol show a template's sections, stacks, and components, nested the way the template is, and jump to the right place in the template.
- **Folding ranges.** Blade block directives, component tags, and the PHP inside a template fold on the correct lines.
- **Custom directives.** Directives a project registers with `Blade::directive()` or `Blade::if()` are completed, and the PHP passed to them is type-checked like any built-in directive.
- **"Create missing view" quick fix.** A `view('name')` call naming a template that does not exist offers to create it where Laravel will look for it, including a package's own view directory.
- **Unbalanced component tags are reported.** An unclosed `<x-alert>`, a mismatched closing tag, or a stray closing tag is flagged, including `<livewire:…>` tags.
- **Named slots belong to the component.** `<x-slot:title>` now defines `$title` as a `ComponentSlot` inside the component's template rather than in the caller's.
- **Attribute completion for components without `@props`.** An anonymous component that reads variables like `$title` without declaring them now offers those variables as attributes. Templates with `@props()` keep their declared list, with any undeclared reads added after it.

#### Laravel

- **Model PHPDoc types resolve to real Eloquent classes.** `builder-of<Model>`, `collection-of<Model>`, `factory-of<Model>`, and `relation-of<Model, 'relation'>` resolve to the builder, collection, factory, or relation the model actually uses, including custom ones. Dotted paths such as `relation-of<User, 'posts.comments'>` follow relations, unions are supported, and anything unresolvable falls back to the framework's base class. Contributed by @calebdw.
- **Model operators work inside `Closure(…)` parameter types.** A parameter like `Closure(builder-of<static>): mixed` now types the closure's argument, so the closure body gets completion, hover, and navigation. Contributed by @calebdw.
- **`view-string` parameters.** A parameter typed `view-string` completes template names, and a literal that names no template is reported as an unknown view. Variables, concatenations, and unregistered package namespaces are not flagged. Contributed by @calebdw.
- **Storage disk names.** Disk names passed to `Storage::disk()`, `fake()`, `persistentFake()`, `forgetDisk()`, and `#[Storage]` complete from `config/filesystems.php`, with hover, go-to-definition, Find References, and typo diagnostics where Laravel requires a configured disk. Contributed by @shuvroroy.
- **More configured service names.** Names passed to framework facades, container attributes, and auth middleware get the same completion, hover, navigation, references, and diagnostics from their config files. `Log::stack()` channel arrays are understood too. Contributed by @shuvroroy.
- **UUID and ULID keys are strings.** Models using `HasUuids` or `HasUlids`, directly or through a parent or trait, type their primary key as `string`. Contributed by @shuvroroy.
- **Custom pivot accessors.** A relationship renamed with `->as('participation')` exposes `$participation` instead of `$pivot`, keeping the configured pivot model. Contributed by @shuvroroy (#381).

#### Configuration and indexing

- **Exclude paths and add PHP extensions.** `[indexing] exclude` in `.phpantom.toml` takes gitignore-style patterns to skip during indexing and `analyze`, and `[indexing] extensions` adds file types to treat as PHP (e.g. `["module", "inc", "theme"]` for Drupal). Open files are always served. Contributed by @syntlyx.
- **Editor file settings reach the index.** The `[indexing]` lists can also come from the editor. VS Code and Cursor send `files.exclude` and `files.associations` automatically, and other editors can pass them as initialization options (see [Editor Setup](editor-setup.md)). Editor and project settings are combined, and changes from either apply mid-session without a restart.
- **Symlinked directories are indexed.** Code linked into the project from elsewhere resolves like any other project code, and paths keep their symlinked spelling. Contributed by @liudashuang.

#### Tooling and platform

- **Semantic export library feature.** The optional `semantic-export` feature lets other tools pass in PHP documents and get back declarations, references, calls, and diagnostics without running an LSP. A separate `offline-stubs` feature prevents build-time stub downloads. Contributed by @aaaaaandrew.

### Changed

#### Behaviour

- **Extract interface is only offered to editors that can create files.** Editors that do not support creating files no longer see an action they cannot apply.
- **Updated the bundled mago toolchain to 1.47.5.** Contributed by @nguyentranchung.

#### Performance and memory

- **Find References and reference CodeLens are much faster.** Searches now run in parallel, skip files whose calls clearly target a different class (such as `$this->save()` or `Invoice::save()`), and only type-check the function bodies that contain the accesses being searched for. Results are kept and reused by later searches, including searches for other names. On a large Laravel app, opening 250 classes in turn went from 3.0 to 1.7 seconds of searching.
- **Repeated reference searches no longer re-read files.** Positions are stored with each result, so a second search for the same declaration takes milliseconds instead of seconds.
- **Editing a signature no longer throws away all reference results.** Only files that depended on the changed class or function are re-checked. Previously one edit made the next search start from scratch (about 120 CPU-seconds on a large Laravel app).
- **Reference searches reuse resolved classes.** Searches used to rebuild the same Laravel models over and over (one model 767 times in a single search). Opening a class with 26 members went from 1.2 to 0.4 seconds. Go-to-definition, hover, signature help, rename, and code actions now share the same cache.
- **Faster hover and completion in long functions.** Functions with many local variables and calls no longer slow down every request: hovering 315 expressions in one file went from 4.3 to 0.8 seconds.
- **Faster analysis of large files.** Looking up `/** @var */` annotations no longer scans back to the top of the file. Analysing php-parser's generated `Php7.php` went from 2.1 to 0.7 seconds.
- **Faster workspace symbol search.** Files are only read when they contain a match, instead of every file on every keystroke.
- **Faster edits.** Editing a file no longer scans the inheritance of the whole workspace.
- **Less copying of Laravel config data.** `config()` lookups and config, view, and translation key checks share data instead of copying it.
- **Faster interface member lookup.** Searching an interface chain for a member no longer visits each parent twice per level.

### Fixed

#### Type inference

- **Constants defined from other constants have a type.** A namespaced `const TWO = ONE * 2;`, and class constants that divide, concatenate, or spread other constants, now resolve. `self::ONE / self::THREE` is a `float` (or the exact integer when it divides evenly), and `[...parent::KEYS, 'c' => 'c']` is the full array shape.
- **Constants built from enum cases resolve.** `const DEFAULT = Suit::Hearts;` holds the enum, including on the enum itself or when typed `static`. `Suit::Hearts->value` and `->name` give the literal behind the case.
- **Expressions over known values resolve to the exact value.** `'1' . 'a'` is `'1a'`, `(int) '1'` is `1`, `"$id"` is `'42'` when `$id` holds `42`, and arithmetic, `%`, `~`, `<=>`, unary signs, and casts on literal operands fold to what PHP computes (including inexact float results). A numeric string counts as its number (`'1' + 1` is `2`), `PHP_INT_MIN` has its value, a comparison of two known numbers is `true` or `false`, and adding two untyped values gives `array|int|float` rather than `mixed`.
- **Magic constants resolve to their value.** `__LINE__`, `__NAMESPACE__`, `__FUNCTION__`, `__METHOD__`, and `__TRAIT__` give what PHP substitutes, naming `{closure}` inside a closure. Inside a property hook, `__FUNCTION__`/`__METHOD__` give the hook's name (`'$name::get'`) and `__PROPERTY__` the property's name.
- **PHP version constants follow the configured PHP version.** `PHP_MAJOR_VERSION` and `PHP_MINOR_VERSION` no longer show the stubs' values, and the patch-dependent constants are typed without claiming a value.
- **`$value::class` and `$value::CONST` resolve.** `$foo::class` is `class-string<Foo>`, and `$pen::LIMIT` reads the constant from the value's class.
- **`isset()` and `empty()` are booleans when used as a value.**
- **A variable assigned from a `void` call is `null`, not `void`.**
- **`??` on a never-assigned or always-`null` left side no longer widens to `mixed`.** `$x = $a ?? 1;` with `$a` never assigned is `1`.
- **Assignments used as values resolve everywhere.** `$x = ($y = 1)`, an assignment inside a `??` chain, a `match` arm, a ternary branch, a constructor argument, or as a call receiver (`($cache[$key] ??= $this->compute($key))->isValid()`) now gives both the target and the surrounding expression the assigned type. `??=` in a condition also leaves its target non-null in the branch.
- **`??=` leaves a value behind and makes its target non-null.** `$this->regexp ??= $this->generate();` no longer leaves the property nullable, and `$x = $cache[$key] ??= expensive();` gives `$x` a type.
- **A reference assignment used as a value reads as its target.** `($a =& $var) ?? 'hello'` leaves `$a` as `$var`'s type.
- **An assignment whose right side cannot be typed forgets the old type.** `$acc = $acc->merge($x)` no longer keeps the variable's previous `null`, which caused false "method call on null" reports in loops.
- **By-reference parameters read back correctly after a call.** The caller's variable now takes what the callee writes on every path, including paths that return early (`4|5` rather than `4`), with a declared nullable type kept only where some path does not write. This works for functions, methods, static calls, constructors, and `self::`/`static::`/`parent::`/`new self` calls. What the variable held before the call is no longer checked against the parameter type, and unpacking an array into a by-reference variadic keeps it an array.
- **Same-named functions in different namespaces no longer share by-reference results.** `A\fill($x)` could report what `B\fill()` writes.
- **Files with several `namespace` blocks resolve names per block.** Class names, `use` imports, `self`/`static`, `key-of<A::FOO>` operands, and by-reference results now resolve against the block they appear in rather than the first block or another block's imports. This fixes false type mismatches, wrong members and hover, missed "unknown class" reports, and empty Find References. A comment directly above a `namespace` no longer hides it.
- **Class names resolve the way PHP resolves them.** Inside `namespace App\Models`, `Event::class` names the sibling model rather than a loaded global `Event`. `new self(...)` in a namespaced `Error` or `Exception` builds that class, not the built-in. A namespaced `Exception extends \Exception` keeps its inherited members even in files that import it.
- **`new` of an unloadable class keeps its type,** and **`new parent(...)` resolves** to the class being extended.
- **A static call on an object resolves through its class.** `$foo::doStaticFoo()` and `$this->prop::doStaticFoo()` resolve like the matching `->` call, including on unions, intersections, and properties holding a `class-string`.
- **Calls on intersections keep every part.** A method both members of `A&B` declare returns the intersection of their return types, and a `@return static` call on `Scope&Invoker&Emitter` keeps all three.
- **Override return types are respected.** An override's narrower return type is no longer replaced by an interface's `: self` higher up, and a method declared `: never` stays `never` when it overrides an interface method.
- **`self` and `static` members resolve against the right class.** A property typed `static` resolves to the class it is read from, a member typed `?self` resolves against the class it is read off rather than the calling code's class, and `self::$items[$key]` is treated as an element of the static property.
- **`$this` in a trait.** A trait with `@phpstan-require-extends`/`-implements` sees the required class's properties, and `return $this` in a trait method is checked against what the classes using it have in common rather than the trait itself.
- **An untyped property assigned an array literal is an `array`.**
- **A nullsafe access on a nullable value can be `null`.** `$e?->getMessage()` on `?Exception` is `string|null`, as is every link after it in a chain. A check like `if (!$tier?->journey instanceof Journey) { return; }` proves `$tier` non-null afterwards.
- **Variable types follow control flow more accurately.** A `catch` sees what the `try` body assigned, `finally` sees every path through `try` and `catch`, a `catch` the `try` body provably cannot reach no longer leaks its assignments, a `switch` arm that exits no longer contributes, `continue` inside `switch` acts as `break`, and a `for` loop known to run is no longer joined with the pre-loop value.
- **Loops settle on the right types.** A read in nested loops sees the settled loop types, a `for` counter is `int` in the body (also in Blade `@for`), a `foreach` over a known-empty array no longer checks its body with a stale loop variable, and a loop inside a provably dead guard keeps its own assignments.
- **The seed-or-merge accumulator idiom keeps its type.** `$acc = null;` followed by a loop that seeds `$acc` once and merges into it afterwards no longer loses the type at the merge.
- **A `static $var;` local carries values from earlier calls.** It is typed from its initialiser plus every assignment in the body.
- **`extract()` defines the right variables.** Shape keys become variables with their value types (optional keys widen), unions of shapes write every key, the `EXTR_*` flags and prefixes are honoured, and an array of unknown keys makes the affected locals `mixed`.
- **Values that are genuinely open are `mixed`.** `mixed` beside a class in a union no longer absorbs the class at a later join, `int|float` no longer collapses to `float` or duplicates members across branches, and `(A&I)|A` simplifies to `A`.
- **A call whose argument can never be reached is `never`.**
- **A `@return mixed` body with disagreeing returns keeps `mixed`.** A body whose returns all agree still narrows the declaration.
- **Standard-library return types follow the arguments.** `var_export($v, true)`, `mb_internal_encoding()`, `version_compare()`, and `sscanf()` return only the form the call selects. `range(1, 10)` is a list of `int`, a seeded `array_reduce()` is never `null`, `pow()` only includes `object` for an operand that is one, and `ini_get()` on a core directive is not `false`.
- **String builtins handed literals return literals.** `strtolower()` and the other case-changing, trimming, and repeating functions compute their result per literal alternative. Values from platform constants like `PHP_OS` are left alone.
- **`get_class()` and `ReflectionClass::getInterfaceNames()` return class names.** `func_get_args()` is a `list<mixed>`, and `(bool) preg_match(…)` narrows the matches array.
- **`new ReflectionProperty(Foo::class, 'bar')` remembers what it reflects,** like `getProperty()` already did.
- **DOM stubs.** `DOMElement::$attributes` is never null (`hasAttributes()` proves it on `DOMNode`), and `DOMNamedNodeMap::item()` returns `DOMAttr|null`.
- **Casting a non-empty array to `object` keeps `stdClass`.** `(object) ['foo' => 1]` is `object{foo: int}&stdClass`.
- **Project files that redeclare PHP builtins no longer override them.** Signature-reference packages such as `phpstan/php-8-stubs` used to replace the built-in signatures and lose precision (for example `array_keys()` key types).
- **A class named `Scalar` or `Numeric` is a class.** Only the lowercase spellings are PHPDoc pseudo-types, so `PhpParser\Node\Scalar` resolves.
- **Two anonymous classes at the same position in different files no longer share one resolution.** Laravel migrations, which all open `new class extends Migration {` at the same offset, now each describe themselves.
- **Binding a template from `$map[']']` keeps its type.**

#### Closures and callables

- **First-class callables resolve.** `$test->$name(...)`, `Test::$name(...)`, and `$invokable(...)` return the right type, and `$closure(...)` has the wrapped closure's signature.
- **Calling a closure or callable held in a variable resolves.** `$c()`, `[$this, 'doFoo']()`, `($this->factory)()` on a `callable(): Scope` property, a closure returned by another closure (`$a()()`), and a closure declared `: static` all return the right type. `(fn () => $this)->call($obj)` binds `$this` to `$obj`.
- **A closure's return type comes from its body.** A narrower body wins over the declared hint (`Box<stdClass>` over `Box`, `class-string` over `string`), and an untyped `fn ($b) => $b` receives the element type the call hands it.
- **Closure parameters are inferred from context.** `usort`/`uasort`/`uksort` callbacks get the element (or key) type, a `@phpstan-type` alias for a `callable`/`Closure` types the closure passed to it (including aliases on traits and parents), `callable(static)` binds to the receiver's class, and a docblock above the statement a closure is assigned in (or inline between `=` and `function`) types its parameters.
- **Closure parameters are typed like function parameters.** A variadic is `array<int|string, T>` rather than a list, an untyped one is `array<int|string, mixed>`, and a `null` default accepts `null` however it is capitalised.
- **`$this` inside a `@param-closure-this` closure is the bound object for diagnostics too,** not only for completion.
- **Variables assigned inside arrow functions have a type,** and an arrow function passed to `Closure::call()` reads the bound `$this`.
- **Variables inside an immediately invoked closure resolve** for hover, completion, and the other position-based features.
- **By-reference captures behave like PHP.** A closure capturing itself by reference sees the closure, not `null`. `use (&$x)` on an undefined variable makes it `null` and is no longer reported as undefined. Reads inside and after the closure see every value the closure assigns, including assignments on early-return paths and inside closures passed to a constructor.
- **Closures in an immediately indexed array literal keep their parameter types.** `['isNull' => static fn (Type $t) => …][$name]`.
- **A self-referencing first-class callable no longer crashes the server.** `$callback = $callback(...)` (found in `league/csv`) used to recurse until the process aborted.

#### PHPDoc

- **Type aliases work outside the file that declares them.** Members typed with a `@phpstan-type`/`@psalm-type` alias resolve wherever the class is used, including imported, inherited, and trait-provided aliases, and hover shows the aliased type. Imported aliases built from other aliases expand fully (cycles stop at `mixed`), parameters typed with an alias narrow correctly, and iterating an aliased array keeps its key type.
- **Class names in `@phpstan-type` and `@phpstan-import-type` are references.** They are now highlighted, navigable, found by Find References, and updated by rename.
- **Inline `@var` annotations are read more completely.** `@phpstan-var` and `@psalm-var` are honoured (and win over `@var`), `@var` above an array-element assignment applies (`$GLOBALS['x']`), stacked docblocks all apply, `@var` above `global` only types the imported variable, an inline `@var` describes the variable after the assignment rather than its right-hand side, and short class names resolve from the current namespace first (so `list<Error>` means the local `Error`).
- **Vendor-prefixed tags win wherever they appear.** `@phpstan-method`/`@psalm-method` beat `@method`, and `@psalm-param`/`@phpstan-param` type the parameter inside the function body.
- **`@param-out` is read,** including conditional `@param-out` types.
- **`@param` handling in methods.** Unnamed `@param` tags match by position and their descriptions appear in hover. A union continued on the next line keeps its full type. A trait method inherits the `@param` of the interface method it implements.
- **Docblock types combine correctly with native types.** `@param array<int, string>` on `array|false` keeps the `false`, `@return never` is honoured, and a `@return` tag no longer adds a value the native return type rules out (such as `false` on a method declared `: DateTimeImmutable`).
- **`self` in a docblock names the class that wrote it.** This covers inherited `@var string|self` properties, `object{foo: self}`, and `list<self>`, and a parameter typed `self` hovers as the class name.
- **`@mixin` on a trait applies to the classes using it,** with `static` in its type arguments bound to the receiving class.
- **`key-of<…>` reads more.** `key-of<array<V>>` and `key-of<V[]>` are `int|string`, method `@return key-of<self::TABLE>` resolves at the call site, list constants contribute integer keys, numeric keys are `int`, and `key-of<A|B>` covers both.
- **Duplicate shape keys are one key.** `array{a?: string, b?: string, a?: string}` lists `a` once.
- **A PHPDoc type that does not parse is ignored** instead of being shown as the type.

#### Generics and templates

- **More templates bind from the call.** Templates now bind from omitted arguments with defaults, promoted `@var T` properties, array literals passed to a constructor, objects built inline in a chain (`(new Proxy(new Engine()))->`), templates reached through another template's bound, a `class-string` argument's `@extends`/`@implements`, an argument's own ancestors (`IntType` implementing `Type<int>`), and every argument that names a template (`new Pair(new Cat(), new Dog())` is `Pair<Cat|Dog>`). `new RecursiveIteratorIterator(new RecursiveDirectoryIterator($dir))` yields what it really yields.
- **Templates keep literals where they should.** `id('hello')` is `'hello'`, a bounded template on a native hint binds the literal passed, array literal elements all contribute (`1|2`), and an empty literal binds `never`. Objects built with `new` still generalise unless the bound is a scalar.
- **Bounded templates behave correctly.** Inside its function a bounded template stays `T` for hover and when passed on, while its bound still supplies members. An argument outside the bound falls back to the bound, and inherited bounds follow `@extends` arguments.
- **Template defaults apply.** `@template U = string` fills in arguments an `@extends` leaves out, unbound method templates use their default rather than their bound, a default can name another template (including a class template), and declared generic types spell out defaulted arguments (`Test<false, true>`).
- **A `null` default no longer makes a template parameter nullable.**
- **A method template with the same name as a class template binds from the call.** `$reflection->getAttributes(Ref::class)` is `ReflectionAttribute<Ref>`.
- **Callables and closures bind templates.** A callable held in a variable binds the templates its return type names, a closure's parameter is typed from the generic the call binds (including through fluent chains), and a call argument the text-based resolver cannot read (`array_keys($a + $b)`) still binds.
- **Templates from arrays.** `@param array<T, mixed>` handed `Foo[]` binds `int|string`, and empty arrays bind `never`.
- **Inheritance carries generics.** `parent::method()` in a subclass of `Foo<Dog>` returns `Dog`, a generic trait used by another generic trait receives its arguments, an override without a docblock keeps the parent's method templates, and `static` inside a generic collapses to the named class on a `Foo::` access.
- **A new object stored in a generic property takes the property's type arguments.**
- **Iterable generics.** A single type argument to `Iterator`, `Traversable`, `IteratorAggregate`, or `Generator` names the value, `foreach` over an object that names only a value gives `mixed` keys, `SplObjectStorage<Order, int>` no longer swaps keys and values, and `new ArrayIterator($items)` keeps `int|string` keys.
- **More conditional return types are decided.** Literal subjects, constant comparisons (`$flag is PREG_SPLIT_NO_EMPTY`), conditions inside generics, offsets of templates (`T[0] is string`), `$this` against the receiver's template arguments, `is non-empty-array`, overrides that rename parameters, and `is true`/`is false` on non-boolean arguments now pick the right branch. A condition on an undeclared name gives both branches, a `never` branch stays `never`, and an intersection branch (`T&MockInterface`) is no longer turned into a union.
- **Offset access types are evaluated.** `@return Bob[TKey]` on a type alias and `Data[value-of<T>]` through an enum case give the selected entry.
- **Assertions on generic types keep their arguments.** `@phpstan-assert-if-true Ok<TOk> $this` gives `Ok<int>`, and an assertion written against an interface's template narrows to the implementation's binding.
- **Template names are not mistaken for classes.** A return typed `T1` is checked against the template's bound rather than a class named `T1`, and hover and go-to-definition on `T` in `@param T` reach the template declaration.

#### Arrays

- **Array shapes survive spreads and writes.** Spreads follow PHP 8.1 rules (string keys overwrite, integer keys renumber), entries written beside a spread keep their own types (`array{admin: AdminUser, ...<int, User>}`, also spellable in PHPDoc), literal and known-variable keys build shapes, and loops writing a list of known keys give `array{a: true, b: true}` (optional when the write can be skipped). `Foo::class` keys name the entry, including in class constants (`const MAP = [Foo::class => 'x']`), and writing through a `class-string` key keeps `class-string` keys.
- **Writes keep the shape.** Writing to an integer key, a new offset, a nested offset, a union of shapes, a property's array offset, or through `ArrayAccess` now updates the right entry. Compound assignments (`+=`, `%=`) update the element, overwriting a key keeps its position, and literal values are kept outside loops (`$data['status'] = 'draft'`). Reads of an offset see writes made below it.
- **Appends outside a loop keep exact values.** `$a = []; $a[] = 'one';` is `array{'one'}`. `array_push()` and `array_unshift()` update the array like an append, and appending to `array<int, T>` no longer claims a list.
- **Lists stay lists.** Writing back through a `foreach` key keeps a list, `unset($list[$k])` makes it `array<int, …>`, a plain `array<int>` is no longer treated as a `list` or non-empty, and the key of a `foreach` over a list is `int<0, max>` (as are `array_keys()` and `array_search()` on a list).
- **Loops build arrays correctly.** A `foreach` that rewrites every element no longer keeps the pre-loop shape, a value written under a dynamic key keeps its literal type across iterations, tuples written in one loop and destructured in another keep their element types, a by-reference `foreach` updates the array it walks, and writing through a key of unknown type no longer produces strict `int|string` keys.
- **Joins and unions of arrays are cleaner.** Joined array types drop alternatives a sibling covers, and a union of shapes folds when one covers another (including positional against keyed spellings).
- **`array{…}|null` and `?array{…}` are the same nullable shape.** Branches writing either spelling now fold into one shape instead of listing each separately. Contributed by @sidux.
- **Offset reads see every alternative.** Reading an offset of a union of shapes answers with every alternative, a `count()` check on an entry no longer leaves it `never`, and `??=` on an offset stores the element whatever the key.
- **Array functions keep shapes and literals.** `array_keys()`, `array_key_first()`/`last()`, `array_shift()`/`pop()`, `array_filter()`, `array_merge()` (including the accumulate-in-a-loop idiom), `min()`/`max()`, `preg_replace()`, and `strlen()` of a literal all keep what the input says. `reset()`/`end()`/`next()`/`prev()` no longer change the array's type, and the key-reading builtins read keys from literals, nested elements, and `array_fill_keys()`.
- **Element functions include the empty-array result.** `reset()`, `end()`, `current()` include `false` and `array_pop()`, `array_shift()`, `array_first()`, `array_last()` include `null` unless the array is provably non-empty. `next()`, `prev()`, and `array_find()` always include it, `explode()` returns a `non-empty-list<string>`, and array functions see what a guard proved about their argument.
- **`array_map()` is more precise.** It keeps the input's shape and keys (no longer inventing `int` keys), maps a `void` callback to `null`, returns what a callable-typed, first-class, or parameterless callback returns, reads the shape the callback body actually builds, and types the callback parameter as one element of a union of shapes.
- **Sorting keeps the values.** `sort()`, `rsort()`, `usort()`, and `shuffle()` give a list of the same values, and key-preserving sorts leave the type alone.
- **Destructuring assigns every target correctly.** Nullable sources add `null`, unknown sources give `mixed`, unions of shapes contribute every alternative, and a skipped position (`[, $second]`) no longer shifts the later targets.
- **A constant array of `Foo::class` entries keeps the class names.** `private const FOOS = [Foo::class];` is `list{'Foo'}`, so iterating it and making static calls on the entries works.
- **`(array)` on a union casts each member.**
- **Key spellings follow PHP.** `'+15'` and `'015'` stay string keys, single-quoted escapes stay strings, and double-quoted escapes decode (`"\x38"` is `8`).
- **`isset()` on a shape read through a variable key keeps the element type.**

#### Narrowing

- **`instanceof` narrows more accurately.** On a union, it keeps the half a subclass could satisfy (`Model&HasName|HasName`), a failed check rules out subclasses of the checked class, a passing check keeps a subclass as itself, and a template-typed value becomes `A&T` inside the branch. It also narrows against an unloadable class, against a value holding a class name (`instanceof $stmtClass`), and with `self` in a trait meaning the using classes. A failed check against a `class-string` variable rules nothing out.
- **An `instanceof` guard rules out a template bounded by the checked class.** A `@template TFactory of Factory` alternative no longer survives `! $x instanceof Factory`, which caused false return-type mismatches (most visible on model traits). Contributed by @calebdw.
- **`$this instanceof Subclass` resolves the subclass's members,** even after other `$this` properties have been read.
- **Negated and combined conditions narrow correctly.** `if (!($a instanceof X || $a instanceof Y)) { return; }` leaves `X|Y`, an `instanceof` beside an unrelated `||` operand no longer narrows the branch, a disjunction narrows each leg on its own, an `&&` operand that pins a class outranks a later disjunction, and an assignment on the right of `&&`/`||` sees what the left proved.
- **`elseif` and fall-through conditions narrow.** An `elseif` condition narrows its own `&&` operands, and falling out of an `if`/`elseif` chain whose branches all exit knows every condition was false.
- **Guards on properties, calls, and array elements hold.** A `!== null` check on a getter returning an array, an `{@inheritDoc}` override, a second accessor on the same object, `&&` chains over elements and getter chains, array elements with variable or computed offsets (`$types[$i]`, `$statements[$count - 2]`), and repeated reads of a guarded getter all keep their narrowing. A ternary re-checking a call (`$p->getDocComment() !== false ? $p->getDocComment() : null`) narrows the arm.
- **A guard's proof is recovered where it is read.** Re-testing the same condition re-applies what its branch proved (including on calls and property paths), a boolean assigned from a condition (including `||` chains) narrows wherever it is tested, a variable filled inside a guarded branch stands for the guard, the failing side of an `instanceof` records what it ruled out, variables set together in one branch keep their correlated nullability, a copy of a property carries its null check back to the property, and closures see the narrowing of what they capture.
- **Comparisons narrow more values.** `===` against a variable already narrowed to one value narrows the other side, `$a === $b` removes `null` from the nullable side, strict comparisons against `0`, `0.0`, `''`, and `[]` narrow both branches, and `<`, `<=`, `>`, `>=` narrow unions of number literals. Loose `==` narrows `mixed` and strings (to `numeric-string`) under PHP 8 rules, and `get_class($x) === Foo::class` narrows properties, elements, and call results.
- **More conditions narrow.** `=== true`/`=== false` on a check, loose `in_array()` over literals, `isset()`/`array_key_exists()` narrowing the key, `is_a()` with class-string variables or `allow_string`, `ReflectionClass::isSubclassOf()`, `enum_exists()` (to `class-string<UnitEnum>`), `strlen()`/`mb_strlen()` comparisons (to `non-empty-string` or `''`), and type checks on array offsets. Global functions written with a leading backslash (`\get_class`, `\class_exists`) narrow like the unqualified spelling, and a guarded class name written with escaped backslashes matches. A failed strict `in_array($x, [null, ''], true)` drops those values.
- **Falsy branches hold the falsy values.** In the else of `if ($count)`, under `if (!$name)` or `empty($x)`, or after `$id == null`, an `int` is `0`, a `string` is `''|'0'`, an array is `array{}`, and objects and class-strings drop out. A `non-empty-string` is no longer always truthy, a `(bool)` cast narrows like its operand, and the skipped branch of a truthy test keeps exactly the members the test rules out.
- **`count()` narrows arrays.** `count()` sizes shapes with optional entries, rules out impossible sizes, pins lists to a length (also with `COUNT_NORMAL` and, for lists of scalars, `COUNT_RECURSIVE`), and `count($xs) > 0` or writing an element proves the array is non-empty, so a loop over it is known to run.
- **`isset()` narrows more.** A named offset rules out a `string`, `isset($arr[$key])` types the offset it guards (also on static properties), `isset($var)` proves the variable exists for the whole branch, and a seed-if-absent write (`if (!isset($t[$k])) { $t[$k] = 0; }`) is no longer undone by the guard.
- **`array_key_exists()` narrows.** It narrows an optional shape key, accepts a variable key, drops the key when it fails, and with an unknown key proves the array non-empty.
- **Union and tag narrowing.** Tagged unions of shapes narrow on their tag (also in `switch`), a `foreach` key over a shape is the shape's keys and comparing it narrows the matching value, an offset read through a variable holding one literal key reads just that entry, a `switch` arm sees which `case` it came in by, `is_callable()` keeps classes that may be invokable (and calling a union of callables gives the union of their results), `is_resource()` keeps `resource` in the else branch, and type checks on `iterable` split into array and `Traversable` halves.
- **A value every check has ruled out is `never`,** and its branch no longer widens the types after it. Code in a branch a `!== null` test rules out is no longer checked against `null`.
- **Narrowing ends where it should.** A ternary's or `match`'s narrowing ends with it, a branch narrowing to an intersection no longer erases the other path's type at the join, and a write the resolver cannot type drops a stale property narrowing.
- **Calls that change state forget what was proved.** Methods returning nothing or `$this`, `@impure`/`@phpstan-impure`/`@psalm-impure` methods, objects passed to them, `parent::__construct()`, `clearstatcache()`/`unlink()`, and stateful built-ins (`SplFileObject`, `DateTime` setters) invalidate earlier narrowing, including static properties. `@pure` and value-returning methods no longer invalidate. Closures run inline, called immediately, or called later through a variable apply the same invalidation to `$this` and their captures.
- **The arguments of a `?->` call see the receiver as not null,** and a guard like `if ($queue?->pop() === null) { return; }` no longer re-proves an impure call.
- **Assertions narrow more.** A class assertion drops `null` from a union of classes, `-if-true`/`-if-false` assertions narrow untyped parameters, assertions on variadics narrow every argument, `@phpstan-assert` can narrow `$this->prop`, assertion types resolve in the declaring file's namespace, and predicates apply through properties, intersections, `&&` chains, and interfaces.
- **Readonly properties remember the constructor.** What the constructor assigns or proves (including on early-return paths and nested readonly properties) holds in every other method when it is narrower than the declaration.
- **`is_a($name, Foo::class, true)` on a string stays a string,** and an array literal built from an `@var`-annotated variable respects the guard around it.
- **Loops prove what they check.** A `do`/`while` condition narrows the body after the first pass, a `while` condition that assigns narrows, a loop that does `break 2` on a bad entry proves every entry passed, and a ternary in a `foreach` header narrows its arms.

#### Diagnostics

- **Fewer false positives on loosely typed code.** Arithmetic on untyped values, `foreach` keys of undescribed arrays, keys collected from them, and values with lenient failure branches (`tempnam()`, `curl_init()`) only need one branch to fit, even past an `if`. Declared types are still enforced. `int<0, max>` passed to `float`, `class-string` to `non-empty-string`, `array-key` to `int` or `string`, and `$a & $b` on untyped closure parameters are accepted.
- **Unverifiable members name the cause.** A member read on a value PHP leaves open now reports `mixed`, and a value guards can refine, instead of "type could not be resolved". A missing method in a loop accumulator is reported once at the call instead of blaming the variable.
- **Nothing is reported in code that cannot run.** `if (false)` branches and the negated arm of a satisfied `method_exists()` are skipped, while the guard and imports in the branch still count. A dead branch in one file no longer hides diagnostics in another, and a `}` in a comment no longer cuts an existence guard short.
- **Arrays are checked without scalar coercion.** Keys and values of an array are checked strictly in every file, since PHP never converts them.
- **New checks on generics and shapes.** Closures passed to a `callable(…)` with incompatible parameters, wrong shape keys or values, string keys passed to a list, `[]` passed to a non-empty type, subclasses binding a template differently (`IntBox` for `Box<string>`), and arguments outside a template's bound are now reported. `array_filter()` callbacks work in every mode, and `chunk()` callbacks receive the model's custom collection.
- **`deprecated_usage` is more precise.** Imports, trait methods implementing a deprecated method, overrides delegating to the deprecated method, and usages inside deprecated code are no longer flagged. Overrides inherit `@deprecated`.
- **Undefined and unused variables.** A `use (&$total)` capture and an `isset($var)` guard no longer report the variable as undefined, and a variable in a scope that `require`s a file is not reported as unused.
- **Docblock and type checks.** A `@param Closure` above `?Closure` is no longer flagged, while a docblock admitting `null` on a non-nullable signature is. Constants inside `int-mask<…>` are no longer reported as missing classes, and `Composer\Autoload\ClassLoader` resolves.
- **Unused group imports are found.** Multi-line group imports are checked member by member (including aliases and sub-namespaces), and removing every member removes the whole statement instead of leaving `use App\Models\{`.
- **Saving a class refreshes diagnostics in grandchildren,** not only in direct children.

#### Laravel

- **Class-based casts resolve to their value.** `AsEnumCollection::of(Status::class)` is `Collection<array-key, Status>`, and the other built-in cast classes (`AsCollection`, `AsArrayObject`, `AsStringable`, `AsFluent`, `AsUri`, and the rest) resolve to what they return. Contributed by @calebdw.
- **Relationships without docblocks know their related model.** `posts(): HasMany { return $this->hasMany(Post::class); }` types `$user->posts` as a collection of `Post`.
- **A base model's Eloquent settings apply to its subclasses.** `$fillable`, `$casts`, `$hidden`, `$primaryKey`, `$connection`, and `CREATED_AT` are inherited from the nearest declaring class, and a child's `casts()` is merged over an inherited `$casts`.
- **Model columns and finders.** `SoftDeletes` models have `deleted_at` and `whereDeletedAt()`, every model has `whereId()`, a single-line `casts()` keeps its last entry, the base model's settings are no longer offered as columns, and migrations' index and constraint calls no longer create or retype columns.
- **Column names complete after a relationship.** `$user->posts()->where('` offers `Post` columns, and `with('` offers its relations.
- **`$pivot` is known without opening the relationship's file,** and `morphedByMany()` relationships take effect as soon as they are edited.
- **Custom collections only where Eloquent provides them.** A `@param Collection<int, Product>` no longer offers the model's custom collection methods, and `instanceof` updates the tracked type.
- **Scopes and accessors in Find References, lenses, and rename.** `scopeActive()` used as `active()` and accessors used as `->full_name` are found, counted, and renamed with their declaration. Invalid new names are refused. Hover on them shows their docblock, and go-to-definition on a scope named like a column finder lands on the scope.
- **Go-to-definition on a facade opens the forwarded method,** including methods reached through the target's `@mixin`.
- **A class name inside a namespace no longer falls back to a facade alias.** `Cache::forget()` without an import is no longer treated as the facade.
- **Container keys resolve reliably.** Requests arriving during startup wait for the binding tables instead of seeing them empty.
- **Routes.** Only calls on the router register route names, `Route::as()` prefixes like `Route::name()`, names written before the registration count, Find References works from `->name()`, and group names compose the way Laravel does.
- **Views.** Published package views in `resources/views/vendor/<ns>` take precedence, a `resources/views` directory created later is found, view roots follow unsaved edits to `config/view.php`, a variable passed twice keeps the last value, Find References on a view name includes `@include` and `@each`, and the fully qualified `View::share()` reaches templates.
- **Config.** Config groups replace the package's copy the way Laravel merges them, go-to-definition lands in the package file for unpublished keys, lists of classes hover correctly, an empty string types as `string`, completion and type resolution read the same files, and keys set at runtime (`Config::set()`, `config([...])`, `Storage::fake()`) count as declared. Package code is no longer checked against the application's config.
- **Translations.** Subdirectory groups complete, `lang/vendor/<package>` overrides apply, keys require the package namespace, hover quotes the configured locale, resolution no longer waits for the workspace index, and translation directories refresh when a provider changes them.
- **Escaped quotes in keys.** Config and translation keys declared or used with an escaped quote (`'it\'s'`) are known by the name PHP reads.
- **Package resource paths.** `dirname(__DIR__)` paths and `lang_path()` are followed in service provider loaders, and `spatie/laravel-package-tools` providers' translations, views, and config are known.
- **`.env` files.** `.env.<environment>` files count as declarations, and a variable declared twice hovers with its last value.
- **Deleted files stop contributing.** A deleted service provider's macros, gates, commands, morph aliases, storage drivers, bindings, and paths are removed, and so are runtime config keys from deleted files.
- **Completing a string key after a non-ASCII named argument no longer fails.**
- **PHPStan runs for projects using `calebdw/phpstan-laravel`.** Either Laravel extension (or a fork) now enables PHPStan diagnostics. Contributed by @calebdw.

#### Blade templates

- **Blade parsing follows the compiler.** An `@` inside a word is not a directive, `)` or `]` inside PHP comments no longer closes directive arguments, a directive ends a multi-line echo that contains it, quotes in `@verbatim` no longer leak into PHP, and a commented-out `@use` is not an import.
- **Escaped frontend interpolations are left alone.** `@{{ name }}` and `@{!! name !!}` are not parsed as PHP, including across lines. Contributed by @shuvroroy.
- **Highlighting and hover agree with the compiler.** `@@if`, `@{{ … }}`, and `@verbatim`/`@php` content are no longer coloured as Blade, and echo-delimiter lookalikes no longer trigger hover or go-to-definition.
- **Unbalanced-directive and slot checks.** Two-argument `@push`, `@prepend`, and `@slot` are not unclosed blocks, `<x-slot:title>` is closed by `</x-slot>`, and `<x-slot name="footer">` shows as `x-slot:footer` in the outline.
- **Component attributes are typed from the right expression** even when `@class`, `@style`, or `@json` appear earlier.
- **Components in every PSR-4 directory are found,** and `$loop->parent` offers the outer loop's members.
- **`@lang` and `@choice` are translation keys.**
- **Class completion offers classes that need importing,** inserting `@use('…')`.
- **Template types stay current.** Variable types no longer go stale after saving a controller, Find References lands on the right line, and Laravel macros called from templates are found.
- **Internal Blade declarations stay hidden** from workspace symbols and completion.
- **Deleted templates are released from memory,** and section and stack pairing no longer rescans every template for each layout.

#### Editor features

- **Go-to-definition on a subclass name no longer jumps to its parent.** At its own declaration it answers with itself, so "Declaration or Usages" in PhpStorm and Zed can show usages. The `extends` clause still navigates to the parent.
- **Go-to-definition on a member declaration no longer jumps to the prototype.** Methods, constants, and enum cases answer with their own location, and Go to Implementation and the `implements`/`extends` clauses still reach the prototype. Contributed by @EranNL.
- **Methods implementing an interface keep their reference-count lens** alongside the prototype lens.
- **Go to Implementation finds trait methods.** Methods a class takes from a trait are listed, as are classes using a trait's abstract method. Contributed by @sidux.
- **Find References is more reliable.** It finds implementations inside dependencies, no longer mixes up cached results between files, and shows progress for constants.
- **The reference-count lens stays accurate** when files are closed or changed on disk, and new files are counted.
- **Hover works on an unfinished `$user->` or `Model::`.**
- **Array key completion works for type aliases** named on `@var` or `@param`, including chained aliases.
- **Code actions.** Import-class fixes are available with the cursor on the first character, "Extract variable/constant (all occurrences)" no longer produces overlapping edits or fails on multibyte names, and PHPDoc quick fixes work on files with Windows line endings.
- **Columns with emoji.** Go-to-definition on an `@method` tag and override completion land on the right column on lines with characters outside the Basic Multilingual Plane.
- **The document outline no longer goes blank** when a multibyte character in a comment precedes a symbol's name.

#### Rename and move

- **Variable renames respect scope.** Closures that do not capture the variable and functions in the same file are left alone, `global $name` links are followed, and invalid names (`1bad`, `$this`) are refused.
- **Renames and moves reach Blade templates,** including `@php` blocks, `@var` docblocks, directive arguments, and `@use`. Find References includes templates without opening them.
- **Imports are rewritten correctly.** Wrapped `use` statements, group imports (`use App\Models\{User, Post};`), and comma lists are updated, rewritten imports keep their indentation, and moving a class out of a group's prefix gives it its own `use`.
- **References keep their meaning after a namespace move.** Fully qualified and relative names are rewritten so they still resolve to the moved class, including `::class` strings in Laravel config.
- **Moves keep code working.** Files that reached the class by short name gain an import (aliased if needed), and the moved class gains imports for its former namespace siblings, functions, and constants.
- **Moving to and from the global namespace.** The `namespace` line is removed when moving to global, and a class leaving the global namespace is no longer reported as left behind.
- **Namespace renames put files in the right place.** Files follow the destination's PSR-4 mapping, merging into an existing namespace moves files one by one, `App\Internal` no longer claims `App\InternalTools`, and a namespace served by two PSR-4 roots is refused with both named.
- **Moves onto taken names are refused** with a reason instead of being half-applied.
- **Moving a class no longer writes the namespace above `declare(strict_types=1)`.**
- **Directory walks no longer loop on symlinks** when moving onto an existing directory or scanning PSR-0 packages.

#### Formatting and command line

- **phpcbf works on projects without a `phpcs.xml`.**
- **phpcbf exit codes 2 and 4 no longer fail formatting.** These report remaining or conflicting issues, not errors.
- **Pint uses the project's `pint.json`.** External formatters now run from the workspace root.
- **Pint no longer stalls on large files.**
- **An external formatter that prints nothing no longer empties the file.**
- **Whole-document formatting edits measure the last line correctly** when it contains multibyte text.
- **`phpantom_lsp fix` no longer silently skips files.** Blade templates crashed workers, which looked like a clean result. `fix` now also shares `analyze`'s stack size, parsing, and Laravel discovery.
- **`analyze` output is stable between runs,** sorting same-line diagnostics by column and code.

#### Performance and stability

- **Deeply chained and guarded code no longer stalls analysis.** Methods with many chained calls in branches, or property guards mixed with chained calls, used to take exponentially longer (some never finished). Long methods are no longer re-read for every question.
- **The editor stays responsive during slow requests.** PHPDoc generation, symbol pickers, selection ranges, quick-fix resolution, inlay hints, type hierarchy, and Laravel startup indexing no longer block other messages.
- **A slow editor response no longer crashes the server.** Diagnostic refresh, progress, and navigation requests now wait for late answers.
- **A running Laravel app no longer slows the editor.** Changes under `storage/framework/views` and `bootstrap/cache` are ignored, and file changes that add or remove no class keep the type caches.
- **Find References is faster on widely used names.** Positions are computed without rescanning files, and member searches share resolved receivers with the reference-count lens.
- **Reference lenses reuse the completed workspace index** instead of walking the filesystem for every declaration. Contributed by @sidux.
- **The reference-count worker no longer reruns workspace searches while you type.**
- **Renaming a local variable no longer re-indexes the project.** Contributed by @calebdw.
- **Requests parse the document once.** Hover, completion, go-to-definition, signature help, and inlay hints share one parse, and code lenses and inlay hints no longer slow down further down a file.
- **Closing a file and workspace symbol search no longer block editing.**
- **Laravel projects read the installed-package list once** per indexing pass and per edit.
- **Symlinked dependencies are treated as dependencies.** Completion ranking and removed functions after `composer update` are correct with symlinked paths. Contributed by @sidux.
- **`analyze` and the indexer skip `vendor/` however it is reached,** including through symlinks or aliased paths.
- **New `[indexing] extensions` are watched without a restart.**
- **`analyze` no longer intermittently misses members** due to a race between workers.
- **Crashes no longer degrade later analysis,** and Find References on `new self()` no longer risks a crash on a shrunken file.

## [0.10.0] - 2026-08-20

### Added

#### Whole-project analysis

- **Full workspace indexing.** PHPantom now parses every PHP file in the project in the background after startup, so Find References, Rename, Go to Implementation, and Type Hierarchy cover the whole project instead of only the files you have opened. Lighter modes remain available for projects that prefer a smaller footprint. Contributed by @sidux in https://github.com/PHPantom-dev/phpantom_lsp/pull/186.
- **Workspace-wide diagnostics.** Set `workspace = true` under `[diagnostics]` in `.phpantom.toml` to see problems in every file, not just open ones. Results stream into the problems panel once startup and indexing finish, and configured external tools (PHPStan, PHPCS, Mago) run once over the whole project afterwards. Off by default.

#### Blade templates

- **Template variables come from a clear priority chain.** A template's variables are resolved from, in order: a `@bladestan-signature` docblock, `@props` and `@aware`, the component class (or Livewire's `$this`), `View::share()` and `View::composer()` registrations, the layout it extends, and finally the data passed at its render sites. Completion, hover, go-to-definition, and undefined-variable diagnostics all use the same set, alongside the variables Blade injects itself.
- **Every way of rendering a template is recognised.** `view()`, `View::make()`, `Route::view()`, `Response::view()`, the view factory's render helpers, mailable views, and Blade's `@include` family, `@extends`, and `@each` all navigate, hover, complete, and pass their data to the template. This works however the view factory or mailable is obtained, and data passed as a typed array is read from its type. Contributed by @shuvroroy (#337).
- **Render calls are checked against the template's contract.** When a template declares what it needs, a missing variable, a variable of the wrong type, or a key the template never reads is reported at the render call, matching what Bladestan reports in CI. Templates that declare nothing are not checked, so this is opt-in.
- **Blade components are first-class.** `<x-alert>`, `<x-forms.date-picker>`, and `<livewire:counter>` navigate to their class or template, complete after `<x-` or `<livewire:`, and complete attributes from the constructor, `mount()`, or `@props`. Wrong attribute types and missing required attributes are reported on the tag, and `$component` works inside the tag body.
- **Sections and stacks are linked.** Ctrl+Click, completion, and hover connect `@yield` with `@section` and `@stack` with `@push` across files. A `@section` or `@push` that no layout in its chain ever renders is reported.
- **Directive completion and block checks.** Typing `@` completes every known directive and inserts the matching `@end…` for block directives. A block closed by the wrong directive, or never closed, is reported in the template itself instead of as a parse error in a compiled cache file.

#### Laravel

- **Route, config, view, and translation keys are real symbols.** Strings passed to `route()`, `config()`, `view()`, `__()`, and related helpers complete, hover, navigate to their definition, and report typos such as `route('dashbaord')`. Keys registered by installed packages and service providers are included, as are container attributes and facade methods that take a config sub-key (`DB::connection()`, `Cache::store()`, `Log::channel()`). Contributed by @calebdw.
- **More places that name a Laravel key are recognised.** Route names in signed URLs, the `Redirect`, `URL`, and `Response` facades and helpers, `#[RedirectToRoute]`, and `Route::is()` / `routeIs()` patterns all get the same support. Notification mail views, `Lang::hasForLocale()`, and `Config::getMany()` are covered too.
- **Environment variables.** `env('APP_NAME')` and `Env::get()` complete from `.env` and `.env.example`, hover to show the value and file, and appear in Find References. Values of names that look like secrets (`STRIPE_SECRET`, `APP_KEY`) are not shown. Unknown names are never reported, since the real runtime environment is not on disk.
- **Route parameters complete from the route's URI.** The keys of `route('users.show', ['user' => $user])` complete from the route's `{parameters}`, including group prefixes and the URIs `Route::resource()` and `apiResource()` generate. Contributed by @shuvroroy (#301, #308).
- **Artisan commands.** Command names complete, navigate, hover with their arguments and options, and are checked, including aliases. Inside a command, `$this->argument()` and `$this->option()` complete and are typed from the command's own `$signature`, and `Artisan::call()` parameters complete the target command's keys. Contributed by @shuvroroy (#274) and @krist7599555.
- **Config values are typed from your `config/` files.** `config('database.default')`, `Config::get()`, and repository `get()` calls resolve to the type the config file actually holds, including nested array shapes and `env()` fallbacks. Framework defaults fill in keys a published config leaves out. Contributed by @calebdw.
- **Path helpers link to files.** The argument of `base_path()`, `app_path()`, `config_path()`, `resource_path()`, and the other path helpers is a clickable link and completes one directory segment at a time. Contributed by @shuvroroy (#334).
- **Authorization abilities and policies.** Abilities in `Gate::allows()`, `$user->can()`, `$this->authorize()`, `can:` middleware, and Blade's `@can` family complete, hover, navigate, and are checked against `Gate::define()` registrations and model policies. An ability that belongs to a different model's policy is reported as such. Contributed by @shuvroroy (#330).
- **Container bindings and facades resolve to the bound class.** String keys bound in a service provider make `app('sentry')` and `resolve('sentry')` resolve to the bound class, with hover, navigation, and Find References on the key. Facades whose accessor names such a key, including hand-written facades with no `@method` docblock, now get their members from the class behind it. Contributed by @shuvroroy (#335).
- **Eloquent models know their database columns.** Columns from schema dumps in `database/schema` and from migrations become typed model properties with nullability and defaults. Custom connections, table names, and `Blueprint::macro()` helpers are respected, and editing one migration does not re-read the rest. Configure with `[laravel.migrations]` in `.phpantom.toml`. Contributed by @calebdw.
- **Model factories build the right thing.** `has{Relationship}()`, `for{Relationship}()`, and `trashed()` complete and chain on factories, and `count(3)`, `times(3)`, or `factory(3)` make `create()` and `make()` return the model's collection instead of a single model. Contributed by @shuvroroy (#260, #315).
- **Request input and `validated()` are typed from validation rules.** Field names complete wherever request input is read, with the rule shown and a link to where it is declared. `validated()` returns an array shape typed from the rules (nullable, optional, nested `items.*`, files, and enums included). Contributed by @shuvroroy (#292, #294, #307).
- **Higher-order collection proxies.** `$users->map->email` is typed like `$users->map(fn ($u) => $u->email)`, with completion and hover through the proxy and chaining afterwards. Contributed by @shuvroroy (#314).
- **`$pivot` on many-to-many related models.** Models reached through `belongsToMany` or `morphToMany` expose a typed `$pivot`, using the relationship's pivot model or `->using()` class, and hover lists the `->withPivot()` columns. Contributed by @shuvroroy (#266).
- **Morph map aliases.** Aliases registered with `Relation::morphMap()` or `enforceMorphMap()` hover to their model, navigate to both the registration and the model, and appear in Find References. Unknown aliases are only reported when the map is enforced.
- **`Storage::disk()` resolves to the real adapter.** Disks are typed from `config/filesystems.php` (and `Storage::extend()` for custom drivers), so adapter-only methods like `assertExists()` and `download()` are no longer reported missing.
- **Macros registered with `mixin()`.** `Str::mixin()`, `Collection::mixin()`, and Carbon's trait-based `mixin()` add their methods to the target class, with completion, hover, and type checking. Contributed by @shuvroroy (#256) and @calebdw.
- **Larastan's `model-property<Model>` is checked and completed.** String literals that name no property on the model are flagged, and the model's property names complete inside such arguments. Contributed by @calebdw.

#### Diagnostics

- **Access to `private` and `protected` members is reported.** Reading or calling a member you cannot reach from the current scope is now flagged, checked against the class that declares it. Classes with magic methods, trait members, and `@see` references are left alone. Contributed by @petrovo-as.
- **Illegal `readonly` writes and contradicting docblocks.** Writes to a `readonly` property from places PHP forbids (including `unset()`, `foreach` targets, and references), and `@param` or `@return` tags whose nullability contradicts the declaration, are now reported.
- **Four new declaration diagnostics.** Enum cases that do not match the enum's backing type, overrides that drop an inherited `static` return type, abstract trait methods nothing implements (with "Implement missing methods" stubbing them), and `match` arms that can never match. Contributed by @calebdw.

#### Type inference

- **`preg_match()` gives `$matches` the right keys.** With a literal pattern, `$matches` becomes an array shape with the whole match, each group, and named groups, honouring `PREG_OFFSET_CAPTURE`, `PREG_UNMATCHED_AS_NULL`, and `preg_match_all()` modes. The shape also depends on whether the match succeeded in the current branch.
- **By-reference closure captures update the outer variable.** A closure capturing `use (&$var)` that is passed to an immediately invoked callable can update the outer variable's type, following PHPStan's defaults. Contributed by @calebdw.
- **`@psalm-this-out` and `@phpstan-self-out`.** A method call can now retype the object it was called on, as mutable generic containers describe.
- **`@phpstan-require-implements` in traits.** `$this` inside the trait sees the required interface's members, matching `@phpstan-require-extends`. Contributed by @calebdw.

#### Editing and navigation

- **Override completion.** Completion in a class body, or after `function`, `$`, or `const`, offers every parent, interface, and trait member the class can still override or implement, inserting the full declaration with `#[\Override]` where supported. Contributed by @calebdw.
- **Reference and implementation counts.** Classes, members, and functions show how many times they are used, as an inlay hint and above the declaration, with implementation counts on interfaces and abstract classes. Following a count lists exactly what it counted. Contributed by @calebdw and @petrovo-as.
- **PHPUnit coverage metadata navigates both ways.** Ctrl+Click a target in `#[CoversClass]`, `#[CoversMethod]`, `#[CoversFunction]`, their `Uses` variants, or `@covers` / `@uses` / `@coversDefaultClass` to open it, and rename keeps them in step. Covered classes show a lens naming the tests that cover them.
- **Two new code actions.** "Sort use statements" sorts imports like PhpStorm's Optimize Imports. "Convert to string interpolation" turns `'Hello ' . $name . '!'` into `"Hello {$name}!"` where that is safe.
- **Semantic token modes.** `[semantic_tokens] mode = "contextual" | "full" | "off"` in `.phpantom.toml`. The default `contextual` mode only adds highlighting the editor's grammar cannot provide, `full` keeps the previous behaviour, and `off` disables it. Contributed by @calebdw.
- **`@phpstan-ignore` identifiers are highlighted and completed.** Identifiers are highlighted in docblocks and `//` comments, and complete from the diagnostic codes already seen in the file. Contributed by @calebdw.

#### Tooling and platform

- **Global settings.** `phpantom_lsp init --global` creates a config in your platform's config directory (`~/.config/phpantom_lsp/.phpantom.toml` on Linux) that every project inherits. A project's own `.phpantom.toml` is merged over it key by key, and errors in the global file are reported against that file.
- **Config changes apply without a restart.** Editing the global config or a project's `.phpantom.toml` reloads settings within a couple of seconds. Previously only Laravel projects reloaded, and the global config never did.
- **Analyze verbosity flags.** `phpantom_lsp analyze` supports `--debug` (prints each file as it is analysed, so hangs are easy to pin down) and `-v`/`-vv`/`-vvv` for timings, worker details, and memory usage.
- **PHPantom runs in the browser.** The type engine compiles to WebAssembly, giving web editors completion, hover, go-to-definition, highlighting, and rename without a server. Every release ships a prebuilt module. See [wasm.md](wasm.md) for the host interface. Contributed by @ondrejmirtes.

### Changed

#### Behaviour

- **One parser for all PHPDoc.** `@psalm-` and `@phpstan-` prefixed tags are treated as the same tag as their plain form everywhere, with the prefixed version taking precedence. Multi-line types, variance annotations, tags indented with extra spaces, and half-written docblocks are all handled more reliably, and prose after a type no longer leaks into it.
- **Property hover shows the effective type on a `var` line.** Like method hover, the resolved type is shown above the code, and the code itself shows only the native declaration. Contributed by @calebdw.
- **Live progress counts.** Indexing progress advances file by file with live counts (e.g. "Scanning vendor packages (3201/8544 files)"). Go to Implementation, Find References, and Type Hierarchy show the same live progress.
- **One slow file no longer stops workspace diagnostics.** Previously a single file the analyser could not finish froze the progress bar and left the rest of the project undiagnosed. Now the other files keep going, a file still running after two seconds is named in the progress message, and one that takes more than ten is skipped and logged so it can be reported. Opening the file still diagnoses it normally.
- **External tool re-runs only refresh files that changed.** A project-wide PHPStan, PHPCS, or Mago run no longer re-sends diagnostics for every file, only for files whose results differ.
- **Updated the bundled mago toolchain to 1.46.0.** Contributed by @enwi in https://github.com/PHPantom-dev/phpantom_lsp/pull/234.
- **Updated embedded phpstorm-stubs.** Adds PHP 8.6 coverage and corrects several signatures, including Redis, FFI, enchant, xmlreader, `openssl_x509_parse()`, and `htmlspecialchars()`.

#### Performance and memory

- **Saving no longer re-analyses every open tab.** Only open files that use something the save changed are re-checked, keeping completion and hover responsive after a save. Saving a Laravel config, translation, or route file still refreshes every open file.
- **Classes are pre-resolved after startup.** Once indexing finishes, every class is resolved across multiple workers, so the first completion, hover, or go-to-definition reads a warm cache. This also shortens the pause before diagnostics on large Laravel projects.
- **Lower memory use for types and class hierarchies.** Identical types are stored once and shared, and inherited members are shared with their source instead of copied. On large Laravel projects this roughly halves the memory held by resolved classes and also speeds up analysis.
- **Lower memory use in the reference index and member lookups.** The data behind Find References, method lookups, and member access positions is stored more compactly, removing many short-lived allocations.
- **Faster startup.** File discovery and indexing use all CPU cores and no longer get held up by one very large dependency. On a large Laravel project indexing is about a quarter faster and peak memory is about 25 MB lower. When two files declare the same class, the same one now wins on every run.
- **Faster class-name lookups.** Lookups are cached, making whole-project analysis 8-12% faster on large Laravel projects, with the same gain for hover, completion, and go-to-definition.
- **Faster vendor scanning.** Vendor files are read once instead of twice, and package origin for completion ranking is worked out during the same parallel scan.
- **Argument checks no longer slow down on large files.** On a 370 KB file with 2200 calls, the argument checks went from 13.7 seconds to 0.2, and the whole file from 16.7 seconds to 3.4.
- **Faster diagnostics on long method chains.** Files built from long fluent chains report diagnostics around a quarter faster, and the deprecated-usage check roughly halves.
- **Faster diagnostics in general.** Several checks reuse already-resolved class data instead of recomputing it (and now also see interface-declared members), and calls that resolve to no class are no longer resolved twice, roughly halving diagnostics time on files with many unresolved chains.
- **Faster narrowing, docblock, and Eloquent scope handling.** `assert()` and type-guard checks skip statements that cannot be assertions, `@method` and `@property` tags are parsed once per class, and Eloquent scopes are resolved once per model.
- **Faster workspace symbol search.** Matching no longer allocates a copy of every symbol name on each keystroke.
- **`analyze` and `fix` skip the reference index.** The CLI commands do not need it, so whole-project runs do less work.
- **The first reference search of a session is fast.** Files are pre-walked in the background after indexing, so the first Find References or reference CodeLens is as fast as later ones. On a large Laravel app, reference counts for 250 files went from 1.4 seconds to 0.14. The background pass uses a quarter of the CPU cores and stops after 30 seconds on very large projects.

### Removed

- **Bundled Zed extension.** PHPantom's support has merged into Zed's official PHP extension. See [Editor Setup](editor-setup.md) for the updated Zed configuration.
- **Linked editing.** Editors could mirror ordinary edits into a linked range and corrupt the line being edited. Use rename (explicit, cross-file, previewable, one undo step) or your editor's multi-cursor instead.

### Fixed

#### Narrowing

- **`instanceof` narrows to exactly what it proves.** A successful check keeps a generic's type arguments (`Collection<User>` stays `Collection<User>`), replaces a wider type such as `object|null` instead of adding the class beside it, and drops the array half of a union like `UploadedFile|array<UploadedFile>|null`. Ruling a class out of a class-and-scalar union such as `Decimal|float` now leaves the scalar instead of dropping the whole union, and the `if (!$x instanceof Foo) { throw … }` guard narrows the same way as `assert()`.
- **An `instanceof` check on an `object|string` value narrows it to the class.** `if ($server instanceof Server)` on a route parameter typed `object|string` used to give `object|string|Server`, so passing it on was reported as a type error. The check's result is now the whole answer, including in guards like `if (! $server instanceof Server || ! canManage($server))`.
- **`instanceof` on a nested property keeps the declared class.** `$this->holder->service instanceof MockInterface` now gives both the declared class and the interface, as the one-level form already did.
- **A value proven to be two types at once is an intersection.** `$x instanceof Reader && $x instanceof Writer`, or `assertInstanceOf()` and `assert()` on a mock, now give `Reader&Writer` (or `MethodNode&MockObject`) instead of a union that satisfies neither. An existing intersection narrows within itself, and an `||` check still only proves one of its alternatives.
- **`is_a($x, Foo::class, true)` keeps the class-string case.** On an `object|string` value the check now narrows to `Foo|class-string<Foo>`, so a following `is_string()` check is no longer reported as always false.
- **`get_class($v) !== Foo::class` keeps subclasses.** A subclass's `get_class()` names the subclass, so it now survives the check instead of being ruled out with `Foo`.
- **`match ($value::class)` narrows its subject in each arm.** Each arm narrows the subject to the classes it names, so handlers typed for those classes no longer report a mismatch.
- **Negated `null` checks are read correctly.** `!($v != null)` no longer narrows to `null` (loose equality also matches `''`, `0`, and `[]`), and `!($v === null)` now proves the value is not null.
- **`filled()` and `blank()` narrow their argument.** `if (filled($search))` now removes `null` from a `?string`. Assertions written as unions (`!=null|''`) work, and equality-form assertion tags no longer narrow the opposite branch to a wrong type.
- **Laravel's bail-out helpers narrow the code after them.** After `abort_if($user === null, 404)`, `abort_unless()`, `throw_if()`, or `throw_unless()`, the value is narrowed with every guard form an `if` understands. More generally, any function whose conditional return type has a `never` branch now narrows the argument that would select it, for variables, properties, and array elements.
- **Calling a `never` function or method ends the branch.** Guards like `if (!$x instanceof Foo) { abort(); }` narrow what follows when `abort()` returns `never`, for functions, static calls, and methods (declared, inherited, or from a trait), whatever the call is written on (`app()->abort(422)`, `$this->responder->abort(422)`, …). Assignments in such a branch are treated as dead code.
- **Guards written with `if: … endif;` narrow.** The colon syntax now exits the same way the brace form does.
- **`&&` and `||` guards narrow the next operand wherever they are written.** Assignments, arguments, array entries, ternary conditions, `match` arms, and `do`/`while` and `for` conditions now narrow like an `if` condition does, so `$ok = is_string($v) && strlen($v) > 0` no longer reports anything.
- **A negated compound guard narrows by every part.** After `if (!is_string($payload) || $payload === '') { return; }` each operand's inverse applies, for any exit (`return`, `throw`, `continue`, `abort()`) and in the `else` branch. `is_resource()` is now a type guard, and `!== ''` / `!== []` give `non-empty-string` / `non-empty-array`.
- **Checks against `false` narrow both ways.** `if ($handle !== false)` rules `false` out inside the branch, `if ($value === false) { … } else { … }` and `!empty($value)` rule it out in the `else`, and `if ($x === false) { throw …; }` narrows what follows. A `T|false|null` value keeps its `null`.
- **A truthy check rules out every always-falsy value.** `if ($value)` and `!empty($value)` now remove `[]`, `''`, `'0'`, `0`, and `0.0` as well as `null` and `false`, so `$markets = $this->option('markets') ?? []` followed by `if ($markets)` leaves a string. `'0'` is also treated as falsy in `@phpstan-assert` checks.
- **A doubly negated guard narrows like the bare one.** `if (!(!$user))` now narrows, so Blade's `@unless (!$user)` does too.
- **A check written beside an assignment narrows the variable.** `while (($line = fgets($handle)) !== false)`, `while ($parent = $parent->getParent())`, `if (!$row = $query->first()) { throw … }`, and `} elseif ($token = $request->bearerToken()) {` all narrow the assigned variable in the body.
- **Read loops keep the narrowing their condition established.** In `while ($line !== false) { useString($line); $line = readLine(); }` the read at the top of the body is no longer judged against the reassigned type. `for` conditions now narrow the body and the code after the loop the same way `while` conditions do.
- **`for` loop details are tracked.** An `instanceof` in a `for` condition narrows the rest of the condition, and the update clause (`$node = $node->next`) now feeds its type into the next iteration and the code after the loop.
- **A loop over a non-empty array runs at least once.** After `if (!$qtys) { return 0; }`, a `foreach` over `$qtys` replaces the `$max = null;` sentinel, and variables first assigned inside the loop are defined after it. Arrays proven non-empty by guards, `non-empty-array` annotations, required shape keys, or literals all count.
- **Null-initialized variables reassigned in an untyped `foreach` are not stuck as `null`.** When the iterable has no known element type, the loop value is `mixed`, so later merges and `is_null` early returns work. Contributed by @calebdw.
- **Writes to a `foreach` value variable stay in their iteration.** The value and key variables are reset to the element type on every pass, so a write at the end of the body no longer leaks into the next iteration's guards.
- **`break` carries its state out of the loop.** Variables assigned before a `break` (including in `switch` arms and inner loops) now contribute to the type after the loop.
- **An assignment inside `try` survives a `catch` that rethrows.** A `catch` that throws or returns no longer puts back the type the variable had before the `try`.
- **Branch merges use what each branch ends with.** After an `if`, a variable's type is the union of what its branches end with. A guard that repairs a value (`if (!$value) { $value = 'fallback'; }`, `if (!is_array($s)) { $s = [$s]; }`) keeps the repaired type, a branch that only narrowed no longer leaks its narrower type, and a property assigned inside a guarded `if` keeps that type after the block.
- **Ternaries and `match` arms see what their condition proved.** Each arm of a ternary is resolved under its own side of the condition, wherever the ternary appears (assignments, arguments, returns, `throw`, `new Foo(…)`), for every kind of check, including the explicit `$x ? $x : ''` form and repeated properties like `$article->alt ? $article->alt : $article->title`. `match (true)` arms narrow their results the same way, and later arms know the earlier ones failed.
- **A guard inside an `echo` narrows.** Ternaries, `&&` chains, and `match (true)` in an echoed expression now narrow, which fixes Blade `{{ … }}` echoes as well as plain PHP.
- **Checks on a function or static call carry to the same call written again.** `if (currentUser()) { render(currentUser()); }`, `Session::current()`, and guards like `if (mb_strpos($slug, $m) !== false) { … mb_strpos($slug, $m) … }` now narrow the repeated call. Arguments are part of the match, and the proof ends when anything the call reads is written, at the end of the branch, or on the next loop iteration. Calls that return something different each time (`fgets()`, `array_shift()`, `time()`, `rand()`) are never remembered.
- **A check on a method call narrows the same call.** `if ($this->getHttpKernel() instanceof TerminableInterface) { $this->getHttpKernel()->terminate(...); }` and scalar checks like `$this->value() !== false` now narrow the repeated argument-less call, through `instanceof`, `assert()`, `is_a()`, and truthy or null tests, including where the call is passed as an argument. Completion, hover, and go-to-definition see it too.
- **Remembered checks expire when their state may have changed.** A call on a receiver drops what was proven about other calls, properties, and elements read through it (unless the callee is `@pure`), and assigning a new object to `$a` ends earlier checks on `$a->value`. Properties checked before a method call on the same object keep their narrowing.
- **`?->` chains narrow their receivers.** Inside `if ($image?->file_id !== null)`, after `$period = $agreement?->latestPeriod(); if (!$period instanceof Period) { return; }`, and when a `?->` chain is compared with a non-null value, every receiver along the chain is known to be non-null.
- **Static properties remember writes and checks.** The lazy-initialisation idiom `if (self::$repo === null) { self::$repo = new Repo(); } return self::$repo;` now returns `Repo`, not `?Repo`.
- **Guard clauses on properties narrow them.** `if ($this->handle === false) { return; }`, `!$this->handle`, and `empty($this->handle)` now narrow the property for the rest of the method, at any depth. A check on an indexed entry like `$category->translations[0]` keeps the collection's element type instead of turning it into `mixed`.
- **Checks on a property can narrow a union of objects.** Testing a property that tells classes apart (`is_string($b->v)`, `$r->tag === 'ok'`) now narrows the object itself to the matching classes.
- **Checks on array elements narrow those elements.** `isset($m[0])`, `$m[0] !== null`, `!empty($row['name'])`, and `isset($state['files'][$path]['violations'])` with a variable index all narrow the element they name, without claiming anything about other keys.
- **`assert()` proves whatever the same condition proves in an `if`.** `assert($handle !== false)`, `assert(is_string($v))`, `&&` chains, checks on properties and array elements (`assert($items[0] instanceof Foo)`), and the fully-qualified `\assert()` all narrow now.
- **`assert($this instanceof …)` narrows `$this` in Pest closures.** A `@param-closure-this` binding is now a starting point that assertions can refine, so members of your real test base class resolve.
- **More assertion tags narrow.** `@phpstan-assert-if-true` tags about the receiver's own members (as on PHPStan's `Scope::isInTrait()`), `!null` assertions on parameters, and pseudo-types like `resource` and `null` (`assertIsResource()`, `assertNull()`) are now applied.
- **`iterable` is a type guard.** `is_iterable($x)` and `assertIsIterable()` keep arrays and `Traversable` objects and remove everything else.
- **A strict `in_array()` narrows the needle.** `if (!in_array($email, self::APPROVED, true)) { abort(403); }` now proves the value is one of the listed values, for lists, inline arrays, and constants. Constant lists of literals also keep their values.
- **Fully-qualified type guards narrow.** `\is_array($x)`, `\is_a()`, `\class_exists()`, `\property_exists()`, and `\method_exists()` now work like their unqualified spellings.
- **A type guard types a value that had no type.** `assert(is_string($version))` and `if (!is_string($v)) { return; }` now establish the type even when nothing else said what the value was.
- **A check stored in a variable still narrows.** `$isHtml = $raw instanceof HtmlString;` followed by `if ($isHtml)`, a ternary, or a guard clause now narrows `$raw`, until either variable is written.
- **Guards reach reads derived from the narrowed value.** Array reads (`$violationMessage['args']`), array functions (`array_slice($cached, …)`), and reads after assertions like `assertNotNull()` now use the guarded type instead of the declaration.
- **Comparing with an enum case rules out `null`.** `$land === Land::Be && $this->takes($land)` no longer reports `?Land`. Other class constants work the same way, based on the constant's own type.
- **Inline `@var` annotations no longer block narrowing.** An annotated assignment no longer skips narrowing for the rest of its statement, and the annotated type now flows normally, so later guards narrow it and reassignments replace it.

#### Type inference

- **Reflection-style accessors are typed at each call site.** A method with an untyped parameter that returns whatever its arguments name (`Sudo::fetchProperty($config, 'shell')`) now resolves to the named property's type, with completion and references.
- **Property reads through reflection are typed.** `(new ReflectionObject($config))->getProperty('shell')->getValue($config)` now resolves to the property's declared type when the name is a literal, and `new ReflectionObject($x)` keeps the class it reflects.
- **A class's own `offsetGet()` beats the `ArrayAccess` docblock.** `$pens[0]` on a class with `offsetGet(): Pen` now resolves to `Pen` instead of an unknown `TValue`.
- **Magic constants have types.** `__LINE__` is an `int` and `__FILE__`, `__DIR__`, `__CLASS__`, and the rest are strings, so `__LINE__ + 3` stays an `int`. `__CLASS__` keeps the class it names.
- **Constants keep their values.** Class constants with a scalar initializer resolve to that value (`'foo'`, `1`, `3.14`) wherever they are used, typed class constants (`private const int FLAGS = …`) keep their initializer, and constants built from other constants are folded through bitwise operators, so `json_encode($v, self::FLAGS)` knows about `JSON_THROW_ON_ERROR`. Flag masks stored in variables keep their value too.
- **More constant references resolve.** `parent::SOME_CONSTANT` and members read off a constant (`self::TYPED->value` on an enum case) now resolve, and a variable holding a global constant (`$mode = PHP_ROUND_HALF_UP;`) is typed during diagnostics as it already was on hover.
- **Conditional return types pick the right branch.** Conditions are decided by the argument's resolved type and class hierarchy (so `Order::find(7)` is `Order|null` and a list of ids gives a collection), by named constants and literal values, and by a parameter's default when the argument is left out (as in `str_word_count()`). A default like `self::EXCEPTION_ON_INVALID_REFERENCE` resolves against the declaring class. Conditions that cannot be decided still report every branch.
- **Conditional return types recognize interpolated strings as strings.** `config("{$prefix}.host")` now takes the `string` branch. Contributed by @calebdw.
- **A standalone `@var` cast above `return` is honoured.** `/** @var int */` written above `return giveString();` now casts the returned value, as PHPStan does.
- **`@return mixed` no longer hides a method's real return type.** Such a method now gets its return type from its body, as an undeclared method does.
- **An untyped property takes its type from its assignments.** `private $context;` assigned from a typed parameter or `new` in the constructor or setters now has that type.
- **Overrides inherit what they should.** An override without its own docblock uses the parent's `@param` types inside its body, its native return type limits the `@return` union it inherits, and a `parent` or `self` parameter type on an inherited method refers to the classes the declaration meant.
- **`@method` and `@property` tags follow PHP's rules.** A `@method` tag no longer replaces a method that really exists, a `@property` tag wins over an inherited property that is not accessible (such as Eloquent's protected `$connection`), and tags on implemented interfaces are always applied.
- **`@method` and `@property` tags on a parent's trait are inherited.** Subclasses now see them, with the trait's template parameters resolved. Contributed by @shuvroroy (#314).
- **A `@method` tag's own template parameter is understood.** `@method TVal get<TVal of mixed>(TVal $default)` no longer reports `TVal` as an unknown class.
- **Docblock reading is more robust.** A tag written on the `/**` line itself is now read, a one-line function no longer picks up the previous function's `@param`, a `/* … */` comment between two annotated assignments no longer brings back the earlier annotation, and `/** @var \Closure(User $user): string $callback */` annotates `$callback` rather than `$user`.
- **A standalone `@var` block keeps its variable in scope.** A `/** @var Model $model */` that does not sit directly above an assignment (as at the top of a Blade template) now survives comments and blocks written after it.
- **A docblock can refine one member of a native union.** `/** @return false|string */` on a `bool|string` return now gives `false|string`.
- **Literal types are kept precisely.** Exact string, integer, and float values survive parentheses, signs, `match`, ternaries, and `??`, and are widened only where PHP requires. Contributed by @snowyukitty.
- **A ternary with a constant condition only uses the live branch.** `true ? 1 : 2` is `1`, not `1|2`.
- **`true` and `false` keep their own type.** `$time = false; … if ($time) { … }` now narrows to the non-false value, and `$found = false; … if ($found)` reports `true` inside the branch. Generated return types still use `bool`.
- **`?:` drops the value it replaces.** `$response->getContent() ?: ''` is now a `string`, not `string|false`.
- **Casts are typed everywhere.** `(int) $a` in a ternary branch, `(string) $value` as an argument, and `!` and `~` now resolve to their result type.
- **Integer arithmetic stays integer.** Refined ints like `int<0,max>` (from `strlen()` or `count()`) add up to `int`, and `-$count` on an `int` stays an `int`.
- **Arrow functions keep their parameters in their type.** `fn (BrandView $view) => …` is now `Closure(BrandView): …`, so it satisfies a matching `Closure(…)` parameter.
- **Union types are cleaner.** Repeated alternatives are dropped, a class and its generic form are not listed twice, `true|false` becomes `bool`, and `(A|B)&C` keeps its parentheses in hover and messages. Two classes with the same short name in different namespaces are no longer collapsed into one.
- **An untypeable branch widens the result instead of vanishing.** `array_key_exists('k', $s) ? $s['k'] : 'exception'` on `array|string|null` no longer resolves to just `'exception'`, and reading an offset from a string gives `string`.
- **Writes through `__set` no longer change what `__get` returns.** `$bag->a = 9` on a class with a magic setter no longer makes the next read `9`.
- **Assignments through a by-reference closure capture are kept.** A closure with `use (&$var)` now widens the outer variable even when PHPantom cannot prove when it runs, as PHPStan does.
- **`$this` in nested `@param-closure-this` closures resolves to the innermost binding.** Nested route groups and macros resolve `$this`, `self::`, and `static::` correctly, including when the outer closure is stored in a variable or returned. The tag's class is now looked up in the file that declares it, not the calling file.
- **Static calls resolve as accurately as instance calls.** `Foo::bar()` now handles `@phpstan-type` aliases, inherited generic return types, and the `__callStatic()` fallback.
- **`$string::method()` is not reported as scalar access.** A static call on a `string` is valid PHP and is now left unchecked, and on a `class-string<T>` it resolves against `T` (`$job->class_name::dispatch()`).
- **Every feature resolves types as well as hover does.** Return types inferred from method bodies, Laravel auth guard models, and validation rule shapes were only used by hover, completion, and diagnostics. Go-to-definition, references, signature help, code actions, rename, and inlay hints now use them too.
- **PHP 8.4 property hook bodies are analysed.** Navigation, hover, and completion now work inside `get` and `set` hooks, including hooks on promoted constructor properties, with `$this` and `$value` typed.
- **Parent property hook calls are understood.** `parent::$label::get()` is no longer reported as two missing members, and an ordinary `Registry::$instance::get('service')` is no longer mistaken for a hook call.

#### Generics and templates

- **A missing generic argument uses the `@template` default.** `@template TAsync of bool = false` now applies when no argument is given, so `Http::get()` returns a `Response` (with `json()` and the rest) and `async()` still returns a `PromiseInterface`. Contributed by @shuvroroy (#377).
- **Changing a `@template` default takes effect immediately.** Editing `= false` to `= true` now updates dependent return types.
- **A generic class named without type arguments uses the template bounds.** `@var ItemCollection $items` followed by `$items->first()` now gives the declared bound (or `mixed`) instead of a nonexistent `TModel` class.
- **Short `@implements` argument lists bind the value parameter.** `@implements Bag<User>` now binds `User` to `TValue`, not `TKey`, when merging interface members.
- **Templates bind from every place they appear.** A template used by several parameters (`@param T[] $first, @param T[] $second`) is the union of all of them, a template in one alternative of a union `@param` (`Collection<TKey, TValue>|array<TKey, TValue>`, `iterable<…>|TWrapValue`) binds from the alternative the argument matches, and a property argument (`$this->items`) binds as well as a local does.
- **Templates keep the full type of an argument.** `passthrough(Carbon::create(2024))` now binds `?Carbon` rather than `Carbon`, the same as when the call is assigned to a variable first.
- **An argument is not checked against a template only it bound.** `assertSame(url('/login'), $x)` and `$this->travelTo(Carbon::create(2024))` no longer report circular mismatches.
- **Callback return types bind templates correctly.** Every template in a callback's declared return shape now binds (so `Collection::flatMap()` gives the right key and value types), and the body is used when the closure's annotation says less.
- **A callback whose body is a call binds the template it returns.** `->keyBy(fn (Review $r) => $r->getRating())` now binds `int` as the key.
- **Closure parameters typed `array` take the element type from the call.** `array_map(static fn (array $case) => $case[0]->name, …)` now types `$case` from the array, including when the array is an inline call like `iterator_to_array(self::cases())`.
- **Named arguments bind to the right parameters.** Reordered or skipped named arguments no longer break template bindings or closure parameter types.
- **Static factory templates survive chaining.** `Wrapper::make(names())->push([1])` now reports the same as the two-line form.
- **Constant tables work with `key-of`, `value-of`, and lookups.** `@param key-of<ID_TABLE>` and `@return value-of<ID_TABLE>` hold plain functions to the table's keys and values, and `@return ID_TABLE[T]` resolves each call to its own entry, including when the argument is omitted and its default names the key. `Class::TABLE` and `self::TABLE` work too.
- **`key-of` and `value-of` keep a literal's precision.** `firstValue(['low' => 1, 'high' => 10])` is `1|10`, and a key not in a literal passed to a `key-of<T>` function is now reported.
- **Conditionals inside generic return types are resolved.** `$collection->groupBy('key')` no longer leaks a raw `$groupBy is array|string ? …` type into later calls.
- **A generic class is no longer rejected by a parameter of the same class.** `new Decimal('0.00')` now satisfies a `Decimal $amount` parameter.

#### Arrays and built-in functions

- **`array_filter()` narrows what survives.** A callback that tests values (`fn ($v) => $v !== null`, `'is_int'`, `instanceof`) narrows the element type, one that tests keys narrows the key type, and both narrow together with `ARRAY_FILTER_USE_BOTH`. This also works for `array<T>` and `T[]`.
- **`array_filter()` without a callback removes falsy values.** An `array<string, string|null>` comes back without `null`. This also improves `if ($x)` and `$x ?:` on unions.
- **A filtered list is no longer a `list`.** `array_filter()` keeps keys, so its result is `array<int, T>`. `array_values()` still gives a list.
- **Array builtins answer in terms of the array they were given.** `array_keys()`, `array_values()`, `array_search()`, `array_key_first()`, `array_key_last()`, and `key()` use the array's real key and value types. `array_pop()`, `array_slice()`, `array_merge()`, `current()`, and the rest now keep scalar element types (a `list<string>` pops a `string`) and unwrap nested arrays by one level.
- **`array_key_first()`, `array_key_last()`, and `key()` drop `null` for non-empty arrays.**
- **`array_sum()` and `array_product()` over ints return `int`.**
- **`array_chunk()` returns chunks.** Each chunk is an array of the input's elements, not a single element.
- **`array_map()` handles string callbacks and keeps keys.** `array_map('intval', $ids)` uses `intval()`'s return type, and single-array calls keep the input's keys.
- **`max()` and `min()` return the type of the values compared.**
- **Fully-qualified builtins work like unqualified ones.** `\array_sum()`, `\array_pop()`, and the rest of the family now get the same precise return types.
- **Replace functions return the shape of their subject.** `preg_replace()`, `preg_replace_callback()`, `preg_filter()`, `str_replace()`, `str_ireplace()`, and `substr_replace()` return a string for a string subject and an array for an array subject, instead of `array|string`.
- **More builtins return what their arguments select.** `pathinfo($p, PATHINFO_FILENAME)`, `print_r($v, true)`, `hrtime(true)`, `microtime(true)`, `getenv('HOME')`, `mb_convert_encoding()` on a string, `abs()` on an `int`, and `SimpleXMLElement::asXML()` now return the single type they produce, including through constants and parameter defaults.
- **`json_encode()` with `JSON_THROW_ON_ERROR` is never `false`.** The flag is recognized on its own, combined with other flags, as a number, or held in a constant.
- **Builtins whose failure branch is rarely checked are not reported.** Functions like `tempnam()` that PHPStan exempts no longer trigger `string|false` errors. Hover still shows `string|false`, and functions where `false` is meaningful, like `strpos()`, are still checked.
- **Out-parameters are typed after the call.** A by-reference parameter that defaults to `null` (`preg_match()`'s `$matches`, `parse_str()`, `exec()`, and your own functions) is no longer nullable afterwards. A stored result (`$ok = preg_match($p, $s, $m); if ($ok)`) narrows the matches the same way testing the call directly does.
- **Builtin arguments are read in more forms.** `$error ?? ''`, `$data['message']`, global constants like `PHP_VERSION`, concatenation, and `?:` now decide a builtin's return type like a variable does.
- **Stubs that were too strict or too loose are fixed.** `ctype_digit()` and the `ctype_*` family accept `mixed`, `define()` accepts any value, `ReflectionClass::newInstanceArgs()` no longer returns `null`, and the closure passed to `spl_autoload_register()` gets a `string` parameter.
- **Array literals record exactly what they hold.** `[$violation, $file, $line]` keeps each position's type for destructuring and indexing, literal values like `[1, 1.5, '123']` keep their values (so they satisfy `numeric`), and mixed literals like `['first', 'b' => 1]` keep their positional entries. Each entry is still checked against a `list<int>` or similar parameter.
- **Array literals with runtime keys are typed by keys and values.** `[$name => 1]` is `array<K, int>` instead of `array{mixed: 1}`, `[Event::class => $handler]` satisfies `array<class-string, …>`, and `null`, `true`, and `false` keys map to the keys PHP uses.
- **`(object) []` is a `stdClass`.**
- **Element writes update the array's type.** `$row[] = $pen` and `$row[$key] = 1` add to a tracked shape, `$grouped[$id][] = $row` builds `array<int, list<Row>>`, and writing into an `array<string, int>` keeps its key and value types instead of turning it into a one-key shape.
- **Array keys keep their type.** Non-literal `string` keys stay `string` (only literal numeric strings become ints), computed indices like `$mapping[$line + 1]` and `$result[max($a - 1 - $b, 0)]` are `int`, and `++$i` works as a key.
- **Arrays built across several branches read as one array.** `$rows = []` followed by writes in several `if`s is now a single `array<int, A|B|C>`, and an empty `[]` no longer trails along beside what is written into it.
- **Accumulators that start as `[]` stay integers.** `$totals[$k] = ($totals[$k] ?? 0) + $n` now gives `int`, and arrays filled by a loop or by-reference closure no longer carry the empty-array alternative.
- **Appending keeps `false`.** `$files[] = realpath($path)` stores `string|false`, not `string|bool`, so `assertNotFalse()` on an element works.
- **`unset()` on an array element updates the array.** `unset($config['driver'])` removes a shape key and the non-empty guarantee, so a later loop may not run.
- **Reading an optional shape key may give `null`.** `type?: string` now reads as `string|null`.
- **Array union keeps both sides' keys.** `$config += ['slot' => $default]` and `$defaults + $overrides` now keep their shape instead of becoming `array`.
- **`list<T>` and `array<int, T>` are the same type.** Array types written with different numbers of type arguments are now compared properly, and shapes satisfy `list` and `non-empty-array` according to their actual keys.
- **`foreach` keys are typed from the subject.** A `list` or `T[]` gives an `int` key, and a shape gives the key types it has.
- **Inline array function calls keep their element types.** `iterator_to_array($it)[0]->name` and nested `array_map()`/`array_filter()` calls now resolve the same as when assigned to a variable, and `array_keys($this->templates())` uses the method's return type.
- **Array literals missing required shape keys are reported.** `takesConfig(['host' => 'localhost'])` and `takesConfig([])` against `array{host: string, port: int}` now name the missing keys.

#### Name resolution

- **Names resolve against the current namespace like PHP does.** Unqualified and qualified class names (`B::x()`, `new View\Event()`) in a namespace now resolve to the same-namespace class before a global one, and imports of the first segment take precedence. This applies inside plain functions, file-scope closures, and top-level code as well as classes.
- **Files with several `namespace` blocks are analysed correctly.** Code after the second `namespace` line is resolved against that namespace, calls to functions in another block of the same file have their return type, and classes in a global `namespace { }` block keep their global name (`PDO` is no longer `Pdo\PDO`).
- **Functions called through an imported namespace are found.** `use Core\Ip;` followed by `Ip\isIpAllowed()` is no longer reported as undefined. Contributed by @petrovo-as.
- **Namespaced constants are found however they are written.** `Config\GRADES`, `\App\Config\GRADES`, `use const` imports, and fully-qualified globals like `\PHP_EOL` now resolve.
- **Whole-namespace imports resolve to the right class.** `use App\Support;` with `Support\Pen` now always means `App\Support\Pen`, including in `::class` strings passed to other files.
- **A leading backslash in a `use` import is ignored.** `use \Foo\Bar;` resolves the same as `use Foo\Bar;`.
- **Built-in classes win over vendor polyfills.** Built-in classes like `RoundingMode` always resolve to PHP's own definition, so enum cases no longer become int constants. Results no longer vary between runs.

#### Type checking

- **Nullable values are checked consistently.** `?string` and `string|null` are now treated the same, so passing a `?string` to a `string` parameter is reported either way.
- **Union arguments must fit completely.** An argument typed `1|99` passed where `1|10` is expected, or `int|string` where `int` is expected, is now reported, and the message names the member that does not fit.
- **Passing a base type where a subclass is expected is reported.** Returning an `Animal` from a method declared to return `Cat` is now a type mismatch, unless the code proves the narrower type first.
- **Wrong generic type arguments are reported.** `Box<string>` where `Box<int>` is expected is now reported for arguments, returns, and properties.
- **Closures passed to `callable(...)` have their return type checked.** `static fn (int $v): int => $v` for a `callable(int): string` parameter is now reported. `array_filter()` callbacks no longer have to return `bool`.
- **Passing the result of a `void` call is reported.** `takesString(logRequest($request))` now says the expression returns no value.
- **A plain function's `@return` docblock is checked.** Returns that satisfy the native type but not the docblock (such as `array<string, int>`) are now reported, as they already were for methods.
- **Unsupported native type hints are reported.** `resource`, `integer`, `boolean`, `double`, `real`, `number`, `scalar`, and `list` in a native type hint are now reported as unknown classes. Docblocks can still use them.
- **Classes named like scalar aliases are classes.** `Integer`, `Boolean`, `Double`, `Resource`, and `Real` classes are no longer read as `int`, `bool`, `float`, or `resource`. Only the lowercase spelling is the alias.
- **More string refinements are understood.** `non-empty-literal-string`, `uppercase-string`, `non-empty-lowercase-string`, `non-empty-uppercase-string`, `trait-string`, and `enum-string` are recognized, string literals satisfy `lowercase-string`, `uppercase-string`, and `callable-string` when they should, and `non-falsy-string` (and `truthy-string`) is accepted where `non-empty-string` is expected.
- **`interface-string` requires an interface.** `SomeClass::class` is reported and `SomeInterface::class` accepted.
- **Escaped backslashes in class names are understood.** `'App\\Model'` matches `App\Model` for `class-string<T>`, `interface-string`, and `model-property<Model>`.
- **`object{prop: Type}` shapes accept matching objects.** Classes and `(object) [...]` literals are checked property by property instead of always being rejected.
- **Unknown PHPDoc pseudo-types are not enforced as classes.** Spellings like `pure-callable` or `literal-int` widen to the type they refine (or `mixed`) instead of being reported as missing classes, and `key-of<…>`/`value-of<…>` over a concrete array are evaluated.
- **`array-key` is treated as `int|string`.** It is now accepted wherever `int|string` is.
- **Out-of-order array literals are not lists.** `[1 => 'x', 0 => 'y']` passed where `list<string>` is expected is reported with a message about key order, and hover shows list parameters as written.
- **`Stringable` respects `strict_types`.** A `Stringable` object passed to a `string` parameter is reported under `declare(strict_types=1)`. `static` and `$this` types of a `Stringable` class (such as `SimpleXMLElement`) are accepted otherwise.
- **`int / int` and `int ** int` fit `int` positions.** Their `int|float` result is accepted where one branch fits, and a `float` reaching an `int` outside `strict_types` is accepted as PHP does.
- **`self`, `static`, and `parent` in parameter types resolve to real classes.** `canChangeTo(self $next)` no longer reports "expects self, got State", and `parent` parameters are now checked.
- **`class-string<static>` is checked correctly.** Sibling subclasses' `::class` constants are accepted, unrelated classes are reported, and `static` and `$this` now keep the class they are bound to, shown in hover as `static(App\Foo)`. Contributed by @calebdw.
- **The `object` and `iterable` rules apply everywhere.** Completion filtering and hover know a class is an `object` and a `Traversable` is `iterable`, and a nullable value no longer satisfies an `object` or `iterable` parameter.
- **`?->` on a `null` subject is not reported.** Only a plain `->` on `null` is a crash.
- **`isset()` in a short-circuit marks the variable defined.** `isset($x) && $x == 1` (common in Blade `@if`) no longer reports `$x` as undefined.
- **By-reference method out-parameters define the variable.** Passing an undeclared variable to a by-ref method parameter no longer reports it as undefined. Contributed by @calebdw.
- **Diagnostics run inside property hooks.** Misspelled variables in `get`/`set` hooks are reported, closures in one-line hooks have their members checked, and extract/inline code actions use the hook's scope.
- **Deprecation warnings use each variable's own type.** Two methods with a same-named parameter of different types no longer share a deprecation result (such as `Request::get` reported on an HTTP client call).
- **Calls with the same text are resolved separately.** `self::make()` in two classes, or `$var->method()` with different `$var` types, no longer share argument-count or argument-type results.
- **Unquoted keys in interpolated strings are keys.** `"$data[code]"` no longer reports `unknown_class`, and `"${data}"` resolves as the variable. Contributed by @HelgeSverre (#352).
- **`@see` tags with prose are not reported as missing symbols.** A `@see` target that resolves to nothing is now ignored, while PHPUnit's `@covers` and `@uses` are still checked. Contributed by @HelgeSverre (#353).
- **`@phpstan-type` aliases on traits and enums work.** They are no longer reported as unknown classes. Contributed by @calebdw.

#### Laravel

- **Custom Eloquent builders keep their model.** `SiteCertificate::query()->whereKey($id)->firstOrFail()` now returns `SiteCertificate` when the model uses a custom builder, with or without generics and at any depth of builder inheritance.
- **Custom builders are no longer cached half-built.** A builder could get stuck without its virtual members and mixin methods until the file was edited.
- **Query chains keep the model through `Query\Builder` methods.** Methods reached through `@mixin` (such as `lockForUpdate()`) keep `Builder<TModel>`, including on relations. Contributed by @calebdw.
- **Eloquent inference works for aliased models.** `use App\Models\Channel as ChannelModel` now types `ChannelModel::whereName(...)->firstOrFail()` correctly. Contributed by @calebdw.
- **Builder methods chained on a relation stay on the relation.** `$this->belongsTo(Author::class)->withTrashed()` returns a `BelongsTo`, so methods declared `: BelongsTo` are no longer reported.
- **Queries return the model's own collection class.** `get()`, relation properties, and relation queries now return the collection set by `#[CollectedBy]`, `HasCollection`, or `newCollection()`.
- **Models always have a primary key.** `$model->id` resolves without a migration, honouring `$primaryKey` and `$keyType`.
- **Framework internals no longer appear as model properties.** Methods like `hasMany` and `belongsTo` are no longer offered as relationship properties.
- **Factories use their declared `$model`.** A factory's `$model` property now decides the model for `make()`, `create()`, and the rest, through shared base factories too, and a nullable `count()` keeps the model-or-collection union. Contributed by @shuvroroy (#364).
- **Factories keep their model through a shared base factory.** Naming conventions now apply to the concrete factory, not the base. Contributed by @shuvroroy (#356).
- **Collection re-keying methods rebind keys correctly.** `keyBy()`, `groupBy()`, and `mapWithKeys()` work on collection subclasses that only use `@extends`, `mapWithKeys()` binds the array's key rather than the whole array, and `static fn` and unannotated callbacks bind the key from their body.
- **`auth()->user()` and `Auth::user()` return the configured model.** Completion, hover, navigation, and diagnostics see the concrete model.
- **`App::make()`, `makeWith()`, and `resolve()` return the requested class.** `App::make(CurrencyHelper::class)->format()` now resolves, including when chained directly.
- **Facade static calls keep concrete return types.** Calls through facades resolve via `getFacadeAccessor()` and `@mixin` targets before `__callStatic()`. Contributed by @calebdw.
- **Dotted container keys are not truncated.** `app('demo.bakery')` no longer resolves to a class named `Demo`.
- **`$this`, `self::`, and `static::` in macro closures resolve to the macro target.** Diagnostics, hover, and go-to-definition now agree with completion, and Carbon's `self::this()` idiom works.
- **Macros registered through facades bind `$this` to the facade target.** `Request::macro()` and `Context::macro()` closures see the class behind the facade. Contributed by @calebdw.
- **Request accessors return what each call returns.** `header()`, `query()`, `cookie()`, `post()`, `input()`, and `file()` return the whole bag without a key, the item with one, and drop `null` when a non-null default is given. `file('photo')` uses validation rules to tell single uploads from lists, and named arguments (`file(key: 'photos')`) bind to the right parameter.
- **Translation helpers return what the key names.** `__()`, `trans()`, and `Lang::get()` return `string` for a single line, an array for a group, and `null` with no key, so `{{ __('messages.welcome') }}` is no longer reported.
- **`url()` with a path returns `string`.** Contributed by @shuvroroy (#337).
- **`view('name')` returns the concrete `Illuminate\View\View`.** Components declaring `render(): View` no longer report a mismatch.
- **View names with `/` resolve.** `view('redirects/create')` and `@include('partials/modal')` work like the dotted spelling.
- **Custom view directories are recognized.** Views under paths registered in `config/view.php` resolve, complete, and navigate.
- **Routes under dynamic groups are not reported as unknown.** Groups named with a variable (`Route::name($panelId)`, `Route::name('filament.' . $panelId . '.')`, `Route::group(['as' => $dynamic], …)`, as Filament does) no longer flag the routes they register, while other route names are still checked.
- **`Route::resource()` names with a slash are correct.** `Route::resource('photos/comments', …)` registers `comments.show`, not `photos.comments.show`. Contributed by @shuvroroy (#308).
- **Laravel Folio page routes are recognized.** Named Folio pages complete, hover, navigate, and are no longer reported as unknown routes.
- **Service provider changes apply immediately.** Container bindings, view and translation paths, routes, config files, and component namespaces registered by a provider update when it is edited or added.
- **Non-Laravel projects are left alone.** A project's own `config()`, `route()`, `view()`, `__()`, or `trans()` no longer gets Laravel hover, navigation, or rename.
- **Virtual property hover and navigation prefer accessors.** Go-to-definition on accessor properties lands on the accessor, and legacy mutators appear in hover. Contributed by @calebdw.

#### Blade templates

- **Raw echoes work.** `{!! $html !!}` is now analysed as PHP, and each echo form only closes at its own terminator.
- **An unterminated echo no longer breaks the rest of the template.** A `{{` or `{!!` without a closer only affects its own line.
- **Inline `@php(…)` no longer hides the rest of the template.** It ends at its closing parenthesis and updates the template's variables.
- **Blade comments are just text.** Quotes, `*/`, `@endphp`, and commented-out echoes inside `{{-- … --}}` no longer break the code after them, and an unterminated comment no longer swallows the file.
- **Blade comments with quotes no longer corrupt the file.** Contributed by @krist7599555 (#303).
- **All of Laravel's Blade directives are understood.** `@can`, `@lang`, `@choice`, `@unset`, `@js`, `@vite`, `@dd`, `@pushIf`, `@pushOnce`, `@hasSection`, `@json`, `@dump`, and others are now analysed. `@class`, `@style`, `@checked`, `@selected`, `@disabled`, `@readonly`, `@required`, `@stack`, `@unless`, `@isset`, `@empty`, and `@switch`/`@case` with class constants no longer cause cascading errors, and `<?xml ?>` declarations are no longer read as PHP.
- **`@use`, `@inject`, and imports in templates work.** `use` statements in `@php` or `<?php` blocks and `@use(...)` import classes for the whole template, `@inject` defines its variable, and unused imports are reported.
- **Raw `<?php ?>` tags are passed through.** Strings like `'@context'` inside them are no longer read as directives.
- **Bound component attributes are analysed as PHP.** `:src="$image"` and `:$message` support hover, navigation, and completion, and their variables count as used.
- **`@extendsFirst` and `@componentFirst` are understood.** Every candidate layout or component is used.
- **`@var` annotations apply inside echoes.** A standalone `/** @var Collection<string, Loaf> $byName */` now types `{{ $byName->get(...) }}` with its generic arguments.
- **Go-to-definition on `{{` agrees with hover.** Both now point at the implicit `e()` call.
- **Completion edits land in the right place.** Completions that insert text (view names, string keys, `use` statements) no longer edit a spot several lines below the cursor.
- **`analyze` reports Blade diagnostics on the right line.** They were six lines too high.
- **Parameter-name inlay hints use the right viewport.** Hints in templates no longer go missing or appear for off-screen arguments.
- **Renaming a class no longer writes into templates that only receive it.** Rename and references ignore PHPantom's generated template header.
- **Formatting a `.blade.php` file does nothing.** It is no longer sent through a PHP formatter.

#### Navigation, references, and rename

- **Find References matches receivers by type.** Receivers reached through properties or calls are now typed, so `$context->getAll()` is no longer counted as a reference just because of its spelling. Deprecated methods reached through calls (`request()->get(...)`) are now reported.
- **Find References and Rename no longer match unrelated same-named methods.** Calls on untyped receivers are no longer matched, and references on an interface method include its implementations. Contributed by @sidux in https://github.com/PHPantom-dev/phpantom_lsp/pull/186.
- **References match PHP's case rules.** Calls to `HELPER()` or `new WIDGET()` are found and renamed with `helper` and `Widget`, including through imports. Constants stay case-sensitive.
- **Constructor references include `new self()`, `new static()`, and `new parent()`.**
- **Global functions used from namespaced code are counted.** A `helper()` call inside `namespace App;` now counts as a reference to the global `helper()`.
- **Same-named symbols are no longer mixed up.** Renaming `App\A\VERSION` no longer renames `App\B\VERSION`, renaming a global constant no longer touches a class constant of the same name, and Find References on a constant respects `includeDeclaration: false`.
- **Constants declared with `define()` are navigable.** Rename, references, hover, and highlights include the name in `define('FOO', 1)`, `defined('FOO')`, and `constant('FOO')`.
- **`use function` and `use const` imports are part of their symbol.** They navigate, hover, appear in references, and keep their namespace and aliases when renamed. Global `const FOO = 1;` declarations can start a rename. Contributed by @petrovo-as.
- **Rename works from a fully-qualified call.** `\Support\shout()` can now start a rename.
- **Renaming a promoted constructor property updates `$this->prop` uses.**
- **Renaming a namespace keeps group `use` statements valid.**
- **Rename no longer edits stale positions.** If a file changed since its last parse, rename offers nothing rather than editing the wrong code.
- **Method chains no longer use another file's imports.** Find References and reference counts no longer mix up same-named classes imported in different files.
- **Docblock navigation is more reliable.** `{@see}` nested in other text or tags, multi-line `@method` and `@property` tags, and types mixing `*` with non-ASCII names now navigate correctly.
- **`{@see method()}` finds the class's own method.**
- **Go-to-definition on an override jumps to what it overrides.** This works for methods, properties, constants, and classes with a parent. Contributed by @calebdw.
- **Code lens navigation works in every editor.** Lenses adapt to what the editor supports instead of guessing by name. Contributed by @calebdw.
- **Go-to-implementation is more reliable.** Results no longer depend on which files you viewed earlier, interfaces from dependencies (such as Symfony's `HttpKernelInterface`) are answered from their packages, abstract classes with a method body and classes inheriting a method are included. Built-in interfaces like `Countable` still search only your project.
- **Go-to-implementation works when an interface and class share a short name.** Contributed by @calebdw.
- **Searches during indexing show what they are waiting for.** Find References, Rename, and Go to Implementation show "Waiting for workspace index" in their progress.
- **Hover no longer depends on indexing timing.** Requests on a file that has not been parsed yet parse it immediately.
- **Hover is quiet on every declaration.** Global functions, top-level `const`, and promoted constructor parameters no longer repeat their own signature.

#### Completion, hover, and code actions

- **Comments and line breaks no longer break string completion.** Completion inside strings (Eloquent columns and relations, request keys, route, config, view, and translation keys, `model-property`, and route and Artisan parameters) now works when comments or line breaks sit between the call and the string, and knows when the cursor is outside a string. Escaped quotes in names are read correctly.
- **String completion no longer scans the whole file.** The search for the enclosing call is bounded.
- **Completion works after a multi-line closure argument.** `->map(function () { … })->` now offers members. Contributed by @calebdw.
- **Member names no longer suggest classes.** Typing after `function`, `const`, or `case` no longer offers class names. Contributed by @calebdw.
- **`extends` is not offered for enums.**
- **Vendor functions and constants rank as vendor code in completion.**
- **Generated PHPDoc uses the file's imports.** Completions and code actions write `Collection<TKey, TValue>` instead of the fully-qualified name.
- **Override completion writes valid return types.** `@return $this` becomes `: static`, template types are not used as native hints, and overrides of trait methods restate the docblock types the signature cannot express.
- **Generated return types understand any expression.** Calls, ternaries, `match`, property access, and multi-line arrays are resolved instead of becoming `mixed`.
- **"Extract function" leaves by-reference writes alone.** Selections that write to a by-reference variable are no longer extracted.
- **"Promote to constructor property" keeps attributes and docblock.** Attributes move to the parameter, and the property's docblock is removed with it.
- **"Make constructor final" puts `final` on the constructor.**
- **Semantic highlighting is more accurate.** Class constants use the constant color, attributes use the decorator color, and deprecated enum cases are marked. Contributed by @calebdw.
- **`@property` tag names are colored as properties.**
- **Hover lists a one-sided `if` union in source order.**
- **Short chains after `new X(...)` are not broken across lines.** From mago 1.44.0.

#### External tools and configuration

- **PHPStan is detected more accurately.** It runs for Laravel projects with Larastan (or a fork), for any project with a `phpstan.neon`, `phpstan.neon.dist`, or `phpstan.dist.neon`, and for projects that require `phpstan/phpstan` directly. A `vendor/bin/phpstan` from a transitive dependency is ignored, and plain PHPStan is not run on a Laravel application without Larastan, whether from `vendor/bin` or `$PATH`.
- **PHPStan sees the file's real location.** Location-aware rules (such as Larastan's `env()` check) no longer fire while you edit.
- **Project-wide scans no longer overwrite newer results.** PHPStan, PHPCS, and Mago scans no longer restore diagnostics to closed files or replace newer per-file results.
- **phpcbf is used when the project has a PHPCS config.** A `phpcs.xml` (or variant) enables it even when PHP_CodeSniffer is a transitive dependency.
- **Mago formatting wins when configured.** A `[formatter]` table in `mago.toml` keeps formatting with Mago even when a `phpcs.xml` exists.
- **Mago checks run only when configured.** `mago lint` runs with a `[linter]` table and `mago analyze` with an `[analyzer]` table, only when `carthage-software/mago` is a direct dependency. On Laravel projects, `mago analyze` runs only with a Laravel extension configured. `[mago]` settings in `.phpantom.toml` override this.
- **`.phpantom.toml` completion knows the Mago `lint` and `analyze` keys.**
- **The macOS global config path is documented correctly.** It is `~/.config/phpantom_lsp/.phpantom.toml`.
- **Drupal test base classes resolve.** `web/core/tests/` is now indexed. Contributed by @syntlyx.
- **`analyze` and `fix` resolve paths from the working directory.** A path that matches nothing is now an error.
- **Linux binaries run on any distribution.** They are statically linked, so they work on older glibc and musl systems, and use less memory.

#### Editor integration and stability

- **Workspace diagnostics settings apply without a restart.** Turning `workspace` on starts a scan, and turning it off stops the scan and clears its results.
- **Workspace scans finish even when files stall.** Workers that time out are replaced, so the rest of the project is still checked.
- **Background requests to the editor time out.** Diagnostic refresh requests give up after ten seconds.
- **Diagnostics stay current.** Closing a file mid-computation no longer brings its problems back, results that finish out of order no longer overwrite newer ones, and reopened files no longer show diagnostics from before they were closed.
- **Inlay hints update after edits.** Contributed by @calebdw.
- **Parameter-name inlay hints are correct for partly visible calls.**
- **Editing during indexing shows current results.** Open files keep their edited state while the workspace indexes.
- **Workspaces opened through symlinks work.** `vendor/` is recognized under either path spelling, Blade templates match their view roots, and a new `vendor/` from `composer install` is picked up without a restart. Contributed by @shuvroroy.
- **`analyze` gives the same results every run.** Results no longer depend on worker timing or core count, and classes or functions declared in more than one file always resolve to the same declaration, even after the winning file changes.
- **Functions behind `__DIR__`-relative `require_once` chains are indexed.** Calls like `\Safe\base64_decode()` are found. (#318)
- **Phar archives without a line break after the stub are read.**
- **Crashes and hangs are fixed.** Very long fluent chains, long `??` chains and nested ternaries, deeply nested `array_map`/`array_filter` closures, Pest expectation chains through union types, `"\x8b"` byte escapes, and non-ASCII characters in edited files no longer crash, hang, or stop analysis.
- **Variable resolution with `global` no longer hangs.**
- **Deeply nested class hierarchies resolve completely.** Only real dependency cycles fall back to partial results, and they do so consistently.
- **Lower memory use and faster analysis.** Assertion narrowing no longer leaks memory, stub indexes are shared instead of copied (roughly halving `analyze` time on large Laravel projects), and the deprecation check parses each file once.

## [0.9.0] - 2026-07-20

### Added

#### Diagnostics

- **Return type mismatches (`type_mismatch_return`).** `return` statements are checked against the declared return type, including values returned from `void` functions and bare `return;` in non-void functions. Generators are skipped. Contributed by @calebdw.
- **Property type mismatches (`type_mismatch_property`).** Plain `=` assignments to typed properties (`$this->prop = …`, `self::$prop = …`) are checked against the declared type. Untyped and `mixed` properties are not flagged. Contributed by @calebdw.
- **PSR-4 mismatches and rename-based moves.** A file whose namespace or class name does not match its PSR-4 path warns, with quick fixes. Renaming a class from its declaration lets you edit the full name to move it between namespaces, and renaming a namespace moves PSR-4 directories and updates references across the project. Contributed by @calebdw.
- **Case-sensitive autoloading.** A class reference whose casing differs from the declaration is flagged with a quick fix, catching code that works on macOS or Windows but fails on Linux.
- **Ignore rules in `.phpantom.toml`.** `[[diagnostics.ignore]]` silences matching diagnostics project-wide by file path (glob), message (regex), and/or diagnostic code, similar to PHPStan's `ignoreErrors`.

#### Type inference

- **Conditional return types keep intersections.** A `@return ($x is class-string<T> ? T&SomeInterface : SomeInterface)` resolves to the class intersected with the interface, so `mock(Foo::class)` and `$this->mock(Foo::class)` resolve to `Foo&MockInterface` without spurious mismatches.
- **`array_map` infers its output from the callback.** The result reflects what the callback returns, from its return type hint or its body, so `array_map(fn(Item $item): string => $item->id, $items)` is `list<string>`. Fixes #147. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/195.
- **`@template` on `@method` tags.** Virtual methods can declare their own template parameters (`@method TVal get<TVal of mixed>(TVal $default)`), inferred at call sites like real methods.
- **`#[ArrayShape]` attribute.** Functions annotated with `#[ArrayShape([...])]`, such as `parse_url`, `stat`, and `pathinfo`, give array key completion, hover types, and correct resolution.

#### Laravel

- **Macros are real methods.** Methods registered with `SomeClass::macro()` in your providers or installed packages complete, show their parameters and return type in hover and signature help, chain, and jump to their registration. Facade macros attach to the underlying class, and Find References and rename link registrations and calls.
- **Macro hover shows origin and inferred return types.** Hover labels macro methods as "macro" and, when the closure has no return type, shows the type inferred from its body marked "(inferred)". Regular methods with inferred return types get the same marker. Contributed by @calebdw.
- **Custom Eloquent builders.** Models using `#[UseEloquentBuilder]` forward the builder's methods as static methods, and `query()`, `newQuery()`, and `newModelQuery()` return the custom builder. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/118.
- **Relation and column name completion.** Strings passed to `with()`, `load()`, `whereHas()`, and similar methods complete relationship names (with dot notation for nested relations), and `where()`, `orderBy()`, `select()`, `pluck()`, and similar complete column names.
- **The authenticated user resolves to your model.** `$request->user()`, `auth()->user()`, and `Auth::user()` resolve to the model configured in `config/auth.php`, including per-guard models like `auth('admin')->user()`. When the model could vary at runtime, the result is a union of the candidates.
- **Container string aliases and global facades.** `resolve('blade.compiler')` and `app('cache')` resolve to the class Laravel binds, and global aliases such as `\App` and `\DB` resolve to their facades. Both tables are read from the Laravel version you have installed, and your own classes still win over an alias of the same name.
- **`model-property<T>` is recognised.** Larastan's pseudo-type no longer triggers "unknown class" and is treated as a string.
- **Route controller actions.** Method-name strings inside `Route::controller(X::class)->group(…)` resolve to the controller's methods, with completion, go-to-definition, Find References, rename, hover, and diagnostics. Contributed by @calebdw.

#### Completion and hover

- **Completion ranked by where symbols come from.** Project code ranks first, then PHP core, then your direct Composer dependencies, then transitive ones. Contributed by @calebdw.
- **Imported and same-namespace symbols rank first.** Symbols already imported or in the current namespace always appear above others. Contributed by @calebdw.
- **Static methods complete after `->`.** PHP allows calling static methods through an instance, so they are now offered. Static properties remain `::` only. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/174.
- **Magic methods complete when implemented.** `__invoke`, `__toString`, and other magic methods a class declares are offered (below regular methods) and support go-to-definition.
- **Package badges in hover.** Hover shows where a symbol comes from: 🟢 direct Composer dependency, 🟠 transitive dependency, 🟣 PHP core or extension. Project symbols show no badge. Contributed by @calebdw.
- **Correct badges for external path-repository packages.** Symlinked path-repository packages outside the workspace show their package badge, while modules inside the workspace still show none. Contributed by @calebdw.
- **`@phpstan-sealed` tag.** Classes named in the tag count as used imports, and docblock completion offers the tag. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/190.

#### Editing and navigation

- **Array-callable navigation.** Method names in `[Controller::class, 'method']` and `[$object, 'method']` support go-to-definition, Find References, and rename, which covers Laravel routes like `Route::get('/', [IndexPageController::class, 'indexPage'])`.
- **Array-callable method completion.** Typing inside the method-name string of an array callable completes the class's methods, including inherited and trait methods. (thanks @calebdw)
- **`compact()` strings are linked to variables.** Rename, Find References, and go-to-definition treat `compact('user')` as a use of `$user`. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/159.
- **Rename updates conditional return types.** Renaming a parameter also renames it inside `@return ($param is true ? T : U)`. Contributed by @calebdw.
- **`@param-closure-this` in hover and navigation.** Hover, go-to-definition, and go-to-type-definition on `$this` inside such a closure use the overridden type, as completion already did. Contributed by @calebdw.

#### Code actions

- **Convert arrow function to closure.** Rewrites `fn($x) => $x * 2` as a closure, capturing outer variables with `use()`. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/191.
- **Convert to arrow function.** Rewrites a single-expression closure as an arrow function when that is safe (PHP 7.4+).
- **Convert switch to match.** Rewrites a `switch` whose arms all return or assign to the same variable as a `match` expression (PHP 8.0+).
- **Extract interface.** Generates `{ClassName}Interface.php` from a class's public methods, keeps relevant `@template` tags, and adds `implements` to the class.

#### Tooling and platform

- **`analyze` and `fix` work without composer.json.** Projects that never adopted Composer (WordPress sites, legacy code) are analysed as plain PHP trees, with a note on stderr so a mistyped `--project-root` is not missed.
- **`update` command.** `phpantom_lsp update` downloads the latest release and replaces the binary. `--check` reports whether an update exists and `--no-confirm` suits CI. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/194.
- **Indexes stay fresh automatically.** Files created, deleted, or edited outside the editor (e.g. by `git checkout`) are picked up without a restart, and vendor packages are rescanned when `composer.json` or `composer.lock` changes.
- **Built-in formatter respects `mago.toml`.** The fallback formatter applies the `[formatter]` settings from a `mago.toml` at the workspace root. Contributed by @enwi in https://github.com/PHPantom-dev/phpantom_lsp/pull/233.
- **Path-repository packages are part of the project.** Local Composer packages installed through path repositories (such as `internachi/modular` modules) have their PSR-4 mappings included. Packages living outside `vendor/` are analysed as your own code, the rest as dependencies. Contributed by @calebdw.

### Changed

- **Laravel analysis runs only in Laravel projects.** Projects that do not depend on Laravel or an Illuminate component skip all Laravel-specific work and index faster.
- **More responsive editing.** Parsing and diagnostics run in the background, so completion and hover no longer wait for a full-file parse while you type. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/118.
- **Faster repeat completions.** Narrowing a completion by typing more characters returns instantly. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/118.
- **No first-use delay on Eloquent completions.** Common builder types are prepared at startup. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/118.
- **Faster code actions.** The lightbulb menu appears sooner because all refactorings share one parse of the file.
- **Lower memory use while indexing.** Files are read through the operating system's page cache instead of being copied into memory.

### Fixed

#### Type inference

- **Array keys written in only one branch are kept.** When `if`/`else` branches write different keys, the merged shape keeps all of them and marks one-sided keys optional (`array{a: int, b?: string}`).
- **Conditional `@return` types declared on interfaces are evaluated.** Calls like Spatie LaravelData's `Data::collect([...])` resolve to the narrowed branch instead of the broad union, removing false "incompatible with declared type" reports.
- **`new ReflectionClass($class)` instances.** With a `class-string<T>` argument, `newInstance()` and `newInstanceArgs()` resolve to `T`. Docblock types also refine native `object|string` hints now.
- **Return types inferred from a method body stay put.** Variables assigned from an un-annotated method (such as a query builder) keep their type across the whole method, removing intermittent "type could not be resolved" warnings.
- **`for` loop variables resolve in the condition and update clauses**, not only in the body.
- **Closure parameters keep their declared union type** when passed to a method on a union of differently typed collections.
- **Callable return templates bind from unannotated closures.** `$items->reduce(fn(Decimal $carry, $op) => $carry->add(…), new Decimal('0'))` resolves to `Decimal`.
- **Dynamic array keys.** `$prices[$key]` resolves to the union of the shape's value types, and maps written through dynamic keys in loops read back correctly, including nested key paths.
- **`\response()->json(...)` with a leading backslash** resolves like the unqualified call.
- **Conditional returns with generic `static<...>` branches** keep their type arguments, so `foreach ($items->chunk(500) as $batch)` gives `$batch` a type.
- **Conditional returns that select `mixed`** give the value `mixed` so it can be narrowed later (e.g. Laravel's `session($key)`), and out-of-order named arguments now pick the right branch.
- **Fluent chains through a trait's `return $this`** continue on the class using the trait, fixing chained test-assertion helpers.
- **`@phpstan-require-extends` in traits.** `$this` inside the trait sees the base class's members even when the trait is viewed on its own.
- **Parenthesized return types** such as `(Foo&object{pivot: Bar})|null` resolve instead of being dropped, fixing `$model->relation()->first()`.
- **Indexing a call result inline.** `$node->findChildrenOfType(Attr::class)[0]->getParent()` and `Status::cases()[0]->value` resolve.
- **`array_map` and `array_filter` type their callback parameter** from the array's element type, including when the array comes from a call.
- **Class-string unions through `foreach`.** Iterating `[A::class, B::class]` gives each element its class, so `app()->make($r)` resolves.
- **SPL iterators in `foreach`.** Iterators with a third inner-iterator generic argument and directly constructed ones like `new DirectoryIterator($dir)` give the loop variable the right type.
- **Inline `@var` before `foreach` refines a broad iterable.** The annotation now applies even if the variable was `mixed`, a bare `array`, or a method chain.
- **`$this` inside an anonymous class** resolves to the anonymous class, not the class containing it.
- **A namespaced class named like a built-in** (e.g. `App\Input\Iterator`) resolves to the project's class before the global one, as PHP does.
- **Conditional types nested inside generics** (as on `Collection::groupBy`/`keyBy`) are evaluated at the call site, and branches are chosen from the argument's resolved type.
- **Assigning an object to a property tracks its type**, so `stdClass` configuration graphs built field by field resolve. Later not-null assertions clear a previously assigned `null`.
- **Quoted namespaced class names** like `'App\\Models\\User'` resolve to the class.
- **Static calls** see return types from parents, interfaces, and framework corrections, as instance calls already did.
- **Positional array shapes.** `$pair[0]` on `array{Foo, Bar}` resolves to `Foo`, and multi-line shapes work too.
- **`Class::class` is a `class-string<Class>`** instead of a plain `string`.
- **Tuple elements with a class-string fallback** (`$row[2] ?? Fallback::class`) stay a `class-string`.
- **Mocks from test helpers keep the mocked class.** `$this->mock(Foo::class)`, `partialMock()`, and `spy()` resolve to `Foo` intersected with the Mockery contract.
- **Mockery `shouldHaveReceived()` / `shouldHaveBeenCalled()` chains** such as `->with(...)->once()` resolve.
- **Classes implementing `Iterator` directly** type their `foreach` values, fixing `SimpleXMLElement::children()`.
- **The `@` operator no longer blocks type resolution** (`$xml = @simplexml_load_string($content)`).
- **Assignments inside conditions are tracked**, including `if (!$item = find())` and `while (is_object($token = $iter->next()))`.
- **`iterator_to_array()` returns an array** with the iterator's key and value types. Contributed by @calebdw.
- **Reassigning a variable from its own element** (`$value = $value[0]`) updates its type. Contributed by @calebdw.
- **A class named after a pseudo-type**, such as PHP 8.4's `BcMath\Number`, resolves to the real class. Fixes #170.
- **Generator closures bind templates.** `LazyCollection::make(function() { yield (string) $x; })` resolves to `LazyCollection<int, string>`. Contributed by @calebdw.
- **`foreach` keys through a generic `IteratorAggregate`** use the declared key type. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/216.
- **`Generic<T>[]`, `array{…}[]`, and `(…)[]` types** parse as arrays. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/215.
- **`+=` on arrays infers `array`** instead of `int|float`. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/214.
- **`$str[0] = 'z'` keeps a string a string.** Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/209.
- **`mixed` array values.** Reading a key from `array<string, mixed>` gives `mixed` instead of nothing, removing false type mismatches and member warnings. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/210.
- **Classes defined inside conditional blocks** (like Doctrine's per-version `ServiceEntityRepository`) keep their parent and generics. Contributed by @MrSrsen in https://github.com/PHPantom-dev/phpantom_lsp/pull/154.
- **PDO fetch methods follow the fetch mode.** `fetch(PDO::FETCH_OBJ)` is an object and `fetch(PDO::FETCH_ASSOC)` an array. Conditional return types keyed on class constants are evaluated in general.
- **Chained and untyped access.** Null-safe chains like `$a->b?->c()` resolve, array access on unknown types gives `mixed`, `foreach` resolves through distant iterable interfaces, and nested shape narrowing targets the right key.
- **`self::` inside class attributes** resolves against the decorated class.
- **`@method` tags override inherited methods**, so repository `findOneBy()` returns the concrete entity.
- **`??=` keeps the resolved type.**
- **Generics with fewer arguments than parameters.** `@extends Collection<User>` binds `User` to the value parameter.
- **Nullable generic returns through inheritance.** A repository's `find()` returns `Entity|null` instead of `object|null`. Contributed by @MrSrsen in https://github.com/PHPantom-dev/phpantom_lsp/pull/152.
- **Conditional `is null` return types** resolve consistently, and an explicit `null` argument picks the null branch.
- **`@mixin Foo|Bar`** exposes members from every class in the union.
- **`@mixin` of an Eloquent model** exposes the model's relationships, scopes, casts, and accessors.
- **`@mixin T` of a template parameter** resolves through its bound, using the most specific bound in the chain.
- **Method-level templates bound to array types** resolve inside the method's own body.
- **Generic inference through call arguments.** `first(self::getEmailConfigs())` binds the template, and closure parameters no longer borrow an outer variable's type.
- **Templated helpers with a `class-string` default** resolve when called with no arguments, so `app()` resolves like `app(Foo::class)`.
- **Callbacks passed to generic helpers.** `Cache::remember($key, $ttl, fn() => new Order())` resolves to `Order` (and likewise `rememberForever`, `sear`, `flexible`, `withoutOverlapping`).
- **`__benevolent<T>`** resolves to its inner type.
- **`ArrayAccess` indexing** resolves through the generic annotation or `offsetGet()`. A class's own template parameter used in its `@implements`/`@extends` also resolves correctly.
- **Self-referencing reassignment.** In `$items = implode(', ', array_map($fn, $items))`, the `$items` on the right uses its previous type.
- **PHPDoc tags indented with extra spaces** after the `*` are read like normal tags.
- **Leading-backslash types** like `\Redis` resolve to the global class even when a same-named class is imported.

#### Type narrowing

- **Assertions on destructured variables.** `assertInstanceOf()` narrows variables destructured from an untyped array.
- **`assertInstanceOf` with a class held in a variable** narrows like the inline `Wanted::class`, including in loop-based data provider patterns.
- **Narrowing applies to array elements.** `assertInstanceOf()` and `is_a(..., true)` narrow `$arr['key']` and `$arr[0]`.
- **`array<T>|false` keeps its element type** after a `false` check.
- **A guard that reassigns one path** (`if (!$type instanceof Country) { $type = Country::ADMIN; }`) leaves the variable typed afterwards.
- **Member-existence guards.** `property_exists()`, `method_exists()`, and `isset($obj->name)` prove the member exists inside the guarded branch.
- **`assertTrue()` / `assertFalse()`** narrow like the equivalent `if`.
- **`assertIs*()` / `assertIsNot*()`** narrow like the matching `is_*()` checks.
- **`class_exists()` keeps `class-string<Foo>`**, so `new $var()` still resolves to `Foo`.
- **Each `if`/`elseif` branch narrows a property path independently.**
- **`instanceof` narrows inside arrow-function bodies** across `&&`.
- **Type guards trust the runtime check** when the static type was incomplete, removing "cannot access property on scalar" warnings.
- **`is_a($value, Class::class, true)` and `class_exists()`** narrow a string to `class-string`.
- **`is_numeric()` on a string** narrows to `numeric-string`.
- **A bare truthy check strips `null`.**
- **Compound conditions and non-variable subjects.** Narrowing carries across `&&` and `||`, applies to property paths, array elements, and inline assignments in conditions, and `@phpstan-assert` works on properties and indexed arguments.
- **`assertInstanceOf()` with an unknown variable class** keeps the subject's existing type minus `null`.
- **`$this` narrowed by `assert()`** works in top-level test closures (e.g. Pest) and for subclasses in regular methods.
- **Short-circuit conditions narrow later operands**, so `if (!$x instanceof Foo || !$x->method())` resolves `$x->method()`.
- **Assertion methods narrow however they are called**: via `$this->`, `self::`, `static::`, `parent::`, or a subclass. The `@phpstan-assert =Foo` form is parsed too.
- **Reassignments in an `if` branch** no longer leak into later `elseif` or `else` branches. Contributed by @calebdw.
- **The alternate `if:`/`endif;` syntax narrows** like the brace syntax.
- **Ternary conditions narrow properties and method calls**, as in `$this->node instanceof Artifact ? $this->node->getCompilationUnit() : null`.

#### Argument type checks

- **`array-key` and `int|string` are interchangeable.**
- **`class-string<A|B>` satisfies `class-string<T>`.**
- **A project class sharing a global interface's short name** (e.g. `App\Input\Iterator`) no longer breaks checks against the global `\Iterator` or `\Traversable`.
- **Implicitly nullable parameters accept `null`**, both with a narrowing `@param` over a nullable native type and with a `= null` default.
- **Class names as string literals** satisfy `class-string<Bound>` when the class fits, as in `$this->expectException('RuntimeException')`.
- **Class constants passed to generic parameters** bind the constant's value type, fixing `assertSame(Command::INVALID, $exitCode)`.
- **Class names passed to `class-string<T>`** bind the named class, and bare `class-string` values are accepted.
- **Unions of class names passed to `class-string<T>`** bind each class and check each against the bound.
- **`::class` arguments bound to a bare template** no longer report a mismatch, fixing `Mockery::type(SomeClass::class)`.
- **Generic calls no longer borrow types from other call sites** with the same text.
- **Overloaded built-in functions** like `strtr()` are only flagged when no signature matches. Contributed by @calebdw.
- **`@phpstan-type` / `@psalm-type` aliases** are expanded before checking, and imported aliases are not mistaken for classes. Contributed by @calebdw.
- **`int<min,max>` ranges and refined ints** (`positive-int`, `non-negative-int`, and so on) are compatible with each other where they should be. Contributed by @calebdw.
- **Intersections with extra members** satisfy narrower intersections, as with `Mockery::mock(Foo::class)` passed to `Foo&MockInterface`.
- **Resource handles that became objects** (`finfo_open`, `imap_open`, `pg_connect`, and others) return the object type for your PHP version. Fixes #164.
- **Integer literals satisfy integer ranges**, e.g. `usleep(10_000)` against `int<0, max>`. Contributed by @calebdw.
- **Literal types.** `orderBy('id', 'desc')` matches `'asc'|'desc'`, and a provably wrong literal is flagged. Fixes #180. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/191.
- **`declare(strict_types=1)` is honoured.** Coercions PHP disallows under strict types (such as int to string) are flagged. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/193.
- **Named arguments match parameters by name**, fixing conditional returns, missing-argument errors, and by-reference inference with out-of-order named arguments.
- **Argument-count false positives.** Extra arguments to a class without a constructor, `\mt_rand()` with a leading backslash, and immediately invoked returned callables (`makeHandler($a, $b)($request)`) are checked correctly, including their inlay hints. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/191.
- **Integer literals and refined ints.** `1` satisfies `positive-int`, `0` does not, and `non-zero-int` and `callable-string` types are recognised.

#### Diagnostics

- **No PSR-4 warnings for inline test fixtures.** Test files that mix top-level calls like Pest's `it(...)` with inline classes are skipped. Contributed by @calebdw.
- **No duplicate native diagnostics in pull-mode editors.** Contributed by @calebdw.
- **`compact()` with an array argument** counts its variables as used, with rename and navigation support.
- **Variables used only as dynamic member names** (`$obj->{$name}()`) count as used.
- **Array literals that only look like callables** (`[Foo::class, 'label']` used as data) are no longer reported as missing methods.
- **`isset()` and `empty()` on unknown properties** are no longer flagged.
- **Results of methods returning `object` or `?object`** allow member access.
- **`@see Class#method` references** resolve the class instead of reporting the whole string as unknown.
- **Calls handled by `__call` / `__callStatic`** are no longer flagged, and their return type keeps the chain going.
- **By-reference closure captures** (`use (&$var)`) count as used.
- **`stream_bucket_make_writeable()` results** no longer warn on PHP versions before 8.4.
- **Diagnostics update after a function signature changes.** Other open files calling the function are refreshed on save, and a `didSave` handler helps editors like Neovim. Fixes #123. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/196.
- **Inherited members are not flagged right after opening a project.**
- **`@var` annotations no longer leak between functions.**
- **Unused-import dimming** lands on the right line when imports share a prefix.
- **`get_defined_vars()` counts as using every variable in scope.** Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/158.

#### Laravel

- **Date helpers respect `Date::use()`.** `now()`, `today()`, and `Date` facade calls resolve to the class your provider configures (e.g. `CarbonImmutable`), and changes apply during the session.
- **Values typed as a Laravel contract** use their concrete class, so macros and other `__call` methods are not reported missing.
- **Relations resolve regardless of casing**, as Laravel does (`$order->orderproducts` for `orderProducts()`).
- **Paginated results carry their model.** `foreach (User::paginate() as $user)` gives `$user` the model type.
- **`Storage::fake()` and `persistentFake()`** resolve to `FilesystemAdapter`, so test assertions complete.
- **`Conditionable::when()` callbacks** get the argument's type instead of `null`. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/205.

#### Editing and navigation

- **Extract Variable is no longer offered on declarations.** Contributed by @calebdw.
- **Extract Method handles more selections.** Variables read before being assigned are passed in as well as returned, and early returns that use selection-local variables stay inside the new method.
- **"Go to Declaration or Usages" lists usages** when invoked on a declaration. Fixes #125.
- **Edited functions and constants update immediately** in completion, hover, and go-to-definition.
- **Reloaded files no longer leave ghost classes** in completion, implementations, or type hierarchy.
- **`@see self::member()`** navigates. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/212.
- **Multi-line grouped imports** no longer cause false unknown-class errors, and go-to-definition works on grouped names. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/213.
- **Type Hierarchy works in more editors**, notably Zed. Contributed by @sidux in https://github.com/PHPantom-dev/phpantom_lsp/pull/179.
- **More accurate go-to-definition and rename.** Qualified names in `@see` land correctly, and renaming a property selects the whole `$name`.
- **`@phpstan-require-extends` and `@phpstan-require-implements` navigation.** Names in these tags support go-to-definition and hover and count as used imports. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/172.
- **Renaming variables in nested closures and arrow functions** updates every occurrence. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/145.
- **Variables in dynamic property access** (`$message->{$attribute}`) count as used and are included in references and rename. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/174.
- **Member rename stays on its own declaration**, leaving unrelated classes, sibling implementations, and unresolvable receivers alone. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/160.
- **Find References on a constructor** lists `new` calls, attribute usages, and `parent::__construct()`-style calls, including for subclasses that inherit it. Contributed by @RemcoSmitsDev in https://github.com/PHPantom-dev/phpantom_lsp/pull/155.
- **Correct columns on lines with multibyte characters** for signature help, navigation, named-argument completion, import removal, and the `@phpstan-ignore` quick fix.
- **Document outline ranges** cover the whole declaration, as editors expect.
- **Type Hierarchy** finds the class name when it is on a different line from `class`.
- **Edits on Windows (CRLF) files land correctly** for rename, import removal, and the PHPStan return-type quick fix.
- **Code lens clicks work in all editors**, including Zed, Neovim, and Emacs.
- **Promote to constructor property** no longer deletes sibling properties declared on the same line.

#### Completion, hover, and highlighting

- **Symfony polyfills count as PHP core** for completion ranking and hover badges. Contributed by @calebdw.
- **Misspelled members are not coloured as valid code.** Semantic highlighting checks that a member exists, and existing members get deprecated and static styling. Fixes #187.
- **Highlighting no longer goes stale while typing.**
- **`namespace` and `use` lines keep the editor's own colouring** in PHP files.
- **`throw new` and `catch` completion** offer only Throwable classes and rank, shorten, and style names like other class completion.
- **HTML lists in docblocks render on hover.** Contributed by @calebdw.
- **No duplicate parentheses** when you type `(` yourself after a method name instead of accepting the completion.

#### External tools

- **External diagnostics are no longer hidden** by a native diagnostic on the same line, and diagnostics on a line are ordered most severe first.
- **External formatters no longer break the connection.** php-cs-fixer and PHP_CodeSniffer could swallow editor input and drop the server. Timeouts now name the tool and can be raised with `[formatting] timeout`. Fixes #149.
- **PHPStan, PHPCS, and Mago run on open and save only**, without a debounce delay. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/196.
- **`@phpstan-ignore` with a reason** clears the diagnostic immediately. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/196.
- **Large external tool reports** no longer time out.

#### Indexing

- **Framework helpers loaded outside Composer autoload are indexed**, such as CakePHP's `__()`, `h()`, and `env()`. Contributed by @dereuromark in https://github.com/PHPantom-dev/phpantom_lsp/pull/175.
- **Vendor symbols removed by `composer update`** disappear from completion and navigation.

#### Performance and stability

- **Deeply nested code no longer crashes** the analyser or editor (e.g. WordPress' bundled getID3 tables).
- **Large procedural files no longer stall.** The worst observed file went from over two minutes to under a second.
- **Self-referencing property assignments no longer crash** (`$this->items = array_unique(array_merge($this->items, $more))`).
- **Results are the same on every run.** Two timing issues could make types fail to resolve depending on which files were analysed together.
- **The editor stays responsive during fast typing.** Request bursts are processed concurrently, diagnostics compute in the background, and superseded requests are dropped instead of piling up.
- **Large files no longer peg the CPU while typing.** Semantic highlighting, the outline, and folding are now effectively instant on large files.
- **The first use of a global helper no longer stalls.** Helpers like Laravel's `app()` are parsed during indexing.
- **Editing a widely extended base class stays responsive.**
- **Completion latency stays flat during long typing bursts.**
- **The server no longer freezes** after a burst of cancelled requests.
- **No hang on cyclic class inheritance** written mid-refactor.
- **Returning to a backgrounded editor stays responsive.**
- **A rare internal parser error no longer breaks a file** for the rest of the session.
- **Malformed `@method` tags no longer crash requests.**
- **Analysis deadlock fixed** between lazy vendor parsing and file-change handling.
- **Memory no longer grows** as files are opened and closed over a long session.

## [0.8.0] - 2026-05-14

### Added

#### Blade templates

- **Blade template support.** Completion, hover, go-to-definition, diagnostics, semantic tokens, and inlay hints work inside `.blade.php` files. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/100.
- **Blade highlighting.** Directives, echo delimiters, PHP keywords, casts, comments, and PHPDoc tags in Blade files are coloured.
- **View navigation from directives.** Go-to-definition on view names in `@include`, `@extends`, `@component`, `@each`, and the other include directives opens the template.

#### Diagnostics

- **Argument type mismatches.** Calls whose argument types are incompatible with the parameter types are flagged.
- **Invalid class kinds.** Using a class-like where it is guaranteed to fail at runtime is flagged: `new` on an abstract class, interface, trait, or enum, `extends` on a final class, `implements` with a non-interface, `catch` with a non-Throwable, and similar.
- **Unused variables.** Variables that are assigned but never read are dimmed. Names like `$_` or starting with `$_` are exempt.
- **Mago diagnostics.** Mago lint and analyze results appear in the editor with quick fixes. Configure under `[mago]` in `.phpantom.toml`.
- **PHPCS diagnostics.** PHP_CodeSniffer violations appear in the editor. Configure under `[phpcs]` in `.phpantom.toml`.
- **Magic property diagnostics.** With `report-magic-properties` under `[diagnostics]`, classes with `__get` and declared virtual properties (such as `@property` tags or Eloquent columns) report unknown properties instead of allowing everything.
- **Inline suppression.** `// @phpantom-ignore code` on the same line or the line above suppresses a diagnostic. Separate several codes with commas, or leave the code off to suppress everything on that line.

#### Type inference

- **Broader type narrowing.** `instanceof`, type-guard functions, strict `in_array()`, `assert()`, `@phpstan-assert-if-true`/`-if-false`, and `&&`/`||` conditions narrow types in branches, guard clauses, `while` bodies, ternaries, and `match(true)` arms.
- **Return types inferred from method bodies.** Methods without a declared return type get one from their `return` statements.
- **Closure parameter inference.** Untyped closure parameters are inferred from the callable signature they are passed to, including through `static`-returning chains and generics.
- **Generics.** `@mixin T` resolves through the template bound, `new $var()` with `class-string<T>` gives `T`, and SPL collection classes carry template parameters.
- **Untyped properties are inferred from the constructor.** Contributed by @lucasacoutinho in https://github.com/PHPantom-dev/phpantom_lsp/pull/81.
- **Binary operator types.** Hover and resolution show results for arithmetic operators (`int + float` is `float`, `int / int` is `int|float`), and compound assignments update the variable.
- **Nested array shapes from key assignments.** `$b['a']['b'] = 'x'` produces `array{a: array{b: string}}`, so incrementally built arrays get key completion.
- **Loop type propagation.** Variables assigned late in a loop body are visible from the start of later iterations.
- **`global` variables** resolve to their top-level type.
- **`array_reduce`, `array_sum`, and `array_product`** return the expected types.

#### Laravel

- **View, route, and translation key navigation.** Go to Definition works for `view('...')`, `route('...')`, `__('...')`, `trans(...)`, and `Lang::get(...)`. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/101.
- **Config and env key navigation.** Go to Definition and Find All References work for `config('app.name')` and `env('APP_KEY')`. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/93.

#### Editing and navigation

- **Namespace renaming.** Renaming a namespace segment updates declarations, imports, and references across the workspace and moves the PSR-4 directory.
- **Linked editing ranges.** All occurrences of a variable in its scope can be edited together.
- **Find References and rename for PHPDoc virtual members.** `@property`, `@property-read`, `@property-write`, and `@method` declarations are included alongside their usages, including on nullable and union types. Contributed by @AbyssWaIker in https://github.com/PHPantom-dev/phpantom_lsp/pull/115.
- **Closure inlay hints.** Closures passed to callable parameters show their inferred parameter and return types.

#### Code actions

- **Replace FQCN with import.** Adds a `use` statement and shortens every occurrence of the name in the file. A second action does this for every fully-qualified name at once, skipping conflicts.
- **Import all missing classes.** Imports every unresolved class in the file at once, leaving ambiguous names for you to choose.
- **Context-aware import candidates.** Import suggestions only offer interfaces after `implements`, traits after `use`, and so on.
- **Convert to instance variable.** Promotes a local variable to a class property and rewrites its uses to `$this->prop` (or `self::$prop` in static methods).

#### Tooling and platform

- **Laravel Pint formatting.** Projects with `laravel/pint` in `require-dev` format with Pint automatically. Set `pint = "path"` or `pint = ""` under `[formatting]` to override or disable.
- **Machine-readable CLI output.** `analyze` and `fix` accept `--format table|github|json`, and table output adds GitHub annotations when `GITHUB_ACTIONS` is set.

### Changed

#### Behaviour

- **First-class callables use the shared return type logic.** `$fn = $obj->method(...)` is more accurate for chained calls and generics.
- **Diagnostics arrive sooner.** Editors with pull diagnostics get results on first open without a delay, and external tool updates no longer re-run native diagnostics.
- **Mixins and virtual accessors are always fully resolved**, so they no longer go missing after edits.
- **Consistent diagnostic codes.** All codes use `snake_case` noun phrases such as `unknown_variable`, `type_mismatch_argument`, `argument_count_mismatch`, `deprecated_usage`, and `missing_implementation`. Editor filters on the old codes need updating.
- **Updated embedded phpstorm-stubs.**

#### Performance and memory

- **Faster and fresher Find References.** Searches skip more unnecessary work, still follow aliased imports, and pick up newly added files. Contributed by @MingJen in https://github.com/PHPantom-dev/phpantom_lsp/pull/116.
- **Incremental text sync.** The editor sends only changed ranges instead of the whole file on every keystroke.
- **More responsive requests.** Hover, go-to-definition, signature help, code actions, rename, and other requests run on background threads, so slow ones no longer block others.
- **Faster analysis** on large projects.
- **Less duplicate parsing.** Threads needing the same vendor file wait for one parse instead of each parsing it.
- **Faster recovery after edits.** Classes affected by an edit are re-resolved eagerly in dependency order.
- **Lower memory use.** Vendor and stub files keep less data after parsing, go-to-implementation uses a dedicated index, and variable type tracking uses less memory.
- **Faster variable completion and go-to-definition.** Both use precomputed data instead of re-parsing. Variable completion also keeps `foreach` variables after the loop, includes `@var` names, and drops `unset()` variables.

### Fixed

#### Completion

- **`throw new` and `catch` completion include vendor classes** whose Throwable ancestry was not yet known.
- **No phantom function suggestions** from `use function` lines being mistaken for declarations.
- **No duplicate `use function`** when the import already exists.
- **Function import conflicts.** If a different function with the same short name is imported, completion inserts the fully-qualified name.
- **Standalone `/** @var Type $var */` docblocks** give the variable member completion and go-to-definition.
- **Completion in loops and branches.** Shape keys added in `if` blocks, variables assigned later in loops, and variables on the right of a reassignment resolve.
- **`class-string<T>` parameters** resolve to the bound class for member access.

#### Type inference

- **Mixin members update after edits** without restarting the server.
- **`@removed` constants** (e.g. `MCRYPT_ENCRYPT`) are hidden when your PHP version is newer.
- **`@var` docblocks with extra tags** like `@psalm-suppress` keep the right type.
- **`foreach` `@var` annotations** for both key and value are honoured.
- **`foreach` over a bare `array`** gives `mixed` instead of nothing.
- **`break` in `else`** contributes to the type after the loop.
- **The pre-loop value of a loop target** no longer survives after looping over a non-empty literal array.
- **`foreach` over `::class` literal arrays** supports `$className::CONST` and `$className::method()`.
- **Hover on the left side of a reassignment** shows the new type.
- **Magic `__get` and `__call`** return their declared types, and `__get` with `key-of<T>` infers the property's type.
- **`SoapClient`** accepts any method call.
- **Literal `true`/`false`** stay precise in template inference.
- **`@psalm-method` overrides `@method`.**
- **Prefixed tag priority.** `@phpstan-param` beats `@psalm-param`, which beats `@param`, as in PHPStan and Psalm.
- **`self`/`static` handling.** `static` survives through first-class callables, `@var self|null` properties show the owning class, and trait methods returning `self` resolve to the declaring class.
- **Interface return types are inherited** by overriding methods without a return type, with templates substituted.
- **Conditional return types with scalar arguments** (`$param is string`) resolve.
- **SPL iterators.** Decorators like `CachingIterator` and `LimitIterator` keep the wrapped iterator's generics, and `new ArrayIterator($typedArray)` infers its types.
- **`range()`** returns `list<string>` or `list<int|float>`.
- **`(object)` casts** give an object shape matching the operand.
- **`$obj[$key] = $val` on `ArrayAccess`** keeps the object's type.
- **`$variable::method()` on a union of class-strings** resolves through every class.
- **Array shape keys with special characters** are quoted and escaped in type display.
- **Chained calls with complex arguments.** `redirect($string . $var)->with(...)` resolves to `RedirectResponse`.
- **Mixins.** Static calls resolve through `@mixin`, `@method` and `@property` tags on mixin classes carry over, and `$this` returns resolve to the consuming class.
- **`@method` tags.** Colon return syntax, parenthesised returns, and a lone `static` parse correctly, and templates in `@method` returns are substituted through `@extends` and `@implements`.
- **Immediately invoked first-class callables** (`Foo::method(...)()`) resolve to the method's return type.
- **`@return numeric`** resolves correctly.
- **Array access on bare `array` and `mixed`** gives `mixed`.
- **Promoted properties** honour inline `/** @var */` inside the constructor.
- **Backed enums.** `->value` resolves to the backing type, and `@implements` generics on enums work.
- **Inherited class constants** resolve through several levels via `self::` or a child class.
- **Type display.** `T[]` shows as `array<T>`, `mixed[]` as `array`, aliases are normalised, and `parent` returns show the real class.
- **Chain assignments** like `$a = $b = new Foo()` type every variable.
- **Destructuring** with `[...]`, `list()`, keys, nesting, and in `foreach` resolves.
- **Short class names** in `@var`, `@param`, and `new` are fully qualified before use.
- **`$this` no longer resolves inside static methods.**
- **Hover reflects docblock edits** in other files immediately.
- **`foreach` element types** resolve through nested generic access, static properties, type aliases, and by-reference bindings.
- **Variables stay visible after a closure argument** in a chained call.
- **`@param` annotations no longer leak** across sibling methods or closures.
- **Inherited parameter types** carry over to child methods.
- **Union-typed method calls** keep resolving on their second occurrence.
- **Fluent `static`/`self` chains** work in namespaced classes.
- **Type narrowing.** `is_*()` guards narrow multi-member unions, `instanceof` narrows `mixed` and `object`, `=== null` and `== null` narrow, `assert()` narrowing persists, `isset()`/`empty()` strip `null`, properties and shape keys narrow, OR'd `instanceof` gives a union, the loop condition's inverse applies after the loop, and branch merges keep nullability.

#### Generics

- **`@template-implements` on stub interfaces** passes substituted return types to child methods.
- **Generic `@var` annotations** substitute class templates into method return types.
- **Several arguments bound to one template** produce a union.
- **Nested templates** are inferred when a template has a generic bound.
- **`@template K as key-of<TData>`** resolves to the matching array shape value type.
- **`@psalm-if-this-is`** infers method templates from the receiver's type.
- **`self::class` and `static::class`** passed to `class-string<T>` resolve to the enclosing class.
- **Constructor and chained generics.** Inherited constructors infer generics through `@extends` chains, class templates survive chained calls, templates fall back to their bound, unions of generic types resolve per branch, `key-of<T>` and `value-of<T>` evaluate, and array literal keys and values are inferred separately.
- **`(new Box(new Product()))->get()`** keeps the constructor's inferred generics.
- **Closure inlay hints** substitute templates from sibling arguments.

#### Name resolution

- **Files with several namespaces.** Class names, variables, function return types, and static calls resolve against the namespace block they appear in.
- **Short names in type hints** prefer the owning type's namespace.
- **Unqualified class names** in namespaced code fall back to the global class when no namespaced one exists.
- **Imported classes are not shadowed** by global stubs, fixing Laravel facade static calls.
- **Same-named classes in other namespaces** no longer hide inherited members or count as the same class.
- **Transitive interface inheritance** is recognised in subtype checks.
- **Conditional return types** check interface implementations and resolve names through the defining file's imports.

#### Diagnostics

- **Fewer unused-variable false positives** for `compact()`, by-reference out-parameters like `preg_match()`'s `$matches`, and `global` variables.
- **Fewer type-mismatch false positives** for bare `array` arguments, properties narrowed with `instanceof`, type alias parameters, and shadowed imports.
- **Functions inside `if (!function_exists(...))`** no longer produce unresolved-member errors.
- **`parent::__construct()` with `@extends` generics** no longer reports false type errors.
- **Vendor functions and constants are indexed at startup**, removing false unknown-function errors.
- **`\Closure` satisfies `callable`.**
- **Fewer undefined-variable false positives** for by-reference parameters, nested array assignments, and names starting with `$this`.
- **`catch` types match across namespaces.**
- **Nested `match(true)`** no longer produces wrong diagnostics.
- **Lowercase built-in class names** count as objects.
- **No false "class not found"** for global classes loaded through Composer `files` autoloading.
- **Generic class methods** have templates substituted before argument checks.
- **No false errors on startup** for files opened while indexing.
- **Duplicate or stale pull diagnostics** no longer appear.
- **Non-deterministic results** on generics-heavy projects are gone.

#### Editing and navigation

- **Renaming a class keeps `self`, `static`, and `parent`.**
- **Renaming a variable follows closure `use` captures and arrow functions.**
- **Eloquent `$dates` properties and `where{Property}()` methods** support go-to-definition.
- **Type hierarchy registration** respects the editor's capabilities.
- **Auto-import formatting.** A blank line is added before the first import, and removing unused imports works in braced namespaces.
- **Implement methods** no longer writes generic docblock syntax as a native return type.
- **Laravel scopes.** Public methods with `#[Scope]` are no longer treated as scopes.

#### Indexing

- **Composer `files` autoload packages** have their classes discovered.
- **Class name collisions** prefer the file matching PSR-4.

#### Performance and stability

- **No deadlock** when navigating to an unparsed vendor class.
- **No freezes under heavy editor activity.** Server-to-client requests can no longer deadlock, and the process exits cleanly if its main loop stops.
- **No hangs on deeply nested loops** or on `$arr['key'] = f($arr['key'])` patterns.
- **No stack overflows** on large files.
- **Hover scales linearly** on files with many method calls.
- **`analyze` and `fix` run at the same speed** however they are invoked.

## [0.7.0] - 2026-04-08

### Added

#### Type inference

- **`@psalm-return`, `@psalm-param`, and `@psalm-var`.** Psalm-prefixed tags are recognised alongside their PHPStan equivalents everywhere types are read.
- **Type-guard narrowing.** `is_array()`, `is_string()`, `is_int()`, `is_float()`, `is_bool()`, `is_object()`, `is_numeric()`, and `is_callable()` narrow unions in branches and after guard clauses, keeping generic element types.
- **Array value tracking.** Arrays built with variable keys in loops keep their element types through `foreach`, bracket access, and `??`, and `foreach` over generic arrays keeps shape and scalar element types.
- **Inherited docblock types.** A child method without its own `@return` or `@param` docblock inherits the parent's richer types and descriptions.
- **Templates inferred from closures in both directions.** Templates in callable signatures are inferred from a closure's return type and its parameter types.
- **Element types from property generics.** `$this->cache[$key]->` resolves the element type of generic array and collection properties, including through chains.
- **`@phpstan-assert-if-true $this`.** Methods that assert on `$this` narrow the receiver in the matching branch. Contributed by @syntlyx in https://github.com/PHPantom-dev/phpantom_lsp/pull/52.
- **Standalone `@var` for untyped closure parameters.** A `@var` block above the usage types the parameter.
- **Method-level templates inside the method body.** `@template T of Builder` with `@param T $query` gives `$query` the bound's members inside the method.
- **By-reference parameter types for methods and constructors.** Passing a variable to a typed `&$param` gives it that type afterwards, now for method, static, and constructor calls too.

#### Completion and hover

- **Keyword completion.** PHP keywords are suggested by context, such as `return` only inside functions and `break` only inside loops. Contributed by @ryangjchandler in https://github.com/PHPantom-dev/phpantom_lsp/pull/43.
- **Attribute completion.** Inside `#[…]` only attribute classes valid for the target are offered.
- **`new self`, `new static`, and `new parent`.** Constructor snippets and signature help work for these. Contributed by @RemcoSmitsDev in https://github.com/PHPantom-dev/phpantom_lsp/pull/51.
- **Hover on parameters at their definition.** Shows the resolved type, using the `@param` type when it is richer. Contributed by @RemcoSmitsDev in https://github.com/PHPantom-dev/phpantom_lsp/pull/68.
- **Namespace completion from the file path.** Typing `namespace ` in a new file suggests the namespace from its location and PSR-4 mappings, with the most specific one preselected.

#### Code actions

- **Refactoring actions.** Extract function, method, variable, and constant, inline variable, promote constructor parameter, generate constructor, getters/setters, and property hooks (PHP 8.4+). The lightbulb menu appears instantly because edits are only computed when you pick an action.
- **PHPStan quick fixes.** Automatic fixes for many PHPStan errors, including mismatched `@return`/`@param`/`@var` tags, unsafe `new static()`, `#[Override]`, `#[\ReturnTypeWillChange]`, void return mismatches, unreachable statements, always-true `assert()` calls, visibility of overrides, and ternaries that can become `??` or `?->`. Each fix clears its diagnostic immediately.

#### Diagnostics

- **Undefined variables.** Reading a variable with no prior definition in the same scope is flagged, catching use-before-assign bugs. Assignments in branches, by-reference parameters (built-in and your own), superglobals, `isset()`/`empty()`, `compact()`, `extract()`, variable variables, and `@var` annotations are all accounted for. Top-level code is skipped.

#### Laravel

- **Eloquent model improvements.** Timestamps are typed as `Carbon` (respecting `$timestamps = false` and custom columns), `$dates` and `$appends` produce typed properties, `where{Column}()` methods are available on the model and builder, `whereHas` closures get the related model's builder (with dot notation), and `when()`/`unless()` chains keep their type.

#### Tooling and platform

- **`fix` command.** `phpantom_lsp fix` applies automated fixes across a project. Choose rules with `--rule` or run all preferred fixers, and use `--dry-run` to preview. The first rule, `unused_import`, removes unused `use` statements (contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/54).
- **Drupal support.** Drupal projects are detected and their extra PHP file types (`.module`, `.install`, `.theme`, `.profile`, `.inc`, `.engine`) are indexed. Contributed by @syntlyx in https://github.com/PHPantom-dev/phpantom_lsp/pull/52.
- **`--stdio` flag.** Accepted and ignored, for clients that pass it by default. Contributed by @markkimsal in https://github.com/PHPantom-dev/phpantom_lsp/pull/67.
- **`--tcp` flag.** `phpantom_lsp --tcp 9257` listens on a TCP port (or a full address like `127.0.0.1:9257`) instead of stdin/stdout, serving one connection.
- **Zed extension setup instructions.** Contributed by @daronspence in https://github.com/PHPantom-dev/phpantom_lsp/pull/47.
- **SETUP.md improvements.** Contributed by @mattsches in https://github.com/PHPantom-dev/phpantom_lsp/pull/61.

### Changed

- **Fewer false-positive diagnostics.** Completion, hover, and diagnostics now agree on every variable's type.
- **`@phpstan-ignore` is never the preferred quick fix**, so keyboard shortcuts no longer apply it by accident.
- **Generate PHPDoc infers `@return` from the body**, e.g. `@return list<string>` instead of `@return array<mixed>`.
- **Faster startup.** Stubs load much faster.
- **More accurate generics** for unions, intersections, array shapes, and deeply nested arguments.
- **`null` is case-insensitive** (`NULL`, `Null`) everywhere, as in PHP.
- **Go-to-definition on a declaration returns its own location**, so editors can fall back to Find References. Contributed by @lucasacoutinho in https://github.com/PHPantom-dev/phpantom_lsp/pull/76.

### Fixed

#### Type inference

- **Case-insensitive `self`, `static`, and `parent`**, including `parent::method(...)` in chained callables.
- **Intersection types** display as intersections in hover, extracted parameters, and generated docblocks instead of unions.
- **Return types carry class information** so hover, narrowing, and completion need no second pass.
- **Generic parameters are kept** on catch variables, by-reference and closure parameters, and constructor calls.
- **Type guards keep the remaining class** when narrowing unions like `Foobar|string|int`.
- **`$this->prop` as an argument** keeps its generic, nullable, or union type.
- **`@phpstan-assert` and `@psalm-assert` with generics** parse the full type.
- **`parent::method()` as an argument** resolves its return type.
- **Conditional return types show the resolved class in hover**, e.g. Symfony's `SerializerInterface::deserialize()`.
- **Null narrowing from `!== null`, `!is_null()`, and truthy checks**, in chains, ternaries, and `return` statements.
- **Assignments in `if`/`while` conditions** type the variable in the body.
- **Loop-body assignments** are visible throughout the loop.
- **`@var` annotations no longer leak** across classes and methods.
- **An inline `@var` cast** does not affect the right side of the same assignment.
- **`foreach` over unions containing arrays** (`User|array<User>`) yields the element type.
- **`@param` overrides a native type hint** when it is more specific. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/55.
- **Reassignments in `try`/`catch`/`finally`** are tracked.
- **`instanceof` with an unloadable class** treats the variable as unknown instead of keeping the old type.
- **`object` and `stdClass`** allow any property, and `is_object()` narrows `mixed` to `object`.
- **Docblock refinement** no longer treats a class like `PointOfInterest` as an `int` refinement.
- **Static calls on `class-string<Foo>`** resolve, including `static`.
- **`self`, `static`, and `$this` in return types from other files** resolve without needing an import.
- **`in_array` guard clauses** no longer wipe out the variable's type.
- **Chains through `__call`** keep their type when `__call` returns `$this`, `static`, or `self`.
- **Generic types in hover** for closure parameters, chained `static` returns, and assignments.
- **Callable parameters keep the receiver's generics**, so `fn(Builder $q)` in a `Builder<Product>` chain sees the model's scopes.
- **Nullable `static` returns** resolve to the calling subclass across files.
- **`foreach` over a variadic parameter** gives the element type.
- **Anonymous class variables** resolve through inherited members.
- **`parent::method()` assignments** get the parent's return type.
- **Closure parameters inside `switch` cases and `if` conditions** are inferred.
- **No stack overflow** when a `foreach` value shadows the iterated receiver (`foreach ($category->getBranch() as $category)`).
- **PHPStan `*` wildcards and variance annotations** in generic arguments parse correctly.
- **Interleaved array and property access** like `$results[$i]->activities[$id]->extras` resolves.
- **`\assert()` with a leading backslash** narrows.
- **Array shapes built in branches inside loops** are kept through `foreach`.
- **Self-referential reassignments in nested loops** no longer cause false "type could not be resolved" errors.

#### Generics

- **Nested generics in template binding** (`Wrapper<Collection<T>, V>`) work.
- **`Collection<User>`** binds to the value parameter.
- **`@return TValue|null`** keeps `|null` after substitution.
- **`@mixin T`** pulls in the concrete type's methods.
- **`@property` and `@method`** keep nullable types.
- **Function-level templates with `array`, `iterable`, and `list` wrappers** substitute at call sites.
- **Closure parameters for `array_any` and `array_all`** are inferred from the array.
- **`$this->items` as a templated argument** resolves for binding.
- **Generic arguments flow through transitive `@extends` chains.**
- **Templates as the base of a generic** (`T<int>` with `T` = `Collection<string>`) produce `Collection<int>`.

#### Laravel

- **Eloquent builder and collection types** with generics or nullability resolve for scopes, custom collections, and relationships.
- **Scope methods returning a bare `Builder`** keep the model in the chain.
- **Scope methods on relationship results** appear in completion.
- **`DB::select()`** returns `array<int, stdClass>` and `DB::selectOne()` returns `?stdClass`.
- **Redis connection methods** resolve through the phpredis stubs.

#### Diagnostics

- **Static property access** (`self::$prop`) is not reported as an undefined variable. Contributed by @lucasacoutinho in https://github.com/PHPantom-dev/phpantom_lsp/pull/75.
- **Files without a namespace** get correct "class not found" diagnostics and import actions.
- **Broken chains report only the first missing link.**
- **Vendor files open in the editor** get diagnostics.
- **PHPStan diagnostics are not hidden** by unrelated native ones on the same line.
- **Aliased namespace imports used in attributes** (`#[Assert\Uuid]`) are not reported as unused.
- **Deprecated classes in `implements`** are shown with strikethrough.
- **Callable types inside unions** are parenthesised correctly in hover and completion, e.g. `(Closure(int): string)|Foo`.

#### Editing and navigation

- **Completion no longer triggers on `<?php`.**
- **Update docblock** compares types structurally and no longer proposes needless changes.
- **No crash generating docblocks** on lines with multibyte characters.
- **`@throws` on methods** resolves imported short names.
- **Find References for global classes** no longer includes same-named namespaced classes.
- **`@see` tags in loose docblocks** support go-to-definition.
- **Attributes** on properties, constants, parameters, and enum cases support hover and go-to-definition.
- **Functions imported with `use function`** resolve.
- **Generate getter** uses `isFoo()` for `?bool` properties.

## [0.6.0] - 2026-03-26

### Added

#### Diagnostics

- **PHPStan diagnostics.** PHPStan errors appear inline as you edit, using `vendor/bin/phpstan` or `$PATH`, without blocking native diagnostics. Configure with `[phpstan]` in `.phpantom.toml` (`command`, `memory-limit`, `timeout`). Code actions add or remove inline `@phpstan-ignore` comments.
- **Syntax errors** appear instantly as you type.
- **Missing implementations.** Concrete classes that do not implement all required interface or abstract methods are flagged, with the "Implement missing methods" quick fix alongside.
- **Argument count.** Calls with too few arguments are flagged. Too many arguments is off by default (PHP ignores extras) and can be enabled with `extra-arguments = true` under `[diagnostics]`.

#### Completion and hover

- **Semantic tokens.** Type-aware highlighting gives classes, interfaces, enums, traits, methods, properties, parameters, variables, functions, constants, and template parameters distinct colours, with modifiers for declarations, static, readonly, deprecated, and abstract.
- **Inlay hints.** Parameter names and by-reference markers appear at call sites, except where the argument already makes the parameter obvious.
- **PHPDoc generation.** Typing `/**` above a declaration generates a docblock, adding tags only where they add information beyond the native types, including `@extends`/`@implements` and `@throws` with auto-import.
- **Completion documentation.** The completion popup shows the full signature, description, deprecation notice, and parameter details.
- **Method commit character.** Typing `(` on a highlighted method completion accepts it and starts the argument list.
- **PHPDoc `@var` completion.** Inline `@var` above an assignment sorts first and pre-fills the inferred type, and `@template` parameters enrich `@param`, `@return`, and `@var` suggestions.

#### Editing and navigation

- **Document symbols.** The outline and breadcrumbs show classes, members, and functions with nesting, icons, visibility, and deprecation.
- **Workspace symbols.** "Go to Symbol in Workspace" searches all indexed files, including vendor classes.
- **Type hierarchy.** Browse a class's supertypes and subtypes across files.
- **Code lens.** Methods that override or implement a parent method show a clickable link to the original.
- **File rename on class rename.** Renaming a class in a PSR-4 file also renames the file, when the file holds a single class and the editor supports it.
- **Folding ranges** for class and function bodies, closures, arrays, argument lists, control flow, doc comments, and comment groups.
- **Selection ranges.** Smart expand selection follows the code structure.
- **Document links.** `require`/`include` paths are Ctrl+Clickable, including `__DIR__` and `dirname()` forms.
- **`@see` and `@link`.** `@see` references support go-to-definition, hover shows `@link` and `@see` URLs, and deprecation messages include `@see` targets.
- **Progress indicators** for Go to Implementation and Find References.

#### Code actions

- **Update docblock.** Brings an out-of-date docblock in line with the signature: adds, removes, and reorders `@param` tags, fixes contradicted types, and drops redundant `@return void`, keeping refinements and other tags.
- **Change visibility** of a method, property, constant, or promoted parameter.
- **`@throws` fixes.** Add missing or remove unnecessary `@throws` tags from PHPStan diagnostics, with imports and cleanup handled. The diagnostic clears immediately.

#### Type inference

- **`??` refinement.** If the left side can never be null, the result is just its type. Otherwise `null` is removed from the left side and combined with the right.
- **`@mixin Foo<T>` generics** are substituted into the mixin's members, including through inheritance.

#### Tooling and platform

- **Formatting.** Built-in PER-CS 2.0 formatting works out of the box. Projects with php-cs-fixer or PHP_CodeSniffer in `require-dev` use those instead (both can run in sequence). Configure under `[formatting]` in `.phpantom.toml`.
- **`analyze` command.** `phpantom_lsp analyze` reports PHPantom's diagnostics for a whole Composer project in a PHPStan-like table, optionally limited to a path. Supports `--severity` and `--no-colour`.
- **Classes in `.phar` archives** (such as `phpstan.phar`) are indexed, with no PHP runtime needed. Only uncompressed phars are supported.
- **PSR-0 autoloading** is supported.
- **Global config.** A `.phpantom.toml` in your config directory (typically `~/.config/phpantom_lsp/.phpantom.toml`) provides defaults that project configs override. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/39.
- **Config schema.** A bundled JSON schema enables completion and validation of `.phpantom.toml` in supporting editors. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/38.

### Changed

#### Completion and hover

- **More accurate hover.** Hover uses the same type resolution as completion, so all narrowing applies and only the type visible in the current branch is shown.
- **Cleaner completion labels.** Methods show parameter names and their return type (e.g. `setName($name): User`), properties and constants show their type, and the class detail line moved to the documentation panel.
- **Member completion is grouped by kind**: constants, then properties, then methods.
- **Better class name ranking.** Exact matches come first, then prefix matches, then substring matches, with imported and same-namespace classes first within each group.
- **Smarter `use` completion.** Same-namespace and already-imported classes are left out.
- **Deprecation uses the modern `tags` field.** Rendering is unchanged.
- **Import Class orders candidates** by the namespaces you already import.

#### Behaviour

- **Pull diagnostics.** Editors that support LSP 3.17 pull diagnostics only request them for visible files. Other editors keep the push model.
- **Version-aware stub types.** Built-in signatures match your project's PHP version, removing false positives from outdated types.
- **Leading backslashes no longer break cross-file resolution.**
- **Embedded stubs track upstream master**, picking up fixes and new PHP versions sooner, as PHPStan does.

### Fixed

#### Performance

- **Faster `analyze`.** Single files are up to 5.8× faster and full projects of ~2 500 files up to 10× faster.
- **Faster unknown-member diagnostics** on large files, up to 7×.

#### Editing and navigation

- **Correct positions** in files containing emoji or other supplementary Unicode characters.
- **Parameter rename and references** cover the body and the `@param` tag, from either side. Document highlight is fixed too.
- **Class rename updates imports**, keeping aliases and adding one if the new name collides.
- **Trait alias go-to-definition** jumps to the trait method.

#### Diagnostics

- **`$this`, `self::`, `static::`, and `parent::` inside traits** no longer report missing members.
- **Same-named variables in different methods** are resolved independently.
- **Namespaced constants** like `\PHPStan\PHP_VERSION_ID` are not reported as unknown classes.
- **Diagnostics on the same line are no longer merged.** When PHPantom and PHPStan report the same issue, the more precise one is kept.
- **More accurate checks.** Enums are checked for missing interface methods, scalar access through method chains is detected, and by-reference `@param` tags no longer report unknown classes.
- **Removed PHP functions and classes** (e.g. `mysql_tablename`, `each`) are hidden when your PHP version is newer.

#### Completion and hover

- **Hover on union members** shows every branch that declares the member, deduplicated when they share a declaring class.
- **Hover on inherited members** shows the declaring class.
- **No double parentheses** when completing a call that already has `()` after the cursor.
- **Namespace alias completion** (`OA\Re` with `use OpenApi\Attributes as OA`) suggests classes.
- **Catch completion** includes Throwable interfaces and abstract exceptions.
- **Traits are excluded** from type-hint and PHPDoc type completion.

#### Type inference

- **Constants.** Variables assigned from untyped constants get the type of the constant's value.
- **Reassigned parameters** use their new type afterwards.
- **Variables reassigned inside `foreach`** are visible after the loop.
- **Variable-to-variable assignments** (`$found = $pen`) carry the type.
- **Self-referencing assignments.** In `$request = new Foo(arg: $request->uuid)`, the inner `$request` uses its original type.
- **Variables inside anonymous classes** resolve.
- **Closure and arrow function scope** follows PHP's rules for variable completion.
- **Function return types from other files** resolve imported short names, as do parameter and `@throws` types.
- **Docblock types only override native types** when they are a compatible refinement.
- **PHPStan pseudo-types** like `non-positive-int`, `lowercase-string`, and `callable-object` are recognised.
- **`?ClassName` and `Collection<Item>`** resolve everywhere.
- **Generics through transitive interfaces** are substituted at every level.
- **Templates inside array and object shapes** are substituted through `@extends`.
- **Narrowing distinguishes same-named classes** from different namespaces.
- **Guard clauses** no longer affect later `instanceof` checks on the same variable.
- **`instanceof self`, `static`, and `parent`** narrow in all contexts.
- **Narrowing inside `return` statements** works for `&&` chains and ternaries.
- **Inline array access on method returns** like `$c->items()[0]->getLabel()` resolves.
- **Array shape access.** `$data['name']` and chained `$result['items'][0]` give the right types.
- **Members of ternary and `??` expressions** like `($a ?: $b)->property` resolve.
- **Null-safe method calls** resolve their return type, including across files.
- **`(clone $var)->`** has the same type as `$var`.
- **`self::Active->value`** and similar chains resolve.
- **Inherited methods through deep stub chains** are found.
- **Interface constants through several parent interfaces** are found.
- **No crash on `$numbers['price'] = $numbers['price']->add(...)`.**

#### Laravel

- **`morphedByMany` relationships** produce virtual and `_count` properties.
- **Native property types** are considered when merging virtual properties, so they are not overridden by less specific ones.

## [0.5.0] - 2026-03-12

### Added

#### Diagnostics and code actions

- **Diagnostics.** Unknown classes, members, and functions are flagged. An opt-in unresolved member access diagnostic is available via `.phpantom.toml`.
- **Deprecation support.** `@deprecated` tags and `#[Deprecated]` attributes show in hover, completion (strikethrough), and diagnostics. A quick fix rewrites deprecated calls when a `replacement` template is available.
- **Implement missing methods.** Generates stubs for missing interface or abstract methods.

#### Editing and navigation

- **Find References.** Find every usage of a class, method, property, constant, function, or variable. Variables are scoped to their function or closure, and members to the class hierarchy.
- **Rename.** Rename variables, classes, methods, properties, functions, and constants across the workspace.
- **Document highlighting.** Highlights all occurrences of the symbol under the cursor, distinguishing reads and writes for variables.
- **Reverse go-to-implementation.** Jump from a concrete method to the interface or abstract declaration, and back.
- **Go to Type Definition.** Jump from a variable, property, or call to the class of its type. Union types give several locations.
- **Docblock navigation.** Go-to-definition and hover work on class names inside callable types and array/object shapes.
- **Go-to-definition from parameters and properties** jumps to the type hint's class.

#### Type inference

- **`@implements` generics.** `@implements Interface<ConcreteType>` substitutes types into the interface's members, and `foreach` over generic iterables resolves keys and values.
- **Interface template inheritance.** Implementing classes inherit templates, conditional return types, and assertions from their interfaces.
- **Function-level `@template` in generic return types** resolves from call arguments.
- **`@phpstan-assert` with `class-string<T>`** narrows to the passed class.
- **Property narrowing.** `if ($this->prop instanceof Foo)` narrows the property in branches and after guard clauses.
- **`&&` narrowing.** The right side of `&&` sees the left side's narrowing.
- **Compound negated guards.** `if (!$x instanceof A && !$x instanceof B) { return; }` narrows `$x` to `A|B`.
- **Closure scope isolation.** Outer variables are only offered inside a closure if captured with `use()`.
- **Pipe operator (PHP 8.5).** `$input |> trim(...) |> createDate(...)` resolves through the chain.
- **Array type inference.** Shape keys, element access, spreads, and push-style assignments resolve.
- **`new $classStringVar` and `$classStringVar::method()`** resolve.
- **Invoked closures** like `(fn(): Foo => ...)()` resolve to their return type.
- **PHP version-aware stubs.** The target PHP version is read from `composer.json` and built-in signatures are filtered to match.
- **`@param-closure-this`.** `$this` inside a closure uses the declared type.
- **By-reference parameter types.** A variable passed to a typed `&$var` parameter takes that type.
- **`iterator_to_array()`** resolves the element type from the iterator's generics.
- **Enum case properties.** `$case->name` and `$case->value` resolve.
- **Inline `@var` on promoted constructor properties** overrides the native type.

#### Tooling and platform

- **Project configuration.** `.phpantom.toml` sets the PHP version, diagnostic toggles, and indexing strategy. Run `phpantom --init` to create one.
- **Works without an optimised classmap.** PHPantom scans autoload directories itself when `composer dump-autoload -o` has not been run, and supports non-Composer projects by scanning all PHP files.
- **Monorepo support.** Subdirectories that are separate Composer projects are each handled fully.
- **Non-Composer functions and constants** support completion, go-to-definition, and resolution across files.
- **Indexing progress** is shown in the editor, per subproject in monorepos.
- **`--version` and `--help` flags.** Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/7.

### Changed

- **Resolution engine rewritten on the AST** for more accurate types and navigation.
- **Hover redesigned.** Short names with a `namespace` line, real default values, `@link` URLs, constructor signatures on `new`, template details, enum cases, trait members, origin indicators, and deprecation explanations.
- **Richer signature help.** Compact parameters with native types, per-parameter `@param` descriptions, defaults, and attribute support.
- **Faster resolution and lower memory usage.**
- **Parallel workspace indexing** across all CPU cores, respecting `.gitignore`.
- **Diagnostics in two phases.** Cheap diagnostics (unused imports, deprecation) appear immediately, and expensive ones follow.
- **Classmaps and scanning combined.** Stale Composer classmaps are supplemented by scanning automatically.
- **Automatic stub fetching.** The build downloads phpstorm-stubs when missing, so Composer is no longer needed to build PHPantom. Contributed by @calebdw in https://github.com/PHPantom-dev/phpantom_lsp/pull/16.
- **Feature comparison table corrected.** Phactor capabilities updated in the README. Contributed by @dantleech in https://github.com/PHPantom-dev/phpantom_lsp/pull/10.

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

[Unreleased]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.11.1...HEAD
[0.11.1]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.11.0...0.11.1
[0.11.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.10.0...0.11.0
[0.10.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.9.0...0.10.0
[0.9.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.8.0...0.9.0
[0.8.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.7.0...0.8.0
[0.7.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.5.0...0.6.0
[0.5.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.3.0...0.4.0
[0.3.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.2.0...0.3.0
[0.2.0]: https://github.com/PHPantom-dev/phpantom_lsp/compare/0.1.0...0.2.0
[0.1.0]: https://github.com/PHPantom-dev/phpantom_lsp/commits/0.1.0
