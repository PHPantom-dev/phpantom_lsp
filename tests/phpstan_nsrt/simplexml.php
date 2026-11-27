<?php

namespace SimpleXMLIteratorBug;

use SimpleXMLElement;
use function PHPStan\Testing\assertType;

class Foo
{

	public function getAddressByGps()
	{
		/** @var SimpleXMLElement|null $data */
		$data = doFoo();

		if ($data === null) {
			return;
		}

		assertType('(SimpleXMLElement|null)', $data->item);
		foreach ($data->item as $item) {
			assertType('SimpleXMLElement', $item);
			assertType('SimpleXMLElement|null', $item['name']);
		}
	}

}

class Bar extends SimpleXMLElement
{

	public function getAddressByGps()
	{
		/** @var self|null $data */
		$data = doFoo();

		if ($data === null) {
			return;
		}

		assertType('(SimpleXMLIteratorBug\Bar|null)', $data->item);
		foreach ($data->item as $item) {
			assertType(self::class, $item);
			assertType('SimpleXMLIteratorBug\Bar|null', $item['name']);
		}
	}

}

class Baz
{

	public function getAddressByGps()
	{
		/** @var Bar|null $data */
		$data = doFoo();

		if ($data === null) {
			return;
		}

		assertType('(SimpleXMLIteratorBug\Bar|null)', $data->item);
		foreach ($data->item as $item) {
			assertType(Bar::class, $item);
			assertType('SimpleXMLIteratorBug\Bar|null', $item['name']);
		}
	}

}

class AsXML
{

	public function asXML(): void
	{
		$element = new SimpleXMLElement('');

		assertType('string|false', $element->asXML());

		assertType('bool', $element->asXML('/tmp/foo.xml'));
	}

	public function saveXML(): void
	{
		$element = new SimpleXMLElement('');

		assertType('string|false', $element->saveXML());

		assertType('bool', $element->saveXML('/tmp/foo.xml'));
	}

}
