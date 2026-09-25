<?php declare(strict_types = 1);

namespace Bug13358;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-sealed SystemActor|AnonymousVisitorActor
 */
abstract class Actor
{
	/**
	 * @phpstan-assert-if-true SystemActor $this
	 */
	public function isSystem() : bool
	{
		return $this instanceof SystemActor;
	}

	/**
	 * @phpstan-assert-if-true AnonymousVisitorActor $this
	 */
	public function isAnonymousVisitor() : bool
	{
		return $this instanceof AnonymousVisitorActor;
	}
}

class SystemActor extends Actor
{
}

class AnonymousVisitorActor extends Actor
{
}

function (AnonymousVisitorActor|SystemActor $actor): void {
	assertType('Bug13358\AnonymousVisitorActor|Bug13358\SystemActor', $actor);

	if ($actor->isSystem()) {
		assertType('Bug13358\SystemActor', $actor);
	} else {
		assertType('Bug13358\AnonymousVisitorActor', $actor);
	}
};
