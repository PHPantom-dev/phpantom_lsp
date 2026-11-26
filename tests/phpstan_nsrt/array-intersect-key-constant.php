<?php

namespace ArrayIntersectKeyConstant;

use function array_intersect_key;
use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array{name: string|null, description: string|null, author: string|null, type: string|null, homepage: string|null, require: array<int, string>, require-dev: array<int, string>, stability: string|null, license: string|null, repository: array<int, string>, autoload: string|null, help: bool, quiet: bool, verbose: bool, version: bool, ansi: bool, no-interaction: bool, profile: bool, no-plugins: bool, no-scripts: bool, working-dir: string|null, no-cache: bool} $options
	 * @return void
	 */
	public function doFoo(array $options): void
	{
		assertType('array{name: string|null, description: string|null, author: string|null, type: string|null, homepage: string|null, require: array<int, string>, require-dev: array<int, string>, stability: string|null, license: string|null, repository: array<int, string>, autoload: string|null, help: bool, quiet: bool, verbose: bool, version: bool, ansi: bool, no-interaction: bool, profile: bool, no-plugins: bool, no-scripts: bool, working-dir: string|null, no-cache: bool}', $options);

		$allowlist = ['name', 'description', 'author', 'type', 'homepage', 'require', 'require-dev', 'stability', 'license', 'autoload'];
		$options = array_filter(array_intersect_key($options, array_flip($allowlist)));
		assertType('array{name?: non-falsy-string, description?: non-falsy-string, author?: non-falsy-string, type?: non-falsy-string, homepage?: non-falsy-string, require?: non-empty-array<int, string>, require-dev?: non-empty-array<int, string>, stability?: non-falsy-string, license?: non-falsy-string, autoload?: non-falsy-string}', $options);
	}

	public function doBar(): void
	{
		assertType('array{a: 1}', array_intersect_key(['a' => 1]));
		assertType('array{}', array_intersect_key(['a' => 1], []));

		$a = ['a' => 1];
		if (rand(0, 1)) {
			$a['b'] = 2;
		}

		assertType('array{a: 1, b?: 2}', array_intersect_key(['a' => 1, 'b' => 2], $a));
	}

	public function doBaz(): void
	{
		$a = [];
		if (rand(0, 1)) {
			$a['a'] = 1;
		}
		$a['b'] = 2;

		assertType('array{a?: 1, b: 2}', array_intersect_key($a, ['a' => 1, 'b' => 2]));
	}

}
