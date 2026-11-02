<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function add_link;
use function edit_link;
use function get_default_link_to_edit;
use function wp_insert_link;
use function wp_update_link;
use function PHPStan\Testing\assertType;

/*
 * add_link()
 * calls wp_insert_link() with default $wp_error (false)
 */
assertType('int<0, max>', add_link());

/*
 * edit_link()
 * calls wp_update_link() if ! empty($link_id), wp_insert_link() with default $wp_error (false) otherwise
 */
assertType('int<0, max>', edit_link());
assertType('int<0, max>', edit_link(0));
assertType('int<0, max>', edit_link(123));
assertType('int<0, max>', edit_link(Faker::int()));

/*
 * wp_insert_link()
 */
assertType('int<0, max>', wp_insert_link([]));
assertType('int<0, max>', wp_insert_link([], false));
assertType('int<0, max>|WP_Error', wp_insert_link([], true));
assertType('int<0, max>|WP_Error', wp_insert_link([], Faker::bool()));

/*
 * wp_update_link()
 * calls wp_insert_link() with default $wp_error (false)
 */
assertType('int<0, max>', wp_update_link([]));
assertType('int<0, max>', wp_update_link(['link_id' => 123]));
assertType('int<0, max>', wp_update_link(Faker::array()));

/*
 * get_default_link_to_edit()
 */
assertType("object{link_url: string, link_name: string, link_visible: 'Y'}&stdClass", get_default_link_to_edit());

/*
 * wp_get_link_cats()
 */
assertType('array{}', wp_get_link_cats());
assertType('array{}', wp_get_link_cats(0));
assertType('array<int, int<1, max>>', wp_get_link_cats(123));
assertType('array<int, int<1, max>>', wp_get_link_cats(Faker::int()));
