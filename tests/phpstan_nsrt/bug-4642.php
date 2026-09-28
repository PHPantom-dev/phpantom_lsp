<?php

namespace Bug4642;

use function PHPStan\Testing\assertType;

interface IEntity {}
/** @template E of IEntity */
interface IRepository {}

interface I {
	/**
	 * Returns repository by repository class.
	 * @template E of IEntity
	 * @template T of IRepository<E>
	 * @phpstan-param class-string<T> $className
	 * @phpstan-return T
	 */
	function getRepository(string $className): IRepository;
}

class User implements IEntity {}
/** @implements IRepository<User> */
class UsersRepository implements IRepository {}

function (I $model): void {
	assertType(UsersRepository::class, $model->getRepository(UsersRepository::class));
};
