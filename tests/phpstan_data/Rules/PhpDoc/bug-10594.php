<?php declare(strict_types = 1);

namespace Bug10594;

use Closure;
use ReflectionException;
use ReflectionFunction;
use ReflectionNamedType;
use function PHPStan\Testing\assertType;

/**
 * @phpstan-type Callback1 Closure(string):string
 * @phpstan-type Callback2 Closure(string, string):string
 */
class NarrowingCallbackTest
{
	/**
	 * @param Callback1|Callback2 $callback
	 *
	 * @phpstan-assert-if-true Callback1  $callback
	 * @phpstan-assert-if-false Callback2 $callback
	 * @throws ReflectionException
	 */
	public function isCallback1(Closure $callback): bool {
		assertType('(Closure(string): string)|(Closure(string, string): string)', $callback);

		$refl = new ReflectionFunction($callback);
		$parameters = $refl->getParameters();
		if (count($parameters) >= 2) {
			return $parameters[1]->getType() !== null && ($parameters[1]->getType() instanceof ReflectionNamedType && $parameters[1]->getType()->getName() !== 'string');
		}
		return true;
	}
}
