<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function wp_clear_scheduled_hook;
use function wp_insert_attachment;
use function wp_insert_category;
use function wp_insert_post;
use function wp_reschedule_event;
use function wp_schedule_event;
use function wp_schedule_single_event;
use function wp_set_comment_status;
use function wp_unschedule_event;
use function wp_unschedule_hook;
use function wp_update_comment;
use function wp_update_post;
use function PHPStan\Testing\assertType;

/*
 * wp_insert_category()
 */
assertType('int<0, max>', wp_insert_category([])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('int<0, max>', wp_insert_category([], false));
assertType('int<1, max>|WP_Error', wp_insert_category([], true));
assertType('int<0, max>|WP_Error', wp_insert_category([], Faker::bool()));

/*
 * wp_set_comment_status()
 */
assertType('bool', wp_set_comment_status(1, 'spam')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_set_comment_status(1, 'spam', false));
assertType('WP_Error|true', wp_set_comment_status(1, 'spam', true));
assertType('bool|WP_Error', wp_set_comment_status(1, 'spam', Faker::bool()));

/*
 * wp_update_comment()
 */
assertType('0|1|false', wp_update_comment([])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('0|1|false', wp_update_comment([], false));
assertType('0|1|WP_Error', wp_update_comment([], true));
assertType('0|1|WP_Error|false', wp_update_comment([], Faker::bool()));

/*
 * wp_schedule_single_event()
 */
assertType('bool', wp_schedule_single_event(1, 'hook')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_schedule_single_event(1, 'hook', [])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_schedule_single_event(1, 'hook', [], false));
assertType('WP_Error|true', wp_schedule_single_event(1, 'hook', [], true));
assertType('bool|WP_Error', wp_schedule_single_event(1, 'hook', [], Faker::bool()));

/*
 * wp_schedule_event()
 */
assertType('bool', wp_schedule_event(1, 'daily', 'hook')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_schedule_event(1, 'daily', 'hook', [])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_schedule_event(1, 'daily', 'hook', [], false));
assertType('WP_Error|true', wp_schedule_event(1, 'daily', 'hook', [], true));
assertType('bool|WP_Error', wp_schedule_event(1, 'daily', 'hook', [], Faker::bool()));

/**
 * wp_reschedule_event()
 */
assertType('bool', wp_reschedule_event(1, 'daily', 'hook')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_reschedule_event(1, 'daily', 'hook', [])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_reschedule_event(1, 'daily', 'hook', [], false));
assertType('WP_Error|true', wp_reschedule_event(1, 'daily', 'hook', [], true));
assertType('bool|WP_Error', wp_reschedule_event(1, 'daily', 'hook', [], Faker::bool()));

/*
 * wp_unschedule_event()
 */
assertType('bool', wp_unschedule_event(1, 'hook')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_unschedule_event(1, 'hook', [])); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('bool', wp_unschedule_event(1, 'hook', [], false));
assertType('WP_Error|true', wp_unschedule_event(1, 'hook', [], true));
assertType('bool|WP_Error', wp_unschedule_event(1, 'hook', [], Faker::bool()));

/*
 * wp_clear_scheduled_hook()
 */
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook')); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook', [])); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook', [], false)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('int<0, max>|WP_Error', wp_clear_scheduled_hook('hook', [], true)); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated
assertType('int<0, max>|WP_Error|false', wp_clear_scheduled_hook('hook', [], Faker::bool())); // SKIP: a conditional type inside a union, an intersection or a generic argument is not evaluated

/*
 * wp_unschedule_hook()
 */
assertType('int<0, max>|false', wp_unschedule_hook('hook')); // SKIP: a default written `\true`, `\false` or `\null` is not read
assertType('int<0, max>|false', wp_unschedule_hook('hook', false));
assertType('int<0, max>|WP_Error', wp_unschedule_hook('hook', true));
assertType('int<0, max>|WP_Error|false', wp_unschedule_hook('hook', Faker::bool()));

/*
 * wp_insert_post()
 */
assertType('int<0, max>', wp_insert_post([])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_post([], false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_post([], false, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_post([], false, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_post([], true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_post([], true, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_post([], true, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool(), true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool(), false)); // SKIP: the last of two `@phpstan-return` tags is not the one read

/*
 * wp_update_post()
 */
assertType('int<0, max>', wp_update_post([])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_update_post([], false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_update_post([], false, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_update_post([], false, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_update_post([], true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_update_post([], true, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_update_post([], true, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool(), true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool(), false)); // SKIP: the last of two `@phpstan-return` tags is not the one read

/*
 * wp_insert_attachment()
 */
assertType('int<0, max>', wp_insert_attachment([])); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], true, 1)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], true, 1, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], true, 1, false, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>', wp_insert_attachment([], true, 1, false, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true, true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true, false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool())); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool(), true)); // SKIP: the last of two `@phpstan-return` tags is not the one read
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool(), false)); // SKIP: the last of two `@phpstan-return` tags is not the one read
