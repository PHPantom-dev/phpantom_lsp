# PHPantom — LSP Features

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## F7. Evaluatable expression support (DAP integration)

**Impact: Low-Medium · Complexity: Low**

Tell debuggers (Xdebug via DAP) which expression sits under the cursor
during a debug session. Given a cursor position, the handler returns
the expression text and range that the debugger should evaluate in the
running PHP process.

LSP has no standard request for this. Phpactor uses a custom
`textDocument/xevaluatableExpression` (advertised as
`experimental.xevaluatableExpressionProvider`), which only its own
client calls. The choice is between a custom method that our VS Code
extension consumes, or the standard 3.17 `textDocument/inlineValue`
request (`InlineValueParams` and `inline_value_provider` are available). Decide that
before implementing.

### Supported expression kinds

- **Variables**: `$var` — return the variable name and its span.
- **Property access**: `$obj->prop`, `$this->prop` — return the full
  member access expression.
- **Array access**: `$arr[0]`, `$arr['key']` — return the full
  subscript expression including brackets.
- **Static property access**: `Foo::$bar` — return the full expression.
- **Parameters**: function/method parameters at declaration sites.

### Why this is cheap

The symbol map already identifies all of these constructs with precise
byte ranges. The handler is a thin layer: look up the `SymbolSpan` at
the cursor position, check that it's a variable, member access, or
subscript expression, and return the source text and range. No type
resolution needed.

### What this enables

When a user is debugging PHP with Xdebug and hovers over `$user->name`
in their editor, the editor asks the LSP "what expression is here?"
and forwards it to the debug adapter for evaluation. Without this
handler, the editor falls back to selecting the word under the cursor,
which gives `name` instead of `$user->name` — useless for the
debugger.

---

## F11. Windows code signing

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | Medium                   |
| **Complexity** | Medium               |

macOS release binaries are signed and notarized
(`.github/workflows/release.yml`), but the Windows packaging step signs
nothing, so SmartScreen flags the binary the VS Code extension
downloads. Add an Authenticode certificate (or Azure Trusted Signing)
and `signtool` to the release workflow.

---

## F12. IntelliJ / PHPStorm plugin

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | High                     |
| **Complexity** | Medium-High          |

Create an IntelliJ plugin that depends on
[LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) and
bundles PHPantom. Publish it to the JetBrains Marketplace. Works in
all IntelliJ-based IDEs (PHPStorm, IntelliJ IDEA, WebStorm, etc.).

### Approach

Fork [clojure-lsp-intellij](https://github.com/clojure-lsp/clojure-lsp-intellij)
(MIT-licensed). It is a Kotlin/Gradle plugin that registers a
language server via lsp4ij's `com.redhat.devtools.lsp4ij.server`
extension point. Strip the Clojure-specific parts and replace them
with PHPantom:

- Register PHPantom as the language server in `plugin.xml`.
- Map the `PHP` language and file type via
  `com.redhat.devtools.lsp4ij.languageMapping`.
- Bundle or auto-download the PHPantom binary.
- Add a settings page for the binary path and any PHPantom-specific
  options.

### Scope

1. **`plugin.xml` registration.** Server definition, language
   mapping, file type mapping (`.php`, `.phtml`, `.inc`).
2. **Binary management.** Auto-download from GitHub Releases on
   first run, with a manual path override in settings.
3. **Settings UI.** Binary path, PHP version override, diagnostic
   toggles.
4. **JetBrains Marketplace listing.** Icon, description, plugin
   compatibility range (2024.2+, matching lsp4ij's requirement).
5. **CI.** GitHub Actions workflow using `gradlew buildPlugin` and
   `gradlew publishPlugin`.

### Why not use the built-in IntelliJ LSP API

IntelliJ's native LSP support (since 2023.2) is only available in
Ultimate editions and is still limited in capability. LSP4IJ is free,
works in all editions (including Community), and supports a broader
set of LSP features. Using lsp4ij also means the plugin works in
IntelliJ IDEA (for PHP projects opened there) and other JetBrains
IDEs, not just PHPStorm.

---

## F14. Helix upstream PR

| Field      | Value                    |
| ---------- | ------------------------ |
| **Impact** | Low-Medium               |
| **Complexity** | Low                  |

Submit a PR to the [Helix editor](https://github.com/helix-editor/helix)
adding `phpantom_lsp` as a language server option in the default
`languages.toml`.

### Change

Add a `phpantom` server definition and include it in the `php`
language entry (alongside `intelephense`):

```toml
[language-server.phpantom]
command = "phpantom_lsp"

# In the [[language]] entry for php, add "phpantom" to language-servers.
```

### Prerequisites

- Helix maintainers can point users at `brew install phpantom-lsp`
  (the formula is in homebrew-core).
- Helix maintainers may want a brief README section documenting the
  server and its feature set.

---

## F16. On-type `}` brace de-indent

**Impact: Low · Complexity: Low**

Extend the existing on-type formatting handler (currently triggered on
`\n` for docblock generation) to also trigger on `}`, automatically
de-indenting the closing brace to match its opening `{`.

### Behaviour

When the user types `}`:

1. From the `}` position, scan backward through the document text to
   find the matching `{` (tracking brace depth, skipping strings and
   comments).
2. Read the indentation of the line containing the matching `{`.
3. If the `}` line has more indentation than the `{` line, return a
   `TextEdit` that replaces the leading whitespace on the `}` line
   with the `{` line's indentation.

This is a pure text-based operation — no AST needed. Register `}` as
an additional `on_type_formatting_trigger_character` alongside the
existing `\n`.

---

## F17. Wire class move to `workspace/willRenameFiles`

**Impact: Medium · Complexity: Medium**

Renaming a class's FQN via `textDocument/rename` already moves the file
and rewrites references across the project: renaming a class's
declaration accepts the full FQCN so it can move between namespaces in
one step, and renaming a namespace segment rewrites every affected
`namespace` declaration, `use` statement, and FQN reference while
moving the PSR-4 directories to match (see `build_class_move_edit` in
`src/rename/class/` and `build_namespace_rename_edit` in
`src/rename/namespace/`). What's still missing is the editor-triggered
path: when the user renames or moves a PHP file in the editor's file
tree (rather than through the LSP rename command), nothing updates the
file's `namespace` declaration or the workspace's `use` imports.

Wire the existing move logic to `workspace/willRenameFiles` (declared
via server capabilities `workspace.fileOperations.willRename`): on a
file-tree rename/move, recompute the namespace from the destination
path using the PSR-4 autoload map, and reuse the same reference-rewrite
machinery to produce the `WorkspaceEdit`. The companion
`workspace/willCreateFiles` can then insert a PSR-4-derived `namespace`
+ class stub into newly created files.

**References:**
- Phpactor: `MoveClass` refactoring in the class-mover package.

---

## F22. Merge a namespace onto one that shares a class name

**Impact: Low-Medium · Complexity: Medium-High**

Renaming `App\Internal` to an `App\Support` that already exists now
merges: the files move into the existing directory one at a time
instead of the source directory being renamed on top of the
destination. Where the two namespaces declare the same class name
(`App\Internal\Helper` and `App\Support\Helper` both exist), the whole
rename is refused with a message naming the clash, because the merge
has no well-defined answer for that name — see
`namespace_merge_conflict` in `src/rename/namespace/layout.rs`.

Refusing is the safe answer, not the desirable one: a user merging
twenty files should not be blocked by one clash. What is missing is a
partial merge that moves everything except the clashing names and
leaves those behind *intact*. "Intact" is the hard part and the reason
this is deferred rather than done:

- The clashing class's file must keep its own `namespace` declaration,
  so that file's namespace-declaration edit has to be suppressed while
  its siblings' are kept. Today the namespace rename rewrites by
  string prefix over raw text with no per-class granularity.
- Every reference to the clashing FQN (`use App\Internal\Helper;`,
  `\App\Internal\Helper`, and unqualified `Helper` inside the old
  namespace) must be left pointing at the old name. Rewriting them is
  what makes a naive partial merge worse than a refusal: the reference
  silently resolves to the *other* class.
- A file holding both a clashing class and a non-clashing one cannot
  half-move, so it has to be reported as a clash too.
- The classes that do move out of the old namespace stop being
  namespace-siblings of the ones left behind, so the files left behind
  need `use` imports added for them — the same rule
  `build_class_move_edit` already applies for a single class move.

The refusal message should then list only the names that could not
move, and the response should still carry the edits for everything
that could.

**Where to look:** `src/rename/namespace/mod.rs`
(`build_namespace_prefix_rename_edit`), `src/rename/namespace/layout.rs`
(`namespace_merge_conflict`, `collect_merge_move_ops`),
`src/rename/class/mod.rs` (the import-adding rule to mirror).

---

## F23. Rename a class through its XML entity-escaped occurrences

**Impact: Low · Complexity: Medium**

Rename rewrites a class, controller action, or namespace in YAML and XML
files in the spelling each occurrence uses, doubled YAML backslashes
included (`collect_framework_name_edits` in `src/framework.rs`). An XML
attribute can also spell the separator as a character reference,
`App&#92;Handler&#92;Run`, and neither the resource scanner nor the
framework index decodes those, so such an occurrence is not found by
Find References and is left behind by a rename.

The scanners would need to decode `&#92;`/`&#x5C;` while tokenising,
record where each decoded character came from, and re-encode the
replacement the same way when the rename writes it back.

**Where to look:** `src/resource_navigation.rs` (`scan_symbols`,
`normalize_fqn`), `src/framework.rs` (`scan_class_like_tokens`,
`rewrite_framework_fqn_literal`).

---

## F24. Doctrine lenses miss attribute mappings and generic repositories

**Impact: Medium · Complexity: Medium**

```php
#[ORM\Entity(repositoryClass: AccountStore::class)]
class Account {}

/** @extends ServiceEntityRepository<Account> */
class AccountStore extends ServiceEntityRepository {}
```

The CodeLens that links a Doctrine entity and its repository finds the
pair through a YAML/XML mapping or the naming convention
(`App\Entity\User` and `App\Repository\UserRepository`). The modern
form above is not read in either direction. Doctrine ORM 3 dropped YAML
mappings, so `#[ORM\Entity(repositoryClass: ...)]` is how most projects
name a repository, and the repository's own
`@extends ServiceEntityRepository<Account>` (or the class it passes to
`parent::__construct()`) names its entity. A pair that breaks the naming
convention gets no lens.

The attribute needs reading at parse time, with its class name resolved
the way the parser resolves `extends`, into `ClassInfo` (a field every
class pays for, or a slot in an optional per-class box). The
entity-to-repository direction can then read the entity's `ClassInfo`,
and the repository-to-entity direction the first generic argument of
its `ServiceEntityRepository`/`EntityRepository` ancestor in
`extends_generics`. T45 needs the same attribute.

**Where to look:** `src/framework.rs`
(`doctrine_repository_fqns_for_entity`,
`doctrine_entity_fqns_for_repository`), `src/parser/classes/class_likes.rs`
(`extract_class`), `src/parser/attributes.rs`.

---

## F25. Names a PHP configurator spells as strings

**Impact: Medium · Complexity: Medium**

```php
return static function (ContainerConfigurator $container): void {
    $container->services()->load('App\\Service\\', '../src/Service/');
};

return static function (RoutingConfigurator $routes): void {
    $routes->add('legacy', '/legacy')
        ->controller('App\\Controller\\DemoController::legacy');
};
```

The same names in `services.yaml` and `routes.yaml` take part in rename and
navigation: a namespace-prefix key follows a namespace rename, a
`resource:` path follows its directory, and a `Class::method` controller
string navigates and renames with the method. Written in a PHP
configurator they are plain strings. `load()`'s namespace prefix and
resource path, and a controller given as one `Class::method` string, need
reading from the AST where the receiver is confirmed as a configurator
(`TypedReceiverSite`), the way the container's service IDs are, and
feeding to the framework index the YAML occurrences are kept in. Sidux's
#398 did this by scanning the PHP text.

**Where to look:** `src/framework.rs` (`FrameworkReference`,
`collect_framework_name_edits`), `src/symbol_map/extraction/symfony.rs`,
`src/blade/typed_receiver.rs`.

---

## F26. Symfony service IDs a bundle or a definition API declares

**Impact: Low-Medium · Complexity: Medium**

Service IDs and container parameters are read from the application's YAML,
XML, and PHP configurators. A bundle's own declarations, and the ones a
kernel or an extension makes through the `ContainerBuilder` API
(`register('app.mailer', …)`, `setAlias()`, `setParameter()` on a builder,
`setDefinition()`), are not, so those names neither complete nor navigate,
and only names under the application's own `app.` prefix can be reported
as undeclared. Two reading shapes are missing too: a configurator's
`->parent('app.base')`, and the `%name%` placeholders in a string a
configurator passes as an argument (`->args(['%app.sender%'])`).

Reading the installed bundles' `Resources/config/` would also let the
unknown-ID check judge names outside `app.`; it has to stay cheap enough
for a project with dozens of bundles.

**Where to look:** `src/symbol_map/extraction/symfony.rs`
(`symfony_receiver_method`), `src/symbol_map/string_keys.rs`
(`ReceiverClass`), `src/symfony/`.

---

## F27. Symfony route parameters

**Impact: Medium · Complexity: Medium**

Blocks full Symfony support: the README keeps Symfony marked as partial until this lands.

```php
#[Route('/post/{slug}', name: 'blog_post')]
public function show(string $slug): Response { … }

$this->generateUrl('blog_post', ['slug' => $post->getSlug()]);
```

```twig
{{ path('blog_post', {slug: post.slug}) }}
```

Route names complete and navigate, but the parameters a route's path
declares do not: the `slug` key above neither completes from `{slug}` nor
navigates to it. Laravel completes the keys of `route('name', [...])` from
the URI it keeps with each route name (`completion/laravel_route_params.rs`).
A parameter only means something under its route, so it does not fit a
`StringKey` span on its own: a key spelled as one global name would merge
every route's `id`. It needs the path kept beside each route declaration
(the attribute's path, the YAML `path:`, the XML `path` attribute, the
configurator's second argument, and the class-level prefix in front of a
method's path), and the parameter array of each `generateUrl()`,
`redirectToRoute()`, `generate()`, `path()`, and `url()` call read against
it.

**Where to look:** `src/symbol_map/extraction/symfony.rs`
(`emit_symfony_route_spans`), `src/symfony/routes.rs`,
`src/completion/laravel_route_params.rs`.

---

## F28. Symfony route names a loader composes

**Impact: Low-Medium · Complexity: Medium**

A route's name is read where it is declared: a `#[Route]` attribute, with
its class's name prefix, a routing YAML or XML entry, or a routing
configurator's `add()`. A few names are put together elsewhere, and are
missed:

- an import's `name_prefix:` in the routing YAML (or `->namePrefix()` in a
  configurator), which prefixes every route it loads, attribute routes
  included;
- the routes a collection's `add()` declares under its
  `$routes->collection('blog_')` prefix;
- the `@Route` docblock annotations older applications still use;
- the routes bundles declare in their own routing files, which is also why
  only names under the application's own `app_` prefix are reported as
  undeclared.

**Where to look:** `src/symfony/routes.rs`,
`src/symbol_map/extraction/symfony.rs`, `src/symbol_map/string_keys.rs`
(`ReceiverClass::RoutingConfigurator`).

---

## F29. Report Symfony translation keys nothing declares

**Impact: Medium · Complexity: Medium**

Blocks full Symfony support: the README keeps Symfony marked as partial until this lands.

```php
$translator->trans('demo.titel'); // a typo of demo.title
```

Translation keys complete and navigate within their domain, but one that no
catalogue declares is not reported. Service IDs and routes are only judged
under the application's own `app.` and `app_` prefixes; translation keys
have no such convention, so judging needs a fuller picture of what is
declared:

- the catalogues installed bundles ship (`validators`, `security`, and any
  bundle's own domain), which a key in the same domain may come from;
- the project's messages that are their own translation (`trans('Save')`
  with no catalogue entry translates to `Save`), which only a domain the
  project declares keys in, and keys that look like keys, can tell apart.

A starting point is to judge only domains no installed package ships a
catalogue for, and only keys spelled like dotted identifiers.

**Where to look:** `src/diagnostics/laravel_string_keys.rs`
(`CheckedStringKind::of`), `src/symfony/translations.rs`.

---

## F30. Symfony translations outside YAML, XLIFF, and the `trans` filter

**Impact: Low-Medium · Complexity: Medium**

Catalogues are read when they are YAML or XLIFF files under a
`translations/` directory, and keys are read from a translator's `trans()`,
`TranslatableMessage`, `t()`, and Twig's `trans` filter. These are missed:

- PHP catalogues (`translations/messages.en.php` returning an array). They
  are PHP, so they have to be read from the AST during symbol-map
  extraction, which does not know the file's path today. The array walk
  can share `virtual_members/laravel/array_file.rs` (`for_each_entry`,
  `dotted_key`) with Laravel's `trans_keys.rs`.
- JSON, CSV, INI, and the other formats Symfony loads, and catalogue
  directories configured under `framework.translator.paths`.
- The catalogues installed bundles ship, so their keys neither complete
  nor navigate.
- Twig's `{% trans %}…{% endtrans %}` block and its `from 'domain'`
  clause.
- Keys in other domains by convention: validation constraint messages
  (`validators`) and form labels under a form's `translation_domain`.

**Where to look:** `src/symfony/translations.rs`,
`src/symbol_map/extraction/symfony.rs` (`push_translatable_spans`).

---

## F31. Symfony Twig templates outside `templates/`

**Impact: Low-Medium · Complexity: Medium**

A template is known by its path under the project's `templates/`
directory, and a bundle override under `templates/bundles/AcmeBundle/` by
its `@Acme/` name as well. These are missed:

- the directories `config/packages/twig.yaml` adds or moves the default
  one to (`default_path:`, and `paths:` with or without a namespace);
- a project's own bundle's templates (`src/Resources/views/`, or a
  bundle's `templates/`);
- the templates installed bundles ship, both under their `@Twig`-style
  namespace and the ones registered without one, such as the form themes
  `twig-bridge` provides (`bootstrap_5_layout.html.twig`), so their names
  neither complete nor navigate.

A name under a namespace is never reported as unknown, but one in an
extra directory `paths:` adds without a namespace is, so reading
`twig.yaml` matters most.

**Where to look:** `src/twig/roots.rs` (`TwigRoots::discover`,
`TwigRoots::template_names`).

---

## F59. Send Twig, YAML, and XML documents to the server from Zed

**Impact: Medium · Complexity: Low**

The Zed extension sends the server nothing but PHP and Blade, so
everything PHPantom reads inside a Twig template or a YAML or XML file
(template names, route names, translation keys, service IDs, parameters,
configuration keys) never reaches a Zed user. Those files are still
indexed from disk, so the PHP side of each name works, but completion,
go-to-definition, references, and rename started from the file itself do
not. The server already answers only those requests in such a file, and
the VS Code extension sends them.

Joining those languages has to add to what the user already has, never
replace it: the file keeps its language, and the Twig, YAML, and XML
extensions keep serving it. Taking a file over (mapping `.html` to Twig
with Zed's `file_types`, say) trades the HTML, CSS, and JS support it had
for the one syntax PHPantom answers, and is not an option.

Open a pull request to `zed-extensions/php` that adds `Twig`, `YAML`,
and `XML` to `languages` under `[language_servers.phpantom]` in
`extension.toml`, with `language_ids` (`twig`, `yaml`, `xml`). Those
languages come from other extensions, so first confirm that Zed starts
the server for a language another extension defines, and check that
Zed does not offer PHPantom as the formatter of those files: the server
advertises formatting statically and answers nothing there, which Zed
may still treat as a formatter that applies. A YAML file in a project
without PHP should not start a server either.

**Where to look:** the Zed PHP extension's `extension.toml`; the VS Code
extension's `src/client.ts` for which requests a resource document is
registered for.

---

## F32. Report unknown Symfony names in Twig templates

**Impact: Medium · Complexity: Medium**

Blocks full Twig support: the README keeps Twig marked as partial until this lands.

```twig
{% include 'partials/crad.html.twig' %}
<a href="{{ path('app_hme') }}">Home</a>
```

An unknown template or `app_` route is reported where PHP names it, but not
in a Twig template: diagnostics are not run on resource documents at all.
They need scheduling for a Twig file the way a PHP file's are, the
string-key check reading the template's resource symbol map, and the
"Create missing template" quick fix offered there too. A template being
created or deleted should also refresh the open files that name it.

**Where to look:** `src/backend/documents.rs` (`did_open`, `did_change`
for resource documents), `src/diagnostics/laravel_string_keys.rs`,
`src/code_actions/create_missing_view.rs`.

---

## F58. Configured Twig roots and namespaces

**Impact: Low-Medium · Complexity: Medium**

Twig roots are discovered from the names templates reference
(`src/twig/roots.rs`), which cannot find everything:

- **Namespaced names.** `{% include '@admin/menu.html' %}` names a root
  through a namespace the loader registers at runtime
  (`$loader->addPath($dir, 'admin')`), and the namespace says nothing
  about which directory it maps to.
- **Roots no template references into.** A directory whose templates
  are only ever rendered from PHP, and never extended or included by
  another template, is not discovered, so its names neither navigate nor
  count as known.

Where the loader's arguments are literals (`new FilesystemLoader(__DIR__
. '/views')`, `addPath('templates/admin', 'admin')`), read them
statically first. For the rest, add a `[twig]` section to
`.phpantom.toml`: `paths` (highest precedence first) and `namespaces`
(`admin = "templates/admin"`), merged with the discovered roots.

**Where to look:** `src/config.rs`, `src/twig/roots.rs`
(`TwigRoots::discover`), `src/twig/mod.rs` (`TwigState`).

---

## F34. Template names a PHP wrapper builds

**Impact: Medium · Complexity: Medium-High**

```php
public function render(string $template = 'index', array $data = []): string
{
    return $this->twig->render($template . '.html', $data);
}

app(RenderService::class)->render('product', $data);
```

Applications and frameworks rarely call Twig with a literal: they pass a
short name to a wrapper that adds the extension (or a theme directory)
and renders it. The literal at the call site names a template, but
nothing says so. The wrapper's parameter needs marking as a template
name, together with how the wrapper completes it (the suffix it appends,
the directory it prepends). That could be a docblock type in the spirit
of Laravel's `view-string`, or a `.phpantom.toml` entry for wrappers a
project does not own (OpenCart's `Loader::view()`).

**Where to look:** `src/symbol_map/extraction/symfony.rs`
(`symfony_receiver_method`), how `view-string` parameters are read for
Laravel views. TwigStan's context collectors (the ones that find
`render()` calls and `#[Template]` returns) show which call shapes it
recognises as rendering a template.

---

## F35. Suggest a close name for an unknown template or view

**Impact: Low-Medium · Complexity: Low**

```php
return $this->render('blog/shw.html.twig');   // blog/show.html.twig exists
return view('emails.welcom');                  // emails.welcome exists
```

An unknown Twig template or Laravel view offers only "Create missing
template/view", which is the wrong fix for a typo. Offer the closest
declared names first ("Change to 'blog/show.html.twig'"), ranked by edit
distance and limited to a few, the way Symfony's language tools do. Both
kinds already go through one collector, so one change serves both.

**Where to look:** `src/code_actions/create_missing_view.rs`.

---

## F36. Twig block navigation

**Impact: Medium · Complexity: Medium**

Blocks full Twig support: the README keeps Twig marked as partial until this lands.

```twig
{% extends 'base.html.twig' %}
{% block body %}…{% endblock %}
```

A `{% block %}` in a child template overrides the block of the same name
in the template it extends, possibly several levels up, and `{{ block('x') }}`
and `{{ parent() }}` refer to them. None of that navigates. It is the Twig
counterpart of Blade's section and stack navigation (`blade/blocks.rs`),
and the `extends` links are already `TwigTemplate` spans, so it needs
the blocks each template declares and a resolver that walks up the
chain: go to the overridden block, find the overrides of a parent's
block, and complete the block names a child can override.

**Where to look:** `src/twig/templates.rs`, `src/blade/blocks.rs`,
`src/blade/block_index.rs`.

---

## F37. Twig template variables

**Impact: Medium · Complexity: Medium-High**

Blocks full Twig support: the README keeps Twig marked as partial until this lands.

```php
return $this->render('post/show.html.twig', ['post' => $post]);
```

```twig
{{ post.title }}
```

The variables a template receives come from the keys every `render()`
of it passes, the names a `#[Template]` action returns, Twig's globals,
and the `{% types %}` tag. Completing and hovering those names is feasible
from the call sites alone, the way Blade's call-site inference collects a
template's variables (`blade/call_site_inference.rs`). Typing what follows
(`post.title` as the `Post` class's `getTitle()`) is a much larger
project: TwigStan does it by compiling templates to PHP and analysing
that. Start with the names; decide on types separately.

**Where to look:** `src/blade/call_site_inference.rs`,
`src/symbol_map/extraction/symfony.rs` (the render sites).

---

## F38. Twig components

**Impact: Low-Medium · Complexity: Medium-High**

```twig
<twig:Alert type="success" message="Saved" />
{{ component('Alert', {type: 'success'}) }}
```

With `symfony/ux-twig-component`, a `<twig:Name>` tag or `component()`
call names a component class (`#[AsTwigComponent]`) and its template,
and its attributes name the class's public properties. None of it
completes or navigates. It is the counterpart of Blade's `<x-…>`
components, so check what `blade/component_tags/` and `blade/discovery.rs`
can share before writing a second implementation.

**Where to look:** `src/blade/component_tags/`, `src/blade/discovery.rs`,
`src/twig/templates.rs`.

---

## F39. Report unknown Symfony configuration keys

**Impact: Medium · Complexity: Medium-High**

```yaml
# config/packages/acme_demo.yaml
acme_demo:
    mailer:
        dns: smtp://localhost # a typo of dsn
```

A key the project's own `TreeBuilder` schema declares completes and
navigates, but one it does not declare is not reported. Judging needs the
schema's open nodes, which the walk does not record today: a
`variableNode()` and a prototyped node take any key, `ignoreExtraKeys()`
lets an array node take any, `canBeEnabled()` and `canBeDisabled()` add an
`enabled` key no string names, and `fixXmlConfig()` lets the YAML spell a
key in its plural. A key is only worth judging under a root a project
schema declares, and only once its parent is a closed array node the schema
fully describes. Diagnostics are not run on resource documents yet either
(see F32).

**Where to look:** `src/symfony/config_tree.rs` (`tree_builder_keys`),
`src/diagnostics/laravel_string_keys.rs` (`CheckedStringKind::of`).

---

## F40. Symfony configuration schemas the `TreeBuilder` walk misses

**Impact: Low-Medium · Complexity: Medium**

Keys are declared by a chain of node builders in a
`getConfigTreeBuilder()`, and read from YAML under `config/packages/`.
These are missed:

- the keys under a prototyped node (a map of named entries), whose path
  holds a name the configuration chooses;
- a bundle's schema in `AbstractBundle::configure(DefinitionConfigurator
  $definition)`, whose root is the bundle's extension alias;
- nodes a helper method builds and `append()` adds, and node definitions
  held in a variable between statements;
- installed bundles' schemas (`framework`, `doctrine`, `twig`), so their
  keys neither complete nor navigate;
- configuration written in PHP (`config/packages/*.php`) or XML.

**Where to look:** `src/symfony/config_tree.rs`, and PHPStan's Symfony
extension (`Type/Symfony/Config/`), which tracks the same parent each
builder's `end()` returns.

---

## F41. Symfony forms and validation beyond a form type's own data class

**Impact: Low-Medium · Complexity: Medium**

A form field navigates to the property of the `data_class` its form type's
`configureOptions()` sets, and a validation mapping's `properties:` to the
class's properties. These are missed:

- a `data_class` a parent form type sets, or one passed to `createForm()`
  in its options;
- a field the property accessor reaches through a getter, an isser, or a
  hasser rather than a property, which navigates nowhere;
- the fields of a child builder, `$builder->get('address')->add('street')`,
  which belong to the child's data class;
- a validation mapping's `getters:`, which name the `get`, `is`, or `has`
  method they read by its property-like name;
- in YAML, the constraints another constraint's options hold (`All`,
  `Sequentially`, `AtLeastOneOf`), and constraints under a `namespaces:`
  alias.

**Where to look:** `src/symfony/forms.rs`, `src/symfony/validation.rs`.

---

## F42. Symfony console arguments and options

**Impact: Medium · Complexity: Medium**

```php
protected function configure(): void
{
    $this->addArgument('name', InputArgument::REQUIRED)
        ->addOption('dry-run', null, InputOption::VALUE_NONE);
}

protected function execute(InputInterface $input, OutputInterface $output): int
{
    $input->getArgument('nmae');   // a typo, not reported
    $input->getOption('dry-run');  // mixed, should be bool
}
```

A command declares its arguments and options in `configure()`, or as the
`#[Argument]` and `#[Option]` parameters of an invokable command's
`__invoke()`. The names `getArgument()`, `getOption()`, `hasArgument()`,
and `hasOption()` take neither complete nor navigate, an undefined one is
not reported, and the value is `mixed` where the declaration settles it:
`bool` for `VALUE_NONE`, an array for `IS_ARRAY`, and nullable unless the
argument is required or the option has a default. PHPStan's Symfony
extension does all of this from a booted console application; the command's
own source carries it. It also checks a default value against its mode.

This is Laravel's Artisan signature support over again (`CommandOwnParam`
spans), so share the span kinds and the completion with it rather than
building a second set.

The extension's fixtures are `tests/phpstan_symfony/data/Example*Command.php`
for the types and the `tests/fixtures/phpstan_symfony/` fixtures on
`rules/Symfony/ExampleCommand.php` for the checks.

**Where to look:** `src/symbol_map/` (`CommandOwnParam`),
`src/virtual_members/laravel/` (the Artisan signature parsing), and
PHPStan's Symfony extension (`Rules/Symfony/Undefined*Rule`,
`Rules/Symfony/Invalid*DefaultValueRule`,
`Type/Symfony/InputInterface*Extension`).

---

## F43. Doctrine entity fields in repository calls

**Impact: Medium · Complexity: Medium-High**

```php
$users->findByEmail($email);                    // unknown method
$users->findBy(['emial' => $email], ['name' => 'ASC']); // a typo, not reported
```

An entity's repository answers `findBy<Field>()`, `findOneBy<Field>()`, and
`countBy<Field>()` for each of the entity's fields, and its `findBy()`,
`findOneBy()`, `count()`, and `findBy()`'s `$orderBy` take field names as
array keys. None of the magic methods resolve, and the keys neither
complete, navigate, nor get checked. The magic methods are virtual members
of the repository, the way Eloquent's are, which needs the repository's
entity (T45). The keys are member references to the entity's mapped
properties, the same member spans a form field and a validation mapping
record.

The fields come from the entity's mapping: `#[ORM\Column]`,
`#[ORM\Id]`, and the relation attributes, or the XML mapping the CodeLens
already reads.

The extension's fixtures are `magic_repository_method_call.fixture` and
`repository_method_call.fixture` in `tests/fixtures/phpstan_doctrine/`.

**Where to look:** `src/virtual_members/`, `src/framework.rs` (the
Doctrine mapping index), and PHPStan's Doctrine extension
(`Reflection/Doctrine/EntityRepositoryClassReflectionExtension`,
`Rules/Doctrine/ORM/RepositoryMethodCallRule`).

---

## F44. Doctrine DQL and query builder strings

**Impact: Medium · Complexity: High**

```php
$em->createQuery('SELECT u FROM App\Entity\User u WHERE u.emial = :e');
$qb->select('u')->from(User::class, 'u')->where('u.email = :e')->getQuery()->getResult();
```

DQL names entity classes and their fields in strings, directly or through
a query builder's `select()`, `from()`, `join()`, and `where()`. Nothing in
them completes or navigates, an unknown entity or field is not reported,
and `getResult()` is `mixed` where the query settles it (`list<User>`, or
an array shape for a scalar select). PHPStan's Doctrine extension parses the
DQL with Doctrine's own parser and the booted metadata; PHPantom would need
a DQL reader of its own over the static mapping, with the query builder's
chain followed the way T45's repository is.

Navigation and completion of the entity and field names are the cheaper
half and the one an editor user notices; result types follow. The extension
also types a query as `Query<TKey, TResult>`, so `getResult()` and its
siblings follow from a `Query` passed in with its result type, and reads
the expression builder's methods (`expr()->isNull('u.email')`) as the DQL
fragment they build.

The extension's fixtures are `tests/phpstan_doctrine/data/QueryResult/`
for the types and the `dql*.fixture` and `query_builder_dql*.fixture`
fixtures in `tests/fixtures/phpstan_doctrine/` for the checks.

**Where to look:** `src/framework.rs` (the Doctrine mapping index), and
PHPStan's Doctrine extension (`Rules/Doctrine/ORM/DqlRule`,
`QueryBuilderDqlRule`, `Type/Doctrine/Query/`, `Type/Doctrine/QueryBuilder/`).

---

## F45. Symfony events named by their class or a constant

**Impact: Medium · Complexity: Medium**

```php
$dispatcher->dispatch(new OrderPlaced($order));   // which listeners?

#[AsEventListener]
public function onOrderPlaced(OrderPlaced $event): void {}

public static function getSubscribedEvents(): array
{
    return [KernelEvents::REQUEST => 'onRequest'];
}
```

An event name written as a string navigates between its dispatchers and
its listeners. Most events are not: an event dispatched without a name is
named by its class, an `#[AsEventListener]` without an `event:` subscribes
to the class its method's first parameter is typed with, and a
subscriber's map is usually keyed by `OrderPlaced::class` or a constant
such as `KernelEvents::REQUEST`. None of these link a dispatch to its
listeners. Symfony's own language server shows a CodeLens over an event
class listing its listeners, and one over a listener listing its events,
the way the Messenger lenses link a message and its handlers.

A constant key resolves to the string it holds, which also gives
completion the framework's own events (`kernel.request`,
`console.command`, `security.interactive_login`): only the names a
project's listeners subscribe to are offered today, since nothing the
installed bundles declare is read.

**Where to look:** `src/symbol_map/extraction/symfony.rs`
(`emit_symfony_listener_spans`), `src/symfony/messenger.rs` (the handler
side table to mirror), and `references/symfony-language-tools`
(`docs/features/events.rst`).

---

## F46. Symfony listener methods a tag or a default names

**Impact: Low-Medium · Complexity: Low-Medium**

The method an `#[AsEventListener(method: …)]` or a subscriber's map names
navigates to the listener's method, and one that does not exist is
reported. Two other ways of naming it do not: the `method` of a
`kernel.event_listener` tag in YAML or XML, which names a method of the
tagged service's class, and no method at all, where Symfony calls
`on` followed by the event name in camel case, else `__invoke()`. The
second also leaves a listener whose method name is never written
anywhere looking unused.

**Where to look:** `src/symfony/tags.rs` (the tag scan, which already
reads the `event` beside the `method`), `src/resource_navigation.rs` (the
`Class::member` spans a resource map already holds).

---

## F47. Symfony Messenger transports

**Impact: Low-Medium · Complexity: Medium**

```yaml
framework:
    messenger:
        failure_transport: failed
        transports:
            async: '%env(MESSENGER_TRANSPORT_DSN)%'
            failed: 'doctrine://default?queue_name=failed'
        routing:
            App\Message\SendWelcomeEmail: async
```

A bus name navigates, completes, and is reported when nothing declares
it. A transport name does none of this, in `routing`, `failure_transport`,
a transport's own `failure_transport`, `#[AsMessageHandler(fromTransport:
…)]`, and `#[AsMessage(transport: …)]`. A transport, like a bus, is only
ever declared by the application's Messenger configuration, so an unknown
one can be reported the same way.

Two smaller gaps sit beside it. A bus declared in a PHP configuration file
(`$framework->messenger()->bus('command.bus')`) is not read, so a project
that configures Messenger in PHP gets no bus names at all. And a handler
whose `handles:` names a message its method's parameter cannot accept is
not reported.

**Where to look:** `src/symfony/messenger.rs`, and
`references/symfony-language-tools` (`docs/features/messenger.rst`).

---

## F48. Symfony environment variables

**Impact: Medium · Complexity: Low-Medium**

```yaml
parameters:
    app.secret: '%env(APP_SECRET)%'
    app.port: '%env(int:DATABASE_PORT)%'
```

A Laravel `env('APP_NAME')` completes, jumps to the line of `.env` that
declares it, and lists its references. The `%env(…)%` placeholders a
Symfony project reads its environment through, in YAML, XML, PHP, and
Twig configuration, do none of this: the `ContainerParameter` scan skips
them. The name is the last segment after the processors (`int:`,
`json:`, `default:fallback:`), and Symfony also reads `.env.local`,
`.env.<environment>`, and `.env.<environment>.local`.

Reuse the `Env` string-key kind and its `.env` reader rather than adding
a second one.

**Where to look:** `src/symfony/resources.rs`
(`scan_parameter_placeholders`), `src/virtual_members/laravel/` (the
`.env` reader), and `references/symfony-language-tools`
(`docs/features/environment.rst`).

---

## F49. Symfony security names

**Impact: Low-Medium · Complexity: Medium**

The firewalls and user providers `security.yaml` declares, the provider a
firewall names, and the roles `#[IsGranted]`, `denyAccessUnlessGranted()`,
`isGranted()`, and Twig's `is_granted()` check neither complete nor
navigate. Firewalls and providers are declared only by the application's
security configuration, so an unknown one can be reported. Roles cannot
be: an application can grant any role from code. They can still complete
and navigate from the `role_hierarchy` and `access_control` that name
them.

**Where to look:** `src/symfony/`, and `references/symfony-language-tools`
(`docs/features/security.rst`).

---

## F50. Twig filters and functions

**Impact: Medium · Complexity: Medium**

Blocks full Twig support: the README keeps Twig marked as partial until this lands.

A project's Twig extensions declare filters and functions with `new
TwigFilter('price', …)` and `new TwigFunction('icon', …)` in
`getFilters()` and `getFunctions()`, or with `#[AsTwigFilter]` and
`#[AsTwigFunction]` on a method. A template's `{{ amount|price }}` and
`{{ icon('x') }}` neither complete, navigate to the PHP method behind
them, nor show its signature on hover, and an unknown one is not
reported. Twig's own filters and functions come from its core extension,
which is read the same way.

This applies to Twig outside Symfony too.

**Where to look:** `src/twig/templates.rs` (the Twig scan), and
`references/symfony-language-tools` (`docs/features/twig-callables.rst`)
and `references/twigstan`.

---

## F51. Class names in Twig's `constant()` and `enum()`

**Impact: Low · Complexity: Low-Medium**

```twig
{% if user.status == constant('App\\Entity\\User::ACTIVE') %}
{% for case in enum_cases('App\\Enum\\Status') %}
```

The class, constant, and enum case Twig's `constant()`, `enum()`, and
`enum_cases()` name as strings neither complete nor navigate. They are
ordinary class and member references once read, the same spans the
resource maps record for a `Class::member` in YAML.

**Where to look:** `src/twig/templates.rs`, `src/resource_navigation.rs`
(`scan_symbols`), and `references/symfony-language-tools`
(`docs/features/twig-constants-enums.rst`).

---

## F52. Asset paths in Twig

**Impact: Low · Complexity: Low-Medium**

The path a Twig `asset('images/logo.png')` names, and the entrypoint an
`importmap('app')` names, neither complete nor navigate to the file under
`public/` (or the AssetMapper's `assets/`), and a missing file is not
reported. Laravel's `asset()` has the same gap (see `laravel.md`), so the
two should share the path completion.

**Where to look:** `src/twig/templates.rs`, and
`references/symfony-language-tools` (`docs/features/assets.rst`).

---

## F53. Stimulus controllers in Twig

**Impact: Low · Complexity: Medium**

A Twig template names a Stimulus controller in `data-controller`, its
actions in `data-action`, and its targets in `data-*-target`, or through
`stimulus_controller()`, `stimulus_action()`, and `stimulus_target()`. The
controller is a JavaScript file under `assets/controllers/`
(`hello_controller.js` for `hello`). None of these navigate or complete.
It is the one Symfony integration whose declarations are not PHP, YAML,
or XML, so it needs a scan of the controller files' class members.

**Where to look:** `references/symfony-language-tools`
(`docs/features/stimulus.rst`).

---

## F54. Symfony form options, constraint options, and serializer groups

**Impact: Low-Medium · Complexity: Medium**

```php
$builder->add('email', EmailType::class, ['requird' => true]);  // a typo, not reported
#[Assert\Length(mni: 3)]                                        // likewise
#[Groups(['user:read'])]                                         // used where?
```

A form type's options are the ones its `configureOptions()` and its
parents' define, and a constraint's are its constructor's parameters. The
keys of a `createForm()` or `$builder->add()` options array neither
complete nor get checked against them. Serializer groups, named by
`#[Groups]`, `#[Context]`, the `groups` a serializer or a controller's
`json()` is given, and the `groups` keys of a YAML serializer mapping,
neither navigate between their uses nor complete.

**Where to look:** `src/symfony/forms.rs`, `src/symfony/validation.rs`,
and `references/symfony-language-tools` (`docs/features/metadata.rst`).

---

## F55. Symfony ExpressionLanguage strings

**Impact: Low · Complexity: Medium-High**

```php
#[IsGranted(new Expression('is_granted("ROLE_ADMIN") or user.isOwner(subject)'))]
#[Route('/admin', condition: "request.headers.get('X-Admin') == '1'")]
```

An ExpressionLanguage string is a small language of its own, with
variables Symfony binds per site: `user`, `subject`, `token`, and
`request` in a security expression, `context` and `request` in a route
condition, the service container's functions in `@=` service
arguments. The member chains in it (`user.isOwner(subject)`) neither
navigate nor get checked.

The built-in sites bind fixed variables, so they can work without
configuration: parse the expression, give each variable its class, and
hand the member chains to the shared type engine. A per-project rule
language for custom attributes was proposed and declined, since it does
nothing for a project until someone writes its rules.

**Where to look:** `src/symbol_map/extraction/symfony.rs`, and Symfony's
`ExpressionLanguage` component (`Lexer`, `Parser`) for the grammar.

---

## F56. Live components

**Impact: Low-Medium · Complexity: Medium-High**

```php
#[AsLiveComponent]
final class SearchBox
{
    use DefaultActionTrait;

    #[LiveProp(writable: true)]
    public string $query = '';

    #[LiveAction]
    public function clear(): void { … }
}
```

```twig
<input data-model="query">
<button data-action="live#action" data-live-action-param="clear">Clear</button>
```

With `symfony/ux-live-component`, a component's template names its class's
`#[LiveProp]` properties in `data-model` (and `data-model="on(change)|query"`),
and its `#[LiveAction]` methods in `data-live-action-param`. None of these
complete, navigate, or show up in the property's or method's references and
renames, and a name the class does not declare is not reported. Only a
writable prop can be bound by `data-model`, and only a `#[LiveAction]`
method can be called, so completion should offer just those. The template
also sees the component's public properties and methods as `this.…` and
bare variables, which F37's template variables would type. Builds on F38's
link between a component class and its template.

**Where to look:** `src/twig/templates.rs`, `src/blade/component_tags/`
(the Livewire `wire:model` handling is the closest counterpart).

---

## F57. "Add translation" quick fix for an undeclared Symfony key

**Impact: Medium · Complexity: Medium** (depends on F29)

```php
$translator->trans('checkout.confirm'); // not in any catalogue
```

Laravel offers to add a missing translation to each locale's language file
(`insert_translation_key.rs`). Symfony keys get no such fix. Once F29
reports an undeclared key, offer one action per catalogue of the key's
domain (`messages.en.yaml`, `messages.da.xlf`, …) that inserts the key with
an empty or source-text value: nested under its dotted parents in YAML,
or flat when the file already spells its keys flat, and as a new
`<trans-unit>` (XLIFF 1.2) or `<unit>` (XLIFF 2) with a fresh id in XLIFF.
When the domain has no catalogue yet, offer to create
`translations/<domain>.<default_locale>.yaml`.

**Where to look:** `src/code_actions/insert_translation_key.rs`,
`src/symfony/translations.rs` (`catalogue_spans`, `scan_yaml`,
`scan_xliff`).
