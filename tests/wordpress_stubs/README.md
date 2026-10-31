# WordPress stubs fixtures

The type tests of [php-stubs/wordpress-stubs](https://github.com/php-stubs/wordpress-stubs),
ported to run in a project of their own. They check what the stubs declare,
argument-dependent return types, `@phpstan-assert` narrowing,
never-returning functions, and purity, against what PHPantom reads from
them.

- `data/` holds the `assertType()` fixtures of upstream's `tests/data/`
  (`assert/`, `impure/`, `pure/`, and `return/`), run by
  `tests/assert_type_runner.rs`. `data/output-constants.php` is
  PHPantom's own, for what upstream's fixtures take for granted.
- `Faker.php` is upstream's `tests/Faker.php`, the class the fixtures take
  values of a given type from.
- `stubs/wordpress-stubs.php` is the slice of the stubs the fixtures touch,
  copied by `scripts/extract-wordpress-stubs.php`, and `.phpantom.toml`
  makes it a stub path, the role the Composer package has in a project that
  installs it. Upstream's `tests/bootstrap.php` defines `OBJECT`, `ARRAY_A`
  and `ARRAY_N`; here PHPantom supplies them, as it does for every project
  the WordPress stubs are a stub path of.

A gap is an assertion marked `// SKIP: reason`, the reason describing what
PHPantom does not do yet. Each gap is filed in `docs/todo/`. Upstream's
`data/param/` fixtures are rule tests of the argument types the stubs
declare, not `assertType()` fixtures, and are not ported yet (see "The
argument types the WordPress stubs declare" in `docs/todo/wordpress.md`).

## Refreshing

The fixtures and the stubs are those of the release `composer.json` names
(v7.1.2).

```sh
mkdir -p target/scratch && cd target/scratch
git clone --depth 1 --branch v7.1.2 https://github.com/php-stubs/wordpress-stubs.git
cd -
rm -rf tests/wordpress_stubs/data/{assert,impure,pure,return}
cp -r target/scratch/wordpress-stubs/tests/data/{assert,impure,pure,return} tests/wordpress_stubs/data/
rm tests/wordpress_stubs/data/pure/*.neon
cp target/scratch/wordpress-stubs/tests/Faker.php tests/wordpress_stubs/
php scripts/extract-wordpress-stubs.php target/scratch/wordpress-stubs/wordpress-stubs.php \
    tests/wordpress_stubs/stubs/wordpress-stubs.php \
    tests/wordpress_stubs/data tests/wordpress_stubs/Faker.php
```

Then mark the assertions that fail again.
