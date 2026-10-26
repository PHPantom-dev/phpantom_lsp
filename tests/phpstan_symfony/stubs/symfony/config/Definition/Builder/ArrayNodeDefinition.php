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

use Symfony\Component\Config\Definition\ArrayNode;
use Symfony\Component\Config\Definition\Exception\InvalidDefinitionException;
use Symfony\Component\Config\Definition\NodeInterface;
use Symfony\Component\Config\Definition\PrototypedArrayNode;

/**
 * This class provides a fluent interface for defining an array node.
 *
 * @template TParent of NodeParentInterface|null = null
 *
 * @extends NodeDefinition<TParent>
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class ArrayNodeDefinition extends NodeDefinition implements ParentNodeDefinitionInterface
{
    protected bool $performDeepMerging = true;
    protected bool $ignoreExtraKeys = false;
    protected bool $removeExtraKeys = true;
    /**
     * @var NodeDefinition<$this>[]
     */
    protected array $children = [];
    /**
     * @var NodeDefinition<$this>
     */
    protected NodeDefinition $prototype;
    protected bool $atLeastOne = false;
    protected bool $allowNewKeys = true;
    protected ?string $key = null;
    protected bool $removeKeyItem = false;
    protected bool $addDefaults = false;
    protected int|string|array|false|null $addDefaultChildren = false;
    /**
     * @var NodeBuilder<static>
     */
    protected NodeBuilder $nodeBuilder;
    protected bool $normalizeKeys = true;

    /**
     * @var list<ExprBuilder::TYPE_*>|null
     */
    protected ?array $allowedTypes = null;

    /**
     * @param TParent $parent
     */
    public function __construct(?string $name, ?NodeParentInterface $parent = null)
    {}

    /**
     * @return $this
     */
    public function defaultValue(mixed $value): static
    {}

    /**
     * @param NodeBuilder<static> $builder
     */
    public function setBuilder(NodeBuilder $builder): void
    {}

    /**
     * Allows alternative types and wraps them into arrays.
     *
     * @param list<ExprBuilder::TYPE_INT|ExprBuilder::TYPE_STRING|ExprBuilder::TYPE_BOOL|ExprBuilder::TYPE_NULL|ExprBuilder::TYPE_BACKED_ENUM> $allowedTypes
     * @param string|null                                                                                                                      $key          The key to wrap the value in
     *
     * @return $this
     */
    public function acceptAndWrap(array $allowedTypes, ?string $key = null): static
    {}

    /**
     * @return NodeBuilder<static>
     */
    public function children(): NodeBuilder
    {}

    /**
     * Sets a prototype for child nodes.
     *
     * @template T of 'array'|'variable'|'scalar'|'string'|'boolean'|'integer'|'float'|'enum'
     *
     * @param T $type
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
     */
    public function prototype(string $type): NodeDefinition
    {}

    /**
     * @return VariableNodeDefinition<$this>
     */
    public function variablePrototype(): VariableNodeDefinition
    {}

    /**
     * @return ScalarNodeDefinition<$this>
     */
    public function scalarPrototype(): ScalarNodeDefinition
    {}

    /**
     * @return StringNodeDefinition<$this>
     */
    public function stringPrototype(): StringNodeDefinition
    {}

    /**
     * @return BooleanNodeDefinition<$this>
     */
    public function booleanPrototype(): BooleanNodeDefinition
    {}

    /**
     * @return IntegerNodeDefinition<$this>
     */
    public function integerPrototype(): IntegerNodeDefinition
    {}

    /**
     * @return FloatNodeDefinition<$this>
     */
    public function floatPrototype(): FloatNodeDefinition
    {}

    /**
     * @return self<$this>
     */
    public function arrayPrototype(): self
    {}

    /**
     * @return EnumNodeDefinition<$this>
     */
    public function enumPrototype(): EnumNodeDefinition
    {}

    /**
     * Adds the default value if the node is not set in the configuration.
     *
     * This method is applicable to concrete nodes only (not to prototype nodes).
     * If this function has been called and the node is not set during the finalization
     * phase, it's default value will be derived from its children default values.
     *
     * @return $this
     */
    public function addDefaultsIfNotSet(): static
    {}

    /**
     * Adds children with a default value when none are defined.
     *
     * This method is applicable to prototype nodes only.
     *
     * @param int|string|array|null $children The number of children|The child name|The children names to be added
     *
     * @return $this
     */
    public function addDefaultChildrenIfNoneSet(int|string|array|null $children = null): static
    {}

    /**
     * Requires the node to have at least one element.
     *
     * This method is applicable to prototype nodes only.
     *
     * @return $this
     */
    public function requiresAtLeastOneElement(): static
    {}

    /**
     * Disallows adding news keys in a subsequent configuration.
     *
     * If used all keys have to be defined in the same configuration file.
     *
     * @return $this
     */
    public function disallowNewKeysInSubsequentConfigs(): static
    {}

    /**
     * Sets a normalization rule for XML configurations.
     *
     * @param string      $singular The key to remap
     * @param string|null $plural   The plural of the key for irregular plurals
     *
     * @return $this
     */
    public function fixXmlConfig(string $singular, ?string $plural = null): static
    {}

    /**
     * Sets the attribute which value is to be used as key.
     *
     * This is useful when you have an indexed array that should be an
     * associative array. You can select an item from within the array
     * to be the key of the particular item. For example, if "id" is the
     * "key", then:
     *
     *     [
     *         ['id' => 'my_name', 'foo' => 'bar'],
     *     ];
     *
     *   becomes
     *
     *     [
     *         'my_name' => ['foo' => 'bar'],
     *     ];
     *
     * If you'd like "'id' => 'my_name'" to still be present in the resulting
     * array, then you can set the second argument of this method to false.
     *
     * This method is applicable to prototype nodes only.
     *
     * @param string $name          The name of the key
     * @param bool   $removeKeyItem Whether or not the key item should be removed
     *
     * @return $this
     */
    public function useAttributeAsKey(string $name, bool $removeKeyItem = true): static
    {}

    /**
     * Sets whether the node can be unset.
     *
     * @return $this
     */
    public function canBeUnset(bool $allow = true): static
    {}

    /**
     * Adds an "enabled" boolean to enable the current section.
     *
     * By default, the section is disabled. If any configuration is specified then
     * the node will be automatically enabled:
     *
     * enableableArrayNode: {enabled: true, ...}   # The config is enabled & default values get overridden
     * enableableArrayNode: ~                      # The config is enabled & use the default values
     * enableableArrayNode: true                   # The config is enabled & use the default values
     * enableableArrayNode: {other: value, ...}    # The config is enabled & default values get overridden
     * enableableArrayNode: {enabled: false, ...}  # The config is disabled
     * enableableArrayNode: false                  # The config is disabled
     *
     * @param string|null $info A description of what happens when the node is enabled or disabled
     *
     * @return $this
     */
    public function canBeEnabled(/* ?string $info = null */): static
    {}

    /**
     * Adds an "enabled" boolean to enable the current section.
     *
     * By default, the section is enabled.
     *
     * @param string|null $info A description of what happens when the node is enabled or disabled
     *
     * @return $this
     */
    public function canBeDisabled(/* ?string $info = null */): static
    {}

    /**
     * Disables the deep merging of the node.
     *
     * @return $this
     */
    public function performNoDeepMerging(): static
    {}

    /**
     * Allows extra config keys to be specified under an array without
     * throwing an exception.
     *
     * Those config values are ignored and removed from the resulting
     * array. This should be used only in special cases where you want
     * to send an entire configuration array through a special tree that
     * processes only part of the array.
     *
     * @param bool $remove Whether to remove the extra keys
     *
     * @return $this
     */
    public function ignoreExtraKeys(bool $remove = true): static
    {}

    /**
     * Sets whether to enable key normalization.
     *
     * @return $this
     */
    public function normalizeKeys(bool $bool): static
    {}

    public function append(NodeDefinition $node): static
    {}

    /**
     * Returns a node builder to be used to add children and prototype.
     *
     * @return NodeBuilder<static>
     */
    protected function getNodeBuilder(): NodeBuilder
    {}

    protected function createNode(): NodeInterface
    {}

    /**
     * Validate the configuration of a concrete node.
     *
     * @throws InvalidDefinitionException
     */
    protected function validateConcreteNode(ArrayNode $node): void
    {}

    /**
     * Validate the configuration of a prototype node.
     *
     * @throws InvalidDefinitionException
     */
    protected function validatePrototypeNode(PrototypedArrayNode $node): void
    {}

    /**
     * @return NodeDefinition<$this>[]
     */
    public function getChildNodeDefinitions(): array
    {}

    /**
     * Finds a node defined by the given $nodePath.
     *
     * @param string $nodePath The path of the node to find. e.g "doctrine.orm.mappings"
     */
    public function find(string $nodePath): NodeDefinition
    {}
}
