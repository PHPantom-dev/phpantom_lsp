<?php

namespace PHPStan\Type\Symfony\Extension\IgnoreImplemented;

use Symfony\Component\Config\Definition\ConfigurationInterface;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use \Symfony\Component\DependencyInjection\Extension\Extension;

class IgnoreImplementedExtension extends Extension
{
	public function load(array $configs, ContainerBuilder $container): void
	{
		\PHPStan\Testing\assertType(
			'Symfony\Component\Config\Definition\ConfigurationInterface|null',
			$this->getConfiguration($configs, $container)
		);
	}

	public function getConfiguration(array $config, ContainerBuilder $container): ?ConfigurationInterface
	{
		return null;
	}
}
