<?php // lint >= 8.0

namespace Bug4757;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function sayHello(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo()) {
			assertType(Reservation::class, $oldReservation);
			assertType('true', $oldReservation->isFoo());
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello2(?Reservation $oldReservation): void
	{
		if (!$oldReservation?->isFoo()) {
			assertType(Reservation::class . '|null', $oldReservation);
			return;
		}

		assertType(Reservation::class, $oldReservation);
		assertType('true', $oldReservation->isFoo());
	}

	public function sayHello3(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo() === true) {
			assertType(Reservation::class, $oldReservation);
			assertType('true', $oldReservation->isFoo());
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello4(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo() === false) {
			assertType(Reservation::class , $oldReservation);
			assertType('false', $oldReservation->isFoo());
			return;
		}

		//assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello5(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo() === null) {
			assertType(Reservation::class . '|null', $oldReservation);
			return;
		}

		assertType(Reservation::class, $oldReservation);
	}

	public function sayHello6(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo() !== null) {
			assertType(Reservation::class, $oldReservation);
			assertType('bool', $oldReservation->isFoo());
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
		assertType('bool', $oldReservation->isFoo());
	}

	public function sayHelloPure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFoo()) {
			assertType(Reservation::class, $oldReservation);
			assertType('true', $oldReservation->isFoo());
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHelloImpure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFooImpure()) {
			assertType(Reservation::class, $oldReservation);
			assertType('bool', $oldReservation->isFooImpure());
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello2Impure(?Reservation $oldReservation): void
	{
		if (!$oldReservation?->isFooImpure()) {
			assertType(Reservation::class . '|null', $oldReservation);
			return;
		}

		assertType(Reservation::class, $oldReservation);
	}

	public function sayHello3Impure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFooImpure() === true) {
			assertType(Reservation::class, $oldReservation);
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello4Impure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFooImpure() === false) {
			assertType(Reservation::class , $oldReservation);
			return;
		}

		//assertType(Reservation::class . '|null', $oldReservation);
	}

	public function sayHello5Impure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFooImpure() === null) {
			assertType(Reservation::class . '|null', $oldReservation);
			return;
		}

		assertType(Reservation::class, $oldReservation);
	}

	public function sayHello6Impure(?Reservation $oldReservation): void
	{
		if ($oldReservation?->isFooImpure() !== null) {
			assertType(Reservation::class, $oldReservation);
			return;
		}

		assertType(Reservation::class . '|null', $oldReservation);
	}
}

interface Reservation {
	public function isFoo(): bool;

	/** @phpstan-impure */
	public function isFooImpure(): bool;
}

interface Bar
{
	public function get(): ?int;

	/** @phpstan-impure */
	public function getImpure(): ?int;
}

class Foo
{

	public function getBarOrNull(): ?Bar
	{
		return null;
	}

	public function doFoo(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->get() === null) {
			assertType(Bar::class . '|null', $barOrNull);
			// PHPantom is more precise than PHPStan here: with the receiver set, the check proved what the pure call returns, and on a null receiver the call throws.
			assertType('null', $barOrNull->get());
			//assertType('null', $barOrNull?->get());
			return;
		}

		assertType(Bar::class, $barOrNull);
		assertType('int', $barOrNull->get());
		assertType('int', $barOrNull?->get());
	}

	public function doFooImpire(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->getImpure() === null) {
			assertType(Bar::class . '|null', $barOrNull);
			assertType('int|null', $barOrNull->getImpure());
			assertType('int|null', $barOrNull?->getImpure());
			return;
		}

		assertType(Bar::class, $barOrNull);
		assertType('int|null', $barOrNull->getImpure());
		assertType('int|null', $barOrNull?->getImpure());
	}

	public function doFoo2(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->get() !== null) {
			assertType(Bar::class, $barOrNull);
			assertType('int', $barOrNull->get());
			assertType('int', $barOrNull?->get());
			return;
		}

		assertType(Bar::class . '|null', $barOrNull);
		// PHPantom is more precise than PHPStan here: with the receiver set, the check proved what the pure call returns, and on a null receiver the call throws.
		assertType('null', $barOrNull->get());
	}

	public function doFoo2Impure(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->getImpure() !== null) {
			assertType(Bar::class, $barOrNull);
			assertType('int|null', $barOrNull->getImpure());
			assertType('int|null', $barOrNull?->getImpure());
			return;
		}

		assertType(Bar::class . '|null', $barOrNull);
		assertType('int|null', $barOrNull->getImpure());
		assertType('int|null', $barOrNull?->getImpure());
	}

	public function doFoo3(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->get()) {
			assertType(Bar::class, $barOrNull);
			return;
		}

		assertType(Bar::class . '|null', $barOrNull);
		// PHPantom is more precise than PHPStan here: the falsy int the check left is 0.
		assertType('0|null', $barOrNull->get());
	}

	public function doFoo3Impure(Bar $b): void
	{
		$barOrNull = $this->getBarOrNull();
		if ($barOrNull?->getImpure()) {
			assertType(Bar::class, $barOrNull);
			assertType('int|null', $barOrNull->getImpure());
			return;
		}

		assertType(Bar::class . '|null', $barOrNull);
		assertType('int|null', $barOrNull->getImpure());
	}

}

class Chain
{

	/** @var int */
	private $baz;

	/** @var self|null */
	private $selfOrNull;

	/** @var self */
	private $self;

	public function find(): ?self
	{

	}

	public function get(): self
	{

	}

	/** @phpstan-impure */
	public function findImpure(): ?self
	{

	}

	public function doFoo(): void
	{
		assertType('int', $this->baz);
		assertType('int|null', $this->find()?->baz);
		assertType('int|null', $this->findImpure()?->baz);
	}

	public function doBar(): void
	{
		if ($this->selfOrNull?->find()?->baz !== null) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class, $this->selfOrNull->find());
		}
	}

	public function doBar2(): void
	{
		if ($this->selfOrNull?->find()?->get()->baz !== null) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class, $this->selfOrNull->find());
		}
	}

	public function doBar3(): void
	{
		if ($this->selfOrNull?->find()?->get()->find() !== null) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class, $this->selfOrNull->find());
		}
	}

	public function doBaz(): void
	{
		if ($this->selfOrNull?->findImpure()->baz !== null) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class . '|null', $this->selfOrNull->findImpure());
		}
	}

	public function doBaz2(): void
	{
		if ($this->selfOrNull?->self->baz !== null) {
			assertType(self::class, $this->selfOrNull);
		}
	}

	public function doBaz3(): void
	{
		if ($this->selfOrNull?->findImpure()->baz === 1) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class . '|null', $this->selfOrNull->findImpure());
		}
	}

	public function doBaz4(): void
	{
		if ($this->selfOrNull?->find()?->get()->baz === 1) {
			assertType(self::class, $this->selfOrNull);
			assertType(self::class, $this->selfOrNull->find());
		}
	}

	public function doVariable(): void
	{
		$foo = $this->selfOrNull;
		if ($foo?->get()->selfOrNull !== null) {
			assertType(self::class, $foo);
			assertType(self::class, $foo->get()->selfOrNull);
		}
	}

	public function doLorem(): void
	{
		if ($this->find()?->find()?->find() !== null) {
			assertType(self::class, $this->find());
			assertType(self::class, $this->find()->find());
			assertType(self::class, $this->find()->find()->find());
		}
	}

}
