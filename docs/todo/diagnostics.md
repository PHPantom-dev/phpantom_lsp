# PHPantom — Diagnostics

Items are ordered by **impact** (descending), then **complexity** (ascending)
within the same impact tier.

| Label      | Scale                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| **Impact** | **Critical**, **High**, **Medium-High**, **Medium**, **Low-Medium**, **Low**                                           |
| **Complexity** | **Low** (mechanical/boilerplate, no design decisions), **Medium** (self-contained, follows an existing pattern), **Medium-High** (spans modules, some new design), **High** (shared/core subsystem, correctness or performance tradeoffs), **Very High** (cross-cutting architecture, wide blast radius) |

---

## Severity philosophy

PHPantom assigns diagnostic severity based on runtime consequences:

| Severity        | Criteria                                                                                                                                                                                                                                                                                                                                                                                     | Examples                                                                                                                                                                                                                                                                      |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Error**       | Would crash at runtime. The code is definitively wrong.                                                                                                                                                                                                                                                                                                                                      | Member access on a scalar type (`$int->foo()`). Calling a function that doesn't exist (`doesntExist()`).                                                                                                                                                                      |
| **Warning**     | Likely wrong but could work for reasons we can't verify statically. The types are poor but the code might be correct at runtime.                                                                                                                                                                                                                                                             | Accessing a member that doesn't exist on a non-final class (`$user->grantAccess()` where `User` has no such method but a subclass might). Unknown class in a type position (`Class 'Foo' not found`). Subject type resolved to an unknown class so members can't be verified. |
| **Hint**        | The codebase lacks type information. Off by default or very subtle. Poorly typed PHP is so common that showing these by default would be noise for most users. Anyone who does care about type safety is likely running PHPStan already. Unless our engine becomes very strong, these diagnostics either expose our own inference gaps or bother users who never opted into static analysis. | `mixed` subject member access (opt-in via `unresolved-member-access`). Deprecated symbol usage (rendered as strikethrough).                                                                                                                                                   |
| **Information** | Advisory. Something the developer might want to know.                                                                                                                                                                                                                                                                                                                                        | Unused `use` import (rendered as dimmed). Unresolved type in a PHPDoc tag.                                                                                                                                                                                                    |

---

## D5. External tool diagnostic suppression actions

**Impact: Low · Complexity: Low (per tool, after proxy exists)**

PHPantom's own inline suppression (`// @phpantom-ignore code`) has
shipped. PHPStan suppression is also implemented ("Ignore PHPStan
error" / "Remove unnecessary @phpstan-ignore"). The PHPCS proxy itself
has also shipped (`src/diagnostics/external/phpcs.rs`, `[phpcs]` config
section), but nothing wires up a suppression action for it yet. What
remains is wiring up suppression actions for additional external tool
proxies:

- PHPCS: `// phpcs:ignore [Sniff.Name]` or `// phpcs:disable` /
  `// phpcs:enable` blocks. The proxy exists; only the suppression
  action is missing.
- PHPMD (3.0): `#[SuppressWarnings(RuleName::class)]` as a PHP
  attribute on the enclosing class or method, importing
  `PHPMD\Attribute\SuppressWarnings` and the rule class. The proxy
  (`src/phpmd.rs`, `[phpmd]` config section) carries the rule name as
  the diagnostic code; only the suppression action is missing. The
  rule's class is not in the report, so the action has to map the rule
  name to its `PHPMD\Rule\…` class.

---

## D15. Unused parameter diagnostic

**Impact: Low · Complexity: Medium**

Flag function and method parameters that are never read inside the
body. The `unused_variable` diagnostic deliberately skips parameters
because false positives are common for callbacks, interface
implementations, and framework conventions (e.g. Laravel event
listeners) that require specific parameter signatures even when not
all parameters are used. Users can now silence false positives with
`// @phpantom-ignore unused_parameter`.

### Scope

1. Function and method parameters (including closures and arrow
   functions) that are never read inside their body.
2. Constructor parameters that are not promoted and never read.

### Exclusions

- Parameters named `$_` or starting with `$_` (intentional discard).
- Promoted constructor parameters (they are property assignments).
- Parameters in abstract methods and interface method signatures
  (no body to check).

---

## D16. `unreachable_match_arm` ignores literal subject types

**Impact: Low-Medium · Complexity: Medium**

`scalar_type_label` in `src/diagnostics/match_type_errors.rs` answers
`None` for a literal type (`'exception'`, `42`), so a subject the
resolver typed as one exact value never reaches the arm check and no
arm is ever reported unreachable. The comment there explains why: a
literal was as often what survived after resolution lost an
alternative it could not type as it was a genuine one-value subject,
and taking the claim without that evidence produced false positives.

The resolver no longer loses those alternatives. An unresolvable
branch now widens the union it belongs to instead of dropping out of
it, so a literal that reaches this diagnostic is a claim the resolver
stands behind.

**Fix:** Give `TypeKind::Literal` its scalar kind in
`scalar_type_label` and check the resulting arms. Cover the case the
old comment was guarding against with a test: a subject whose other
branch cannot be typed must still produce no diagnostic, because it
now resolves to `mixed` rather than to the surviving literal.



---

## D19. `invalid_member_access` cannot tell a property read from a write

**Impact: Medium · Complexity: Medium**

PHP dispatches an unreachable property to a different magic method
depending on what is being done to it: `__get` for a read, `__set` for a
write, `__isset` for `isset()`, and `__unset` for `unset()`. The span the
visibility check runs on records only that a property was accessed, so
the check cannot pick the handler that actually applies and treats the
presence of any of the four as reason enough to stand down.

The result is a missed report rather than a wrong one. A class that
declares `__set` but no `__get` silences a read it would in fact fatal
on, and the same holds for every other mismatched pairing.

PHP 8.4's asymmetric visibility needs the same distinction. A
`public private(set)` or `public protected(set)` property, plain or
promoted (and static, since PHP 8.5), can be read from anywhere but
written only from inside its scope. `PropertyInfo` records a single
visibility (`extract_visibility` in `parser/params.rs` keeps the first
modifier it finds), so a write from outside goes unreported, and
completion offers the property in write positions too. Hover drops the
set-visibility as well, showing `public PostTitle $title` for a
`public private(set)` property.

**Fix:** Carry the operation on the `MemberAccess` span — read, write,
`isset`, `unset` — the way `readonly_writes.rs` recovers write targets
from the AST, and require the handler that matches it. The same
information would let the readonly check drop its own separate walk.
Record a property's set-visibility as an optional `set_visibility` on
`PropertyInfo` and check writes against it.

---

## D20. `Foo::$bar` and `Foo::bar` are the same span

**Impact: Low · Complexity: Medium**

Symbol extraction strips the `$` from a static property access
(`member_access.rs`) and records only a name plus `is_static`, so a
static property and a class constant of the same name are
indistinguishable downstream. Every consumer has to guess a precedence;
the visibility check tries the constant first.

Two classes of mistake follow, both needing a class that declares a
constant and a static property under one name:

```php
class Collision {
    private const token = 'constant';
    public static string $token = 'property';
}

echo Collision::$token;   // reported as a private constant
```

and the reverse pairing, where a genuinely unreachable static property
is passed off as a public constant and nothing is reported.

**Fix:** Record the member kind the syntax actually names — instance
property, static property, constant, or method — rather than a name and
a static flag.

---

## D22. Member provenance is recomputed instead of recorded

**Impact: Medium · Complexity: Medium-High**

The inheritance merge knows which class each member it folds in came
from — `merge_traits_into()` is handed the host FQN — and drops that on
the floor. `MethodInfo`, `PropertyInfo`, and `ConstantInfo` have no field
for it, so the assembled class says what a class *has* and never who
declared it.

Every check that needs the declaring class therefore recomputes it by
walking the raw hierarchy, and every such walk is a partial
reimplementation of the merge's own rules. `invalid_member_access` has
one, and it is already known to be incomplete: it does not apply
`trait_aliases`, so a member a `use` clause renamed is untraceable and
the access goes unreported.

```php
trait Opens { public function open(): void {} }

class Vault { use Opens { open as private hidden; } }

class Intruder {
    public function probe(Vault $vault): void
    {
        $vault->hidden();   // fatal at runtime, reported by nothing
    }
}
```

The access has to be written inside a class to show the gap. From
outside every class the check reports anyway, because no scope can reach
a non-public member whoever declared it, and it falls back to naming the
receiver when the declaration cannot be traced.

**Fix:** Record the declaring class during the merge and let the checks
read it, rather than each one re-deriving the merge's rules. The cost is
a field on every member across the whole index, so it wants measuring
against the memory the index already uses before it is committed to.

---

## D23. A rebound closure's scope is added to the lexical one rather than replacing it

**Impact: Low-Medium · Complexity: Medium**

A closure can run with a different class as its scope than the one it is
written in — Laravel's `Macroable`, anything through `Closure::bind`, and
anything a `@param-closure-this` tag describes. The resolver already
works this out. The visibility check does not read it; it infers from the
subject text that `$this`, `self`, or `static` names a binding and adds
the receiver as an *additional* scope, keeping the enclosing class as
well.

Adding rather than replacing is safe in the direction that matters — it
cannot invent a report — but it hides one:

```php
class Owner {
    private function hidden(): void {}

    public function boot(Owner $o): void
    {
        Target::macro('x', function () use ($o): void {
            $o->hidden();        // fatal: the closure's scope is Target
        });
    }
}

class Target extends Owner {
    /** @param-closure-this static $macro */
    public static function macro(string $name, callable $macro): void {}
}
```

The closure is written inside `Owner`, so the enclosing class permits the
call, while at runtime the scope is `Target` and a parent's private
member is out of reach. (`self::hidden()` in the same closure is already
reported, because `self` itself resolves to the bound class.)

**Fix:** Take the bound class from the resolver, which already computes
it, and let it replace the enclosing class rather than joining it.
Inferring a binding from the spelling of the subject is guessing at
something the type engine has already decided.

## D28. "Remove unreachable code" is wired to PHPStan only

**Impact: Low-Medium · Complexity: Medium**

The action reads `phpstan_tool.last_diags` and nothing else
(`code_actions/phpstan/remove_unreachable.rs`), so the native
`unreachable_code` diagnostic never offers it. Adding the code to the
trigger is not enough on its own: the resolve step deletes from the
diagnostic's line to the next closing brace rather than using the
diagnostic's own range, which

- has nothing to delete for a dead run at the top level of a file, where
  no closing brace follows;
- ignores the reported span, so it would remove more than was dimmed;
- can swallow a hoisted declaration or a `goto` label sitting inside the
  run, both of which the diagnostic deliberately leaves reachable.

**Fix:** Take the range from the diagnostic and carry it through to the
resolve payload, and let the action accept a native diagnostic rather
than only a proxied one. Moving the file out of `phpstan/` is the
smallest part of it.

## D29. `namespace` and `declare` bodies break the reachability flow

**Impact: Low · Complexity: Low-Medium**

`unreachable_code` treats a braced `namespace` and a `declare` body as
fresh statement lists rather than as the transparent wrappers they are,
so reachability neither flows into them nor out of them:

```php
<?php
namespace First {
    return;          // ends the whole file
}

namespace Second {
    echo 'never';    // not reported
}
```

and, inside a function:

```php
return;

declare(ticks=1) {
    echo 'never';    // not reported
}
```

Neither wrapper is itself a runtime statement, so neither should be
dimmed, but the state on either side of it has to carry through.

**Fix:** Thread the reachable/unreachable state through both wrappers
instead of restarting the scan inside them.

---

## D30. Getting a private Symfony service from the container is not reported

**Impact: Low-Medium · Complexity: Medium**

```php
$container->get('app.mailer'); // throws: the service is private
```

Services are private by default since Symfony 3.4: a service is public only
when its declaration says `public: true` (or `->public()`, or
`public="true"` in XML), directly or through `_defaults`, or when an alias
to it is public. Getting a private one from the container throws at
runtime. PHPStan's Symfony extension reports it from the compiled
container; the service declarations PHPantom already reads say the same
for the project's own services. Only judge a service the project declares,
leave the test container (which exposes private services) alone, and treat
a service locator or a `ServiceSubscriberInterface` subscriber's own
`get()` as reading its locator rather than the container.

**Where to look:** `src/symfony/resources.rs` (the declarations),
`src/diagnostics/laravel_string_keys.rs` (where unknown service IDs are
judged), and PHPStan's Symfony extension (`ContainerInterfacePrivateServiceRule`).
The extension's fixture, `tests/fixtures/phpstan_symfony/private_service_in_test.fixture`,
guards the test container.

---

## D31. Doctrine mappings that disagree with the entity are not reported

**Impact: Low-Medium · Complexity: Medium-High**

```php
#[ORM\Column(type: 'string', nullable: true)]
private string $name; // the column can be null, the property cannot
```

PHPStan's Doctrine extension checks an entity's mapping against its PHP:

- a column's type and nullability against the property's type, both ways
  (what the database hands the property, and what the property writes);
- a relation against the property's type (a to-one is the target entity,
  a to-many a `Collection` of it);
- a `final` entity or constructor, which Doctrine cannot proxy unless
  native lazy objects are enabled;
- a mapping Doctrine itself rejects, such as an entity without an
  identifier;
- a Doctrine proxy class named directly.

The column types map to PHP types through a table of Doctrine's built-in
types, which custom types have to extend. Read the mapping from attributes
and from XML, the same mapping the entity and repository CodeLens reads.

**Where to look:** `src/framework.rs` (the Doctrine mapping index), and
PHPStan's Doctrine extension (`Rules/Doctrine/ORM/`, `Type/Doctrine/Descriptors/`).
The extension's fixtures are the `entity_*.fixture` fixtures in
`tests/fixtures/phpstan_doctrine/`, on the entities under
`tests/phpstan_doctrine/rules/Doctrine/ORM/`, whose custom column types
`entity-manager.php` registers.

---

## D40. A Doctrine entity mapped in XML or YAML is not checked for uninitialized properties

**Impact: Low · Complexity: Medium**

`uninitialized_property` reads a Doctrine entity's mapping attributes to
tell the fields Doctrine generates (an identifier, a version field) from
the ones the class has to initialize. An entity mapped in XML or YAML
carries none, so the check skips the entities the Doctrine mapping index
pairs with a repository, and still reports the generated identifier of
one mapped without a repository class. Reading each mapped entity's
generated fields into the index would let the check treat both kinds the
same.

**Where to look:** `src/framework.rs` (the Doctrine mapping index, which
D31 also needs) and `src/diagnostics/uninitialized_properties.rs`.

---

## D48. A `catch` its `try` cannot reach is not reported

**Impact: Low-Medium · Complexity: Medium**

```php
try {
    $this->repo->save($user);        // @throws ORMException only
} catch (JsonException $e) {         // can never run: not reported
}
```

PhpStorm reports a `catch` whose class nothing in the `try` body can throw,
and so does PHPStan (`catch.neverThrown`). The reachability check already
works out what a `try` body can throw (`ThrowPoints` in
`src/type_engine/variable/forward_walk/throw_points.rs`), and treats a
catch no exception can reach as dead code for variable flow, but nothing
reports the clause itself. With a diagnostic for it, PHPStan's fix (H24)
would work without PHPStan.

Only a body whose every call declares what it throws can rule a catch
out, so this stays quiet wherever an undeclared call could throw anything.
