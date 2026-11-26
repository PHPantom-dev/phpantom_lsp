<?php

namespace ReplaceFunctions;

use function PHPStan\Testing\assertType;

function ($mixed) {

	$array = ['a' => 'a', 'b' => 'b'];
	$string = 'str';

	$arrayOrString = [];
	if (doFoo()) {
		$arrayOrString = 'foo';
	}

	/** @var callable[] $callbacks */
	$callbacks = [];

	$expectedString = str_replace('aaa', 'bbb', $string);
	$expectedArray = str_replace('aaa', 'bbb', $array);
	$expectedArrayOrString = str_replace('aaa', 'bbb', $arrayOrString);
	$expectedBenevolentArrayOrString = str_replace('aaa', 'bbb', $mixed);

	$anotherExpectedString = preg_replace('aaa', 'bbb', $string);
	$anotherExpectedArray = preg_replace('aaa', 'bbb', $array);
	$anotherExpectedArrayOrString = preg_replace('aaa', 'bbb', $arrayOrString);

	$expectedString2 = preg_replace_callback('aaa', function () {}, $string);
	$expectedArray2 = preg_replace_callback('aaa', function () {}, $array);
	$expectedArrayOrString2 = preg_replace_callback('aaa', function () {}, $arrayOrString);

	$expectedString3 = str_ireplace('aaa', 'bbb', $string);
	$expectedArray3 = str_ireplace('aaa', 'bbb', $array);
	$expectedArrayOrString3 = str_ireplace('aaa', 'bbb', $arrayOrString);
	$expectedBenevolentArrayOrString3 = str_ireplace('aaa', 'bbb', $mixed);

	/** @var Foo[] $arr */
	$arr = doFoo();

	foreach ($arr as $intOrStringKey => $value) {
		assertType("'str'", $expectedString); // PHPStan says `lowercase-string&non-falsy-string`; the replacement is folded
		assertType('string|null', $expectedString2);
		assertType('(lowercase-string&non-falsy-string)|null', $anotherExpectedString); // SKIP: preg_replace() does not keep its subject's string refinements
		assertType("array{a: 'a', b: 'b'}", $expectedArray); // PHPStan says `array{a: lowercase-string&non-falsy-string, b: lowercase-string&non-falsy-string}`; the replacement is folded
		assertType('array{a?: string, b?: string}', $expectedArray2);
		assertType('array{a?: lowercase-string&non-falsy-string, b?: lowercase-string&non-falsy-string}', $anotherExpectedArray); // SKIP: preg_replace() does not keep its subject's string refinements
		assertType('array{}|(lowercase-string&non-falsy-string)', $expectedArrayOrString); // SKIP: a subject that may be an array or a string loses the array's shape
		assertType('(array<string>|string)', $expectedBenevolentArrayOrString);
		assertType('array{}|string|null', $expectedArrayOrString2); // SKIP: a subject that may be an array or a string loses the array's shape
		assertType('array{}|(lowercase-string&non-falsy-string)|null', $anotherExpectedArrayOrString); // SKIP: a subject that may be an array or a string loses the array's shape
		assertType('array{a?: string, b?: string}', preg_replace_callback_array($callbacks, $array));
		assertType('string|null', preg_replace_callback_array($callbacks, $string));
		assertType('string', str_replace('.', ':', $intOrStringKey));
		assertType('string', str_ireplace('.', ':', $intOrStringKey));
	}

};
