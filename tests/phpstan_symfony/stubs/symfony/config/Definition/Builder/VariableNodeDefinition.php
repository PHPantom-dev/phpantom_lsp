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

use Symfony\Component\Config\Definition\NodeInterface;
use Symfony\Component\Config\Definition\VariableNode;

/**
 * This class provides a fluent interface for defining a node.
 *
 * @template TParent of NodeParentInterface|null = null
 *
 * @extends NodeDefinition<TParent>
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class VariableNodeDefinition extends NodeDefinition
{
    protected function instantiateNode(): VariableNode
    {}

    protected function createNode(): NodeInterface
    {}
}
