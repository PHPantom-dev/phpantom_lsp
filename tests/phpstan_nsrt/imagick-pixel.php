<?php declare(strict_types = 1);

namespace ImagickPixelReturn;

use function PHPStan\Testing\assertType;

function (): void {
	$imagickPixel = new \ImagickPixel('rgb(0, 0, 0)');

	assertType('array{r: int<0, 255>, g: int<0, 255>, b: int<0, 255>, a: int<0, 1>}', $imagickPixel->getColor());
	assertType('array{r: int<0, 255>, g: int<0, 255>, b: int<0, 255>, a: int<0, 1>}', $imagickPixel->getColor(0));
	assertType('array{r: float, g: float, b: float, a: float}', $imagickPixel->getColor(1));
	assertType('array{r: int<0, 255>, g: int<0, 255>, b: int<0, 255>, a: int<0, 255>}', $imagickPixel->getColor(2));
	assertType('array{}', $imagickPixel->getColor(3));
};
