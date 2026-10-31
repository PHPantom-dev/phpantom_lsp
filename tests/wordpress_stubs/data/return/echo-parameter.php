<?php

/**
 * Note:
 * Starting from PHPStan 1.10.49, void types, including void in unions, are
 * transformed into null.
 *
 * @link https://github.com/phpstan/phpstan-src/pull/2778
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function comment_class;
use function edit_term_link;
use function get_calendar;
use function next_posts;
use function post_type_archive_title;
use function previous_posts;
use function single_cat_title;
use function single_month_title;
use function single_post_title;
use function single_tag_title;
use function single_term_title;
use function the_date;
use function the_modified_date;
use function the_title;
use function wp_loginout;
use function wp_register;
use function wp_title;

use function PHPStan\Testing\assertType;

// Default value of true
assertType('null', comment_class()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', edit_term_link()); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('null', get_calendar()); // SKIP: an array-shape condition matches any array
assertType('null', next_posts()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', post_type_archive_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', previous_posts()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', single_cat_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('false|null', single_month_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', single_post_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', single_tag_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', single_term_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', the_date()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', the_modified_date()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', the_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', wp_loginout()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', wp_register()); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('null', wp_title()); // SKIP: a default written `\true`, `\false` or `\null` is not read

// Explicit value of true
$value = true;
assertType('null', comment_class('', null, null, $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', edit_term_link('', '', '', null, $value)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('null', get_calendar(['initial' => true, 'display' => $value])); // SKIP: an array-shape condition matches any array
assertType('null', next_posts(0, $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', post_type_archive_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', previous_posts($value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', single_cat_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('false|null', single_month_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', single_post_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', single_tag_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', single_term_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', the_date('', '', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', the_modified_date('', '', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', the_title('', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', wp_loginout('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', wp_register('', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('null', wp_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`

// Explicit value of false
$value = false;
assertType('string', comment_class('', null, null, $value));
assertType('string|null', edit_term_link('', '', '', null, $value)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('string', get_calendar(['initial' => true, 'display' => $value]));
assertType('string', next_posts(0, $value));
assertType('string|null', post_type_archive_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string', previous_posts(false));
assertType('string|null', single_cat_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|false', single_month_title('', $value));
assertType('string|null', single_post_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_tag_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_term_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string', the_date('', '', '', $value));
assertType('string', the_modified_date('', '', '', $value));
assertType('string|null', the_title('', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string', wp_loginout('', $value));
assertType('string', wp_register('', '', $value));
assertType('string', wp_title('', $value));

// Unknown boolean value
$value = Faker::bool();
assertType('string|null', comment_class('', null, null, $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', edit_term_link('', '', '', null, $value)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('string|null', get_calendar(['initial' => true, 'display' => $value])); // SKIP: an array-shape condition matches any array
assertType('string|null', next_posts(0, $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', post_type_archive_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', previous_posts($value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_cat_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|false|null', single_month_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_post_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_tag_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', single_term_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', the_date('', '', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', the_modified_date('', '', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', the_title('', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_loginout('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_register('', '', $value)); // SKIP: `void` from a conditional return type is not read as `null`
assertType('string|null', wp_title('', $value)); // SKIP: `void` from a conditional return type is not read as `null`
