<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function sanitize_term_field;
use function PHPStan\Testing\assertType;

$termId = Faker::int();
$taxonomy = Faker::string();

// Int fields
assertType('int<0, max>', sanitize_term_field('parent', Faker::string(), $termId, $taxonomy, 'raw'));
assertType('int<0, max>', sanitize_term_field('term_id', Faker::string(), $termId, $taxonomy, 'raw'));
assertType('int<0, max>', sanitize_term_field('count', Faker::string(), $termId, $taxonomy, 'raw'));
assertType('int<0, max>', sanitize_term_field('term_group', Faker::string(), $termId, $taxonomy, 'raw'));
assertType('int<0, max>', sanitize_term_field('term_taxonomy_id', Faker::string(), $termId, $taxonomy, 'raw'));
assertType('int<0, max>', sanitize_term_field('object_id', Faker::string(), $termId, $taxonomy, 'raw'));
// Also int range if constant numeric string
assertType('int<0, max>', sanitize_term_field('parent', '123', $termId, $taxonomy, 'raw'));
// Also int range in any other context
assertType('int<0, max>', sanitize_term_field('parent', Faker::string(), $termId, $taxonomy, Faker::string()));

// Non int fields in raw context
assertType("'field value'", sanitize_term_field('field', 'field value', $termId, $taxonomy, 'raw'));
assertType('string', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'raw'));

// Non int field values in edit context may be filtered to mixed, but are escaped using esc_html or esc_attr => string
assertType('string', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'edit'));
assertType('string', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'edit'));

// Non int field values in attribute/js context are not filtered, but are escaped using esc_attr/esc_js
// => string, but not as given in the argument
assertType('string', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'attribute'));
assertType('string', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'attribute'));
assertType('string', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'js'));
assertType('string', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'js'));

// Non int fields in any other context may be filtered to mixed => mixed
assertType('mixed', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'db'));
assertType('mixed', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'display'));
assertType('mixed', sanitize_term_field('field', 'field value', $termId, $taxonomy, 'rss'));
assertType('mixed', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'db'));
assertType('mixed', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'display'));
assertType('mixed', sanitize_term_field('field', Faker::string(), $termId, $taxonomy, 'rss'));

// Non constant field in raw context => int<0, max> (from int field) or T (from other field)
assertType("'field value'|int<0, max>", sanitize_term_field(Faker::string(), 'field value', $termId, $taxonomy, 'raw'));
assertType('int<0, max>|string', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'raw'));

// Non constant field in attribute|edit|js context => int<0, max> (from int field) or string (from other field)
assertType('int<0, max>|string', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'attribute'));
assertType('int<0, max>|string', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'edit'));
assertType('int<0, max>|string', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'js'));

// Non constant field in any other context than attribute|edit|js|raw => mixed
assertType('mixed', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'db'));
assertType('mixed', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'display'));
assertType('mixed', sanitize_term_field(Faker::string(), Faker::string(), $termId, $taxonomy, 'rss'));
