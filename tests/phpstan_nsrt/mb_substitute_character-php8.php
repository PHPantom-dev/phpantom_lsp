<?php // lint >= 8.0

\PHPStan\Testing\assertType('\'entity\'|\'long\'|\'none\'|int<0, 55295>|int<57344, 1114111>', mb_substitute_character());
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(''));
\PHPStan\Testing\assertType('\'entity\'|\'long\'|\'none\'|int<0, 55295>|int<57344, 1114111>', mb_substitute_character(null)); // PHPStan says `true`, but `null` reads the setting, as no argument does
\PHPStan\Testing\assertType('true', mb_substitute_character('none'));
\PHPStan\Testing\assertType('true', mb_substitute_character('long'));
\PHPStan\Testing\assertType('true', mb_substitute_character('entity'));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character('foo'));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character('123'));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character('123.4'));
\PHPStan\Testing\assertType('true', mb_substitute_character(0xFFFD));
\PHPStan\Testing\assertType('true', mb_substitute_character(0x10FFFF));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(-1));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(0x110000));
\PHPStan\Testing\assertType('string|int|bool', mb_substitute_character($undefined)); // PHPStan says `bool`, but an argument of unknown type may be `null`, which reads the setting
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(new stdClass()));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(function () {}));
\PHPStan\Testing\assertType('*NEVER*', mb_substitute_character(rand(0xD800, 0xDFFF)));
\PHPStan\Testing\assertType('true', mb_substitute_character(rand(0, 0xDFFF))); // PHPStan says `bool`, but PHP 8 throws a `ValueError` for a code point it does not accept
\PHPStan\Testing\assertType('true', mb_substitute_character(rand(0xD800, 0x10FFFF))); // PHPStan says `bool`, but PHP 8 throws a `ValueError` for a code point it does not accept
