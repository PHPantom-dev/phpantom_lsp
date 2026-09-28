<?php declare(strict_types = 1);

namespace Bug4552;

use function PHPStan\Testing\assertType;

interface OptionPresenter {}

/**
 * @template TPresenter of OptionPresenter
 */
interface OptionDefinition {
	/**
	 * @return TPresenter
	 */
	public function presenter();
}

class SimpleOptionPresenter implements OptionPresenter {
	public function test(): bool
	{

	}
}

/**
 * @template-implements OptionDefinition<SimpleOptionPresenter>
 */
class SimpleOptionDefinition implements OptionDefinition {
	public function presenter()
	{
		return new SimpleOptionPresenter();
	}
}

/**
 * @template T of OptionPresenter
 *
 * @param class-string<OptionDefinition<T>> $definition
 *
 * @return T
 */
function present($definition) {
	return instantiate($definition)->presenter();
}


/**
 * @template T of OptionDefinition
 *
 * @param class-string<T> $definition
 *
 * @return T
 */
function instantiate($definition) {
	return new $definition;
}

function (): void {
	$p = present(SimpleOptionDefinition::class);
	assertType(SimpleOptionPresenter::class, $p);
	$p->test();
};
