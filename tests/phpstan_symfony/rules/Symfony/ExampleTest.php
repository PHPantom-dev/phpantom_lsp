<?php declare(strict_types = 1);

namespace PHPStan\Rules\Symfony;

use Symfony\Bundle\FrameworkBundle\KernelBrowser;
use Symfony\Bundle\FrameworkBundle\Test\KernelTestCase;

abstract class ExampleTest extends KernelTestCase
{

	public function bar(): void
	{
		$container = self::getContainer();
		$container->get('private');
	}

	public function foo(KernelBrowser $browser): void
	{
		$container = $browser->getContainer();
		$container->get('private');
	}

}
