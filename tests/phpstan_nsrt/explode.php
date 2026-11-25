<?php // lint >= 8.0

namespace ExplodeFunction;

use function PHPStan\Testing\assertType;

function (string $delimiter, $mixed) {

	/** @var string $str */
	$str = doFoo();
	$sureArray = explode(' ', $str);
	$sureFalse = explode('', $str);
	$arrayOrFalse = explode($delimiter, $str);

	$emptyOrComma = '';
	if (doFoo()) {
		$emptyOrComma = ',';
	}

	$anotherArrayOrFalse = explode($emptyOrComma, $str);
	$benevolentArrayOrFalse = explode($mixed, $str);

	assertType('non-empty-list<string>', $sureArray);
	assertType('non-empty-list<string>', $arrayOrFalse);
	assertType('non-empty-list<string>', $anotherArrayOrFalse);
	assertType('non-empty-list<string>', $benevolentArrayOrFalse);

};
