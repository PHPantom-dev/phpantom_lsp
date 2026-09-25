<?php

namespace Bug12386;

use function PHPStan\Testing\assertType;

function doFoo() {
	$landMapper = new Application_Model_Mapper_Land();
	assertType('Bug12386\\Clx_Model_Iterator<Bug12386\\Application_Model_Land>', $landMapper->fetchAllActivePrependDefault(12));
}

/**
 * @template T of Clx_Model_Abstract
 */
abstract class Clx_Model_Mapper_Abstract
{
	public function __construct()
	{
	}
}

/**
 * @template T of Application_Model_Land
 *
 * @extends  Clx_Model_Mapper_Abstract<T>
 */
class ClxProductNet_Model_Mapper_Land extends Clx_Model_Mapper_Abstract
{
	/**
	 * @param int $defaultLandid
	 *
	 * @return Clx_Model_Iterator<T>
	 */
	public function fetchAllActivePrependDefault($defaultLandid): Clx_Model_Iterator
	{}
}

/**
 * @template T of Application_Model_Land
 *
 * @extends  ClxProductNet_Model_Mapper_Land<T>
 */
final class Application_Model_Mapper_Land extends ClxProductNet_Model_Mapper_Land
{
}

/**
 * @template T of Clx_Model_Abstract
 *
 * @implements \Iterator<T>
 */
abstract class Clx_Model_Iterator implements \Countable, \Iterator
{}

abstract class Clx_Model_Abstract implements \Stringable
{}

abstract class ClxProductNet_Model_Land extends Clx_Model_Abstract
{}

final class Application_Model_Land extends ClxProductNet_Model_Land
{
	public function __toString()
	{
		return 'foo';
	}

}
