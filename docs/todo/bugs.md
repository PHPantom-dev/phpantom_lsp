# PHPantom — Bug Fixes

Every bug below must be fixed at its root cause. "Detect the
symptom and suppress the diagnostic" is not an acceptable fix.
If the type resolution pipeline produces wrong data, fix the
pipeline so it produces correct data. Downstream consumers
(diagnostics, hover, completion, definition) should never need
to second-guess upstream output.

Each entry below carries an **Impact · Complexity** rating using the same
scale defined in [`docs/todo.md`](../todo.md); that table is also where
each bug's row lives in the current sprint/backlog.

Bugs land here from wherever they surface: found while working on another
task, or sweeps of the sample projects under `projects/`. Entries are
grouped by the mechanism that has to change, not by the symptom that
surfaced: one entry is one root cause, however many shapes it shows up in.

## Crashes

No outstanding items.

## Type comparison

No outstanding items.

## Standard-library return types

No outstanding items.

## Reachability

No outstanding items.

## Narrowing

No outstanding items.

## Arithmetic

No outstanding items.

## Symbol resolution

No outstanding items.

## Array types

No outstanding items.

## Docblock handling

## B366. A union continued on the next docblock line loses the whole type

**Impact: Low · Complexity: Low-Medium**

`@param array<int, Widget>` followed by a line starting `|Widget $items`
is one type, `array<int, Widget>|Widget`. `split_type_token`
(`src/docblock/type_strings.rs`) ends the token at the whitespace before
the continuation's `|`, so the rest is read as the variable name, the tag
matches no parameter, and the parameter keeps no type at all. The
union/intersection suffix it consumes after a closing `>` or `}` has to
look past whitespace (including the joined line break) for a leading `|`
or `&`. Pinned by the ignored
`docblock_types::param_type_continued_on_the_next_line_is_one_union`.

## Laravel

## B360. References, reference lenses, and rename miss an Eloquent magic member's uses

**Impact: Medium · Complexity: Medium-High**

A scope, accessor, or mutator is declared under one name and used under
another: `scopeActive()` is called as `active()`, `getFullNameAttribute()`
and an `Attribute`-returning `fullName()` are read as `->full_name`,
`setLogoAttribute()` is written as `->logo = …`. Go-to-definition follows
a use back to its declaration (`src/definition/member/`), but nothing under
`src/references/` maps the declaration forward, so find-references on the
declaring method finds only direct calls to it. The member reference
lens (`indexed_member_reference_count` in `src/code_lens.rs`) counts the
same way, so live scopes and accessors read "0 references", and a
rename of the declaration strands every use. Relationship methods have
the same shape through their `->posts` property reads. Pinned by the
four ignored `definition_laravel::references_on_*` tests.

## B361. `SoftDeletes` does not contribute a `deleted_at` column

**Impact: Low-Medium · Complexity: Medium**

`SoftDeletes::initializeSoftDeletes()` adds a `datetime` cast for
`getDeletedAtColumn()` (`deleted_at` unless the model defines a
`DELETED_AT` constant), so a soft-deleting model has a `Carbon|null`
`$deleted_at` and a `whereDeletedAt()` finder. Nothing in
`src/virtual_members/laravel/` synthesises either outside the migration
parser, so a model without a migration or schema gets neither. The model
extraction that already reads `CREATED_AT`/`UPDATED_AT` is the place to
record the trait and the column name, including through a parent model.
Pinned by the ignored
`laravel_eloquent_magic::soft_deletes_contributes_a_deleted_at_column`.

## B362. The base `Model`'s own declared properties become `where{Column}` methods

**Impact: Medium · Complexity: Medium**

`collect_column_names` (`src/virtual_members/laravel/where_property.rs`)
counts every property on the inheritance-resolved class as a column, and
that includes the properties Eloquent's `Model` and its traits declare for
configuration. Against the real framework, `User::` offers `whereTable`,
`whereConnection`, `whereIncrementing`, `wherePerPage`, and the rest,
none of which Laravel treats as a column (a declared property is never an
attribute: `__get` only runs for undeclared or inaccessible names). The
properties declared by the framework's base model need excluding the way
`base_model_methods` already excludes its methods in
`src/virtual_members/laravel/mod.rs`; the other callers of
`collect_column_names` (`class_lookup.rs`,
`diagnostics/type_errors/compatibility.rs`) share the fix. Pinned by the
ignored `laravel_eloquent_magic::the_base_models_own_properties_are_not_columns`.

## B363. Find-references on a route's `->name()` registration finds nothing

**Impact: Low-Medium · Complexity: Low-Medium**

Find-references on a `route('users.index')` call lists every call and,
with the declaration included, the `->name('users.index')` that registers
it. Starting from that `->name()` literal returns nothing: the literal is
not a `LaravelStringKey` span, so `src/references/dispatch.rs` never
reaches the string-key path. A config key already answers from its
declaration (`test_find_references_laravel_config_from_declaration_site`);
a route registration needs the same, through the route table
`resolve_route_definitions` reads. Pinned by the ignored
`laravel_route_names::find_references_from_a_route_registration_reaches_its_calls`.

## Blade

No outstanding items.

## Miscellaneous

No outstanding items.
