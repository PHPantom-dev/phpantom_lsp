<?php declare(strict_types = 1);

namespace Bug6138;

use function PHPStan\Testing\assertType;

$indexed = [
	1,
	2,
	3,
];
$associative = [
	'a' => 1,
	'b' => 2,
	'c' => 3,
];
$unordered = [
	0 => 1,
	3 => 2,
	42 => 3,
];

shuffle( $indexed );
shuffle( $associative );
shuffle( $unordered );

assertType( 'non-empty-list<0|1|2>', array_keys( $indexed ) ); // SKIP: `shuffle()` forgets how many entries the shape had
assertType( 'non-empty-list<0|1|2>', array_keys( $associative ) ); // SKIP: `shuffle()` forgets how many entries the shape had
assertType( 'non-empty-list<0|1|2>', array_keys( $unordered ) ); // SKIP: `shuffle()` forgets how many entries the shape had
