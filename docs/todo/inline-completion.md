# PHPantom — Inline Completion

Inline completion provides ghost-text suggestions beyond what traditional
LSP completion offers. Where LSP completion fills in a symbol name after
you type `$user->`, inline completion fills in entire expressions,
statements, and blocks before you ask.

The goal is not to compete with cloud-hosted LLMs on general coding
ability. The goal is to be so fast and so PHP-accurate that the cloud
model is still tokenizing your request by the time PHPantom has already
shown the answer. Every suggestion is grounded in type information the
LSP already has. No network, no subscription, no GPU required for the
base experience.

## Philosophy

Traditional AI coding tools remove the human from the loop. The
developer describes intent, the model writes code, then the developer
reviews output they didn't think through. PHPantom's inline completion
takes the opposite approach: the human is thinking and writing code,
and the tool removes the mechanical friction. It fills in the parts
the developer already knows but hasn't typed yet. The developer stays
in the loop the entire time.

This also means the entire pipeline (training data, training scripts,
model weights, context format) must be open and reproducible. A
company should be able to retrain the model on their own codebase. A
contributor should be able to improve the n-gram corpus or add
template patterns without asking permission. A free tool that's been optimized for the specific team using it can bring a strong challange to a propritery tool trained for the general case.

Everything needed to reproduce the training is published alongside
the model weights:
- The corpus collection scripts (which packages, how they're processed)
- The tokenizer definition
- The training configuration and scripts
- The context format specification (so third parties can train
  compatible models)
- The evaluation suite (so improvements can be measured)

## Architecture

```
PHPantom LSP (main process)
  │
  ├── Template Engine (built-in, zero cost)
  │     Pattern matching on AST + type context
  │     Responds in <1ms
  │
  ├── N-gram Engine (built-in, ~2-5MB model file)
  │     Token-level predictions from PHP corpus
  │     Responds in <5ms
  │
  └── Sidecar: Fine-tuned GGUF model (optional download)
        Fill-in-the-middle with tight PHP context
        ~50-150MB, runs on CPU
        Responds in ~100-500ms
```

Each layer is independently useful. The template engine ships day one
with no model files. The n-gram engine adds a small data file. The
sidecar is an optional download for users who want deeper suggestions.

All three layers share the same context-gathering pipeline: the LSP
already knows the current class outline, variable types, method
signatures, and surrounding code structure. That context is formatted
once and fed to whichever engine is active.

### Protocol

Inline completions use the LSP `textDocument/inlineCompletion` request
(proposed in LSP 3.18). For editors that don't support it yet, we can
fall back to `completionItem/resolve` with snippet insert text, or a
custom `phpantom/inlineCompletion` method.

The response is simple: a string of text to show as ghost text at the
cursor, plus an edit range. The editor renders it dimmed and the user
presses Tab to accept.

### Sidecar Protocol

The sidecar process communicates over stdin/stdout with a minimal
JSON protocol:

```json
{ "id": 1, "context": "...", "cursor": "...", "max_tokens": 64 }
```

```json
{ "id": 1, "text": "return $this->customer->fullName;", "confidence": 0.87 }
```

The LSP spawns the sidecar on first use and keeps it alive. If the
sidecar is not installed or crashes, the LSP falls back to templates
and n-grams silently. The sidecar binary is a separate downloadable
artifact, not bundled in the main LSP binary.

---

## N1. Template Engine

**Complexity:** Medium (per pattern group)
**Dependencies:** None beyond existing type resolution, but
`textDocument/inlineCompletion` is an LSP 3.18 request. Check whether
our `lsp-types` (0.94.1, pinned by `tower-lsp` 0.20) carries it; if
not, this needs F20 or a custom method.

Some templates overlap shipped features: getter/setter and
constructor-assignment bodies duplicate the `generate_getter_setter`
and `generate_constructor` code actions, and the `try/catch` template
overlaps the existing try/catch completion. Reuse those generators
rather than writing second copies.

The template engine is pattern matching on the current AST node and
cursor position, combined with type information from the existing
completion resolver. No model, no data files, no latency.

### How It Works

1. On each keystroke (debounced), check if the cursor is at a
   template trigger point (after a keyword, inside an empty block,
   at a return statement, etc.)
2. Gather context: surrounding variables and their types, the
   containing function's return type, the class outline, nearby
   assignments.
3. Match against template patterns. Each pattern has a confidence
   score based on how much context it could use.
4. Return the highest-confidence suggestion as ghost text.

### Context Gathering

The template engine reuses existing infrastructure:

- **Variable types:** the forward walker
  (`type_engine/variable/forward_walk/`, via `resolve_expression_type`)
  already resolves every variable in scope to a type. The template
  engine calls the same pipeline.
- **Containing function:** The AST map already stores `MethodInfo`
  and `FunctionInfo` with return types, parameter lists, and
  docblocks.
- **Class outline:** `ClassInfo` gives us all properties, methods,
  constants, and their types for `$this->` context.
- **Selection range spans:** `selection_range.rs` already walks the
  AST to find the containing statement, block, function, and class.
  We can reuse this to scope the context window.

A new `InlineContext` struct bundles all of this:

```rust
pub struct InlineContext {
    /// Variables in scope with their resolved types.
    pub variables: Vec<(String, String)>,
    /// The containing method/function, if any.
    pub containing_function: Option<FunctionSummary>,
    /// The containing class, if any.
    pub containing_class: Option<ClassSummary>,
    /// The AST node immediately before the cursor.
    pub preceding_node: Option<NodeKind>,
    /// The line the cursor is on, trimmed.
    pub current_line: String,
    /// Lines above the cursor within the current block.
    pub block_context: Vec<String>,
}
```

This struct is built once per request and shared across all engines.

### Template Patterns

#### `foreach` with Type-Aware Iteration Variable

Trigger: User types `foreach` or `fore` (snippet prefix).

The engine looks at variables in scope whose types implement
`Traversable`, are arrays, or are generic collections. It picks the
most likely candidate based on:
- Proximity (closest assignment wins)
- Naming (plural names like `$items`, `$users`)
- Type (generic collections with known element types)

For each candidate, it generates the iteration variable name by
singularizing the collection name and resolves the element type:

```php
// $users is Collection<User> in scope
foreach ($users as $user) {
    █
}

// $itemsByCategory is array<string, array<Item>> in scope
foreach ($itemsByCategory as $category => $items) {
    █
}

// $results is QueryBuilder (Traversable<Model>) in scope
foreach ($results as $result) {
    █
}
```

Singularization rules (simple, no NLP needed):
- `$users` → `$user` (drop trailing `s`)
- `$entries` → `$entry` (`ies` → `y`)
- `$addresses` → `$address` (drop `es`)
- `$data` → `$datum` or `$item` (known irregulars / fallback)
- `$list` → `$item` (non-plural name, use generic fallback)

When the element type has a key type (e.g. `array<string, User>`),
include `$key => $value` form.

#### `if` with Nullability Awareness

Trigger: User types `if` near a nullable variable.

The engine checks if any variable in the current block is nullable
(`?Type` or `Type|null`) and has not yet been null-checked. It
suggests the guard:

```php
// $user is ?User, not yet checked
if ($user === null) {
    █
}

// Or, if the next line accesses $user->something,
// prefer the positive form:
if ($user !== null) {
    $user->█
}
```

It can also suggest `instanceof` checks when a variable is a union
of class types:

```php
// $shape is Circle|Square|Triangle
if ($shape instanceof Circle) {
    █
}
```

#### `try/catch` with Thrown Exception Detection

Trigger: User types `try` or is inside a `try` block that has no
`catch` yet.

The engine uses the existing throws analysis pipeline
(`completion/source/throws_analysis/`) to detect what exceptions the code
inside the try block can throw:

```php
try {
    $this->repository->save($entity);
    $this->mailer->send($notification);
} catch (DatabaseException $e) {
    █
}
// If multiple throwable types detected, suggest multi-catch:
// catch (DatabaseException | MailerException $e)
```

#### `match` with Enum Exhaustiveness

Trigger: User types `match` with an enum variable in scope, or
starts a match expression on a variable whose type is a backed or
unit enum.

```php
// $status is Status enum with Active, Inactive, Pending
match ($status) {
    Status::Active => █,
    Status::Inactive => ,
    Status::Pending => ,
}
```

The engine knows all enum cases from `ClassInfo` and fills them in.
If the enum is a `BackedEnum`, it can also suggest the value form.

#### `return` with Type-Guided Expression

Trigger: User types `return` inside a function with a known return
type.

The engine looks at what's available in scope that matches the return
type:

```php
/** @return string */
public function getCustomerName(): string
{
    // $this->customer is Customer, Customer has fullName: string
    return $this->customer->fullName;█
}

/** @return array<string, mixed> */
public function toArray(): array
{
    return [
        █
    ];
}

/** @return self */
public function withName(string $name): self
{
    return new self(█);
    // or: return clone $this; if immutable pattern detected
}
```

The return type matching works by:
1. If the return type is scalar and exactly one variable/property
   in scope matches, suggest it.
2. If the return type is `self`/`static`, suggest `new self(...)` or
   `clone $this`.
3. If the return type is `array`, suggest an array literal with
   known shape keys if a `@return` docblock specifies them.
4. If a property or method chain reaches the return type in one or
   two steps, suggest the chain.

#### Function/Method Body from Signature

Trigger: Cursor is on the first line inside an empty method body.

For simple accessor patterns, the engine can suggest the entire body:

```php
public function getName(): string
{
    return $this->name;█
}

public function setName(string $name): void
{
    $this->name = $name;█
}

public function isActive(): bool
{
    return $this->active;█
}

public function hasPermission(string $permission): bool
{
    return in_array($permission, $this->permissions, true);█
}
```

Detection heuristics:
- `getName()` with return type matching `$this->name` type → getter
- `setName($name)` with void return → setter
- `isX()` / `hasX()` with bool return → boolean property accessor
- `with*()` returning `self`/`static` → immutable setter (clone)

#### Assignment from Constructor Parameter

Trigger: Inside a constructor body, after the parameter list.

```php
public function __construct(
    private readonly string $name,
    private readonly int $age,
) {
    // No suggestion needed — promoted properties handle it.
}

// But for non-promoted:
public function __construct(string $name, int $age)
{
    $this->name = $name;
    $this->age = $age;█
}
```

The engine matches parameter names to property names and suggests
assignments for any that aren't yet assigned.

#### Catch Variable Usage

Trigger: Inside a catch block, cursor on the first line.

Based on common patterns per exception type:

```php
catch (ValidationException $e) {
    return response()->json(['errors' => $e->errors()], 422);█
}

catch (\Throwable $e) {
    Log::error($e->getMessage(), ['exception' => $e]);█
}
```

This is more heuristic than type-driven. We match on the exception
class name and suggest common handling patterns. A small hardcoded
table of exception → handler patterns covers the most common cases.

#### Early Return Guard Clauses

Trigger: Start of a method body, or after an assignment, when the
method has validation-like parameters.

```php
public function process(string $input): Result
{
    if ($input === '') {
        throw new \InvalidArgumentException('Input cannot be empty');
    }
    █
}
```

This triggers when:
- The parameter is a string and the method name suggests processing
- The parameter is nullable and the method doesn't return nullable
- The method has a `@throws` tag for an `InvalidArgumentException`

### Template Priority and Confidence

Each template pattern returns a confidence score (0.0 to 1.0):

- **1.0** — Exact type match, single obvious completion (getter body)
- **0.8** — Strong type match with minor ambiguity (foreach with
  typed collection)
- **0.6** — Heuristic match (enum match arms, try/catch from throws
  analysis)
- **0.4** — Name-based heuristic (singularization guess, common
  pattern match)
- **0.2** — Fallback suggestion (generic catch block body)

Only suggestions above a configurable threshold (default 0.4) are
shown. Below that, showing nothing is better than showing something
wrong.

---

## N2. N-gram Engine

**Complexity:** Very High (novel statistical model and training pipeline, no existing pattern to follow)
**Dependencies:** N1 (the `InlineContext` builder), training corpus,
PHP tokenizer

The n-gram engine handles the cases where the template engine has no
pattern match but there's still enough local context to make a useful
prediction. It predicts the next few PHP tokens based on the
preceding tokens.

### Why PHP Tokens, Not BPE

BPE (byte-pair encoding) is designed for natural language and treats
PHP syntax as opaque byte sequences. A PHP-aware tokenizer means:
- `$this->` is one token, not four
- `::` is one token
- String literals are one token regardless of length
- Variable names are one token
- `array_map` is one token, not fragmented

This dramatically reduces the sequence length and makes the n-gram
table smaller and more predictive. A 5-gram over PHP tokens covers
roughly 2-3 lines of code, versus half a line with BPE tokens.

### Token Vocabulary

The vocabulary is built from PHP's own token types plus symbols:

| Category | Examples | Count |
|---|---|---|
| Keywords | `function`, `return`, `class`, `if`, `foreach`, ... | ~70 |
| Operators | `->`, `=>`, `??`, `?->`, `::`, ... | ~40 |
| Delimiters | `(`, `)`, `{`, `}`, `[`, `]`, `;` | ~10 |
| Types | `string`, `int`, `bool`, `float`, `array`, `void`, ... | ~15 |
| Special | `<VAR>`, `<STRING>`, `<NUMBER>`, `<FQCN>`, `<FUNC>` | ~10 |
| Common identifiers | Top 500 function names from corpus | ~500 |
| Common methods | Top 500 method names from corpus | ~500 |
| Common properties | Top 200 property names from corpus | ~200 |

Total vocabulary: ~1300-1500 tokens. Small enough that the n-gram
table stays compact.

Variable names, string literals, and numbers are replaced with their
category token (`<VAR>`, `<STRING>`, `<NUMBER>`). This means the
n-gram engine predicts *structure*, not specific names. The template
engine and type resolver fill in the actual names.

### Training

**Corpus:** Top 500-1000 PHP packages from Packagist by monthly
downloads. This covers Laravel, Symfony, PHPStan, Doctrine, PHPUnit,
Monolog, Guzzle, and the rest of the ecosystem that defines PHP
idioms.

**Process:**
1. Clone packages, extract all `.php` files
2. Tokenize with the PHP-aware tokenizer
3. Count all n-grams (3-gram through 7-gram)
4. Apply Kneser-Ney smoothing for unseen n-grams
5. Prune n-grams with count < 3
6. Serialize to a compact binary format

**Hardware:** N-gram training is embarrassingly parallel (count frequencies per file, merge). No GPU needed. Expected training time: minutes, not hours.

**Output:** A single binary file, ~2-5MB compressed, containing:
- Token vocabulary (string → id mapping)
- N-gram probability tables (5-gram primary, 3-gram fallback)
- Top-k predictions precomputed for the most common contexts

### Runtime

Given the current cursor position:
1. Tokenize the preceding ~50 characters into PHP tokens
2. Look up the 5-gram (or fall back to 3-gram) in the table
3. Get the top-k predicted next tokens
4. If the prediction is a structural token (`return`, `->`, `(`),
   continue predicting up to ~10 tokens to form a complete fragment
5. Replace `<VAR>` placeholders with the most likely variable from
   the type context (using the same `InlineContext` the template
   engine uses)

The n-gram engine never suggests more than one line. Its role is to
predict the likely *shape* of the next expression, then the type
resolver fills in the actual symbols.

### Example

User is inside a method, just typed `$this->`:

1. Preceding tokens: `return`, `$this`, `->`
2. N-gram lookup: after `return <VAR> ->`, the most common next
   tokens in the PHP corpus are property/method names
3. The type resolver narrows this to properties/methods of the
   current class that match the return type
4. Combined suggestion: `return $this->repository->find($id);`

The n-gram engine provides the structural skeleton (`<VAR> -> <FUNC>
( <VAR> )`), and the type resolver fills in `repository`, `find`,
and `$id`.

---

## N3. Fine-Tuned GGUF Model

**Complexity:** Very High (model selection, fine-tuning, and a new sidecar architecture, well outside the existing codebase's patterns)
**Dependencies:** N1 and N2 complete, training infrastructure

This is the "eventually" phase. A small language model fine-tuned
exclusively on PHP code with fill-in-the-middle capability. It runs
as a sidecar process and provides multi-line suggestions that neither
templates nor n-grams can handle.

### Why a Sidecar

- The model is 50-150MB. Embedding it inflates the main binary.
- Model inference takes 100-500ms. It must not block LSP responses.
- Users without the model still get templates and n-grams.
- The model can be updated independently of the LSP.
- Different model sizes can be offered (tiny for laptops, small for
  desktops).

### Base Model Selection

Candidates (as of early 2025, evaluate latest at training time):

| Model | Params | Quantized Size | Notes |
|---|---|---|---|
| SmolLM2-135M | 135M | ~80MB Q4 | Very fast, limited capacity |
| Qwen2.5-Coder-0.5B | 500M | ~300MB Q4 | Good code understanding |
| StarCoder2-164M | 164M | ~100MB Q4 | Code-focused from the start |

Start with 135M-164M class. These run on CPU in <500ms for short
completions. The 0.5B class is better but may be too slow without
quantization tricks.

### Training Approach

**Fill-in-the-middle (FIM) objective.** The model learns to complete
code given a prefix and suffix. This is exactly what inline
completion needs: the user has code above and below the cursor.

**Training data:** Same Packagist corpus as the n-gram engine, but
processed differently:
- Split files into function/method bodies
- Create FIM examples: randomly mask a contiguous span of 1-5 lines
- Prefix and suffix are the surrounding code
- Target is the masked span

**Context format.** The model receives a tightly structured prompt
that mirrors what the LSP knows:

```
<|class|>
class OrderService
  + __construct(OrderRepository $repository, Mailer $mailer)
  + process(Order $order): Result
  + private validate(Order $order): void
<|vars|>
  $order: Order { id: int, customer: Customer, total: Money }
  $this->repository: OrderRepository
<|method|> process(Order $order): Result
<|prefix|>
    $this->validate($order);
    $result = $this->repository->save($order);
<|suffix|>
    return $result;
<|cursor|>
```

The `<|class|>` section is a compressed class outline (method
signatures only, no bodies). The `<|vars|>` section includes resolved
types for variables in scope, with one level of property expansion
for types the cursor is likely to chain through. The `<|method|>`
line identifies which method we're in.

The exact shape of this context will evolve as we implement and
discover what the model actually needs. For example, completing
`$this->customer->fullName` requires knowing that `$this->customer`
resolves to `Customer` and that `Customer` has a `fullName` property.
The LSP already does this multi-hop resolution for completion, so the
context builder can include resolved property types for variables
that appear in the prefix. How deep to go (one hop? two?) and how
to format it compactly are details to figure out during
implementation.

This format is critical. The model is trained on exactly this
structured context, not raw PHP files. At inference time, the LSP
builds the same structure from its type resolver and the AST, so the
model sees exactly the format it was trained on. This tight coupling
between the LSP's knowledge and the model's training data is what
makes a tiny model competitive with a generic 7B model that has to
figure out the class structure from raw text.

**Fine-tuning approach:** LoRA on a small base model, trained on the
Packagist PHP corpus processed into the structured context format
above. Specific hyperparameters, framework choice, and hardware
requirements depend on the base model selected and what's available
at training time. The training scripts and configuration will be
published so anyone can reproduce or adapt the process.

### Inference

The sidecar process loads the GGUF model at startup and keeps it in
memory. On each request:

1. LSP builds the `InlineContext` (same as template engine)
2. LSP formats it into the structured prompt above
3. LSP sends it to the sidecar over stdin
4. Sidecar generates up to `max_tokens` (default 64, ~2-3 lines)
5. Sidecar returns the completion text
6. LSP validates the suggestion (syntax check, type check) and
   shows it or discards it

**Validation is important.** The model will sometimes hallucinate
method names or wrong types. The LSP already has the type resolver,
so it can check if `$this->repository->find($id)` is actually valid
by resolving the chain. Invalid suggestions are silently dropped.
This is a massive advantage over generic AI completion tools that
have no type checker in the loop.

### Model Distribution

- The model is not bundled with the LSP binary
- On first use (or via a command), the LSP downloads it from a
  GitHub release or CDN
- Stored in `~/.phpantom/models/`
- Version-pinned to the LSP version to avoid compatibility issues
- Users can provide their own GGUF model path in config (custom
  trained, company-internal, or community-contributed models)

### Reproducibility

Everything needed to train a compatible model from scratch is
published:

- **Corpus scripts:** Download and process packages from Packagist
- **Tokenizer:** The PHP-aware tokenizer used for n-grams, plus the
  context formatter that builds the structured prompt
- **Training scripts:** End-to-end pipeline from raw PHP to GGUF
- **Evaluation suite:** A set of fill-in-the-middle test cases with
  expected completions, so anyone can measure model quality
- **Context format spec:** Documented well enough that someone could
  train a compatible model without reading the LSP source

A company can clone the training repo, point it at their private
codebase, and produce a model that knows their domain objects, their
naming conventions, and their architectural patterns. The LSP doesn't
care where the model came from as long as it speaks the same context
format.

---

## Context Window Design

All three engines share a common context format. The LSP builds this
once per request.

### For Templates and N-grams

```rust
pub struct InlineContext {
    /// Variables in scope with their resolved types.
    pub variables: Vec<(String, String)>,

    /// The containing method/function, if any.
    pub containing_function: Option<FunctionSummary>,

    /// The containing class, if any.
    pub containing_class: Option<ClassSummary>,

    /// The AST node immediately before the cursor.
    pub preceding_node: Option<NodeKind>,

    /// The return type of the containing function, if known.
    pub expected_return_type: Option<String>,

    /// The line the cursor is on, trimmed.
    pub current_line: String,

    /// Lines above the cursor within the current block (max ~20).
    pub block_prefix: Vec<String>,

    /// Lines below the cursor within the current block (max ~10).
    pub block_suffix: Vec<String>,
}
```

### For the GGUF Model

The model context is built from `InlineContext` plus the class
outline and resolved types for variables the cursor is likely to
interact with:

```rust
pub struct ModelContext {
    /// Compressed class outline: method signatures, no bodies.
    pub class_outline: String,

    /// Resolved types for variables in scope, with property
    /// expansion for types that appear in the prefix.
    /// E.g. "$this->customer: Customer { fullName: string, email: string }"
    pub resolved_variables: String,

    /// The current method signature.
    pub method_signature: String,

    /// Code before the cursor (within the current method).
    pub prefix: String,

    /// Code after the cursor (within the current method).
    pub suffix: String,

    /// Maximum tokens to generate.
    pub max_tokens: u32,
}
```

The prefix and suffix are bounded to the current method body. The
class outline is bounded to signature lines only (one line per
member). Resolved variable types include one or two levels of
property expansion for types that are referenced in the prefix or
are likely chain targets. The exact depth and format will be refined
during implementation. The total context should stay compact enough
for a small model (target: under 512 tokens).

---

## Configuration

```toml
# .phpantom.toml or editor settings
[inline_completion]
# Enable/disable the entire feature
enabled = true

# Minimum confidence to show a suggestion (0.0 - 1.0)
min_confidence = 0.4

# Which engines to use (in priority order)
engines = ["template", "ngram", "model"]

# Debounce delay in milliseconds
debounce_ms = 50

[inline_completion.model]
# Path to a custom GGUF model (overrides default)
# path = "~/.phpantom/models/phpantom-completion-v1.gguf"

# Maximum tokens to generate per request
max_tokens = 64

# Temperature (lower = more conservative)
temperature = 0.2
```

---

## Phasing

### Template engine first (N1)

Implement the `InlineContext` builder and 3-4 high-value template
patterns:
- `foreach` with collection type awareness
- `return` with return-type matching
- Getter/setter body generation
- `try/catch` with throws detection

This is enough to demo the "how did it know" effect. No external
dependencies, no model files, ships in the main binary.

### Then the n-gram engine (N2)

- Build the PHP tokenizer
- Scrape and process the Packagist corpus
- Train and compress the n-gram model
- Integrate with the `InlineContext` for variable substitution
- Ship the model file as a separate downloadable asset or embed it
  if it stays under 5MB

### GGUF sidecar last (N3, when competing with PHPStorm)

- Select and fine-tune the base model
- Build the sidecar binary (Rust + llama.cpp bindings, or a small
  C++ binary)
- Implement the sidecar protocol
- Add validation (type-check generated code before showing)
- Distribution and auto-download
- Publish all training scripts, corpus tools, and evaluation suite
  so anyone can reproduce or retrain

### Ongoing

Each phase builds on the previous one. Templates keep working even
after the model is available (they're faster for the patterns they
cover). The n-gram engine fills gaps between templates and model
suggestions. The model handles the truly open-ended cases.

As we add more type intelligence to the LSP (better generics, better
narrowing, more Laravel magic), every engine automatically benefits
because they all read from the same `InlineContext`.

---

## Success Criteria

A suggestion is good if:
1. It is **correct** (compiles, type-checks, does what the user
   intended)
2. It is **fast** (appears before the user's next keystroke)
3. It is **non-obvious** (the user couldn't have typed it faster
   than accepting the suggestion)

We'd rather show nothing than show something wrong. A 30% hit rate
with 95% accuracy is far more valuable than an 80% hit rate with 60%
accuracy. Users learn to trust the suggestions and press Tab without
reading them carefully. One wrong suggestion breaks that trust.

Metrics to track (via opt-in telemetry or local dev testing):
- **Acceptance rate:** % of shown suggestions that the user accepts
- **Accuracy:** % of accepted suggestions that aren't immediately
  edited
- **Latency:** p50 and p95 time from keystroke to suggestion shown
- **Coverage:** % of cursor positions where we have a suggestion

Target for Phase 1 (templates only):
- Acceptance rate: >50%
- Accuracy: >90%
- Latency p95: <10ms
- Coverage: ~5-10% of cursor positions (only where we have a strong
  pattern match)
