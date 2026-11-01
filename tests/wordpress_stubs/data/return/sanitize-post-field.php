<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function sanitize_post_field;
use function PHPStan\Testing\assertType;

$postId = Faker::int();

// Int fields => int - regardless of (non-constant) $value
assertType('int', sanitize_post_field('ID', Faker::mixed(), $postId, 'raw')); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('int', sanitize_post_field('post_parent', Faker::mixed(), $postId, 'raw')); // SKIP: a conditional over a method template ignores its bound and does not narrow it
assertType('int', sanitize_post_field('menu_order', Faker::mixed(), $postId, 'raw')); // SKIP: a conditional over a method template ignores its bound and does not narrow it
// Also in any other context
assertType('int', sanitize_post_field('ID', Faker::mixed(), $postId, Faker::string())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
// But as-is if $value is int or int range
assertType('123', sanitize_post_field('ID', 123, $postId, 'raw'));
assertType('int<1, max>', sanitize_post_field('ID', Faker::positiveInt(), $postId, 'raw'));
assertType('123', sanitize_post_field('ID', 123, $postId, Faker::string()));

// Int array fields => array<int<0, max>> - regardless of (non-constant) $value and $context
assertType('array<int<0, max>>', sanitize_post_field('ancestors', Faker::mixed(), $postId, Faker::string())); // SKIP: a conditional over a method template ignores its bound and does not narrow it
// But as-is if $value is array<int<0, max>> or list<int<0, max>> - regardless of $context
assertType('array{41, 42, 43}', sanitize_post_field('ancestors', [41, 42, 43], $postId, Faker::string()));
assertType('array{foo: 42}', sanitize_post_field('ancestors', ['foo' => 42], $postId, Faker::string()));
// And retain list type - regardless of $context
assertType('array<int<0, max>, int<0, max>>', sanitize_post_field('ancestors', Faker::list(), $postId, Faker::string())); // SKIP: a conditional over a method template ignores its bound and does not narrow it

// All other constant fields:

// In raw context => as-is
assertType("'field value'", sanitize_post_field('field', 'field value', $postId, 'raw'));
assertType('123', sanitize_post_field('field', 123, $postId, 'raw'));
assertType("array{foo: 'bar'}", sanitize_post_field('field', ['foo' => 'bar'], $postId, 'raw'));
assertType('true', sanitize_post_field('field', true, $postId, 'raw'));
assertType('bool', sanitize_post_field('field', Faker::bool(), $postId, 'raw'));
assertType('int', sanitize_post_field('field', Faker::int(), $postId, 'raw'));
assertType('string', sanitize_post_field('field', Faker::string(), $postId, 'raw'));

// Field values in edit may be filtered to mixed, but are escaped using esc_html or esc_attr => string
assertType('string', sanitize_post_field('field', 'field value', $postId, 'edit'));
assertType('string', sanitize_post_field('field', Faker::string(), $postId, 'edit'));
// Except for field 'post_content' which is only escaped if ! user_can_richedit()
assertType('mixed', sanitize_post_field('post_content', 'field value', $postId, 'edit'));
assertType('mixed', sanitize_post_field('post_content', Faker::string(), $postId, 'edit'));

// Field values in attribute/js context are not filtered, but are escaped using esc_attr/esc_js
// => string, but not as given in the argument
assertType('string', sanitize_post_field('field', 'field value', $postId, 'attribute'));
assertType('string', sanitize_post_field('field', Faker::string(), $postId, 'attribute'));
assertType('string', sanitize_post_field('field', 'field value', $postId, 'js'));
assertType('string', sanitize_post_field('field', Faker::string(), $postId, 'js'));

// Field values in any other context may be filtered to mixed => mixed
assertType('mixed', sanitize_post_field('field', Faker::array(), $postId, 'db'));
assertType('mixed', sanitize_post_field('field', Faker::array(), $postId, 'display'));
assertType('mixed', sanitize_post_field('field', Faker::array(), $postId, 'rss'));
assertType('mixed', sanitize_post_field('field', Faker::bool(), $postId, 'db'));
assertType('mixed', sanitize_post_field('field', Faker::bool(), $postId, 'display'));
assertType('mixed', sanitize_post_field('field', Faker::bool(), $postId, 'rss'));
assertType('mixed', sanitize_post_field('field', Faker::int(), $postId, 'db'));
assertType('mixed', sanitize_post_field('field', Faker::int(), $postId, 'display'));
assertType('mixed', sanitize_post_field('field', Faker::int(), $postId, 'rss'));
assertType('mixed', sanitize_post_field('field', Faker::object(), $postId, 'db'));
assertType('mixed', sanitize_post_field('field', Faker::object(), $postId, 'display'));
assertType('mixed', sanitize_post_field('field', Faker::object(), $postId, 'rss'));
assertType('mixed', sanitize_post_field('field', Faker::string(), $postId, 'db'));
assertType('mixed', sanitize_post_field('field', Faker::string(), $postId, 'display'));
assertType('mixed', sanitize_post_field('field', Faker::string(), $postId, 'rss'));

// Non constant field:

// Non constant field in raw context => int (int field) or array<int<0, max>> (int array field) or T (other field)
assertType("'field value'|array<int<0, max>>|int", sanitize_post_field(Faker::string(), 'field value', $postId, 'raw'));
assertType('array<int<0, max>>|int|string', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'raw'));

// Non constant field in attribute|js context
// => int (int field) or array<int<0, max>> (int array field) or string (other field)
assertType('array<int<0, max>>|int|string', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'attribute'));
assertType('array<int<0, max>>|int|string', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'js'));

// Non constant field in attribute|js context than raw
// => int (int field) or array<int<0, max>> (int array field) or string (other field)
assertType('array<int<0, max>>|int|string', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'attribute'));
assertType('array<int<0, max>>|int|string', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'js'));

// Non constant field in any other context than attribute|js|raw => mixed
assertType('mixed', sanitize_post_field(Faker::string(), 123, $postId, 'db'));
assertType('mixed', sanitize_post_field(Faker::string(), 'field value', $postId, 'display'));
assertType('mixed', sanitize_post_field(Faker::string(), Faker::array(), $postId, 'rss'));
// And because of 'post_content':
assertType('mixed', sanitize_post_field(Faker::string(), Faker::string(), $postId, 'edit'));
