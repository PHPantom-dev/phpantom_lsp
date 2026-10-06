<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug13450;

use function PHPStan\Testing\assertType;

/**
 * @template TRelated of Model
 * @template TDeclaring of Model
 * @template TResult
 */
abstract class Relation
{
	/** @return TResult */
    public function getResults(): mixed
	{
		return []; // @phpstan-ignore return.type
	}
}

/**
 * @template TRelated of Model
 * @template TDeclaring of Model
 * @template TPivot of Pivot = Pivot
 * @template TAccessor of string = 'pivot'
 *
 * @extends Relation<TRelated, TDeclaring, array<int, TRelated&object{pivot: TPivot}>>
 */
class BelongsToMany extends Relation {}

abstract class Model
{
	/**
	 * @template TRelated of Model
	 * @param class-string<TRelated> $related
	 * @return BelongsToMany<TRelated, $this>
	 */
	public function belongsToMany(string $related): BelongsToMany
	{
		return new BelongsToMany(); // @phpstan-ignore return.type
	}

	public function __get(string $name): mixed { return null; }
	public function __set(string $name, mixed $value): void {}
}

class Pivot extends Model {}

class User extends Model
{
	/** @return BelongsToMany<Team, $this> */
	public function teams(): BelongsToMany
	{
		return $this->belongsToMany(Team::class);
	}

	/** @return BelongsToMany<TeamFinal, $this> */
	public function teamsFinal(): BelongsToMany
	{
		return $this->belongsToMany(TeamFinal::class);
	}
}

class Team extends Model {}

final class TeamFinal extends Model {}

function test(User $user): void
{
	assertType('array<int, Bug13450\Team&object{pivot: Bug13450\Pivot}>', $user->teams()->getResults());
	assertType('array<int, Bug13450\TeamFinal&object{pivot: Bug13450\Pivot}>', $user->teamsFinal()->getResults());
}
