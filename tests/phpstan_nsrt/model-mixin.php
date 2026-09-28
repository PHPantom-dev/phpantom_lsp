<?php // lint >= 8.0

namespace ModelMixin;

use function PHPStan\Testing\assertType;

/** @mixin Builder<static> */
class Model
{
	/** @param array<int, mixed> $args */
	public static function __callStatic(string $method, array $args): mixed
	{
		(new self)->$method(...$args);
	}
}

/** @template TModel as Model */
class Builder
{
	/** @return array<int, TModel> */
	public function all() { return []; }
}

class User extends Model
{
}

function (): void {
	assertType('array<int, ModelMixin\User>', User::all());
};

class MixedMethod
{

	public function doFoo(): int
	{
		return 1;
	}

}

/** @mixin MixedMethod */
interface InterfaceWithMixin
{

}

function (InterfaceWithMixin $i): void {
	assertType('int', $i->doFoo());
};
