<?php // lint >= 8.0

declare(strict_types = 1);

namespace Bug13365;

use function PHPStan\Testing\assertType;

class Foo
{
	public function test(\DOMElement $element): void
	{
		$attributes = $element->attributes;

		if ($attributes === null) {
			return;
		}

		assertType('DOMNamedNodeMap<DOMAttr>', $attributes);
		// PHPantom is more precise than PHPStan here: The bundled stub declares `@return Iterator<string, TNode>`, and iterating a DOMNamedNodeMap really does yield attribute names as string keys.
		assertType('Iterator<string, DOMAttr>', $attributes->getIterator());
		assertType('DOMAttr|null', $attributes->getNamedItem('foo'));
		assertType('DOMAttr|null', $attributes->getNamedItemNS('foo', 'bar'));
		assertType('DOMAttr|null', $attributes->item(0));

		foreach ($element->attributes ?? [] as $attr) {
			assertType('DOMAttr', $attr);
		}
	}
}
