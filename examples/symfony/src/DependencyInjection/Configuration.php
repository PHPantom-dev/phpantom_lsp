<?php

namespace App\DependencyInjection;

use Symfony\Component\Config\Definition\Builder\ArrayNodeDefinition;
use Symfony\Component\Config\Definition\Builder\TreeBuilder;

final class Configuration
{
    public function getConfigTreeBuilder(): TreeBuilder
    {
        $treeBuilder = new TreeBuilder('acme_demo');
        $rootNode = $treeBuilder->getRootNode();
        assert($rootNode instanceof ArrayNodeDefinition);
        $rootNode
            ->children()
                ->scalarNode('api_key')->end()
                ->arrayNode('mailer')
                    ->children()
                        ->scalarNode('dsn')->end()
                    ->end()
                ->end();

        return $treeBuilder;
    }
}
