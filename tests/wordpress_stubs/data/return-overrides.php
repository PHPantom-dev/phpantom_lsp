<?php

/**
 * The return types PHPantom gives functions of the WordPress stubs where an
 * argument decides a type narrower than the one the stubs declare.
 */

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

/** @var bool $bool */
$bool = Faker::bool();
/** @var string $string */
$string = Faker::string();

// Meta: one value, every value of a key, or every key
assertType('mixed', get_post_meta(1, 'color', true));
assertType('list<mixed>|false', get_post_meta(1, 'color'));
assertType('list<mixed>|false', get_post_meta(1, 'color', false));
assertType('mixed', get_post_meta(1, 'color', $bool));
assertType('array<string, list<string>>|false', get_post_meta(1));
assertType("array<string, list<string>>|''|false", get_post_meta(1, '', true));
assertType('array<string, list<string>>|false|list<mixed>', get_post_meta(1, $string));
assertType('list<mixed>|false', get_user_meta(1, 'color'));
assertType('list<mixed>|false', get_term_meta(1, 'color'));
assertType('list<mixed>|false', get_comment_meta(1, 'color'));
assertType('list<mixed>|false', get_site_meta(1, 'color'));
assertType('list<mixed>|false', get_metadata('post', 1, 'color'));
assertType('mixed', get_metadata('post', 1, 'color', true));

// Time: a timestamp only for the 'U' and 'G' formats
assertType('int|false', get_post_time());
assertType('int|false', get_post_time('G'));
assertType('string|false', get_post_time('c'));
assertType('int|false|string', get_post_time($string));
assertType('int|false', get_post_modified_time());
assertType('string|false', get_post_modified_time('Y-m-d'));
assertType('int|false|string', get_the_time());
assertType('int|false', get_the_time('U'));
assertType('string|false', get_the_time('H:i'));
assertType('string|false', get_the_modified_time('H:i'));
assertType('int|false|string', get_the_date());
assertType('string|false', get_the_date('Y-m-d'));
assertType('int|false', get_the_modified_date('U'));

// Comments: a string only when 'echo' is false
assertType('null', wp_list_comments());
assertType('null', wp_list_comments(['style' => 'ol']));
assertType('string|null', wp_list_comments(['style' => 'ol', 'echo' => false]));
assertType('string|null', wp_list_comments(['echo' => $bool]));
