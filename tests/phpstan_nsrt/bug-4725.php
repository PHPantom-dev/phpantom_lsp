<?php

namespace Bug4725;

use function PHPStan\Testing\assertType;

class Application_Model_Ada extends Clx_Model_Abstract {}

abstract class Clx_Model_Abstract {}

/**
 * @template T of Clx_Model_Abstract
 */
abstract class Clx_Model_Mapper_Abstract
{
}


/**
 * @template T of Clx_Model_Abstract
 */
class Clx_Paginator_Adapter_Mapper
{
	/**
	 * @phpstan-param Clx_Model_Mapper_Abstract<T> $mapper
	 */
	public function __construct(Clx_Model_Mapper_Abstract $mapper)
	{
	}

	/** @return T */
	public function getT()
	{

	}
}

/**
 * @template T of Application_Model_Ada
 * @extends Clx_Model_Mapper_Abstract<T>
 */
class ClxProductNet_Model_Mapper_Ada extends Clx_Model_Mapper_Abstract
{

	public function x() {
		$map = new Clx_Paginator_Adapter_Mapper($this);
		assertType('Bug4725\Clx_Paginator_Adapter_Mapper<T of Bug4725\Application_Model_Ada (class Bug4725\ClxProductNet_Model_Mapper_Ada, argument)>', $map);
		assertType('T of Bug4725\Application_Model_Ada (class Bug4725\ClxProductNet_Model_Mapper_Ada, argument)', $map->getT());
	}
}
