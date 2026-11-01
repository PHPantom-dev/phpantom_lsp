<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use WP_Comments_List_Table;

use function PHPStan\Testing\assertType;

$commentsListTable = new WP_Comments_List_Table();

assertType('non-falsy-string', $commentsListTable->floated_admin_avatar('', 0));
assertType('non-falsy-string', $commentsListTable->floated_admin_avatar('name', 123));
assertType('non-falsy-string', $commentsListTable->floated_admin_avatar(Faker::string(), Faker::int()));
