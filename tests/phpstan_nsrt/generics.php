<?php

namespace GenericsTest;

use function PHPStan\Testing\assertType;

// ============================================================
// Scaffolding classes
// ============================================================

class User
{
	public string $name;

	public function getName(): string
	{
		return $this->name;
	}
}

class Admin extends User
{
	public function getRole(): string
	{
		return 'admin';
	}
}

/**
 * @template T
 */
class Box
{
	/** @var T */
	private $value;

	/**
	 * @param T $value
	 */
	public function __construct($value)
	{
		$this->value = $value;
	}

	/**
	 * @return T
	 */
	public function getValue()
	{
		return $this->value;
	}

	/**
	 * @param T $value
	 * @return void
	 */
	public function setValue($value): void
	{
		$this->value = $value;
	}
}

/**
 * @extends Box<\DateTime>
 */
class DateTimeBox extends Box
{
	public function __construct()
	{
		parent::__construct(new \DateTime());
	}
}

/**
 * @extends Box<int>
 */
class IntBox extends Box
{
	public function __construct()
	{
		parent::__construct(0);
	}
}

/**
 * @template V
 */
class Collection
{
	/** @var array<int, V> */
	private array $items;

	/**
	 * @param array<int, V> $items
	 */
	public function __construct(array $items = [])
	{
		$this->items = $items;
	}

	/**
	 * @return V|null
	 */
	public function first()
	{
		return $this->items[0] ?? null;
	}

	/**
	 * @param V $item
	 * @return void
	 */
	public function add($item): void
	{
		$this->items[] = $item;
	}

	/**
	 * @return array<int, V>
	 */
	public function all(): array
	{
		return $this->items;
	}

	/**
	 * @template U
	 * @param callable(V): U $callback
	 * @return Collection<U>
	 */
	public function map(callable $callback): self
	{
		return new self(array_map($callback, $this->items));
	}
}

/**
 * @extends Collection<User>
 */
class UserCollection extends Collection
{
}

/**
 * @extends Collection<Admin>
 */
class AdminCollection extends Collection
{
}

/**
 * @template T
 */
interface ContainerInterface
{
	/**
	 * @return T
	 */
	public function get();
}

/**
 * @implements ContainerInterface<string>
 */
class StringContainer implements ContainerInterface
{
	public function get()
	{
		return '';
	}
}

/**
 * @implements ContainerInterface<int>
 */
class IntContainer implements ContainerInterface
{
	public function get()
	{
		return 0;
	}
}

/**
 * @template TKey
 * @template TValue
 */
class Pair
{
	/** @var TKey */
	private $key;

	/** @var TValue */
	private $value;

	/**
	 * @param TKey $key
	 * @param TValue $value
	 */
	public function __construct($key, $value)
	{
		$this->key = $key;
		$this->value = $value;
	}

	/**
	 * @return TKey
	 */
	public function getKey()
	{
		return $this->key;
	}

	/**
	 * @return TValue
	 */
	public function getValue()
	{
		return $this->value;
	}
}

/**
 * @extends Pair<string, User>
 */
class NamedUser extends Pair
{
	/**
	 * @param string $name
	 * @param User $user
	 */
	public function __construct(string $name, User $user)
	{
		parent::__construct($name, $user);
	}
}

/**
 * @template T of \DateTimeInterface
 */
class DateCache
{
	/** @var T */
	private $date;

	/**
	 * @param T $date
	 */
	public function __construct($date)
	{
		$this->date = $date;
	}

	/**
	 * @return T
	 */
	public function getDate()
	{
		return $this->date;
	}
}

/**
 * @extends DateCache<\DateTime>
 */
class MutableDateCache extends DateCache
{
	public function __construct()
	{
		parent::__construct(new \DateTime());
	}
}

/**
 * @extends DateCache<\DateTimeImmutable>
 */
class ImmutableDateCache extends DateCache
{
	public function __construct()
	{
		parent::__construct(new \DateTimeImmutable());
	}
}

class Wrapper
{
	/**
	 * @template T
	 * @param T $value
	 * @return T
	 */
	public function identity($value)
	{
		return $value;
	}

	/**
	 * @template T
	 * @param T $a
	 * @param T $b
	 * @return T
	 */
	public function merge($a, $b)
	{
		return $a;
	}
}

// ============================================================
// 1. Function-level @template T with @param T / @return T
// ============================================================

/**
 * @template T
 * @param T $a
 * @return T
 */
function identity($a)
{
	return $a;
}

/**
 * @template T
 * @param T $a
 * @param T $b
 * @return T
 */
function pick($a, $b)
{
	return $a;
}

/**
 * @param int $int
 * @param string $string
 * @param float $float
 */
function testFunctionTemplates($int, $string, $float): void
{
	assertType('int', identity($int));
	assertType('string', identity($string));
	assertType('DateTime', identity(new \DateTime()));
	assertType('float|int', pick($int, $float));
	assertType('int', pick($int, $int));
}

// ============================================================
// 2. Function-level @template T of Bound
// ============================================================

/**
 * @template T of \DateTimeInterface
 * @param T $a
 * @return T
 */
function bounded($a)
{
	return $a;
}

function testBounded(): void
{
	assertType('DateTime', bounded(new \DateTime()));
	assertType('DateTimeImmutable', bounded(new \DateTimeImmutable()));
}

// ============================================================
// 3. Class-level @template T with @extends Base<Concrete>
// ============================================================

function testClassLevelExtends(): void
{
	$dtBox = new DateTimeBox();
	assertType('DateTime', $dtBox->getValue());

	$intBox = new IntBox();
	assertType('int', $intBox->getValue());
}

// ============================================================
// 4. Class-level @template with @implements Interface<Concrete>
// ============================================================

function testImplementsSubstitution(): void
{
	$sc = new StringContainer();
	assertType('string', $sc->get());

	$ic = new IntContainer();
	assertType('int', $ic->get());
}

// ============================================================
// 5. Generic collections — @extends Collection<User>
// ============================================================

function testCollectionInheritance(): void
{
	$users = new UserCollection();
	assertType('User|null', $users->first());
	assertType('array<int, User>', $users->all());

	$admins = new AdminCollection();
	assertType('Admin|null', $admins->first());
	assertType('array<int, Admin>', $admins->all());
}

// ============================================================
// 6. Multi-template class — @extends Pair<string, User>
// ============================================================

function testMultiTemplateExtends(): void
{
	$named = new NamedUser('alice', new User());
	assertType('string', $named->getKey());
	assertType('User', $named->getValue());
}

// ============================================================
// 7. Bounded class template — @extends DateCache<DateTime>
// ============================================================

function testBoundedClassExtends(): void
{
	$mutable = new MutableDateCache();
	assertType('DateTime', $mutable->getDate());

	$immutable = new ImmutableDateCache();
	assertType('DateTimeImmutable', $immutable->getDate());
}

// ============================================================
// 8. Method-level @template T
// ============================================================

function testMethodLevelTemplate(): void
{
	$w = new Wrapper();
	assertType('42', $w->identity(42));
	assertType('\'hello\'', $w->identity('hello'));
	assertType('DateTime', $w->identity(new \DateTime()));
	assertType('User', $w->identity(new User()));
}

// ============================================================
// 9. Constructor template inference — new Box(expr)
// ============================================================

function testConstructorInference(): void
{
	// PHPantom may or may not track constructor-inferred templates
	// through subsequent method calls. These are speculative.

	$box = new Box(42);
	assertType('int', $box->getValue());

	$box2 = new Box(new User());
	assertType('User', $box2->getValue());

	$box3 = new Box('hello');
	assertType('string', $box3->getValue());
}

// ============================================================
// 10. Chained generic resolution
// ============================================================

/**
 * @template T
 * @param Box<T> $box
 * @return T
 */
function unbox($box)
{
	return $box->getValue();
}

function testGenericParamResolution(): void
{
	$dtBox = new DateTimeBox();
	assertType('DateTime', unbox($dtBox));

	$intBox = new IntBox();
	assertType('int', unbox($intBox));
}

// ============================================================
// 12. Literal types (kept through a function template)
// ============================================================

function testLiterals(): void
{
	assertType('1', identity(1));
	assertType('\'foo\'', identity('foo'));
	assertType('true', identity(true));
}

// ============================================================
// 13. Mixed / unknown propagation
// ============================================================

/**
 * @param mixed $mixed
 */
function testMixedPropagation($mixed): void
{
	assertType('mixed', identity($mixed));
}

// ============================================================
// 14. Two-template function
// ============================================================

/**
 * @template K
 * @template V
 * @param K $key
 * @param V $value
 * @return array<K, V>
 */
function makePair($key, $value)
{
	return [$key => $value];
}

function testTwoTemplateFn(): void
{
	assertType('array<\'a\', 1>', makePair('a', 1));
	assertType('array<0, User>', makePair(0, new User()));
}

// ============================================================
// 15. Deep inheritance chain
// ============================================================

/**
 * @template T
 */
class Base
{
	/**
	 * @return T
	 */
	public function getItem()
	{
		throw new \RuntimeException();
	}
}

/**
 * @template T
 * @extends Base<T>
 */
class Middle extends Base
{
}

/**
 * @extends Middle<string>
 */
class Leaf extends Middle
{
}

function testDeepInheritance(): void
{
	$leaf = new Leaf();
	assertType('string', $leaf->getItem());
}