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
		assertType('DOMElement', $doc->createElement('div'));
		assertType('DOMElement', $doc->createElement('my-element'));
		assertType('DOMElement', $doc->createElement('ns:tag'));
		assertType('DOMElement', $doc->createElement('_private'));
		assertType('DOMElement', $doc->createElement('h1'));
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
		assertType('DOMElement', $doc->createElement($validUnion));
		assertType('(DOMElement|false)', $doc->createElement($mixedUnion));
	}

	public function localVariable(DOMDocument $doc): void
	{
		$name = 'paragraph';
		assertType('DOMElement', $doc->createElement($name));
	}

}
