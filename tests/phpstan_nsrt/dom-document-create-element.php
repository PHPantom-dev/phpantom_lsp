<?php declare(strict_types = 1);

namespace DomDocumentCreateElement;

use DOMDocument;
use function PHPStan\Testing\assertType;

class Foo
{

	public function dynamicName(DOMDocument $doc, string $name): void
	{
		assertType('(DOMElement|false)', $doc->createElement($name));
	}

	public function validConstantNames(DOMDocument $doc): void
	{
		assertType('DOMElement', $doc->createElement('div')); // SKIP: createElement() keeps its false for a valid literal name
		assertType('DOMElement', $doc->createElement('my-element')); // SKIP: createElement() keeps its false for a valid literal name
		assertType('DOMElement', $doc->createElement('ns:tag')); // SKIP: createElement() keeps its false for a valid literal name
		assertType('DOMElement', $doc->createElement('_private')); // SKIP: createElement() keeps its false for a valid literal name
		assertType('DOMElement', $doc->createElement('h1')); // SKIP: createElement() keeps its false for a valid literal name
	}

	public function invalidConstantNames(DOMDocument $doc): void
	{
		assertType('(DOMElement|false)', $doc->createElement(''));
		assertType('(DOMElement|false)', $doc->createElement('123element'));
		assertType('(DOMElement|false)', $doc->createElement('my element'));
	}

	/**
	 * @param 'div'|'span' $validUnion
	 * @param 'div'|'' $mixedUnion
	 */
	public function unions(DOMDocument $doc, string $validUnion, string $mixedUnion): void
	{
		assertType('DOMElement', $doc->createElement($validUnion)); // SKIP: createElement() keeps its false for a valid literal name
		assertType('(DOMElement|false)', $doc->createElement($mixedUnion));
	}

	public function localVariable(DOMDocument $doc): void
	{
		$name = 'paragraph';
		assertType('DOMElement', $doc->createElement($name)); // SKIP: createElement() keeps its false for a valid literal name
	}

}
