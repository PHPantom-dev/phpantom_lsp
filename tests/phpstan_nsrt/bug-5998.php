<?php

namespace Bug5998;

use Generator;
use function PHPStan\Testing\assertType;

abstract class Block{
	abstract public function getPosition() : int;
}

class HelloWorld
{
	/** @var Generator<Block>|null */
	private ?Generator $nullableGenerator;

	/** @var Generator<Block> */
	private Generator $regularGenerator;

	public function iterate() : void{
		foreach($this->nullableGenerator as $object){
			assertType(Block::class, $object);
		}

		foreach($this->regularGenerator as $object){
			assertType(Block::class, $object);
		}
	}
}
