<?php

namespace Bug4643;

use function PHPStan\Testing\assertType;

interface Entity
{

}

class User implements Entity
{

}

class Admin extends User
{

}

class Article implements Entity
{

}

/** @template E of Entity */
class Repository
{

	/**
	 * @template F of E
	 * @param F $entity
	 * @return F
	 */
	function store(Entity $entity): Entity
	{
		assertType('F of E of Bug4643\Entity (class Bug4643\Repository, argument) (method Bug4643\Repository::store(), argument)', $entity);
		return $entity;
	}

}

/** @extends Repository<User> */
class UserRepository extends Repository
{

	function store(Entity $entity): Entity
	{
		assertType('F of Bug4643\User (method Bug4643\Repository::store(), argument)', $entity);
		return $entity;
	}

}

function (UserRepository $r): void {
	assertType(User::class, $r->store(new User()));
	assertType(Admin::class, $r->store(new Admin()));
	// PHPStan leaves `F of Bug4643\User` unresolved here ("should be User::class, but
	// inheriting template tags is now broken like that"); PHPantom falls back to the bound.
	assertType('Bug4643\User', $r->store(new Article()));
};
