<?php

declare(strict_types = 1);

namespace DOMLegacyNamedNodeMap;

use DOMAttr;
use DOMElement;
use DOMNode;
use DOMNamedNodeMap;
use function PHPStan\Testing\assertType;

class Foo
{
	public function basic_node(DOMNode $node): void {
		if ($node->hasAttributes()) {
			assertType('DOMNamedNodeMap<DOMAttr>', $node->attributes);
		} else {
			// Can be either null or an empty DOMNamedNodeMap
			assertType('DOMNamedNodeMap<DOMAttr>|null', $node->attributes);
		}
	}

	public function element_node(DOMElement $element): void
	{
		assertType('DOMNamedNodeMap<DOMAttr>', $element->attributes);
		if ($element->hasAttribute('class')) {
			$attribute = $element->getAttributeNode('class');
			assertType('(DOMAttr|false)', $attribute); // Could be DOMAttr
			assertType('string', $attribute->value);
		} else {
			$attribute = $element->getAttributeNode('class');
			assertType('(DOMAttr|false)', $attribute); // Could be false
		}
	}

	public function element_node_attribute_fetch_via_attributes_property(DOMElement $element): void
	{
		assertType('DOMNamedNodeMap<DOMAttr>', $element->attributes);
		if ($element->hasAttribute('class')) {
			$attribute = $element->attributes->getNamedItem('class');
			if ($attribute === null) {
				return;
			}
			assertType(DOMAttr::class, $attribute);
			assertType('string', $attribute->value);
		}
	}
}
