<?php declare(strict_types = 1);

namespace Bug10438;

use SimpleXMLElement;
use function PHPStan\Testing\assertType;

class HelloWorld
{
	/**
	 * @return array<string, string|string[]>
	 */
	public function extract(SimpleXMLElement $data, string $type = 'Meta'): array
	{
		$keyName = ucfirst($type) . 'Type';
		$valueName = ucfirst($type) . 'Values';
		$meta = [];
		foreach ($data as $tag) {
			$key = (string)$tag->{$keyName};
			if ($tag->{$valueName}->count() == 1) {
				$meta[$key] = (string)$tag->{$valueName};
				continue;
			}
			assertType('array<string, list<string>|string>', $meta);
			$meta[$key] = [];
			assertType('array{}', $meta[$key]);
			foreach ($tag->{$valueName} as $value) {
				assertType('list<string>', $meta[$key]); // SKIP: a nested write under a dynamic key inside a loop is lost after the loop
				$meta[$key][] = (string)$value;
			}
		}
		return $meta;
	}
}
