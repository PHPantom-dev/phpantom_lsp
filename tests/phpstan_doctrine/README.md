# PHPStan Doctrine extension fixtures

The type and rule tests of [phpstan/phpstan-doctrine](https://github.com/phpstan/phpstan-doctrine),
ported to run in a project of their own:

- `data/` holds the `assertType()` fixtures of `tests/Type/Doctrine/`, laid
  out like upstream minus the `data/` segments, run by
  `tests/assert_type_runner.rs`.
- `rules/` holds the files the rule tests of `tests/Rules/` analyse. The
  tests themselves are the `.fixture` files in `tests/fixtures/phpstan_doctrine/`,
  run by `tests/fixture_runner.rs`.
- `stubs/` is the slice of Doctrine ORM 3, DBAL 4, Collections, and
  Persistence the fixtures touch: the classes `stub-classes.txt` lists, with
  their ancestors, copied with their declarations and docblocks and their
  function bodies emptied.
- The `entity-manager*.php` files stand in for the object managers the
  extension boots: the source that builds them, next to the fixtures that
  depend on them. Upstream maps its entities with annotations, which ORM 3
  no longer reads, so the entities here carry the same mapping as
  attributes, and the rule tests' expected lines follow the rewritten files.
- `data/DBAL/pdo/` and `data/DBAL/mysqli/` are projects of their own, one
  per connection driver the row-count fixtures depend on.

A gap is an assertion or expected error marked `// SKIP: reason`, the reason
describing what PHPantom does not do yet. Where Doctrine declares a type
differently from what upstream expects, the expectation follows the
declaration and says so in a comment.

Each rule test is ported in its default configuration: PHPantom has no
setting like `allowNullablePropertyForRequiredField`, and reads the mapping
the same way with or without an object manager, so the variants that differ
only by those are not ported twice. The DBAL 3 row-count fixture is not
ported either.

## Refreshing the stubs

```sh
mkdir -p target/scratch/stubs && cd target/scratch/stubs
# A composer.json requiring the packages composer.json lists, then:
composer install --no-scripts --no-plugins
cd -
rm -rf tests/phpstan_doctrine/stubs
php scripts/extract-framework-stubs.php target/scratch/stubs/vendor \
    tests/phpstan_doctrine/stubs tests/phpstan_doctrine/stub-classes.txt
```
