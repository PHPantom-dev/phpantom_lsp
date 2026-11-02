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
assertType('null', comment_class());
assertType('null', edit_term_link());
assertType('null', get_calendar());
assertType('null', next_posts());
assertType('null', post_type_archive_title());
assertType('null', previous_posts());
assertType('null', single_cat_title());
assertType('false|null', single_month_title());
assertType('null', single_post_title());
assertType('null', single_tag_title());
assertType('null', single_term_title());
assertType('null', the_date());
assertType('null', the_modified_date());
assertType('null', the_title());
assertType('null', wp_loginout());
assertType('null', wp_register());
assertType('null', wp_title());

// Explicit value of true
$value = true;
assertType('null', comment_class('', null, null, $value));
assertType('null', edit_term_link('', '', '', null, $value));
assertType('null', get_calendar(['initial' => true, 'display' => $value]));
assertType('null', next_posts(0, $value));
assertType('null', post_type_archive_title('', $value));
assertType('null', previous_posts($value));
assertType('null', single_cat_title('', $value));
assertType('false|null', single_month_title('', $value));
assertType('null', single_post_title('', $value));
assertType('null', single_tag_title('', $value));
assertType('null', single_term_title('', $value));
assertType('null', the_date('', '', '', $value));
assertType('null', the_modified_date('', '', '', $value));
assertType('null', the_title('', '', $value));
assertType('null', wp_loginout('', $value));
assertType('null', wp_register('', '', $value));
assertType('null', wp_title('', $value));

// Explicit value of false
$value = false;
assertType('string', comment_class('', null, null, $value));
assertType('string|null', edit_term_link('', '', '', null, $value));
assertType('string', get_calendar(['initial' => true, 'display' => $value]));
assertType('string', next_posts(0, $value));
assertType('string|null', post_type_archive_title('', $value));
assertType('string', previous_posts(false));
assertType('string|null', single_cat_title('', $value));
assertType('string|false', single_month_title('', $value));
assertType('string|null', single_post_title('', $value));
assertType('string|null', single_tag_title('', $value));
assertType('string|null', single_term_title('', $value));
assertType('string', the_date('', '', '', $value));
assertType('string', the_modified_date('', '', '', $value));
assertType('string|null', the_title('', '', $value));
assertType('string', wp_loginout('', $value));
assertType('string', wp_register('', '', $value));
assertType('string', wp_title('', $value));

// Unknown boolean value
$value = Faker::bool();
assertType('string|null', comment_class('', null, null, $value));
assertType('string|null', edit_term_link('', '', '', null, $value));
assertType('string|null', get_calendar(['initial' => true, 'display' => $value]));
assertType('string|null', next_posts(0, $value));
assertType('string|null', post_type_archive_title('', $value));
assertType('string|null', previous_posts($value));
assertType('string|null', single_cat_title('', $value));
assertType('string|false|null', single_month_title('', $value));
assertType('string|null', single_post_title('', $value));
assertType('string|null', single_tag_title('', $value));
assertType('string|null', single_term_title('', $value));
assertType('string|null', the_date('', '', '', $value));
assertType('string|null', the_modified_date('', '', '', $value));
assertType('string|null', the_title('', '', $value));
assertType('string|null', wp_loginout('', $value));
assertType('string|null', wp_register('', '', $value));
assertType('string|null', wp_title('', $value));
