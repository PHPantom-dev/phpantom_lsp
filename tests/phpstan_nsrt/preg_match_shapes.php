<?php // lint >= 7.2

namespace PregMatchShapes;

use function PHPStan\Testing\assertType;
use InvalidArgumentException;

function doMatch(string $s): void {
	if (preg_match('/Price: /i', $s, $matches)) {
		assertType('array{string}', $matches);
	}
	assertType('array{}|array{string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/Price: (£|€)\d+/', $s, $matches)) {
		assertType("array{string, '£'|'€'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{string, '£'|'€'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/Price: (£|€)(\d+)/i', $s, $matches)) {
		assertType("array{string, '£'|'€', string}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay
	}
	assertType("array{}|array{string, '£'|'€', string}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('  /Price: (£|€)\d+/ i u', $s, $matches)) {
		assertType("array{string, '£'|'€'}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay
	}
	assertType("array{}|array{string, '£'|'€'}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('(Price: (£|€))i', $s, $matches)) {
		assertType("array{string, '£'|'€'}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay
	}
	assertType("array{}|array{string, '£'|'€'}", $matches); // PHPantom: `/i` cannot change a group of non-letters, so its literals stay // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('_foo(.)\_i_i', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	assertType('array{}|array{string, string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(a)(b)*(c)(d)*/', $s, $matches)) {
		assertType("array{0: string, 1: 'a', 2: string, 3: 'c', 4?: string}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'a', 2: string, 3: 'c', 4?: string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(a)(?<name>b)*(c)(d)*/', $s, $matches)) {
		assertType("array{0: string, 1: 'a', name: string, 2: string, 3: 'c', 4?: string}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'a', name: string, 2: string, 3: 'c', 4?: string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(a)(b)*(c)(?<name>d)*/', $s, $matches)) {
		assertType("array{0: string, 1: 'a', 2: string, 3: 'c', name?: string, 4?: string}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'a', 2: string, 3: 'c', name?: string, 4?: string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(a|b)|(?:c)/', $s, $matches)) {
		assertType("array{0: string, 1?: 'a'|'b'}", $matches);
	}
	assertType("array{}|array{0: string, 1?: 'a'|'b'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz)+/', $s, $matches)) {
		assertType("array{string, 'foo', 'bar', string}", $matches);
	}
	assertType("array{}|array{string, 'foo', 'bar', string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz)*/', $s, $matches)) {
		assertType("array{0: string, 1: 'foo', 2: 'bar', 3?: string}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'foo', 2: 'bar', 3?: string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz)?/', $s, $matches)) {
		assertType("array{0: string, 1: 'foo', 2: 'bar', 3?: 'baz'}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'foo', 2: 'bar', 3?: 'baz'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz){0,3}/', $s, $matches)) {
		assertType("array{0: string, 1: 'foo', 2: 'bar', 3?: string}", $matches);
	}
	assertType("array{}|array{0: string, 1: 'foo', 2: 'bar', 3?: string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz){2,3}/', $s, $matches)) {
		assertType("array{string, 'foo', 'bar', string}", $matches);
	}
	assertType("array{}|array{string, 'foo', 'bar', string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/(foo)(bar)(baz){2}/', $s, $matches)) {
		assertType("array{string, 'foo', 'bar', string}", $matches);
	}
	assertType("array{}|array{string, 'foo', 'bar', string}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function doNonCapturingGroup(string $s): void {
	if (preg_match('/Price: (?:£|€)(\d+)/', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	assertType('array{}|array{string, string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function doNamedSubpattern(string $s): void {
	if (preg_match('/\w-(?P<num>\d+)-(\w)/', $s, $matches)) {
		assertType('array{0: string, num: string, 1: string, 2: string}', $matches);
	}
	assertType('array{}|array{0: string, num: string, 1: string, 2: string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/^(?<name>\S+::\S+)/', $s, $matches)) {
		assertType('array{0: string, name: string, 1: string}', $matches);
	}
	assertType('array{}|array{0: string, name: string, 1: string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/^(?<name>\S+::\S+)(?:(?<dataname> with data set (?:#\d+|"[^"]+"))\s\()?/', $s, $matches)) {
		assertType('array{0: string, name: string, 1: string, dataname?: string, 2?: string}', $matches);
	}
	assertType('array{}|array{0: string, name: string, 1: string, dataname?: string, 2?: string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function doOffsetCapture(string $s): void {
	if (preg_match('/(foo)(bar)(baz)/', $s, $matches, PREG_OFFSET_CAPTURE)) {
		assertType("array{array{string, int<-1, max>}, array{'foo', int<-1, max>}, array{'bar', int<-1, max>}, array{'baz', int<-1, max>}}", $matches);
	}
	assertType("array{}|array{array{string, int<-1, max>}, array{'foo', int<-1, max>}, array{'bar', int<-1, max>}, array{'baz', int<-1, max>}}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function doUnknownFlags(string $s, int $flags): void {
	if (preg_match('/(foo)(bar)(baz)/xyz', $s, $matches, $flags)) {
		assertType('array<array{string|null, int<-1, max>}|string|null>', $matches);
	}
	assertType('array<array{string|null, int<-1, max>}|string|null>', $matches);
}

function doMultipleAlternativeCaptureGroupsWithSameNameWithModifier(string $s): void {
	if (preg_match('/(?J)(?<Foo>[a-z]+)|(?<Foo>[0-9]+)/', $s, $matches)) {
		assertType("array{0: string, Foo: string, 1: string}|array{0: string, Foo: string, 1: '', 2: string}", $matches);
	}
	assertType("array{}|array{0: string, Foo: string, 1: string}|array{0: string, Foo: string, 1: '', 2: string}", $matches);
}

function doMultipleConsecutiveCaptureGroupsWithSameNameWithModifier(string $s): void {
	if (preg_match('/(?J)(?<Foo>[a-z]+)|(?<Foo>[0-9]+)/', $s, $matches)) {
		assertType("array{0: string, Foo: string, 1: string}|array{0: string, Foo: string, 1: '', 2: string}", $matches);
	}
	assertType("array{}|array{0: string, Foo: string, 1: string}|array{0: string, Foo: string, 1: '', 2: string}", $matches);
}

// https://github.com/hoaproject/Regex/issues/31
function hoaBug31(string $s): void {
	if (preg_match('/([\w-])/', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	assertType('array{}|array{string, string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys

	if (preg_match('/\w-(\d+)-(\w)/', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	assertType('array{}|array{string, string, string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

// https://github.com/phpstan/phpstan/issues/10855#issuecomment-2044323638
function testHoaUnsupportedRegexSyntax(string $s): void {
	if (preg_match('#\QPHPDoc type array<string> of property App\Log::$fillable is not covariant with PHPDoc type array<int, string> of overridden property Illuminate\Database\E\\\\\QEloquent\Model::$fillable.\E#', $s, $matches)) {
		assertType('array{string}', $matches);
	}
	assertType('array{}|array{string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function testPregMatchSimpleCondition(string $value): void {
	if (preg_match('/%env\((.*)\:.*\)%/U', $value, $matches)) {
		assertType('array{string, string}', $matches);
	}
}


function testPregMatchIdenticalToOne(string $value): void {
	if (preg_match('/%env\((.*)\:.*\)%/U', $value, $matches) === 1) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchIdenticalToOneFalseyContext(string $value): void {
	if (!(preg_match('/%env\((.*)\:.*\)%/U', $value, $matches) !== 1)) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchIdenticalToOneInverted(string $value): void {
	if (1 === preg_match('/%env\((.*)\:.*\)%/U', $value, $matches)) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchIdenticalToOneFalseyContextInverted(string $value): void {
	if (!(1 !== preg_match('/%env\((.*)\:.*\)%/U', $value, $matches))) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchEqualToOne(string $value): void {
	if (preg_match('/%env\((.*)\:.*\)%/U', $value, $matches) == 1) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchEqualToOneFalseyContext(string $value): void {
	if (!(preg_match('/%env\((.*)\:.*\)%/U', $value, $matches) != 1)) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchEqualToOneInverted(string $value): void {
	if (1 == preg_match('/%env\((.*)\:.*\)%/U', $value, $matches)) {
		assertType('array{string, string}', $matches);
	}
}

function testPregMatchEqualToOneFalseyContextInverted(string $value): void {
	if (!(1 != preg_match('/%env\((.*)\:.*\)%/U', $value, $matches))) {
		assertType('array{string, string}', $matches);
	}
}

function testUnionPattern(string $s): void
{
	if (rand(0,1)) {
		$pattern = '/Price: (\d+)/i';
	} else {
		$pattern = '/Price: (\d+)(\d+)(\d+)/';
	}
	if (preg_match($pattern, $s, $matches)) {
		assertType('array{string, string, string, string}|array{string, string}', $matches);
	}
	assertType('array{}|array{string, string, string, string}|array{string, string}', $matches);
}

function doFoo(string $row): void
{
	if (preg_match('~^(a(b))$~', $row, $matches) === 1) {
		assertType("array{string, 'ab', 'b'}", $matches);
	}
	if (preg_match('~^(a(b)?)$~', $row, $matches) === 1) {
		assertType("array{0: string, 1: string, 2?: 'b'}", $matches);
	}
	if (preg_match('~^(a(b)?)?$~', $row, $matches) === 1) {
		assertType("list{0: string, 1?: string, 2?: 'b'}", $matches);
	}
}

function doFoo2(string $row): void
{
	if (preg_match('~^((?<branchCode>\\d{1,6})-)?(?<accountNumber>\\d{1,10})/(?<bankCode>\\d{4})$~', $row, $matches) !== 1) {
		return;
	}

	assertType("array{0: string, 1: string, branchCode: string, 2: string, accountNumber: string, 3: string, bankCode: string, 4: string}", $matches);
}

function doFoo3(string $row): void
{
	if (preg_match('~^(02,([\d.]{10}),(\d+),(\d+),(\d+),)(\d+)$~', $row, $matches) !== 1) {
		return;
	}

	assertType('array{string, string, string, string, string, string, string}', $matches);
}

function (string $size): void {
	if (preg_match('~^a\.b(c(\d+)(\d+)(\s+))?d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string, string, string}|array{string}', $matches);
};

function (string $size): void {
	if (preg_match('~^a\.b(c(\d+))?d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string}|array{string}', $matches);
};

function (string $size): void {
	if (preg_match('~^a\.b(c(\d+)?)d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{0: string, 1: string, 2?: string}', $matches);
};

function (string $size): void {
	if (preg_match('~^a\.b(c(\d+)?)?d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('list{0: string, 1?: string, 2?: string}', $matches);
};

function (string $size): void {
	if (preg_match('~^a\.b(c(\d+))d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType('array{string, string, string}', $matches);
};

function (string $size): void {
	if (preg_match('~^a\.(b)?(c)?d~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType("list{0: string, 1?: ''|'b', 2?: 'c'}", $matches);
};

function (string $size): void {
	if (preg_match('~^(?:(\\d+)x(\\d+)|(\\d+)|x(\\d+))$~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType("array{string, '', '', '', string}|array{string, '', '', string}|array{string, string, string}", $matches);
};

function (string $size): void {
	if (preg_match('~^(?:(\\d+)x(\\d+)|(\\d+)|x(\\d+))?$~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType("array{string, '', '', '', string}|array{string, '', '', string}|array{string, string, string}|array{string}", $matches);
};

function (string $size): void {
	if (preg_match('~\{(?:(include)\\s+(?:[$]?\\w+(?<!file))\\s)|(?:(include\\s+file)\\s+(?:[$]?\\w+)\\s)|(?:(include(?:Template|(?:\\s+file)))\\s+(?:\'?.*?\.latte\'?)\\s)~', $size, $matches) !== 1) {
		throw new InvalidArgumentException(sprintf('Invalid size "%s"', $size));
	}
	assertType("array{string, '', '', string}|array{string, '', string}|array{string, 'include'}", $matches);
};


function bug11277a(string $value): void
{
	if (preg_match('/^\[(.+,?)*\]$/', $value, $matches)) {
		assertType('array{0: string, 1?: string}', $matches);
		if (count($matches) === 2) {
			assertType('array{string, string}', $matches);
		}
	}
}

function bug11277b(string $value): void
{
	if (preg_match('/^(?:(.+,?)|(x))*$/', $value, $matches)) {
		assertType("array{0: string, 1?: string}|array{string, '', string}", $matches);
	}
}

// https://www.pcre.org/current/doc/html/pcre2pattern.html#dupgroupnumber
// https://3v4l.org/09qdT
function bug11291(string $s): void {
	if (preg_match('/(?|(a)|(b)(c)|(d)(e)(f))/', $s, $matches)) {
		assertType('list{0: string, 1: string, 2?: string, 3?: string}', $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType('array{}|list{0: string, 1: string, 2?: string, 3?: string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function bug11323a(string $s): void
{
	if (preg_match('/Price: (?P<currency>£|€)\d+/', $s, $matches)) {
		assertType("array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function bug11323b(string $s): void
{
	if (preg_match('/Price: (?<currency>£|€)\d+/', $s, $matches)) {
		assertType("array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function unmatchedAsNullWithMandatoryGroup(string $s): void {
	if (preg_match('/Price: (?<currency>£|€)\d+/', $s, $matches, PREG_UNMATCHED_AS_NULL)) {
		assertType("array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{0: string, currency: '£'|'€', 1: '£'|'€'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
}

function (string $s): void {
	if (preg_match('{' . preg_quote('xxx') . '(z)}', $s, $matches)) {
		assertType("array{string, 'z'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{string, 'z'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
};

function (string $s): void {
	if (preg_match('{' . preg_quote($s) . '(z)}', $s, $matches)) {
		assertType("array{string, 'z'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{string, 'z'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
};

function (string $s): void {
	if (preg_match('/' . preg_quote($s, '/') . '(\d)/', $s, $matches)) {
		assertType('array{string, string}', $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType('array{}|array{string, string}', $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
};

function (string $s): void {
	if (preg_match('{' . preg_quote($s) . '(z)' . preg_quote($s) . '(?:abc)(def)?}', $s, $matches)) {
		assertType("array{0: string, 1: 'z', 2?: 'def'}", $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType("array{}|array{0: string, 1: 'z', 2?: 'def'}", $matches); // SKIP: joining two branches merges shapes with different keys into one with optional keys
};

function (string $s, $mixed): void {
	if (preg_match('{' . preg_quote($s) . '(z)' . preg_quote($s) . '(?:abc)'. $mixed .'(def)?}', $s, $matches)) {
		assertType('array<string>', $matches);
	} else {
		assertType('array{}', $matches);
	}
	assertType('array<string>', $matches);
};

function (string $s): void {
	if (preg_match('/^%([0-9]*\$)?[0-9]*\.?[0-9]*([sbdeEfFgGhHouxX])$/', $s, $matches) === 1) {
		assertType("array{string, string, 'b'|'d'|'E'|'e'|'F'|'f'|'G'|'g'|'H'|'h'|'o'|'s'|'u'|'X'|'x'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~^((\\d{1,6})-)$~', $s, $matches) === 1) {
		assertType("array{string, string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~^((\\d{1,6}).)$~', $s, $matches) === 1) {
		assertType("array{string, string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~^([157])$~', $s, $matches) === 1) {
		assertType("array{string, '1'|'5'|'7'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~^([157XY])$~', $s, $matches) === 1) {
		assertType("array{string, '1'|'5'|'7'|'X'|'Y'}", $matches);
	}
};

function bug11323(string $s): void {
	if (preg_match('/([*|+?{}()]+)([^*|+[:digit:]?{}()]+)/', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('/\p{L}[[\]]+([-*|+?{}(?:)]+)([^*|+[:digit:]?{a-z}(\p{L})\a-]+)/', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{([-\p{L}[\]*|\x03\a\b+?{}(?:)-]+[^[:digit:]?{}a-z0-9#-k]+)(a-z)}', $s, $matches)) {
		assertType("array{string, string, 'a-z'}", $matches);
	}
	if (preg_match('{(\d+)(?i)insensitive((?xs-i)case SENSITIVE here.+and dot matches new lines)}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{(\d+)(?i)insensitive((?x-i)case SENSITIVE here(?i:insensitive non-capturing group))}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{([]] [^]])}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{([[:digit:]])}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{([\d])(\d)}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{([0-9])}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{(\p{L})(\p{P})(\p{Po})}', $s, $matches)) {
		assertType('array{string, string, string, string}', $matches);
	}
	if (preg_match('{(a)??(b)*+(c++)(d)+?}', $s, $matches)) {
		assertType("array{string, ''|'a', string, string, string}", $matches);
	}
	if (preg_match('{(.\d)}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{(\d.)}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{(\d\d)}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match('{(.(\d))}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{((\d).)}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{(\d([1-4])\d)}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{(x?([1-4])\d)}', $s, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
	if (preg_match('{([^1-4])}', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
	if (preg_match("{([\r\n]+)(\n)([\n])}", $s, $matches)) {
		assertType('array{string, string, "\n", "\n"}', $matches);
	}
	if (preg_match('/foo(*:first)|bar(*:second)([x])/', $s, $matches)) {
		assertType("array{0: string, 1?: 'x', MARK?: 'first'|'second'}", $matches);
	}
}

function (string $s): void {
	preg_match('/%a(\d*)/', $s, $matches);
	assertType("array{}|array{string, string}", $matches);
};

class Bug11376
{
	public function test(string $str): void
	{
		preg_match('~^(?:(\w+)::)?(\w+)$~', $str, $matches);
		assertType("array{}|array{string, string, string}", $matches);
	}

	public function test2(string $str): void
	{
		if (preg_match('~^(?:(\w+)::)?(\w+)$~', $str, $matches) === 1) {
			assertType('array{string, string, string}', $matches);
		}
	}
}

function (string $s): void {
	if (rand(0,1)) {
		$p = '/Price: (£)(abc)/';
	} else {
		$p = '/Price: (\d)(b)/';
	}

	if (preg_match($p, $s, $matches)) {
		assertType("array{string, '£', 'abc'}|array{string, string, 'b'}", $matches);
	}
};

function (string $s): void {
	if (rand(0,1)) {
		$p = '/Price: (£)/';
	} else {
		$p = '/Price: (£|(\d)|(x))/';
	}

	if (preg_match($p, $s, $matches)) {
		assertType("array{0: string, 1: '£'}|list{0: string, 1: string, 2?: string, 3?: 'x'}", $matches); // PHPantom: the two patterns' shapes stay apart
	}
};

function (string $s): void {
	if (preg_match('/Price: ([a-z])/i', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: ([0-9])/i', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: ([xXa])/i', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: ([xXa])/', $s, $matches)) {
		assertType("array{string, 'a'|'X'|'x'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (ba[rz])/', $s, $matches)) {
		assertType("array{string, 'bar'|'baz'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (b[ao][mn])/', $s, $matches)) {
		assertType("array{string, 'bam'|'ban'|'bom'|'bon'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (\s{3}|0)/', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (a|bc?)/', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (?<named>a|bc?)/', $s, $matches)) {
		assertType("array{0: string, named: string, 1: string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (a|0c?)/', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (a|\d)/', $s, $matches)) {
		assertType("array{string, string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (?<named>a|\d)/', $s, $matches)) {
		assertType("array{0: string, named: string, 1: string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (a|0)/', $s, $matches)) {
		assertType("array{string, '0'|'a'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/Price: (aa|0)/', $s, $matches)) {
		assertType("array{string, '0'|'aa'}", $matches);
	}
};

function (string $s): void {
	if (preg_match('/( \d+ )/x', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $s): void {
	if (preg_match('/( .? )/x', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $s): void {
	if (preg_match('/( .* )/x', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $s): void {
	if (preg_match('/( .+ )/x', $s, $matches)) {
		assertType('array{string, string}', $matches);
	}
};

function (string $value): void
{
	if (preg_match('/^(x)*$/', $value, $matches, PREG_OFFSET_CAPTURE)) {
		assertType("array{0: array{string, int<-1, max>}, 1?: array{string, int<-1, max>}}", $matches);
	}
};

function (string $value): void {
	if (preg_match('/^(?:(x)|(y))*$/', $value, $matches, PREG_OFFSET_CAPTURE)) {
		assertType("array{0: array{string, int<-1, max>}, 1?: array{string, int<-1, max>}}|array{array{string, int<-1, max>}, array{'', int<-1, max>}, array{string, int<-1, max>}}", $matches);
	}
};

class Bug11479
{
	static public function sayHello(string $source): void
	{
		$pattern = "~^(?P<dateFrom>\d)?\-?(?P<dateTo>\d)?$~";

		preg_match($pattern, $source, $matches);

		// for $source = "-1" in $matches is
		// array (
		//  0 => '-1',
		//  'dateFrom' => '',
		//  1 => '',
		//  'dateTo' => '1',
		//  2 => '1',
		//)

		assertType("array{}|array{0: string, dateFrom?: string, 1?: string, dateTo?: string, 2?: string}", $matches);
	}
}

function (string $s): void {
	if (preg_match('~a|(\d)|(\s)~', $s, $matches) === 1) {
		assertType("array{0: string, 1?: string}|array{string, '', string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~a|((u)x)|((v)y)~', $s, $matches) === 1) {
		assertType("array{string, '', '', 'vy', 'v'}|array{string, 'ux', 'u'}|array{string}", $matches);
	}
};

function (string $s): void {
	if (preg_match('~a|(\d)|(\s)~', $s, $matches, PREG_OFFSET_CAPTURE) === 1) {
		assertType("array{0: array{string, int<-1, max>}, 1?: array{string, int<-1, max>}}|array{array{string, int<-1, max>}, array{'', int<-1, max>}, array{string, int<-1, max>}}", $matches);
	}
};

function (string $s): void {
	preg_match('~a|(\d)|(\s)~', $s, $matches);
	assertType("array{}|array{0: string, 1?: string}|array{string, '', string}", $matches);
};

function bug11490 (string $expression): void {
	$matches = [];

	if (preg_match('/([-+])?([\d]+)%/', $expression, $matches) === 1) {
		assertType("array{string, ''|'+'|'-', string}", $matches);
	}
}

function bug11490b (string $expression): void {
	$matches = [];

	if (preg_match('/([\\[+])?([\d]+)%/', $expression, $matches) === 1) {
		assertType("array{string, ''|'+'|'[', string}", $matches);
	}
}

function bug11622 (string $expression): void {
	$matches = [];

	if (preg_match('/^abc(def|$)/', $expression, $matches) === 1) {
		assertType("array{string, ''|'def'}", $matches); // PHPantom: `$` matches the empty string, so the group is one of two texts
	}
}

function bug11604 (string $string): void {
	if (! preg_match('/(XX)|(YY)?ZZ/', $string, $matches)) {
		return;
	}

	assertType("list{0: string, 1?: ''|'XX', 2?: 'YY'}", $matches);
	// could be array{string, '', 'YY'}|array{string, 'XX'}|array{string}
}

function bug11604b (string $string): void {
	if (preg_match('/(XX)|(YY)?(ZZ)/', $string, $matches)) {
		assertType("list{0: string, 1?: ''|'XX', 2?: ''|'YY', 3?: 'ZZ'}", $matches);
	}
}

function testLtrimDelimiter (string $string): void {
	if (preg_match(' /(x)/', $string, $matches)) {
		assertType("array{string, 'x'}", $matches);
	}

	if (preg_match('  /(x)/', $string, $matches)) {
		assertType("array{string, 'x'}", $matches);
	}
}

function testUnescapeBackslash (string $string): void {
	if (preg_match(<<<'EOD'
		~(\[)~
		EOD, $string, $matches)) {
		assertType("array{string, '['}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\d)~
		EOD, $string, $matches)) {
		assertType("array{string, string}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\\d)~
		EOD, $string, $matches)) {
		assertType("array{string, '\\\d'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\\\d)~
		EOD, $string, $matches)) {
		assertType("array{string, string}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\\\\d)~
		EOD, $string, $matches)) {
		assertType("array{string, '\\\\\\\d'}", $matches);
	}
}

function testEscapedDelimiter (string $string): void {
	if (preg_match(<<<'EOD'
		/(\/)/
		EOD, $string, $matches)) {
		assertType("array{string, '/'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\~)~
		EOD, $string, $matches)) {
		assertType("array{string, '~'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\[2])~
		EOD, $string, $matches)) {
		assertType("array{string, '[2]'}", $matches);
	}

	if (preg_match(<<<'EOD'
		[(\[2\])]
		EOD, $string, $matches)) {
		assertType("array{string, '[2]'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~(\{2})~
		EOD, $string, $matches)) {
		assertType("array{string, '{2}'}", $matches);
	}

	if (preg_match(<<<'EOD'
		{(\{2\})}
		EOD, $string, $matches)) {
		assertType("array{string, '{2}'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~([a\]])~
		EOD, $string, $matches)) {
		assertType("array{string, ']'|'a'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~([a[])~
		EOD, $string, $matches)) {
		assertType("array{string, '['|'a'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~([a\]b])~
		EOD, $string, $matches)) {
		assertType("array{string, ']'|'a'|'b'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~([a[b])~
		EOD, $string, $matches)) {
		assertType("array{string, '['|'a'|'b'}", $matches);
	}

	if (preg_match(<<<'EOD'
		~([a\[b])~
		EOD, $string, $matches)) {
		assertType("array{string, '['|'a'|'b'}", $matches);
	}

	if (preg_match(<<<'EOD'
		[([a\[b])]
		EOD, $string, $matches)) {
		assertType("array{string, '['|'a'|'b'}", $matches);
	}

	if (preg_match(<<<'EOD'
		{(x\\\{)|(y\\\\\})}
		EOD, $string, $matches)) {
		assertType("array{string, '', 'y\\\\\\\}'}|array{string, 'x\\\{'}", $matches);
	}
}

function bugUnescapedDashAfterRange (string $string): void
{
	if (preg_match('/([0-1-y])/', $string, $matches)) {
		assertType("array{string, string}", $matches);
	}
}

function bugEmptySubexpression (string $string): void {
	if (preg_match('//', $string, $matches)) {
		assertType("array{string}", $matches); // could be array{''}
	}

	if (preg_match('/()/', $string, $matches)) {
		assertType("array{string, ''}", $matches); // could be array{'', ''}
	}

	if (preg_match('/|/', $string, $matches)) {
		assertType("array{string}", $matches); // could be array{''}
	}

	if (preg_match('~|(a)~', $string, $matches)) {
		assertType("array{0: string, 1?: 'a'}", $matches);
	}

	if (preg_match('~(a)|~', $string, $matches)) {
		assertType("array{0: string, 1?: 'a'}", $matches);
	}

	if (preg_match('~(a)||(b)~', $string, $matches)) {
		assertType("array{0: string, 1?: 'a'}|array{string, '', 'b'}", $matches);
	}

	if (preg_match('~(|(a))~', $string, $matches)) {
		assertType("array{0: string, 1: ''|'a', 2?: 'a'}", $matches);
	}

	if (preg_match('~((a)|)~', $string, $matches)) {
		assertType("array{0: string, 1: ''|'a', 2?: 'a'}", $matches);
	}

	if (preg_match('~((a)||(b))~', $string, $matches)) {
		assertType("list{0: string, 1: ''|'a'|'b', 2?: ''|'a', 3?: 'b'}", $matches);
	}

	if (preg_match('~((a)|()|(b))~', $string, $matches)) {
		assertType("list{0: string, 1: ''|'a'|'b', 2?: ''|'a', 3?: '', 4?: 'b'}", $matches);
	}
}

function bug11744(string $string): void
{
	if (!preg_match('~^((/[a-z]+)?)~', $string, $matches)) {
		return;
	}
	assertType('array{0: string, 1: string, 2?: string}', $matches);

	if (!preg_match('~^((/[a-z]+)?.*)~', $string, $matches)) {
		return;
	}
	assertType('array{0: string, 1: string, 2?: string}', $matches);

	if (!preg_match('~^((/[a-z]+)?.+)~', $string, $matches)) {
		return;
	}
	assertType('array{0: string, 1: string, 2?: string}', $matches);
}

function bug12749(string $str): void
{
	if (preg_match('/[A-Z]/', $str, $match)) {
		assertType('array{string}', $match); // could be string
	}
}

function bug12749a(string $str): void
{
	if (preg_match('/[A-Z]{2,}/', $str, $match)) {
		assertType('array{string}', $match);
	}
}

function bug12749b(string $str): void
{
	if (preg_match('/[0-9][A-Z]/', $str, $match)) {
		assertType('array{string}', $match);
	}
}

function bug12749c(string $str): void
{
	if (preg_match('/[0-9][A-Z]?/', $str, $match)) {
		assertType('array{string}', $match);
	}
}

function bug12749d(string $str): void
{
	if (preg_match('/[0-9]?[A-Z]/', $str, $match)) {
		assertType('array{string}', $match);
	}
}

function bug12749e(string $str): void
{
	// no ^ $ delims, therefore can be anything which contains a number
	if (preg_match('/[0-9]/', $str, $match)) {
		assertType('array{string}', $match);
	}
}

function bug12749f(string $str): void
{
	if (preg_match('/^[0-9]$/', $str, $match)) {
		assertType('array{string}', $match); // could be string
	}
}

function bug12397(string $string): void {
	$m = preg_match('#\b([A-Z]{2,})-(\d+)#', $string, $match);
	assertType("array{}|array{string, string, string}", $match);
}

function bug12792(string $string): void {
	if (preg_match('~a\Kb~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{'b'}
	}

	if (preg_match('~a\K~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{''}
	}

	if (preg_match('~a\K.+~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{string}
	}

	if (preg_match('~a\K.*~', $string, $match) === 1) {
		assertType('array{string}', $match);
	}

	if (preg_match('~a\K(.+)~', $string, $match) === 1) {
		assertType('array{string, string}', $match); // could be array{string, string}
	}

	if (preg_match('~a\K(.*)~', $string, $match) === 1) {
		assertType('array{string, string}', $match);
	}

	if (preg_match('~a\K(.+?)~', $string, $match) === 1) {
		assertType('array{string, string}', $match); // could be array{string, string}
	}

	if (preg_match('~a\K(.*?)~', $string, $match) === 1) {
		assertType('array{string, string}', $match);
	}

	if (preg_match('~a\K(?=.+)~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{''}
	}

	if (preg_match('~a\K(?=.*)~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{''}
	}

	if (preg_match('~a(?:x\Kb|c)~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{'ac'|'b'}
	}

	if (preg_match('~a(?:c|x\Kb)~', $string, $match) === 1) {
		assertType('array{string}', $match); // could be array{'ac'|'b'}
	}

	if (preg_match('~a(y|(?:x\Kb|c))d~', $string, $match) === 1) {
		assertType("array{string, 'c'|'xb'|'y'}", $match); // PHPantom: `\K` moves the start of the whole match, not of the group
	}

	if (preg_match('~a((?:c|x\Kb)|y)d~', $string, $match) === 1) {
		assertType("array{string, 'c'|'xb'|'y'}", $match); // PHPantom: `\K` moves the start of the whole match, not of the group
	}
}

function testExtendedSyntaxEscapedHash(string $string): void {
	// \# in extended mode should be treated as literal hash, not comment
	if (preg_match('/^ ([\#.]) $/x', $string, $matches)) {
		assertType("array{string, '#'|'.'}", $matches);
	}

	// Real comment vs escaped hash
	if (preg_match('/
		(\d+)      # this is a comment
		([\#ab]+)  # hash in character class (escaped)
	/x', $string, $matches)) {
		assertType('array{string, string, string}', $matches);
	}
}
