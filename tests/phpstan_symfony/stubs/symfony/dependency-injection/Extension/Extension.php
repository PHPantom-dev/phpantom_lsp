<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\DependencyInjection\Extension;

use Symfony\Component\Config\Definition\ConfigurationInterface;
use Symfony\Component\Config\Definition\Processor;
use Symfony\Component\DependencyInjection\Container;
use Symfony\Component\DependencyInjection\ContainerBuilder;
use Symfony\Component\DependencyInjection\Exception\BadMethodCallException;
use Symfony\Component\DependencyInjection\Exception\InvalidArgumentException;
use Symfony\Component\DependencyInjection\Exception\LogicException;

/**
 * Provides useful features shared by many extensions.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
abstract class Extension implements ExtensionInterface, ConfigurationExtensionInterface
{
    private array $processedConfigs = [];

    /**
     * @return string|false
     *
     * @deprecated since Symfony 7.4, to be removed in Symfony 8.0 together with XML support.
     */
    public function getXsdValidationBasePath()
    {}

    /**
     * @return string
     *
     * @deprecated since Symfony 7.4, to be removed in Symfony 8.0 together with XML support.
     */
    public function getNamespace()
    {}

    /**
     * Returns the recommended alias to use in XML.
     *
     * This alias is also the mandatory prefix to use when using YAML.
     *
     * This convention is to remove the "Extension" postfix from the class
     * name and then lowercase and underscore the result. So:
     *
     *     AcmeHelloExtension
     *
     * becomes
     *
     *     acme_hello
     *
     * This can be overridden in a sub-class to specify the alias manually.
     *
     * @throws BadMethodCallException When the extension name does not follow conventions
     */
    public function getAlias(): string
    {}

    /**
     * @return ConfigurationInterface|null
     */
    public function getConfiguration(array $config, ContainerBuilder $container)
    {}

    final protected function processConfiguration(ConfigurationInterface $configuration, array $configs): array
    {}

    /**
     * @internal
     */
    final public function getProcessedConfigs(): array
    {}

    /**
     * @throws InvalidArgumentException When the config is not enableable
     */
    protected function isConfigEnabled(ContainerBuilder $container, array $config): bool
    {}
}
