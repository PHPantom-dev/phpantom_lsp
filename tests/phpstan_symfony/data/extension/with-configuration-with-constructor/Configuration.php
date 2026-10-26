<?php

namespace PHPStan\Type\Symfony\Extension\WithConfigurationWithConstructor;

use Symfony\Component\Config\Definition\ConfigurationInterface;

class Configuration implements ConfigurationInterface
{
	public function __construct()
	{
	}

	public function getConfigTreeBuilder()
	{
	}
}
