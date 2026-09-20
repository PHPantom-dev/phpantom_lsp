<?php

// The values PHP itself leaves open are `mixed`, not untyped: the elements
// of an array nobody described, a `@return mixed` accessor's result, a
// `mixed` variadic's entries, and a member name only known at runtime.
// `mixed` is an answer that later narrowing can refine, so each of these
// has to produce it rather than nothing at all.

use function PHPStan\Testing\assertType;

class MixedSourcesValue
{
	public function describe(): string
	{
		return '';
	}
}

/** @return mixed */
function mixedSourceAttribute(string $key)
{
	return null;
}

class MixedSources
{
	public function elementsOfAnUndescribedArray(array $args): void
	{
		foreach ($args as $arg) {
			assertType('mixed', $arg);
		}
		assertType('mixed', $args[0]);
		assertType('mixed', $args['key']);
	}

	/**
	 * A closure's bare `array` parameter reads the same way a method's does.
	 */
	public function elementsOfAClosuresUndescribedArray(): void
	{
		$join = static function (array $types): string {
			foreach ($types as $type) {
				assertType('mixed', $type);
			}
			assertType('mixed', $types[0]);

			return '';
		};
		$join([]);
	}

	public function walkingAndIndexingAMixed(): void
	{
		$attribute = mixedSourceAttribute('args');
		foreach ($attribute as $entry) {
			assertType('mixed', $entry);
		}
		assertType('mixed', $attribute['key']);
		assertType('mixed', $attribute[0]);
	}

	/** @param mixed ...$args */
	public function entriesOfAMixedVariadic(...$args): void
	{
		assertType('mixed', $args[0]);
		foreach ($args as $arg) {
			assertType('mixed', $arg);
		}
	}

	/** @return mixed|null */
	public function loadFromCache()
	{
		return null;
	}

	public function destructuringAMixed(): void
	{
		[$scope, $name] = $this->loadFromCache();
		assertType('mixed', $scope);
		assertType('mixed', $name);
	}

	public function aMemberNameDecidedAtRuntime(string $name): void
	{
		assertType('mixed', MixedSourcesValue::{$name}());
		$value = new MixedSourcesValue();
		assertType('mixed', $value->{$name}());
		assertType('mixed', $value->{$name});
	}

	/**
	 * A `mixed` that survives a guard keeps whatever the guard proved, which
	 * is the whole point of answering `mixed` instead of nothing.
	 */
	public function narrowingAMixedSource(array $args): void
	{
		$arg = $args[0];
		if ($arg instanceof MixedSourcesValue) {
			assertType('MixedSourcesValue', $arg);
		}
		if (is_string($args[1])) {
			assertType('string', $args[1]);
		}
	}
}
