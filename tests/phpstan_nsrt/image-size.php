<?php

namespace ImageSize;


use function PHPStan\Testing\assertType;

function imageFoo(): void
{
	$imagesize = getimagesize("img/flag.jpg");
	if ($imagesize === false) {
		return;
	}

	list($width, $height, $type, $attr) = $imagesize;

	assertType('int', $type);
	assertType('string', $attr);
	assertType('string', $imagesize['mime']);
	assertType('int', $imagesize['channels']);
	assertType('int', $imagesize['bits']);
}

function imagesizeFoo(string $s): void
{
	$imagesize = getimagesizefromstring($s);
	if ($imagesize === false) {
		return;
	}
	list($width, $height, $type, $attr) = $imagesize;

	assertType('int', $type);
	assertType('string', $attr);
	assertType('string', $imagesize['mime']);
	assertType('int', $imagesize['channels']);
	assertType('int', $imagesize['bits']);
}


