<?php declare(strict_types = 1);

use function PHPStan\Testing\assertType;

$kernel = new class ('dev', true) extends \Symfony\Component\HttpKernel\Kernel {

	public function registerBundles(): void
	{
	}

	public function registerContainerConfiguration(\Symfony\Component\Config\Loader\LoaderInterface $loader): void
	{
	}

};

assertType('string', $kernel->locateResource(''));
assertType('string', $kernel->locateResource('', null, true));
// Symfony 6 dropped locateResource()'s $first parameter, so the call
// returns a string whatever it passes.
assertType('string', $kernel->locateResource('', null, false));
