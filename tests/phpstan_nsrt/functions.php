<?php // lint >= 8.0

use function PHPStan\Testing\assertType;

$microtimeStringWithoutArg = microtime();
$microtimeString = microtime(false);
$microtimeFloat = microtime(true);
$microtimeDefault = microtime(null);
$microtimeBenevolent = microtime($undefined);

$versionCompare1 = version_compare('7.0.0', '7.0.1');
$versionCompare2 = version_compare('7.0.0', doFoo() ? '7.0.1' : '6.0.0');
$versionCompare3 = version_compare(doFoo() ? '7.0.0' : '6.0.5', doBar() ? '7.0.1' : '6.0.0');
$versionCompare4 = version_compare('7.0.0', doFoo());
$versionCompare5 = version_compare('7.0.0', '7.0.1', '<');
$versionCompare6 = version_compare('7.0.0', doFoo() ? '7.0.1' : '6.0.0', '<');
$versionCompare7 = version_compare(doFoo() ? '7.0.0' : '6.0.5', doBar() ? '7.0.1' : '6.0.0', '<');
$versionCompare8 = version_compare('7.0.0', doFoo(), '<');

$mbHttpOutputWithoutEncoding = mb_http_output();
$mbHttpOutputWithValidEncoding = mb_http_output('utf-8');
$mbHttpOutputWithInvalidEncoding = mb_http_output('foo');
$mbHttpOutputWithValidAndInvalidEncoding = mb_http_output(doFoo() ? 'utf-8' : 'foo');
$mbHttpOutputWithUnknownEncoding = mb_http_output(doFoo());

$mbRegexEncodingWithoutEncoding = mb_regex_encoding();
$mbRegexEncodingWithValidEncoding = mb_regex_encoding('utf-8');
$mbRegexEncodingWithInvalidEncoding = mb_regex_encoding('foo');
$mbRegexEncodingWithValidAndInvalidEncoding = mb_regex_encoding(doFoo() ? 'utf8' : 'foo');
$mbRegexEncodingWithUnknownEncoding = mb_regex_encoding(doFoo());

$mbInternalEncodingWithoutEncoding = mb_internal_encoding();
$mbInternalEncodingWithValidEncoding = mb_internal_encoding('utf-8');
$mbInternalEncodingWithInvalidEncoding = mb_internal_encoding('foo');
$mbInternalEncodingWithValidAndInvalidEncoding = mb_internal_encoding(doFoo() ? 'utf-8' : 'foo');
$mbInternalEncodingWithUnknownEncoding = mb_internal_encoding(doFoo());

$mbEncodingAliasesWithValidEncoding = mb_encoding_aliases('utf-8');
$mbEncodingAliasesWithInvalidEncoding = mb_encoding_aliases('foo');
$mbEncodingAliasesWithValidAndInvalidEncoding = mb_encoding_aliases(doFoo() ? 'utf-8' : 'foo');
$mbEncodingAliasesWithUnknownEncoding = mb_encoding_aliases(doFoo());

$mbChrWithoutEncoding = mb_chr(68);
$mbChrWithValidEncoding = mb_chr(68, 'utf-8');
$mbChrWithInvalidEncoding = mb_chr(68, 'foo');
$mbChrWithValidAndInvalidEncoding = mb_chr(68, doFoo() ? 'utf-8' : 'foo');
$mbChrWithUnknownEncoding = mb_chr(68, doFoo());

$mbOrdWithoutEncoding = mb_ord('');
$mbOrdWithValidEncoding = mb_ord('', 'utf-8');
$mbOrdWithInvalidEncoding = mb_ord('', 'foo');
$mbOrdWithValidAndInvalidEncoding = mb_ord('', doFoo() ? 'utf-8' : 'foo');
$mbOrdWithUnknownEncoding = mb_ord('', doFoo());

$gettimeofdayArrayWithoutArg = gettimeofday();
$gettimeofdayArray = gettimeofday(false);
$gettimeofdayFloat = gettimeofday(true);
$gettimeofdayDefault = gettimeofday(null);
$gettimeofdayBenevolent = gettimeofday($undefined);

// parse_url
/** @var int $integer */
$integer = doFoo();
$parseUrlWithoutParameters = parse_url();
$parseUrlConstantUrlWithoutComponent1 = parse_url('http://abc.def');
$parseUrlConstantUrlWithoutComponent2 = parse_url('http://def.abc', -1);
$parseUrlConstantUrlUnknownComponent = parse_url('http://def.abc', $integer);
$parseUrlConstantUrlWithComponentNull = parse_url('http://def.abc', PHP_URL_FRAGMENT);
$parseUrlConstantUrlWithComponentSet = parse_url('http://def.abc#this-is-fragment', PHP_URL_FRAGMENT);
$parseUrlConstantUrlWithComponentInvalid = parse_url('http://def.abc#this-is-fragment', 9999);
$parseUrlStringUrlWithComponentInvalid = parse_url($string, 9999);
$parseUrlStringUrlWithComponentPort = parse_url($string, PHP_URL_PORT);
$parseUrlStringUrlWithoutComponent = parse_url($string);

/** @var resource $resource */
$resource = doFoo();
$stat = stat(__FILE__);
$lstat = lstat(__FILE__);
$fstat = fstat($resource);
$fileObject = new \SplFileObject(__FILE__);
$fileObjectStat = $fileObject->fstat();

$base64DecodeWithoutStrict = base64_decode('');
$base64DecodeWithStrictDisabled = base64_decode('', false);
$base64DecodeWithStrictEnabled = base64_decode('', true);
$base64DecodeDefault = base64_decode('', null);
$base64DecodeBenevolent = base64_decode('', $undefined);


//str_word_count
$strWordCountWithoutParameters = str_word_count();
$strWordCountWithTooManyParams = str_word_count('string', 0, 'string', 'extra');
$strWordCountStr = str_word_count('string');
$strWordCountStrType0 = str_word_count('string', 0);
$strWordCountStrType1 = str_word_count('string', 1);
$strWordCountStrType1Extra = str_word_count('string', 1, 'string');
$strWordCountStrType2 = str_word_count('string', 2);
$strWordCountStrType2Extra = str_word_count('string', 2, 'string');

/** @var int $integer */
$integer = doFoo();
$strWordCountStrTypeIndeterminant = str_word_count('string', $integer);

assertType('non-falsy-string', $microtimeStringWithoutArg);
assertType('non-falsy-string', $microtimeString);
assertType('float', $microtimeFloat);
assertType('non-falsy-string', $microtimeDefault); // PHPStan says `float|non-falsy-string`, but `null` is read as `false` outside strict mode
assertType('(float|non-falsy-string)', $microtimeBenevolent);
assertType('-1', $versionCompare1);
assertType('-1|1', $versionCompare2);
assertType('-1|1', $versionCompare3); // PHPStan widens a comparison with two unions; every pair here differs
assertType('-1|0|1', $versionCompare4);
assertType('true', $versionCompare5);
assertType('bool', $versionCompare6);
assertType('bool', $versionCompare7);
assertType('bool', $versionCompare8);
assertType('string', $mbHttpOutputWithoutEncoding);
assertType('true', $mbHttpOutputWithValidEncoding);
assertType('*NEVER*', $mbHttpOutputWithInvalidEncoding); // PHPStan says `false`, but PHP 8 throws a `ValueError` for an unknown encoding
assertType('true', $mbHttpOutputWithValidAndInvalidEncoding); // PHPStan says `bool`, but the unknown encoding throws and the known one returns `true`
assertType('string|bool', $mbHttpOutputWithUnknownEncoding); // PHPStan says `bool`, but an argument of unknown type may be `null`, which reads the encoding
assertType('string', $mbRegexEncodingWithoutEncoding);
assertType('true', $mbRegexEncodingWithValidEncoding);
assertType('*NEVER*', $mbRegexEncodingWithInvalidEncoding); // PHPStan says `false`, but PHP 8 throws a `ValueError` for an unknown encoding
assertType('true', $mbRegexEncodingWithValidAndInvalidEncoding); // PHPStan says `bool`, but the unknown encoding throws and the known one returns `true`
assertType('string|bool', $mbRegexEncodingWithUnknownEncoding); // PHPStan says `bool`, but an argument of unknown type may be `null`, which reads the encoding
assertType('string', $mbInternalEncodingWithoutEncoding);
assertType('true', $mbInternalEncodingWithValidEncoding);
assertType('*NEVER*', $mbInternalEncodingWithInvalidEncoding); // PHPStan says `false`, but PHP 8 throws a `ValueError` for an unknown encoding
assertType('true', $mbInternalEncodingWithValidAndInvalidEncoding); // PHPStan says `bool`, but the unknown encoding throws and the known one returns `true`
assertType('string|bool', $mbInternalEncodingWithUnknownEncoding); // PHPStan says `bool`, but an argument of unknown type may be `null`, which reads the encoding
assertType('list<non-falsy-string>', $mbEncodingAliasesWithValidEncoding);
assertType('*NEVER*', $mbEncodingAliasesWithInvalidEncoding);
assertType('list<non-falsy-string>', $mbEncodingAliasesWithValidAndInvalidEncoding);
assertType('list<non-falsy-string>', $mbEncodingAliasesWithUnknownEncoding);
assertType('string|false', $mbChrWithoutEncoding); // PHPStan says `string`, but a code point the encoding cannot hold (a UTF-16 surrogate) gives `false`
assertType('string|false', $mbChrWithValidEncoding); // PHPStan says `string`, but a code point the encoding cannot hold (a UTF-16 surrogate) gives `false`
assertType('*NEVER*', $mbChrWithInvalidEncoding);
assertType('string|false', $mbChrWithValidAndInvalidEncoding);
assertType('string|false', $mbChrWithUnknownEncoding);
assertType('*NEVER*', $mbOrdWithoutEncoding); // PHPStan says `int`, but PHP 8 throws a `ValueError` for an empty string
assertType('*NEVER*', $mbOrdWithValidEncoding); // PHPStan says `int`, but PHP 8 throws a `ValueError` for an empty string
assertType('*NEVER*', $mbOrdWithInvalidEncoding);
assertType('*NEVER*', $mbOrdWithValidAndInvalidEncoding); // PHPStan says `int|false`, but PHP 8 throws a `ValueError` for an empty string
assertType('*NEVER*', $mbOrdWithUnknownEncoding); // PHPStan says `int|false`, but PHP 8 throws a `ValueError` for an empty string
assertType('array{sec: int, usec: int, minuteswest: int, dsttime: int}', $gettimeofdayArrayWithoutArg);
assertType('array{sec: int, usec: int, minuteswest: int, dsttime: int}', $gettimeofdayArray);
assertType('float', $gettimeofdayFloat);
assertType('array{sec: int, usec: int, minuteswest: int, dsttime: int}', $gettimeofdayDefault); // PHPStan says `array{…}|float`, but `null` is read as `false` outside strict mode
assertType('(array{sec: int, usec: int, minuteswest: int, dsttime: int}|float)', $gettimeofdayBenevolent);
assertType('array|int|string|false|null', $parseUrlWithoutParameters); // SKIP: a call PHP rejects is not reported as an error type
assertType('array{scheme: \'http\', host: \'abc.def\'}', $parseUrlConstantUrlWithoutComponent1); // SKIP: `parse_url()` of a literal URL is not folded
assertType('array{scheme: \'http\', host: \'def.abc\'}', $parseUrlConstantUrlWithoutComponent2); // SKIP: `parse_url()` of a literal URL is not folded
assertType('array{scheme?: lowercase-string, host?: lowercase-string, port?: int<0, 65535>, user?: lowercase-string, pass?: lowercase-string, path?: lowercase-string, query?: lowercase-string, fragment?: lowercase-string}|int<0, 65535>|lowercase-string|false|null', $parseUrlConstantUrlUnknownComponent);
assertType('null', $parseUrlConstantUrlWithComponentNull); // SKIP: `parse_url()` of a literal URL is not folded
assertType('\'this-is-fragment\'', $parseUrlConstantUrlWithComponentSet); // SKIP: `parse_url()` of a literal URL is not folded
assertType('*NEVER*', $parseUrlConstantUrlWithComponentInvalid); // PHPStan says `false`, but PHP 8 throws a `ValueError` for an unknown component
assertType('*NEVER*', $parseUrlStringUrlWithComponentInvalid); // PHPStan says `false`, but PHP 8 throws a `ValueError` for an unknown component
assertType('int<0, 65535>|false|null', $parseUrlStringUrlWithComponentPort);
assertType('array{scheme?: string, host?: string, port?: int<0, 65535>, user?: string, pass?: string, path?: string, query?: string, fragment?: string}|false', $parseUrlStringUrlWithoutComponent);
assertType('array{path: \'abc.def\'}', parse_url('abc.def')); // SKIP: `parse_url()` of a literal URL is not folded
assertType('null', parse_url('abc.def', PHP_URL_SCHEME)); // SKIP: `parse_url()` of a literal URL is not folded
assertType('\'http\'', parse_url('http://abc.def', PHP_URL_SCHEME)); // SKIP: `parse_url()` of a literal URL is not folded
assertType('array{0: int, 1: int, 2: int, 3: int, 4: int, 5: int, 6: int, 7: int, 8: int, 9: int, 10: int, 11: int, 12: int, dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false', $stat);
assertType('array{0: int, 1: int, 2: int, 3: int, 4: int, 5: int, 6: int, 7: int, 8: int, 9: int, 10: int, 11: int, 12: int, dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false', $lstat);
assertType('array{0: int, 1: int, 2: int, 3: int, 4: int, 5: int, 6: int, 7: int, 8: int, 9: int, 10: int, 11: int, 12: int, dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}|false', $fstat);
assertType('array{0: int, 1: int, 2: int, 3: int, 4: int, 5: int, 6: int, 7: int, 8: int, 9: int, 10: int, 11: int, 12: int, dev: int, ino: int, mode: int, nlink: int, uid: int, gid: int, rdev: int, size: int, atime: int, mtime: int, ctime: int, blksize: int, blocks: int}', $fileObjectStat);
assertType('string', $base64DecodeWithoutStrict);
assertType('string', $base64DecodeWithStrictDisabled);
assertType('string|false', $base64DecodeWithStrictEnabled);
assertType('string', $base64DecodeDefault);
assertType('(string|false)', $base64DecodeBenevolent);
assertType('*ERROR*', $strWordCountWithoutParameters); // SKIP: a call PHP rejects is not reported as an error type
assertType('*ERROR*', $strWordCountWithTooManyParams); // SKIP: a call PHP rejects is not reported as an error type
assertType('int', $strWordCountStr);
assertType('int', $strWordCountStrType0);
assertType('list<string>', $strWordCountStrType1); // PHPStan says `array<int, string>`, but format 1 lists the words
assertType('list<string>', $strWordCountStrType1Extra); // PHPStan says `array<int, string>`, but format 1 lists the words
assertType('array<int, string>', $strWordCountStrType2);
assertType('array<int, string>', $strWordCountStrType2Extra);
assertType('array<int, string>|int|list<string>', $strWordCountStrTypeIndeterminant); // PHPStan also has the `false` PHP 7 returned for an unknown format
