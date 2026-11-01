<?php

declare(strict_types=1);

use PhpStubs\WordPress\Core\Tests\Faker;
use function PHPStan\Testing\assertType;

assertType('int<0, max>', block_core_post_time_to_read_word_count(Faker::string(), Faker::string()));

assertType('non-falsy-string', build_dropdown_script_block_core_categories(Faker::string()));

// Submenu icon
assertType('non-falsy-string', block_core_shared_navigation_render_submenu_icon());

// Render block
assertType('non-falsy-string', render_block_core_archives(Faker::array()));
assertType('non-falsy-string', render_block_core_categories(Faker::array(), Faker::string(), Faker::wpBlock()));
assertType('non-falsy-string', render_block_core_latest_comments(Faker::array()));
assertType('non-falsy-string', render_block_core_latest_posts(Faker::array()));
assertType('non-falsy-string', render_block_core_loginout(Faker::array()));
assertType('non-falsy-string', render_block_core_query_total(Faker::array(), Faker::string(), Faker::wpBlock()));
assertType('non-falsy-string', render_block_core_rss(Faker::array()));
assertType('non-falsy-string', render_block_core_search(Faker::array()));
assertType('non-falsy-string', render_block_core_site_logo(Faker::array()));
assertType('non-falsy-string', render_block_core_widget_group(Faker::array(), Faker::string(), Faker::wpBlock()));

// Build CSS
assertType('array{css_classes: list<string>, inline_styles: string}', block_core_home_link_build_css_colors(Faker::array()));
assertType('array{css_classes: list<string>, inline_styles: string, overlay_css_classes: list<string>, overlay_inline_styles: string}', block_core_navigation_build_css_colors(Faker::array()));
assertType('array{css_classes: list<string>, inline_styles: string}', block_core_navigation_build_css_font_sizes(Faker::array()));
assertType('array{css_classes: list<string>, inline_styles: string}', block_core_navigation_link_build_css_colors(Faker::array(), Faker::array()));
assertType('array{css_classes: list<string>, inline_styles: string}', block_core_navigation_link_build_css_font_sizes(Faker::array()));
assertType('array{css_classes: list<string>, inline_styles: string, overlay_css_classes: list<string>, overlay_inline_styles: string}', block_core_page_list_build_css_colors(Faker::array(), Faker::array()));

// Breadcrumbs
assertType('array{label: string, url: string}|array{label: string}', block_core_breadcrumbs_create_item(Faker::string(), Faker::bool())); // PHPantom keeps the two shapes apart where PHPStan merges them into one with an optional key
assertType("array{label: 'foo', url: string}|array{label: 'foo'}", block_core_breadcrumbs_create_item('foo', Faker::bool())); // PHPantom keeps the two shapes apart where PHPStan merges them into one with an optional key
assertType('array{label: string}', block_core_breadcrumbs_create_item(Faker::string(), false));
assertType('array{label: string, url: string}', block_core_breadcrumbs_create_item(Faker::string(), true));
