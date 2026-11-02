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
assertType('int<0, max>', wp_insert_category([]));
assertType('int<0, max>', wp_insert_category([], false));
assertType('int<1, max>|WP_Error', wp_insert_category([], true));
assertType('int<0, max>|WP_Error', wp_insert_category([], Faker::bool()));

/*
 * wp_set_comment_status()
 */
assertType('bool', wp_set_comment_status(1, 'spam'));
assertType('bool', wp_set_comment_status(1, 'spam', false));
assertType('WP_Error|true', wp_set_comment_status(1, 'spam', true));
assertType('bool|WP_Error', wp_set_comment_status(1, 'spam', Faker::bool()));

/*
 * wp_update_comment()
 */
assertType('0|1|false', wp_update_comment([]));
assertType('0|1|false', wp_update_comment([], false));
assertType('0|1|WP_Error', wp_update_comment([], true));
assertType('0|1|WP_Error|false', wp_update_comment([], Faker::bool()));

/*
 * wp_schedule_single_event()
 */
assertType('bool', wp_schedule_single_event(1, 'hook'));
assertType('bool', wp_schedule_single_event(1, 'hook', []));
assertType('bool', wp_schedule_single_event(1, 'hook', [], false));
assertType('WP_Error|true', wp_schedule_single_event(1, 'hook', [], true));
assertType('bool|WP_Error', wp_schedule_single_event(1, 'hook', [], Faker::bool()));

/*
 * wp_schedule_event()
 */
assertType('bool', wp_schedule_event(1, 'daily', 'hook'));
assertType('bool', wp_schedule_event(1, 'daily', 'hook', []));
assertType('bool', wp_schedule_event(1, 'daily', 'hook', [], false));
assertType('WP_Error|true', wp_schedule_event(1, 'daily', 'hook', [], true));
assertType('bool|WP_Error', wp_schedule_event(1, 'daily', 'hook', [], Faker::bool()));

/**
 * wp_reschedule_event()
 */
assertType('bool', wp_reschedule_event(1, 'daily', 'hook'));
assertType('bool', wp_reschedule_event(1, 'daily', 'hook', []));
assertType('bool', wp_reschedule_event(1, 'daily', 'hook', [], false));
assertType('WP_Error|true', wp_reschedule_event(1, 'daily', 'hook', [], true));
assertType('bool|WP_Error', wp_reschedule_event(1, 'daily', 'hook', [], Faker::bool()));

/*
 * wp_unschedule_event()
 */
assertType('bool', wp_unschedule_event(1, 'hook'));
assertType('bool', wp_unschedule_event(1, 'hook', []));
assertType('bool', wp_unschedule_event(1, 'hook', [], false));
assertType('WP_Error|true', wp_unschedule_event(1, 'hook', [], true));
assertType('bool|WP_Error', wp_unschedule_event(1, 'hook', [], Faker::bool()));

/*
 * wp_clear_scheduled_hook()
 */
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook'));
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook', []));
assertType('int<0, max>|false', wp_clear_scheduled_hook('hook', [], false));
assertType('int<0, max>|WP_Error', wp_clear_scheduled_hook('hook', [], true));
assertType('int<0, max>|WP_Error|false', wp_clear_scheduled_hook('hook', [], Faker::bool()));

/*
 * wp_unschedule_hook()
 */
assertType('int<0, max>|false', wp_unschedule_hook('hook'));
assertType('int<0, max>|false', wp_unschedule_hook('hook', false));
assertType('int<0, max>|WP_Error', wp_unschedule_hook('hook', true));
assertType('int<0, max>|WP_Error|false', wp_unschedule_hook('hook', Faker::bool()));

/*
 * wp_insert_post()
 */
assertType('int<0, max>', wp_insert_post([]));
assertType('int<0, max>', wp_insert_post([], false));
assertType('int<0, max>', wp_insert_post([], false, true));
assertType('int<0, max>', wp_insert_post([], false, false));
assertType('int<1, max>|WP_Error', wp_insert_post([], true));
assertType('int<1, max>|WP_Error', wp_insert_post([], true, true));
assertType('int<1, max>|WP_Error', wp_insert_post([], true, false));
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool()));
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool(), true));
assertType('int<0, max>|WP_Error', wp_insert_post([], Faker::bool(), false));

/*
 * wp_update_post()
 */
assertType('int<0, max>', wp_update_post([]));
assertType('int<0, max>', wp_update_post([], false));
assertType('int<0, max>', wp_update_post([], false, true));
assertType('int<0, max>', wp_update_post([], false, false));
assertType('int<1, max>|WP_Error', wp_update_post([], true));
assertType('int<1, max>|WP_Error', wp_update_post([], true, true));
assertType('int<1, max>|WP_Error', wp_update_post([], true, false));
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool()));
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool(), true));
assertType('int<0, max>|WP_Error', wp_update_post([], Faker::bool(), false));

/*
 * wp_insert_attachment()
 */
assertType('int<0, max>', wp_insert_attachment([]));
assertType('int<0, max>', wp_insert_attachment([], true));
assertType('int<0, max>', wp_insert_attachment([], false));
assertType('int<0, max>', wp_insert_attachment([], true, 1));
assertType('int<0, max>', wp_insert_attachment([], true, 1, false));
assertType('int<0, max>', wp_insert_attachment([], true, 1, false, false));
assertType('int<0, max>', wp_insert_attachment([], true, 1, false, true));
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true));
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true, true));
assertType('int<1, max>|WP_Error', wp_insert_attachment([], false, 0, true, false));
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool()));
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool(), true));
assertType('int<0, max>|WP_Error', wp_insert_attachment([], true, 1, Faker::bool(), false));
