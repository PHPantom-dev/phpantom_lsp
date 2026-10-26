<?php

namespace PHPStan\Type\Symfony\Extension\WithConfigurationWithConstructor;

use Symfony\Component\DependencyInjection\ContainerBuilder;
use \Symfony\Component\DependencyInjection\Extension\Extension;

class WithConfigurationWithConstructorExtension extends Extension
{
	public function load(array $configs, ContainerBuilder $container): void
	{
		\PHPStan\Testing\assertType( // SKIP: Symfony return types an argument decides
			Configuration::class,
			$this->getConfiguration($configs, $container)
		);
	}
}
