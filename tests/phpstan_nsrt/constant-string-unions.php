<?php

namespace ConstantStringUnions;

use function PHPStan\Testing\assertType;

// See https://github.com/phpstan/phpstan/issues/6439
class HelloWorld
{
	public function unionOnLeft(string $name, ?int $gesperrt = null, ?int $adaid = null):void
	{
		$string = 'general';
		if (null !== $gesperrt) {
			$string = $string . ' branch-a';
		}
		assertType("'general'|'general branch-a'", $string);
		if (null !== $adaid) {
			$string = $string . ' branch-b';
		}
		assertType("'general'|'general branch-a'|'general branch-a branch-b'|'general branch-b'", $string);
	}

	public function unionOnRight(string $name, ?int $gesperrt = null, ?int $adaid = null):void
	{
		$string = 'general';
		if (null !== $gesperrt) {
			$string = 'branch-a ' . $string;
		}
		assertType("'branch-a general'|'general'", $string);
		if (null !== $adaid) {
			$string = 'branch-b ' . $string;
		}
		assertType("'branch-a general'|'branch-b branch-a general'|'branch-b general'|'general'", $string);
	}

	public function unionOnBoth():void
	{
		$left = rand() ? 'a' : 'b';
		$right = rand() ? 'x' : 'y';
		assertType("'ax'|'ay'|'bx'|'by'", $left . $right);
	}

	public function encapsedString():void
	{
		$str = rand() ? 'a' : 'b';
		$int = rand() ? 1 : 0;
		$float = rand() ? 1.0 : 2.0;
		$bool = (bool) rand();
		$nullable = rand() ? 'a' : null;
		assertType("'.a.'|'.b.'", ".$str.");
		assertType("'.0.'|'.1.'", ".$int.");
		assertType("'.1.'|'.2.'", ".$float.");
		assertType("'..'|'.1.'", ".$bool.");
		assertType("'..'|'.a.'", ".$nullable.");
		assertType("'.a.0.'|'.a.1.'|'.b.0.'|'.b.1.'", ".$str.$int.");
	}

	/**
	 * @param '1'|'2'|'3'|'4'|'5'|'6'|'7'|'8'|'9'|'10'|'11'|'12'|'13'|'14'|'15' $s15
	 * @param '1'|'2'|'3'|'4'|'5'|'6'|'7'|'8'|'9'|'10'|'11'|'12'|'13'|'14'|'15'|'16' $s16
	 * @param '1'|'2'|'3'|'4'|'5'|'6'|'7'|'8'|'9'|'10'|'11'|'12'|'13'|'14'|'15'|'16'|'17' $s17
	 * @param 'a'|'b'|'c'|'d'|'e'|'f'|'g'|'h' $suffix
	 */
	public function testLimit(string $s15, string $s16, string $s17, string $suffix) {
		if (rand(0,1)) {
			// multiply the number of elements by 8
			$s15 .= $suffix;
			$s16 .= $suffix;
			$s17 .= $suffix;
		}

		// union should contain 120 elements
		assertType("'1'|'10'|'10a'|'10b'|'10c'|'10d'|'10e'|'10f'|'10g'|'10h'|'11'|'11a'|'11b'|'11c'|'11d'|'11e'|'11f'|'11g'|'11h'|'12'|'12a'|'12b'|'12c'|'12d'|'12e'|'12f'|'12g'|'12h'|'13'|'13a'|'13b'|'13c'|'13d'|'13e'|'13f'|'13g'|'13h'|'14'|'14a'|'14b'|'14c'|'14d'|'14e'|'14f'|'14g'|'14h'|'15'|'15a'|'15b'|'15c'|'15d'|'15e'|'15f'|'15g'|'15h'|'1a'|'1b'|'1c'|'1d'|'1e'|'1f'|'1g'|'1h'|'2'|'2a'|'2b'|'2c'|'2d'|'2e'|'2f'|'2g'|'2h'|'3'|'3a'|'3b'|'3c'|'3d'|'3e'|'3f'|'3g'|'3h'|'4'|'4a'|'4b'|'4c'|'4d'|'4e'|'4f'|'4g'|'4h'|'5'|'5a'|'5b'|'5c'|'5d'|'5e'|'5f'|'5g'|'5h'|'6'|'6a'|'6b'|'6c'|'6d'|'6e'|'6f'|'6g'|'6h'|'7'|'7a'|'7b'|'7c'|'7d'|'7e'|'7f'|'7g'|'7h'|'8'|'8a'|'8b'|'8c'|'8d'|'8e'|'8f'|'8g'|'8h'|'9'|'9a'|'9b'|'9c'|'9d'|'9e'|'9f'|'9g'|'9h'", $s15);
		// union should contain 128 elements
		assertType("'1'|'10'|'10a'|'10b'|'10c'|'10d'|'10e'|'10f'|'10g'|'10h'|'11'|'11a'|'11b'|'11c'|'11d'|'11e'|'11f'|'11g'|'11h'|'12'|'12a'|'12b'|'12c'|'12d'|'12e'|'12f'|'12g'|'12h'|'13'|'13a'|'13b'|'13c'|'13d'|'13e'|'13f'|'13g'|'13h'|'14'|'14a'|'14b'|'14c'|'14d'|'14e'|'14f'|'14g'|'14h'|'15'|'15a'|'15b'|'15c'|'15d'|'15e'|'15f'|'15g'|'15h'|'16'|'16a'|'16b'|'16c'|'16d'|'16e'|'16f'|'16g'|'16h'|'1a'|'1b'|'1c'|'1d'|'1e'|'1f'|'1g'|'1h'|'2'|'2a'|'2b'|'2c'|'2d'|'2e'|'2f'|'2g'|'2h'|'3'|'3a'|'3b'|'3c'|'3d'|'3e'|'3f'|'3g'|'3h'|'4'|'4a'|'4b'|'4c'|'4d'|'4e'|'4f'|'4g'|'4h'|'5'|'5a'|'5b'|'5c'|'5d'|'5e'|'5f'|'5g'|'5h'|'6'|'6a'|'6b'|'6c'|'6d'|'6e'|'6f'|'6g'|'6h'|'7'|'7a'|'7b'|'7c'|'7d'|'7e'|'7f'|'7g'|'7h'|'8'|'8a'|'8b'|'8c'|'8d'|'8e'|'8f'|'8g'|'8h'|'9'|'9a'|'9b'|'9c'|'9d'|'9e'|'9f'|'9g'|'9h'", $s16);
		// fallback to the more general form
		assertType("literal-string&lowercase-string&non-falsy-string", $s17);

		$left = rand() ? 'a' : 'b';
		$right = rand() ? 'x' : 'y';
		$left .= $right;
		$left .= $right;
		$left .= $right;
		$left .= $right;
		$left .= $right;
		$left .= $right;
		assertType("'axxxxxx'|'axxxxxy'|'axxxxyx'|'axxxxyy'|'axxxyxx'|'axxxyxy'|'axxxyyx'|'axxxyyy'|'axxyxxx'|'axxyxxy'|'axxyxyx'|'axxyxyy'|'axxyyxx'|'axxyyxy'|'axxyyyx'|'axxyyyy'|'axyxxxx'|'axyxxxy'|'axyxxyx'|'axyxxyy'|'axyxyxx'|'axyxyxy'|'axyxyyx'|'axyxyyy'|'axyyxxx'|'axyyxxy'|'axyyxyx'|'axyyxyy'|'axyyyxx'|'axyyyxy'|'axyyyyx'|'axyyyyy'|'ayxxxxx'|'ayxxxxy'|'ayxxxyx'|'ayxxxyy'|'ayxxyxx'|'ayxxyxy'|'ayxxyyx'|'ayxxyyy'|'ayxyxxx'|'ayxyxxy'|'ayxyxyx'|'ayxyxyy'|'ayxyyxx'|'ayxyyxy'|'ayxyyyx'|'ayxyyyy'|'ayyxxxx'|'ayyxxxy'|'ayyxxyx'|'ayyxxyy'|'ayyxyxx'|'ayyxyxy'|'ayyxyyx'|'ayyxyyy'|'ayyyxxx'|'ayyyxxy'|'ayyyxyx'|'ayyyxyy'|'ayyyyxx'|'ayyyyxy'|'ayyyyyx'|'ayyyyyy'|'bxxxxxx'|'bxxxxxy'|'bxxxxyx'|'bxxxxyy'|'bxxxyxx'|'bxxxyxy'|'bxxxyyx'|'bxxxyyy'|'bxxyxxx'|'bxxyxxy'|'bxxyxyx'|'bxxyxyy'|'bxxyyxx'|'bxxyyxy'|'bxxyyyx'|'bxxyyyy'|'bxyxxxx'|'bxyxxxy'|'bxyxxyx'|'bxyxxyy'|'bxyxyxx'|'bxyxyxy'|'bxyxyyx'|'bxyxyyy'|'bxyyxxx'|'bxyyxxy'|'bxyyxyx'|'bxyyxyy'|'bxyyyxx'|'bxyyyxy'|'bxyyyyx'|'bxyyyyy'|'byxxxxx'|'byxxxxy'|'byxxxyx'|'byxxxyy'|'byxxyxx'|'byxxyxy'|'byxxyyx'|'byxxyyy'|'byxyxxx'|'byxyxxy'|'byxyxyx'|'byxyxyy'|'byxyyxx'|'byxyyxy'|'byxyyyx'|'byxyyyy'|'byyxxxx'|'byyxxxy'|'byyxxyx'|'byyxxyy'|'byyxyxx'|'byyxyxy'|'byyxyyx'|'byyxyyy'|'byyyxxx'|'byyyxxy'|'byyyxyx'|'byyyxyy'|'byyyyxx'|'byyyyxy'|'byyyyyx'|'byyyyyy'", $left);
		$left .= $right;
		assertType("literal-string&lowercase-string&non-falsy-string", $left);

		$left = rand() ? 'a' : 'b';
		$right = rand() ? 'x' : 'y';
		$left = "{$left}{$right}";
		$left = "{$left}{$right}";
		$left = "{$left}{$right}";
		$left = "{$left}{$right}";
		$left = "{$left}{$right}";
		$left = "{$left}{$right}";
		assertType("'axxxxxx'|'axxxxxy'|'axxxxyx'|'axxxxyy'|'axxxyxx'|'axxxyxy'|'axxxyyx'|'axxxyyy'|'axxyxxx'|'axxyxxy'|'axxyxyx'|'axxyxyy'|'axxyyxx'|'axxyyxy'|'axxyyyx'|'axxyyyy'|'axyxxxx'|'axyxxxy'|'axyxxyx'|'axyxxyy'|'axyxyxx'|'axyxyxy'|'axyxyyx'|'axyxyyy'|'axyyxxx'|'axyyxxy'|'axyyxyx'|'axyyxyy'|'axyyyxx'|'axyyyxy'|'axyyyyx'|'axyyyyy'|'ayxxxxx'|'ayxxxxy'|'ayxxxyx'|'ayxxxyy'|'ayxxyxx'|'ayxxyxy'|'ayxxyyx'|'ayxxyyy'|'ayxyxxx'|'ayxyxxy'|'ayxyxyx'|'ayxyxyy'|'ayxyyxx'|'ayxyyxy'|'ayxyyyx'|'ayxyyyy'|'ayyxxxx'|'ayyxxxy'|'ayyxxyx'|'ayyxxyy'|'ayyxyxx'|'ayyxyxy'|'ayyxyyx'|'ayyxyyy'|'ayyyxxx'|'ayyyxxy'|'ayyyxyx'|'ayyyxyy'|'ayyyyxx'|'ayyyyxy'|'ayyyyyx'|'ayyyyyy'|'bxxxxxx'|'bxxxxxy'|'bxxxxyx'|'bxxxxyy'|'bxxxyxx'|'bxxxyxy'|'bxxxyyx'|'bxxxyyy'|'bxxyxxx'|'bxxyxxy'|'bxxyxyx'|'bxxyxyy'|'bxxyyxx'|'bxxyyxy'|'bxxyyyx'|'bxxyyyy'|'bxyxxxx'|'bxyxxxy'|'bxyxxyx'|'bxyxxyy'|'bxyxyxx'|'bxyxyxy'|'bxyxyyx'|'bxyxyyy'|'bxyyxxx'|'bxyyxxy'|'bxyyxyx'|'bxyyxyy'|'bxyyyxx'|'bxyyyxy'|'bxyyyyx'|'bxyyyyy'|'byxxxxx'|'byxxxxy'|'byxxxyx'|'byxxxyy'|'byxxyxx'|'byxxyxy'|'byxxyyx'|'byxxyyy'|'byxyxxx'|'byxyxxy'|'byxyxyx'|'byxyxyy'|'byxyyxx'|'byxyyxy'|'byxyyyx'|'byxyyyy'|'byyxxxx'|'byyxxxy'|'byyxxyx'|'byyxxyy'|'byyxyxx'|'byyxyxy'|'byyxyyx'|'byyxyyy'|'byyyxxx'|'byyyxxy'|'byyyxyx'|'byyyxyy'|'byyyyxx'|'byyyyxy'|'byyyyyx'|'byyyyyy'", $left);
		$left = "{$left}{$right}";
		assertType("literal-string&lowercase-string&non-falsy-string", $left);
	}

	/**
	 * @param '1'|'2' $s2
	 */
	public function appendEmpty($s2) {
		if (rand(0,1)) {
			$s2 .= '';
		}
		assertType("'1'|'2'", $s2);

		if (rand(0,1)) {
			$s2 = '';
		}
		assertType("''|'1'|'2'", $s2);
	}

    public function concatCase() {
		$extra = '';
		if (rand(0,1)) {
			$extra = '[0-9]';
		}

		assertType("''|'[0-9]'", $extra);

		$regex = '~[A-Z]' . $extra . '~';
		assertType("'~[A-Z][0-9]~'|'~[A-Z]~'", $regex);
	}
}
