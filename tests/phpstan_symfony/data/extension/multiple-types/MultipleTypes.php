<?php

namespace PHPStan\Type\Symfony\Extension\MultipleTypes;

use PHPStan\Type\Symfony\Extension\WithConfiguration\WithConfigurationExtension;
use PHPStan\Type\Symfony\Extension\WithoutConfiguration\WithoutConfigurationExtension;
use Symfony\Component\DependencyInjection\ContainerBuilder;

/**
 * @param WithConfigurationExtension|WithoutConfigurationExtension $extension
 */
function test($extension, array $configs, ContainerBuilder $container)
{
	\PHPStan\Testing\assertType( // SKIP: Symfony return types an argument decides
		'PHPStan\Type\Symfony\Extension\WithConfiguration\Configuration|null',
		$extension->getConfiguration($configs, $container)
	);
}
