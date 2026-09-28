<?php

namespace Bug6748;

use function PHPStan\Testing\assertType;

class Foo
{
	/** @param \DOMNodeList<\DOMNode> $list */
	public function iterateNodes ($list): void
	{
		foreach($list as $item) {
			assertType('DOMNode', $item);
		}
	}

	/** @param \DOMXPath $path */
	public function xPathQuery ($path)
	{
		assertType('DOMNodeList<DOMNameSpaceNode|DOMNode>|false', $path->query(''));
	}
}
