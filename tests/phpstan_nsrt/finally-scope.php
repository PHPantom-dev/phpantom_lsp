<?php

namespace FinallyBugs;

use function PHPStan\Testing\assertType;

class Foo
{

	public function mightThrowException(): void
	{

	}

	/** @throws \DomainException */
	public function throwsDomainException(): void
	{

	}

	/** @throws \LogicException */
	public function throwsLogicException(): void
	{

	}

	public function doFoo()
	{
		$s = 1;
		try {
			$this->mightThrowException();
			$s = 2;
		} catch (\Throwable $e) { // always catches
			assertType('1', $s);
			$s = 'str';
		} finally {
			assertType("2|'str'", $s);
		}
	}

	public function doBar()
	{
		try {
			$s = 1;
			$this->mightThrowException();
		} catch (\InvalidArgumentException $e) { // might catch
			assertType('1', $s);
			$s = "bar";
		} catch (\Throwable $e) { // always catches what isn't InvalidArgumentException
			assertType('1', $s);
			$s = 'str';
		} finally {
			assertType("1|'bar'|'str'", $s);
		}
	}

	public function doBar2()
	{
		try {
			$s = 1;
			$this->throwsDomainException();
		} catch (\DomainException $e) { // always catches
			assertType('1', $s);
			$s = "bar";
		} catch (\Throwable $e) { // dead catch
			assertType('1', $s);
			$s = 'str';
		} finally {
			assertType("1|'bar'|'str'", $s); // could be 1|'bar'
		}
	}

	public function doBar3()
	{
		try {
			$s = 1;
			$this->throwsDomainException();
		} catch (\LogicException $e) { // always catches
			assertType('1', $s);
			$s = "bar";
		} catch (\Throwable $e) { // dead catch
			assertType('1', $s);
			$s = 'str';
		} finally {
			assertType("1|'bar'|'str'", $s); // could be 1|'bar'
		}
	}

	public function doBar4()
	{
		try {
			$s = 1;
			$this->throwsLogicException();
		} catch (\DomainException $e) { // might catch
			assertType('1', $s);
			$s = "bar";
		} catch (\Throwable $e) { // always catches what isn't DomainException
			assertType('1', $s);
			$s = 'str';
		} finally {
			assertType("1|'bar'|'str'", $s);
		}
	}

	public function doBar5()
	{
		try {
			$s = 1;
			$this->throwsLogicException();
		} catch (\DomainException $e) { // might catch
			assertType('1', $s);
			$s = "bar";
		} catch (\LogicException $e) { // always catches what isn't DomainException
			assertType('1', $s);
			$s = "str";
		} catch (\Throwable $e) { // dead catch
			assertType('1', $s);
			$s = 'foo';
		} finally {
			assertType("1|'bar'|'foo'|'str'", $s); // could be 1|'bar'|'str'
		}
	}

	public function doBar6()
	{
		try {
			$s = 1;
			$this->throwsLogicException();
		} catch (\RuntimeException $e) { // dead catch
			assertType('1', $s);
			$s = "bar";
		} finally {
			assertType('1', $s);
		}
	}

}
