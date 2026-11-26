<?php // lint >= 7.3

namespace StrCasingReturnType;

use function PHPStan\Testing\assertType;

class Foo {
	/**
	 * @param numeric-string $numericS
	 * @param non-empty-string $nonE
	 * @param lowercase-string $lowercaseS
	 * @param literal-string $literal
	 * @param 'foo'|'Foo' $edgeUnion
	 * @param MB_CASE_UPPER|MB_CASE_LOWER|MB_CASE_TITLE|MB_CASE_FOLD|MB_CASE_UPPER_SIMPLE|MB_CASE_LOWER_SIMPLE|MB_CASE_TITLE_SIMPLE|MB_CASE_FOLD_SIMPLE $caseMode
	 * @param 'aKV'|'hA'|'AH'|'K'|'KV'|'RNKV' $kanaMode
	 * @param mixed $mixed
	 */
	public function bar($numericS, $nonE, $lowercaseS, $literal, $edgeUnion, $caseMode, $kanaMode, $mixed) {
		assertType("'abc'", strtolower('ABC'));
		assertType("'ABC'", strtoupper('abc'));
		assertType("'abc'", mb_strtolower('ABC'));
		assertType("'ABC'", mb_strtoupper('abc'));
		assertType("'abc'", mb_strtolower('ABC', 'UTF-8'));
		assertType("'ABC'", mb_strtoupper('abc', 'UTF-8'));
		assertType("'ａｂｃ'", mb_strtolower('Ａｂｃ'));
		assertType("'ＡＢＣ'", mb_strtoupper('Ａｂｃ'));
		assertType("'aBC'", lcfirst('ABC'));
		assertType("'Abc'", ucfirst('abc'));
		assertType("'Hello World'", ucwords('hello world'));
		assertType("'Hello|World'", ucwords('hello|world', "|"));
		assertType("'ČESKÁ REPUBLIKA'", mb_convert_case('Česká republika', MB_CASE_UPPER));
		assertType("'česká republika'", mb_convert_case('Česká republika', MB_CASE_LOWER));
		assertType("'ČESKÁ REPUBLIKA'|'Česká Republika'|'česká republika'", mb_convert_case('Česká republika', $caseMode));

		assertType("numeric-string", lcfirst($numericS));
		assertType("numeric-string", ucfirst($numericS));

		assertType("non-empty-string", lcfirst($nonE));
		assertType("non-empty-string", ucfirst($nonE));

		assertType("lowercase-string", strtolower($literal));
		assertType("uppercase-string", strtoupper($literal));
		assertType("lowercase-string", mb_strtolower($literal));
		assertType("uppercase-string", mb_strtoupper($literal));
		assertType("string", lcfirst($literal));
		assertType("string", ucfirst($literal));
		assertType("string", ucwords($literal));
		assertType("string", mb_convert_case($literal, $mixed));
		assertType("string", mb_convert_kana($literal));
		assertType("string", mb_convert_kana($literal, $mixed));

		assertType("lowercase-string", strtolower($lowercaseS));
		assertType("uppercase-string", strtoupper($lowercaseS));
		assertType("lowercase-string", mb_strtolower($lowercaseS));
		assertType("uppercase-string", mb_strtoupper($lowercaseS));
		assertType("lowercase-string", lcfirst($lowercaseS));
		assertType("string", ucfirst($lowercaseS));
		assertType("string", ucwords($lowercaseS));
		assertType("string", mb_convert_case($lowercaseS, $mixed));
		assertType("string", mb_convert_kana($lowercaseS));
		assertType("string", mb_convert_kana($lowercaseS, $mixed));

		assertType("'foo'", lcfirst($edgeUnion));
	}

	public function foo() {
		// invalid char conversions still lead to non-falsy-string
		assertType("lowercase-string&non-falsy-string", mb_strtolower("\xfe\xff\x65\xe5\x67\x2c\x8a\x9e", 'CP1252'));
		// valid char sequence, but not support non ASCII / UTF-8 encodings
		// invalid UTF-8 sequence
	}
}
