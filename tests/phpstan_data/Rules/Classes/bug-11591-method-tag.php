<?php

namespace Bug11591MethodTag;

use function PHPStan\Testing\assertType;

class Model {
	use SoftDeletes;
}

/** @template TModel of Model */
class Builder {}

class User extends Model {}

/**
 * @method static Builder<static> withTrashed(bool $withTrashed = true)
 * @method static Builder<static> onlyTrashed()
 * @method static Builder<static> withoutTrashed()
 * @method static bool restore()
 * @method static static restoreOrCreate(array<string, mixed> $attributes = [], array<string, mixed> $values = [])
 * @method static static createOrRestore(array<string, mixed> $attributes = [], array<string, mixed> $values = [])
 */
trait SoftDeletes {}

function test(): void {
	assertType('Bug11591MethodTag\\Builder<Bug11591MethodTag\\User>', User::withTrashed());
	assertType('Bug11591MethodTag\\Builder<Bug11591MethodTag\\User>', User::onlyTrashed());
	assertType('Bug11591MethodTag\\Builder<Bug11591MethodTag\\User>', User::withoutTrashed());
	assertType(User::class, User::createOrRestore());
	assertType(User::class, User::restoreOrCreate());
}
