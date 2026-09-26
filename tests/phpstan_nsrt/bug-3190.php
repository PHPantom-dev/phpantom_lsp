<?php

namespace Bug3190;

use function PHPStan\Testing\assertType;

interface Server
{
	public function isDedicated(): bool;

	public function getSize(): int;
}

class Deployer
{
	public function deploy(object $component): void
	{
		$dedicated = $component instanceof Server ? $component->isDedicated() : false;
		if (!$dedicated) {
			return;
		}

		assertType(Server::class, $component);
	}

	public function deploy2(object $component): void
	{
		$dedicated = $component instanceof Server && $component->isDedicated();
		if (!$dedicated) {
			return;
		}

		assertType(Server::class, $component);
	}

	public function deploy3(object $component): void
	{
		$dedicated = $component instanceof Server;
		if (!$dedicated) {
			return;
		}

		assertType(Server::class, $component);
	}

	public function deploy4(object $component): void
	{
		$dedicated = $component instanceof Server ? $component->isDedicated() : rand(0, 1);
		if (!$dedicated) {
			return;
		}

		assertType('object', $component);
	}
}

class Deployer2
{
	public function deploy(object $component): void
	{
		$dedicated = $component instanceof Server ? $component->isDedicated() : false;
		if ($dedicated) {
			assertType(Server::class, $component);
			return;
		}
	}

	public function deploy2(object $component): void
	{
		$dedicated = $component instanceof Server && $component->isDedicated();
		if ($dedicated) {
			assertType(Server::class, $component);
			return;
		}
	}

	public function deploy3(object $component): void
	{
		$dedicated = $component instanceof Server;
		if ($dedicated) {
			assertType(Server::class, $component);
			return;
		}
	}

	public function deploy4(object $component): void
	{
		$dedicated = $component instanceof Server ? $component->isDedicated() : rand(0, 1);
		if ($dedicated) {
			assertType('object', $component);
			return;
		}
	}
}
