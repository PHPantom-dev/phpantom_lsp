<?php

use function PHPStan\Testing\assertType;

// core, https://www.php.net/manual/en/reserved.constants.php
assertType('0', PHP_WINDOWS_EVENT_CTRL_C);
assertType('1', PHP_WINDOWS_EVENT_CTRL_BREAK);
