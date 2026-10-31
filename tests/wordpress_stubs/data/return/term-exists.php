<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function term_exists;
use function tag_exists;
use function PHPStan\Testing\assertType;

$termStr = Faker::string();
$termInt = Faker::int();
$termIntStr = Faker::union(Faker::int(), Faker::string());

// Empty taxonomy
assertType('string|null', term_exists(123)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('string|null', term_exists(123, '')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('string|null', term_exists($termStr)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('string|null', term_exists($termStr, '')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|string|null', term_exists($termInt)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|string|null', term_exists($termInt, '')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|string|null', term_exists($termIntStr)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|string|null', term_exists($termIntStr, '')); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Fixed taxonomy string
assertType('array{term_id: string, term_taxonomy_id: string}|null', term_exists(123, 'category')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array{term_id: string, term_taxonomy_id: string}|null', term_exists($termStr, 'category')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|array{term_id: string, term_taxonomy_id: string}|null', term_exists($termInt, 'category')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|array{term_id: string, term_taxonomy_id: string}|null', term_exists($termIntStr, 'category')); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Unknown taxonomy type
$taxonomy = (string)$_GET['taxonomy'] ?? '';
assertType('array{term_id: string, term_taxonomy_id: string}|string|null', term_exists(123, Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('array{term_id: string, term_taxonomy_id: string}|string|null', term_exists($termStr, $taxonomy)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|array{term_id: string, term_taxonomy_id: string}|string|null', term_exists($termInt, Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('0|array{term_id: string, term_taxonomy_id: string}|string|null', term_exists($termIntStr, Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read

// Term 0
assertType('0', term_exists(0));
assertType('0', term_exists(0, ''));
assertType('0', term_exists(0, 'category'));
assertType('0', term_exists(0, Faker::string()));

// Term empty string
assertType('null', term_exists('')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('null', term_exists('', '')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('null', term_exists('', 'category')); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('null', term_exists('', Faker::string())); // SKIP: the last of two `@phpstan-return` tags is not the one read

// tag_exists()
assertType('0', tag_exists(0));
assertType('null', tag_exists(''));
assertType('array{term_id: string, term_taxonomy_id: string}|null', tag_exists(123));
assertType('array{term_id: string, term_taxonomy_id: string}|null', tag_exists($termStr));
assertType('0|array{term_id: string, term_taxonomy_id: string}|null', tag_exists($termInt));
assertType('0|array{term_id: string, term_taxonomy_id: string}|null', tag_exists($termIntStr));
