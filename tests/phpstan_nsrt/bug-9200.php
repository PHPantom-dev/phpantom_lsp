<?php

namespace Bug9200;

use function PHPStan\Testing\assertType;

function test(\DOMElement $element, \DOMNode $node): void
{
	assertType('(DOMElement|false)', $node->appendChild($element));
	assertType('(DOMNode|false)', $element->appendChild($node));
	assertType('(DOMElement|false)', $node->removeChild($element));
	assertType('(DOMNode|false)', $element->removeChild($node));
	assertType('(DOMElement|false)', $node->insertBefore($element, $node));
	assertType('(DOMNode|false)', $node->insertBefore($node, $element));
	assertType('(DOMNode|false)', $element->insertBefore($node, $node));
}
