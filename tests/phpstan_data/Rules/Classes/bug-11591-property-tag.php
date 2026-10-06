<?php

namespace Bug11591PropertyTag;

use function PHPStan\Testing\assertType;

#[\AllowDynamicProperties]
class Model {
	use SoftDeletes;
}

/** @template TModel of Model */
class Builder {}

class User extends Model {}

/**
 * @property Builder<static> $a
 * @property static $b
 */
trait SoftDeletes {}

function test(User $user): void {
	assertType('Bug11591PropertyTag\\Builder<Bug11591PropertyTag\\User>', $user->a);
	assertType(User::class, $user->b);
}
