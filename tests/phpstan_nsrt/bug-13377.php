<?php

namespace Bug13377;

use function PHPStan\Testing\assertType;

function doFoo() {
	$array = random_array();

	// removing this line prevents the unexpected behavior
	if(empty($array)){
		return;
	}

	// this should not assume result is not empty
	// same result when $strict is set to true
	$possiblyEmptyInvalid = array_keys($array, 'yes');
	assertType('list<(int|string)>', $possiblyEmptyInvalid);

	$possiblyEmptyInvalid = array_keys($array, 'yes', true);
	assertType('list<(int|string)>', $possiblyEmptyInvalid);

	$possiblyEmptyValid = array_keys(array_filter($array, fn($val) => $val == 'yes'));
	assertType('list<(int|string)>', $possiblyEmptyValid);
}



/**
 * @return array<string>
 */
function random_array(): array
{
	$return = [];
	for($i = 0; $i < 10; $i++){
		if(random_int(0, 1) === 1){
			$return[] = 'yes';
		}else{
			$return[] = 'no';
		}
	}

	return $return;
}
