<?php

use function PHPStan\Testing\assertType;

$image = imagecreatetruecolor(1, 1);
$memoryHandle = fopen('php://memory', 'w');

assertType('bool', imagegd($image));
assertType('bool', imagegd($image, null));
assertType('bool', imagegd($image, 'php://memory'));
assertType('bool', imagegd($image, $memoryHandle));
