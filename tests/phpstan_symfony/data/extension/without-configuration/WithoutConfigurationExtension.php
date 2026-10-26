<?php

namespace PHPStan\Type\Symfony\Extension\WithoutConfiguration;

use Symfony\Component\DependencyInjection\ContainerBuilder;
use \Symfony\Component\DependencyInjection\Extension\Extension;

class WithoutConfigurationExtension extends Extension
{
	public function load(array $configs, ContainerBuilder $container): void
	{
		\PHPStan\Testing\assertType( // SKIP: Symfony return types an argument decides
			'null',
			$this->getConfiguration($configs, $container)
		);
	}
}
