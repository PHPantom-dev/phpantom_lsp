<?php

use function PHPStan\Testing\assertType;

assertType('string', number_format(1002.7));
assertType('string', number_format(1002.7, 3));
assertType('string', number_format(1002.7, 3, null));
assertType('string', number_format(1002.7, 3, '.'));
assertType('string', number_format(1002.7, 3, '.', ','));
assertType('string', number_format(1002.7, 3, '.', null));
assertType('string', number_format(1002.7, 3, '', null));
assertType('string', number_format(1002.7, 3, 'b', null));
assertType('string', number_format(1002.7, 3, 'b', ''));


