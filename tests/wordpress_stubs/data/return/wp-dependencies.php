<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

assertType('array<string, int|false>', Faker::wpDependencies()->groups);
assertType('array<string, int|false>', Faker::wpScripts()->groups);
assertType('array<string, int|false>', Faker::wpStyles()->groups);

assertType('non-falsy-string', Faker::wpDependencies()->get_etag(Faker::array(Faker::string())));
assertType('non-falsy-string', Faker::wpScripts()->get_etag(Faker::array(Faker::string())));
assertType('non-falsy-string', Faker::wpStyles()->get_etag(Faker::array(Faker::string())));

// Always false if $handle is not non-empty-string
assertType('false', Faker::wpDependencies()->query(null, Faker::string()));
assertType('false', Faker::wpDependencies()->query('', Faker::string()));
assertType('false', Faker::wpScripts()->query('', Faker::string()));
assertType('false', Faker::wpStyles()->query('', Faker::string()));

assertType('_WP_Dependency|false', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'registered'));
assertType('_WP_Dependency|false', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'scripts'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'enqueued'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'queued'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'to_do'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'to_print'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'done'));
assertType('bool', Faker::wpDependencies()->query(Faker::nonEmptyString(), 'printed'));
assertType('_WP_Dependency|false', Faker::wpScripts()->query(Faker::nonEmptyString(), 'registered'));
assertType('bool', Faker::wpScripts()->query(Faker::nonEmptyString(), 'enqueued'));
assertType('_WP_Dependency|false', Faker::wpStyles()->query(Faker::nonEmptyString(), 'registered'));
assertType('bool', Faker::wpStyles()->query(Faker::nonEmptyString(), 'enqueued'));
