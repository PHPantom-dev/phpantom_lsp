<?php declare(strict_types = 1);

namespace Bug7915;

use function PHPStan\Testing\assertType;

class HelloWorld1 {

	/**
	 * @param 'a'|'b' $k
	 * @param (
	 *	 $k is 'a' ? int<0, 1> : non-falsy-string
	 * ) $v
	 * @return void
	 */
	function foo( $k, $v ) {
		if ( $k === 'a' ) {
			assertType('int<0, 1>', $v);
		} else {
			assertType('non-falsy-string', $v);
		}
	}
}

class HelloWorld2
{
	/**
	 * @param string|array<string, mixed> $name
	 * @param ($name is array ? null : int) $value
	 */
	public function setConfig($name, $value): void
	{
		if (is_array($name)) {
			assertType('null', $value);
		} else {
			assertType('int', $value);
		}
	}

	/**
	 * @param string|array<string, mixed> $name
	 * @param int $value
	 */
	public function setConfigMimicConditionalParamType($name, $value): void
	{
		if (is_array($name)) {
			$value = null;
		}

		if (is_array($name)) {
			assertType('null', $value);
		} else {
			assertType('int', $value);
		}
	}
}

/**
 * @param ($isArray is false ? string : array<mixed>) $data
 *
 * @return ($isArray is false ? string : array<mixed>)
 */
function to_utf8($data, bool $isArray = false)
{
	if ($isArray) {
		assertType('array<mixed>', $data);
		if (is_array($data)) { // always true
			foreach ($data as $k => $value) {
				$data[$k] = to_utf8($value, is_array($value));
			}
		} else {
			assertType('*NEVER*', $data);
			$data = []; // dead code
		}
	} else {
		assertType('string', $data);
		$data = @iconv('UTF-8', 'UTF-8//IGNORE', $data);
	}

	return $data;
}
