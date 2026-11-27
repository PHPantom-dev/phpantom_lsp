<?php // lint >= 8.4

namespace Pr1244Php84;

use function PHPStan\Testing\assertType;

function foo() {
	/** @var string $string */
	$string = doFoo();

	assertType('null', var_export());
	assertType('null', var_export($string));
	assertType('null', var_export($string, false));
	assertType('string', var_export($string, true));

	assertType('true', highlight_string());
	assertType('true', highlight_string($string));
	assertType('true', highlight_string($string, false));
	assertType('string', highlight_string($string, true));

	assertType('bool', highlight_file());
	assertType('bool', highlight_file($string));
	assertType('bool', highlight_file($string, false));
	assertType('string', highlight_file($string, true));

	assertType('bool', show_source());
	assertType('bool', show_source($string));
	assertType('bool', show_source($string, false));
	assertType('string', show_source($string, true));

	assertType('true', print_r());
	assertType('true', print_r($string));
	assertType('true', print_r($string, false));
	assertType('string', print_r($string, true));
}
