<?php // lint >= 8.0

namespace TemplateDefault;

use function PHPStan\Testing\assertType;

/**
 * @template T1 = true
 * @template T2 = true
 */
class Test
{
}

/**
 * @param Test<false> $one
 * @param Test<false, false> $two
 * @param Test<false, false, false> $three
 */
function foo(Test $one, Test $two, Test $three)
{
	assertType('TemplateDefault\\Test<false, true>', $one);
	assertType('TemplateDefault\\Test<false, false>', $two);
	assertType('TemplateDefault\\Test<false, false, false>', $three);
}


/**
 * @template S = false
 * @template T = false
 */
class Builder
{
    /**
     * @phpstan-self-out self<true, T>
     */
    public function one(): void
    {
    }

    /**
     * @phpstan-self-out self<S, true>
     */
    public function two(): void
    {
    }

    /**
     * @return ($this is self<true, true> ? void : never)
     */
    public function execute(): void
    {
    }
}

class FormData {}
class Form
{
	/**
	 * @template Data of object = \stdClass
	 * @param Data|null $values
	 * @return Data
	 */
	public function mapValues(object|null $values = null): object
	{
		$values ??= new \stdClass;
		// ... map into $values ...
		return $values;
	}
}

function () {
	$qb = new Builder();
	assertType('TemplateDefault\\Builder<false, false>', $qb);
	$qb->one();
	assertType('TemplateDefault\\Builder<true, false>', $qb);
	$qb->two();
	assertType('TemplateDefault\\Builder<true, true>', $qb);
	assertType('null', $qb->execute());
};

function () {
	$qb = new Builder();
	assertType('TemplateDefault\\Builder<false, false>', $qb);
	$qb->two();
	assertType('TemplateDefault\\Builder<false, true>', $qb);
	$qb->one();
	assertType('TemplateDefault\\Builder<true, true>', $qb);
	assertType('null', $qb->execute());
};

function () {
	$qb = new Builder();
	assertType('TemplateDefault\\Builder<false, false>', $qb);
	$qb->one();
	assertType('TemplateDefault\\Builder<true, false>', $qb);
	assertType('never', $qb->execute());
};

function () {
	$form = new Form();

	assertType('TemplateDefault\\FormData', $form->mapValues(new FormData));
	assertType('stdClass', $form->mapValues());
};

/**
 * @template T
 * @template U = string
 */
interface Foo
{
	/**
	 * @return U
	 */
	public function get(): mixed;
}

/**
 * @extends Foo<int>
 */
interface Bar extends Foo
{
}

/**
 * @extends Foo<int, bool>
 */
interface Baz extends Foo
{
}

function (Bar $bar, Baz $baz) {
	assertType('string', $bar->get());
	assertType('bool', $baz->get());
};
