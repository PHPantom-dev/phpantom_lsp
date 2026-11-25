<?php // lint >= 8.1

// The class it extends is declared in upstream's companion
// `Analyser/data/properties-defined.php`, which imports a different `Dolor`,
// so each keeps its own namespace block.
namespace PropertiesNamespace {

use AllowDynamicProperties;
use DOMDocument;
use SomeNamespace\Sit as Dolor;

/**
 * @property-read int $readOnlyProperty
 * @property-read int $overriddenReadOnlyProperty
 */
#[AllowDynamicProperties]
class Bar extends DOMDocument
{

	/**
	 * @var Dolor
	 */
	protected $inheritedProperty;

	/**
	 * @var self
	 */
	protected $inheritDocProperty;

	/**
	 * @var self
	 */
	protected $inheritDocWithoutCurlyBracesProperty;

	/**
	 * @var self
	 */
	protected $implicitInheritDocProperty;

	public function doBar(): Self
	{

	}

}

}

namespace PropertiesNamespace {

use function PHPStan\Testing\assertType;

use SomeNamespace\Amet as Dolor;
use SomeGroupNamespace\{One, Two as Too, Three};

/**
 * @property-read string $overriddenReadOnlyProperty
 * @property-read string $documentElement
 */
abstract class Foo extends Bar
{

	private $mixedProperty;

	/** @var Foo|Bar */
	private $unionTypeProperty;

	/**
	 * @var int
	 * @var int
	 */
	private $anotherMixedProperty;

	/**
	 * @vaz int
	 */
	private $yetAnotherMixedProperty;

	/** @var int */
	private $integerProperty;

	/** @var integer */
	private $anotherIntegerProperty;

	/** @var array */
	private $arrayPropertyOne;

	/** @var mixed[] */
	private $arrayPropertyOther;

	/**
	 * @var Lorem
	 */
	private $objectRelative;

	/**
	 * @var \SomeOtherNamespace\Ipsum
	 */
	private $objectFullyQualified;

	/**
	 * @var Dolor
	 */
	private $objectUsed;

	/**
	 * @var null|int
	 */
	private $nullableInteger;

	/**
	 * @var Dolor|null
	 */
	private $nullableObject;

	/**
	 * @var self
	 */
	private $selfType;

	/**
	 * @var static
	 */
	private $staticType;

	/**
	 * @var null
	 */
	private $nullType;

	/**
	 * @var Bar
	 */
	private $barObject;

	/**
	 * @var [$invalidType]
	 */
	private $invalidTypeProperty;

	/**
	 * @var resource
	 */
	private $resource;

	/**
	 * @var array[array]
	 */
	private $yetAnotherAnotherMixedParameter;

	/**
	 * @var \\Test\Bar
	 */
	private $yetAnotherAnotherAnotherMixedParameter;

	/**
	 * @var string
	 */
	private static $staticStringProperty;

	/**
	 * @var One
	 */
	private $groupUseProperty;

	/**
	 * @var Too
	 */
	private $anotherGroupUseProperty;

	/**
	 * {@inheritDoc}
	 */
	protected $inheritDocProperty;

	/**
	 * @inheritDoc
	 */
	protected $inheritDocWithoutCurlyBracesProperty;

	protected $implicitInheritDocProperty;

	public function doFoo()
	{
		assertType('mixed', $this->mixedProperty);
		assertType('int', $this->anotherMixedProperty); // PHPStan reads a repeated `@var` as invalid; PHPantom takes the type both tags agree on
		assertType('mixed', $this->yetAnotherMixedProperty);
		assertType('int', $this->integerProperty);
		assertType('int', $this->anotherIntegerProperty);
		assertType('array', $this->arrayPropertyOne);
		assertType('array<mixed>', $this->arrayPropertyOther);
		assertType('PropertiesNamespace\Lorem', $this->objectRelative);
		assertType('SomeOtherNamespace\Ipsum', $this->objectFullyQualified);
		assertType('SomeNamespace\Amet', $this->objectUsed);
		assertType('*ERROR*', $this->nonexistentProperty);
		assertType('int|null', $this->nullableInteger);
		assertType('SomeNamespace\Amet|null', $this->nullableObject);
		assertType('PropertiesNamespace\Foo', $this->selfType);
		assertType('static(PropertiesNamespace\Foo)', $this->staticType);
		assertType('null', $this->nullType);
		assertType('SomeNamespace\Sit', $this->inheritedProperty);
		assertType('PropertiesNamespace\Bar', $this->barObject->doBar());
		assertType('mixed', $this->invalidTypeProperty);
		assertType('resource', $this->resource);
		assertType('mixed', $this->yetAnotherAnotherAnotherMixedParameter);
		assertType('string', self::$staticStringProperty);
		assertType('SomeGroupNamespace\One', $this->groupUseProperty);
		assertType('SomeGroupNamespace\Two', $this->anotherGroupUseProperty);
		assertType('PropertiesNamespace\Bar', $this->inheritDocProperty);
		assertType('PropertiesNamespace\Bar', $this->inheritDocWithoutCurlyBracesProperty);
		assertType('PropertiesNamespace\Bar', $this->implicitInheritDocProperty);
		assertType('int', $this->readOnlyProperty);
		assertType('string', $this->overriddenReadOnlyProperty);
		assertType('DOMElement|null', $this->documentElement);
	}

}

}
