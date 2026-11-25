<?php

namespace NonEmptyString;

use function htmlspecialchars;
use function lcfirst;
use function PHPStan\Testing\assertType;
use function strtolower;
use function strtoupper;
use function ucfirst;
use const ENT_SUBSTITUTE;

class Foo
{

	public function doFoo(string $s): void
	{
		if ($s === '') {
			return;
		}

		assertType('non-empty-string', $s);
	}

	public function doBar(string $s): void
	{
		if ($s !== '') {
			assertType('non-empty-string', $s);
			return;
		}

		assertType('\'\'', $s);
	}

	public function doFoo2(string $s): void
	{
		if (strlen($s) === 0) {
			return;
		}

		assertType('non-empty-string', $s);
	}

	public function doBar2(string $s): void
	{
		if (strlen($s) > 0) {
			assertType('non-empty-string', $s);
			return;
		}

		assertType('\'\'', $s);
	}

	public function doBar3(string $s): void
	{
		if (strlen($s) >= 1) {
			assertType('non-empty-string', $s);
			return;
		}

		assertType('\'\'', $s);
	}

	public function doFoo5(string $s): void
	{
		if (0 === strlen($s)) {
			return;
		}

		assertType('non-empty-string', $s);
	}

	public function doBar4(string $s): void
	{
		if (0 < strlen($s)) {
			assertType('non-empty-string', $s);
			return;
		}

		assertType('\'\'', $s);
	}

	public function doBar5(string $s): void
	{
		if (1 <= strlen($s)) {
			assertType('non-empty-string', $s);
			return;
		}

		assertType('\'\'', $s);
	}

	/**
	 * @param literal-string $s
	 */
	public function doBar6($s): void
	{
		if (1 === strlen($s)) {
			return;
		}
		assertType('literal-string', $s);
	}

	/**
	 * @param literal-string $s
	 */
	public function doBar7($s): void
	{
		if (0 < strlen($s)) {
			return;
		}
		assertType("''", $s); // SKIP: a `strlen()` comparison does not narrow a string to `''`
	}

	public function doFoo3(string $s): void
	{
		if ($s) {
			assertType('non-falsy-string', $s);
		} else {
			assertType('\'\'|\'0\'', $s);
		}
	}

	/**
	 * @param non-empty-string $s
	 */
	public function doFoo4(string $s): void
	{
	}

	/**
	 * @param non-empty-string $s
	 */
	public function doWithNumeric(string $s): void
	{
		if (!is_numeric($s)) {
			return;
		}

	}

	public function doEmpty(string $s): void
	{
		if (empty($s)) {
			return;
		}

		assertType('non-falsy-string', $s);
	}

	public function doEmpty2(string $s): void
	{
		if (!empty($s)) {
			assertType('non-falsy-string', $s);
		}
	}

}

class ImplodingStrings
{

	/**
	 * @param array<string> $commonStrings
	 */
	public function doFoo(string $s, array $commonStrings): void
	{
		assertType('string', implode($s, $commonStrings));
		assertType('string', implode(' ', $commonStrings));
		assertType('string', implode('', $commonStrings));
		assertType('string', implode($commonStrings));
	}

	/**
	 * @param non-empty-array<string> $nonEmptyArrayWithStrings
	 */
	public function doFoo2(string $s, array $nonEmptyArrayWithStrings): void
	{
		assertType('string', implode($s, $nonEmptyArrayWithStrings));
		assertType('string', implode('', $nonEmptyArrayWithStrings));
		assertType('string', implode($nonEmptyArrayWithStrings));
	}

	/**
	 * @param array<non-empty-string> $arrayWithNonEmptyStrings
	 */
	public function doFoo3(string $s, array $arrayWithNonEmptyStrings): void
	{
		assertType('string', implode($s, $arrayWithNonEmptyStrings));
		assertType('string', implode('', $arrayWithNonEmptyStrings));
		assertType('string', implode(' ', $arrayWithNonEmptyStrings));
		assertType('string', implode($arrayWithNonEmptyStrings));
	}

	/**
	 * @param non-empty-array<non-empty-string> $nonEmptyArrayWithNonEmptyStrings
	 */
	public function doFoo4(string $s, array $nonEmptyArrayWithNonEmptyStrings): void
	{
		assertType('non-empty-string', implode($s, $nonEmptyArrayWithNonEmptyStrings));
		assertType('non-empty-string', implode('', $nonEmptyArrayWithNonEmptyStrings));
		assertType('non-empty-string', implode($nonEmptyArrayWithNonEmptyStrings));
	}

	public function sayHello(int $i): void
	{
		// coming from issue #5291
		$s = array(1, $i);

	}

	/**
	 * @param non-empty-string $glue
	 */
	public function nonE($glue, array $a)
	{
		// coming from issue #5291
		if (empty($a)) {
			return "xyz";
		}

	}

	public function sayHello2(int $i): void
	{
		// coming from issue #5291
		$s = array(1, $i);

	}

	/**
	 * @param non-empty-string $glue
	 */
	public function nonE2($glue, array $a)
	{
		// coming from issue #5291
		if (empty($a)) {
			return "xyz";
		}

	}

}

class LiteralString
{

	function x(string $tableName, string $original): void
	{
	}

	/**
	 * @param non-empty-string $nonEmpty
	 */
	function concat(string $s, string $nonEmpty): void
	{
		assertType('string', $s . '');
		assertType('non-empty-string', $nonEmpty . '');
		assertType('non-empty-string', $nonEmpty . $s);
	}

}

class GeneralizeConstantStringType
{

	/**
	 * @param array<non-empty-string, int> $a
	 * @param non-empty-string $s
	 */
	public function doFoo(array $a, string $s): void
	{
		$a[$s] = 2;

		// there might be non-empty-string that becomes a number instead
		assertType('non-empty-array<non-empty-string, int>', $a);
	}

	/**
	 * @param array<non-empty-string, int> $a
	 * @param non-empty-string $s
	 */
	public function doFoo2(array $a, string $s): void
	{
		$a[''] = 2;
	}

}

class MoreNonEmptyStringFunctions
{

	/**
	 * @param non-empty-string $nonEmpty
	 * @param non-falsy-string $nonFalsy
	 * @param '1'|'2'|'5'|'10' $constUnion
	 */
	public function doFoo(string $s, string $nonEmpty, string $nonFalsy, int $i, bool $bool, $constUnion)
	{
		assertType('string', addslashes($s));
		assertType('string', addcslashes($s));

		assertType('string', escapeshellarg($s));
		assertType('string', escapeshellcmd($s));

		assertType('uppercase-string', strtoupper($s));
		assertType('lowercase-string', strtolower($s));
		assertType('uppercase-string', mb_strtoupper($s));
		assertType('lowercase-string', mb_strtolower($s));
		assertType('string', lcfirst($s));
		assertType('non-empty-string', lcfirst($nonEmpty));
		assertType('string', ucfirst($s));
		assertType('non-empty-string', ucfirst($nonEmpty));
		assertType('string', ucwords($s));
		assertType('string', htmlspecialchars($s));
		assertType('string', htmlspecialchars($s, ENT_SUBSTITUTE));
		assertType('string', htmlspecialchars($s, 0));
		assertType('string', htmlspecialchars($nonEmpty, 0));
		assertType('string', htmlentities($s));
		assertType('string', htmlentities($s, ENT_SUBSTITUTE));
		assertType('string', htmlentities($s, 0));
		assertType('string', htmlentities($nonEmpty, 0));

		assertType('string', urlencode($s));
		assertType('string', urldecode($s));
		assertType('string', rawurlencode($s));
		assertType('string', rawurldecode($s));

		assertType('string', preg_quote($s));

		assertType('string', sprintf($s));
		assertType('string', sprintf($nonEmpty));
		assertType('string', sprintf($s, $nonEmpty));
		assertType('string', sprintf($nonEmpty, $s));
		assertType('string', sprintf($s, $nonFalsy));
		assertType('string', sprintf($nonFalsy, $s));
		assertType('string', vsprintf($s, []));
		assertType('string', vsprintf($nonEmpty, []));

		assertType('non-empty-string', sprintf("%s0%s", $s, $s));
		assertType('non-empty-string', sprintf("%s0%s%s%s%s", $s, $s, $s, $s, $s));
		assertType('non-empty-string', sprintf("%s0%s%s%s%s%s", $s, $s, $s, $s, $s, $s));

		assertType('0', strlen(''));
		assertType('5', strlen('hallo'));
		assertType('int<0, max>', strlen($s));
		assertType('1|2', strlen($constUnion)); // PHPStan says `int<1, 2>`; PHPantom is more precise here

		assertType('string', str_pad($s, 0));

		assertType('\'\'', str_repeat($nonEmpty, 0)); // SKIP: a string function or `++` on literals is not folded
		assertType('string', str_repeat($nonEmpty, $i));
		assertType('\'\'', str_repeat($s, 0)); // SKIP: a string function or `++` on literals is not folded
		assertType('string', str_repeat($s, 1));
		assertType('string', str_repeat($s, $i));
	}

	function multiplesPrintfFormats(string $s) {
		$maybeNonEmpty = '%s';
		$maybeNonFalsy = '%s';
		$nonEmpty = '%s0';
		$nonFalsy = '%sAA';

		if (rand(0,1)) {
			$maybeNonEmpty = '%s0';
			$maybeNonFalsy = '%sAA';
			$nonEmpty = '0%s';
			$nonFalsy = 'AA%s';
		}

		assertType('string', sprintf($maybeNonEmpty, $s));
		assertType('string', sprintf($maybeNonFalsy, $s));
		assertType('non-empty-string', sprintf($nonEmpty, $s));
		assertType('non-falsy-string', sprintf($nonFalsy, $s));
	}

	function subtract($m) {
		if ($m) {
		}
		if ($m != '') {
		}
		if ($m !== '') {
			assertType('string', (string) $m);
		}
		if (!is_string($m)) {
			assertType('string', (string) $m);
		}

		if ($m !== true) {
			assertType('string', (string) $m);
		}
		if ($m !== false) {
			assertType('string', (string) $m);
		}
		if ($m !== false && $m !== '' && $m !== null) {
		}
		if (!is_bool($m) && $m !== '' && $m !== null) {
		}
	}
}
