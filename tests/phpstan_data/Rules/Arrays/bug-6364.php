<?php

namespace Bug6364;

use function PHPStan\Testing\assertType;

class Foo
{

	/**
	 * @param array<string,
	 *     array{
	 *          type: 'Type1',
	 *          id: string,
	 *          jobs: array<string, string>
	 *     } | array{
	 *          type: 'Type2',
	 *          id: string,
	 *          job?: string,
	 *          extractor?: int
	 *     } | array{
	 *          type: 'Type3',
	 *          id: string,
	 *          jobs: array<string, int>
	 *     } | array{
	 *          type: 'Type4',
	 *          id: string,
	 *          job?: string
	 *     }> $array
	 */
	public function doFoo(array $array)
	{
		foreach ($array as $key => $data) {
			switch ($data['type']) {
				case 'Type1':
					assertType("array{type: 'Type1', id: string, jobs: array<string, string>}", $data);
					echo $data['id'];
					print_r($data['jobs']);
					break;
				case 'Type3':
					assertType("array{type: 'Type3', id: string, jobs: array<string, int>}", $data);
					$jobs = [];
					foreach ($data['jobs'] as $job => $extractor) {
						echo $job;
						echo $extractor;
					}
					break;
				case 'Type2':
					assertType("array{type: 'Type2', id: string, job?: string, extractor?: int}", $data);
					echo $data['id'];
					echo $data['job'] ?? 'default';
					echo $data['extractor'] ?? 0;
					break;
				case 'Type4':
					assertType("array{type: 'Type4', id: string, job?: string}", $data);
					echo $data['id'];
					echo $data['job'] ?? 'default';
					break;
				default:
					throw new \RuntimeException('unknown type: ' . $data['type']);
			}
		}
	}

}
