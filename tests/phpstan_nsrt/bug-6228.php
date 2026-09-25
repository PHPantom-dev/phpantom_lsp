<?php

namespace Bug6228;

use function PHPStan\Testing\assertType;

class Bar
{
	/**
	 * @param \DOMNodeList<\DOMNode>|\DOMNode|\DOMNode[]|string|null $node
	 */
	public function __construct($node)
	{
		assertType('array<DOMNode>|DOMNode|DOMNodeList<DOMNode>|string|null', $node);
	}
}
