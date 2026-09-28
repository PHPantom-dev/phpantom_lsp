<?php

namespace Bug4733;

use function PHPStan\Testing\assertType;

class HelloWorld
{
	public function getDescription(?\DateTimeImmutable $start, ?string $someObject): void
	{
		if ($start === null && $someObject === null) {
			return;
		}

		// $start !== null || $someObject !== null

		if ($start !== null) {
			return;
		}

		// $start === null therefore $someObject !== null

		assertType('string', $someObject);
	}

	public function getDescriptionn(?\DateTimeImmutable $start, ?string $someObject): void
	{
		if ($start !== null && $someObject !== null) {
			return;
		}

		// $start === null || $someObject === null

		if ($start === null) {
			return;
		}

		// $start !== null therefore $someObject === null

		assertType('null', $someObject);
	}

	public function getDescription2(?\DateTimeImmutable $start, ?string $someObject): void
	{
		if ($start !== null || $someObject !== null) {
			if ($start !== null) {
				return;
			}

			// $start === null therefore $someObject !== null

			assertType('string', $someObject);
		}
	}

	public function getDescription3(?\DateTimeImmutable $start, ?string $someObject): void
	{
		if ($start === null && $someObject === null) {
			return;
		}

		// $start !== null || $someObject !== null

		if ($someObject !== null) {
			return;
		}

		// $someObject === null therefore $start cannot be null

		assertType('DateTimeImmutable', $start);
	}

	public function getDescription4(?\DateTimeImmutable $start, ?string $someObject): void
	{
		if ($start !== null || $someObject !== null) {
			if ($someObject !== null) {
				return;
			}

			// $someObject === null therefore $start cannot be null

			assertType('DateTimeImmutable', $start);
		}
	}
}
