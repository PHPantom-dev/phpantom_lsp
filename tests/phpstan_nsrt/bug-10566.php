<?php

namespace Bug10566;

use function PHPStan\Testing\assertType;

final class StreamSelectLoop
{
	/** @var bool */
	private $running;
	public function __construct()
	{
	}

	public function run(): void
	{
		$this->running = true;

		while ($this->running) {
			assertType('true', $this->running);
			call_user_func(function () {
				$this->stop();
			});
			assertType('bool', $this->running);

			if (!$this->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function stop(): void
	{
		$this->running = false;
	}

	public function run2(): void
	{
		$s = new self();

		while ($s->running) {
			assertType('true', $s->running);
			call_user_func(function () use ($s) {
				$s->stop();
			});
			assertType('bool', $s->running);

			if (!$s->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function run3(): void
	{
		$s = new self();

		while ($s->running) {
			assertType('true', $s->running);
			call_user_func(function () {
				$s = new self();
				$s->stop();
			});
			assertType('true', $s->running);

			if (!$s->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function run4(): void
	{
		$s = new self();

		while ($s->running) {
			assertType('true', $s->running);
			$cb = function () use ($s) {
				$s = new self();
				$s->stop();
			};
			assertType('true', $s->running);
			call_user_func($cb);
			assertType('bool', $s->running);

			if (!$s->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function run5(): void
	{
		$this->running = true;

		while ($this->running) {
			assertType('true', $this->running);
			$cb = function () {
				$this->stop();
			};
			assertType('true', $this->running);
			call_user_func($cb);
			assertType('bool', $this->running);

			if (!$this->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function run6(): void
	{
		$s = new self();

		while ($s->running) {
			assertType('true', $s->running);
			(function () use ($s) {
				$s = new self();
				$s->stop();
			})();
			assertType('bool', $s->running);

			if (!$s->running) {
				$timeout = 0;
				break;
			}
		}
	}

	public function run7(): void
	{
		$this->running = true;

		while ($this->running) {
			assertType('true', $this->running);
			(function () {
				$this->stop();
			})();
			assertType('bool', $this->running);

			if (!$this->running) {
				$timeout = 0;
				break;
			}
		}
	}

	function run8(): void
	{
		$s = new self();

		while ($s->running) {
			assertType('true', $s->running);
			call_user_func(static function () use ($s) {
				$s->stop();
			});
			assertType('bool', $s->running);

			if (!$s->running) {
				$timeout = 0;
				break;
			}
		}
	}

}
