<?php // lint >= 8.0

namespace Bug5058;

use function PHPStan\Testing\assertType;

class test{

	private string $properString;

	public function doSomething(mixed $string): void
	{

		$errors = [];
		if(is_string($string) === false){
			$errors['string'] = 'fail';
		}

		assertType('mixed', $string);
		if(empty($errors) === false){
			throw new Exception('Epic fail');
		}

		assertType('string', $string);
		$this->properString = $string;
	}

	public function getProperString(): string
	{
		return $this->properString;
	}

}
