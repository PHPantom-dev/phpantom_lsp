<?php

namespace PropertyTemplateTag;

use function PHPStan\Testing\assertType;

class BaseObject { }

class ObjectDatabase
{

	/** Array of class, then key, then value to object array
	 * @template T of BaseObject
	 * @var array<class-string<T>, array<string, array<string, array<string, T>>>> */
	private array $objectsByKey = array();

	public function LoadObjectsByKey() : void
	{
		assertType('array<class-string<PropertyTemplateTag\T>, array<string, array<string, array<string, PropertyTemplateTag\T>>>>', $this->objectsByKey);
	}
}
