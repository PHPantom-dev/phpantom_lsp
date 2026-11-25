<?php

namespace Bug2977;

use function PHPStan\Testing\assertType;

interface MyInterface
{
	/**
	 * @return self|null The parent or null if there is none
	 */
	public function getParent();

	/**
	 * @return mixed data
	 *
	 * @throws \Exception If the form inherits data but has no parent
	 */
	public function getData();
}

class My implements MyInterface
{
	/**
	 * @var MyInterface|null
	 */
	private $parent;

	/**
	 * @var mixed
	 */
	private $data;

	/**
	 * {@inheritdoc}
	 */
	public function getParent()
	{
		return $this->parent;
	}

	/**
	 * {@inheritdoc}
	 */
	public function getData()
	{
		return $this->data;
	}
}

class Bar
{
	public function baz(MyInterface $my): string
	{
		$parent = $my->getParent();
		if (!$parent) {
			assertType('null', $parent);
			return 'ok';
		}

		assertType(MyInterface::class, $parent);

		return $parent->getData();
	}

	public function baz2(MyInterface $my): string
	{
		$parent = $my->getParent();

		$case = $parent;
		if (!$case) {
			assertType('null', $parent); // SKIP: a check on a copy of a variable does not narrow the original
			return 'ok';
		}

		assertType(MyInterface::class, $parent); // SKIP: a check on a copy of a variable does not narrow the original

		return $parent->getData();
	}
}
