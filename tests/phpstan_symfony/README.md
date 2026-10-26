# PHPStan Symfony extension fixtures

The type and rule tests of [phpstan/phpstan-symfony](https://github.com/phpstan/phpstan-symfony),
ported to run in a project of their own:

- `data/` holds the `assertType()` fixtures of `tests/Type/Symfony/data/`,
  run by `tests/assert_type_runner.rs`.
- `rules/` holds the files the rule tests of `tests/Rules/` analyse. The
  tests themselves are the `.fixture` files in `tests/fixtures/phpstan_symfony/`,
  run by `tests/fixture_runner.rs`.
- `stubs/` is the slice of Symfony 7.4 the fixtures touch: the classes
  `stub-classes.txt` lists, with their ancestors, copied with their
  declarations and docblocks and their function bodies emptied.
- `config/` stands in for the booted application the extension reads:
  `services.xml` is the type tests' `container.xml`, and `rules.xml` the
  rule tests'. The console application the extension loads is the command
  classes themselves, which declare their arguments and options in
  `configure()`.
- `data/without-container/` is a second project, with no container
  configured, for the fixture upstream runs without one.

A gap is an assertion or expected error marked `// SKIP: reason`, the reason
describing what PHPantom does not do yet. Where Symfony 7.4 declares a type
differently from the version upstream targets, the expectation follows
Symfony 7.4 and says so in a comment.

Not ported, because upstream skips them on Symfony 6 and later: the tests of
the removed `Controller` class (`ExampleController*.php`, and the container
rule tests that need it), the `MessageSubscriberInterface` fixture, and
`request_get_session_null.php`.

## Refreshing the stubs

```sh
mkdir -p target/scratch/stubs && cd target/scratch/stubs
# A composer.json requiring the packages composer.json lists, then:
composer install --no-scripts --no-plugins
cd -
rm -rf tests/phpstan_symfony/stubs
php scripts/extract-framework-stubs.php target/scratch/stubs/vendor \
    tests/phpstan_symfony/stubs tests/phpstan_symfony/stub-classes.txt
```
