<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\Config\Definition\Builder;

/**
 * This class provides a fluent interface for building a node.
 *
 * @template TParent of (NodeDefinition&ParentNodeDefinitionInterface)|null = null
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class NodeBuilder implements NodeParentInterface
{
    /**
     * @var TParent
     */
    protected (NodeDefinition&ParentNodeDefinitionInterface)|null $parent = null;
    /**
     * @var array<string, class-string<NodeDefinition>>
     */
    protected array $nodeMapping;

    public function __construct()
    {}

    /**
     * Set the parent node.
     *
     * @template TNewParent of (NodeDefinition&ParentNodeDefinitionInterface)|null
     *
     * @psalm-this-out static<TNewParent>
     *
     * @return $this
     */
    public function setParent((NodeDefinition&ParentNodeDefinitionInterface)|null $parent): static
    {}

    /**
     * Creates a child array node.
     *
     * @param string|null $singular The singular name of the node when $name is plural
     *
     * @return ArrayNodeDefinition<$this>
     */
    public function arrayNode(string $name/* , ?string $singular = null */): ArrayNodeDefinition
    {}

    /**
     * Creates a child scalar node.
     *
     * @return ScalarNodeDefinition<$this>
     */
    public function scalarNode(string $name): ScalarNodeDefinition
    {}

    /**
     * Creates a child Boolean node.
     *
     * @return BooleanNodeDefinition<$this>
     */
    public function booleanNode(string $name): BooleanNodeDefinition
    {}

    /**
     * Creates a child integer node.
     *
     * @return IntegerNodeDefinition<$this>
     */
    public function integerNode(string $name): IntegerNodeDefinition
    {}

    /**
     * Creates a child float node.
     *
     * @return FloatNodeDefinition<$this>
     */
    public function floatNode(string $name): FloatNodeDefinition
    {}

    /**
     * Creates a child EnumNode.
     *
     * @return EnumNodeDefinition<$this>
     */
    public function enumNode(string $name): EnumNodeDefinition
    {}

    /**
     * Creates a child variable node.
     *
     * @return VariableNodeDefinition<$this>
     */
    public function variableNode(string $name): VariableNodeDefinition
    {}

    /**
     * Creates a child string node.
     *
     * @return StringNodeDefinition<$this>
     */
    public function stringNode(string $name): StringNodeDefinition
    {}

    /**
     * Returns the parent node.
     *
     * @return TParent
     */
    public function end(): (NodeDefinition&ParentNodeDefinitionInterface)|null
    {}

    /**
     * Creates a child node.
     *
     * @template T of 'array'|'variable'|'scalar'|'string'|'boolean'|'integer'|'float'|'enum'
     *
     * @return (
     *    T is 'array' ? ArrayNodeDefinition<$this>
     *    : (T is 'variable' ? VariableNodeDefinition<$this>
     *    : (T is 'scalar' ? ScalarNodeDefinition<$this>
     *    : (T is 'string' ? StringNodeDefinition<$this>
     *    : (T is 'boolean' ? BooleanNodeDefinition<$this>
     *    : (T is 'integer' ? IntegerNodeDefinition<$this>
     *    : (T is 'float' ? FloatNodeDefinition<$this>
     *    : (T is 'enum' ? EnumNodeDefinition<$this>
     *    : NodeDefinition<$this>)))))))
     * )
     *
     * @throws \RuntimeException When the node type is not registered
     * @throws \RuntimeException When the node class is not found
     */
    public function node(?string $name, string $type): NodeDefinition
    {}

    /**
     * Appends a node definition.
     *
     * Usage:
     *
     *     $node = new ArrayNodeDefinition('name')
     *         ->children()
     *             ->scalarNode('foo')->end()
     *             ->scalarNode('baz')->end()
     *             ->append($this->getBarNodeDefinition())
     *         ->end()
     *     ;
     *
     * @return $this
     */
    public function append(NodeDefinition $node): static
    {}

    /**
     * Adds or overrides a node Type.
     *
     * @param string                       $type  The name of the type
     * @param class-string<NodeDefinition> $class The fully qualified name the node definition class
     *
     * @return $this
     */
    public function setNodeClass(string $type, string $class): static
    {}

    /**
     * Returns the class name of the node definition.
     *
     * @throws \RuntimeException When the node type is not registered
     * @throws \RuntimeException When the node class is not found
     */
    protected function getNodeClass(string $type): string
    {}
}
