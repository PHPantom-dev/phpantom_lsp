<?php

namespace Analyser\Bug2574;

use function PHPStan\Testing\assertType;

abstract class Model {
	/** @return static */
	public function newInstance() {
		return new static();
	}
}

class Model1 extends Model {
}

/**
 * @template T of Model
 * @param T $m
 * @return T
 */
function foo(Model $m) : Model {
	assertType('T of Analyser\Bug2574\Model (function Analyser\Bug2574\foo(), argument)', $m);
	$instance = $m->newInstance();
	assertType('T of Analyser\Bug2574\Model (function Analyser\Bug2574\foo(), argument)', $m);
	return $instance;
}

function test(): void {
	assertType('Analyser\Bug2574\Model1', foo(new Model1()));
}
