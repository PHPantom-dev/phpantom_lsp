<?php

namespace Nullsafe;

use function PHPStan\Testing\assertType;

class Foo
{

	private ?self $nullableSelf;

	private self $self;

	public function doFoo(?\Exception $e)
	{
		assertType('string|null', $e?->getMessage());
		assertType('Exception|null', $e);

		assertType('Throwable|null', $e?->getPrevious());
		assertType('string|null', $e?->getPrevious()?->getMessage());

		$e?->getMessage(assertType('Exception', $e));
	}

	public function doBar(?\ReflectionClass $r)
	{
		assertType('class-string<object>', $r->name);
		assertType('class-string<object>|null', $r?->name);

		assertType('Nullsafe\Foo|null', $this->nullableSelf?->self);
		assertType('Nullsafe\Foo|null', $this->nullableSelf?->self->self);
	}

	public function doBaz(?self $self)
	{
		if ($self?->nullableSelf) {
			assertType('Nullsafe\Foo', $self);
			assertType('Nullsafe\Foo', $self->nullableSelf);
			assertType('Nullsafe\Foo', $self?->nullableSelf);
		} else {
			assertType('Nullsafe\Foo|null', $self);
			//assertType('null', $self->nullableSelf);
			//assertType('null', $self?->nullableSelf);
		}

		assertType('Nullsafe\Foo|null', $self);
		assertType('Nullsafe\Foo|null', $self->nullableSelf);
		assertType('Nullsafe\Foo|null', $self?->nullableSelf);
	}

	public function doLorem(?self $self)
	{
		if ($self?->nullableSelf !== null) {
			assertType('Nullsafe\Foo', $self);
			assertType('Nullsafe\Foo', $self->nullableSelf);
			assertType('Nullsafe\Foo', $self?->nullableSelf);
		} else {
			assertType('Nullsafe\Foo|null', $self);
			// PHPantom is more precise than PHPStan here: with $self set, the check proved the property null, and reading it off a null $self gives null too.
			assertType('null', $self->nullableSelf);
			assertType('null', $self?->nullableSelf);
		}

		assertType('Nullsafe\Foo|null', $self);
		assertType('Nullsafe\Foo|null', $self->nullableSelf);
		assertType('Nullsafe\Foo|null', $self?->nullableSelf);
	}

	public function doIpsum(?self $self)
	{
		if ($self?->nullableSelf === null) {
			assertType('Nullsafe\Foo|null', $self);
			assertType('Nullsafe\Foo|null', $self);
			assertType('null', $self?->nullableSelf);
		} else {
			assertType('Nullsafe\Foo', $self);
			assertType('Nullsafe\Foo', $self->nullableSelf);
			assertType('Nullsafe\Foo', $self?->nullableSelf);
		}

		assertType('Nullsafe\Foo|null', $self);
		assertType('Nullsafe\Foo|null', $self->nullableSelf);
		assertType('Nullsafe\Foo|null', $self?->nullableSelf);
	}

	public function doDolor(?self $self)
	{
		if (!$self?->nullableSelf) {
			assertType('Nullsafe\Foo|null', $self);
			//assertType('null', $self->nullableSelf);
			//assertType('null', $self?->nullableSelf);
		} else {
			assertType('Nullsafe\Foo', $self);
			assertType('Nullsafe\Foo', $self->nullableSelf);
			assertType('Nullsafe\Foo', $self?->nullableSelf);
		}

		assertType('Nullsafe\Foo|null', $self);
		assertType('Nullsafe\Foo|null', $self->nullableSelf);
		assertType('Nullsafe\Foo|null', $self?->nullableSelf);
	}

	public function doNull(): void
	{
		$null = null;
		assertType('null', $null?->foo);
		assertType('null', $null?->doFoo());
	}

}
