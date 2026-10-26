<?php

namespace PHPStan\Type\Symfony\Extension\WithConfigurationWithConstructorRequiredParams;

use Symfony\Component\Config\Definition\ConfigurationInterface;

class Configuration implements ConfigurationInterface
{
	public function __construct($foo)
	{
	}

	public function getConfigTreeBuilder()
	{
	}
}
