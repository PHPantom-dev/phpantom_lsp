<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function get_submit_button;
use function PHPStan\Testing\assertType;

assertType('non-falsy-string', get_submit_button());
assertType('non-falsy-string', get_submit_button('Button Text'));
assertType('non-falsy-string', get_submit_button('Button Text', 'primary small'));
assertType('non-falsy-string', get_submit_button('Submit', 'primary small', 'submit'));
assertType('non-falsy-string', get_submit_button('Submit', 'primary small', 'submit', true));
assertType('non-falsy-string', get_submit_button('Submit', 'primary small', 'submit', false));
assertType('non-falsy-string', get_submit_button('Submit', 'primary small', 'submit', true, 'id="custom-value"'));
assertType('non-falsy-string', get_submit_button('Submit', 'primary small', 'submit', true, ['id' => 'custom-value']));
