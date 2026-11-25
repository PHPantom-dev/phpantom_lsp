<?php

namespace Bug9784;

use Iterator;
use stdClass;
use function PHPStan\Testing\assertType;

final class DemoFile
{
	private function complementArticleData(Iterator $articles): void
	{
		$result = [
			'artikel' => $articles,
			'farben' => null,
			'artikel_ids' => [],
		];

		// collect article ids
		foreach ($result['artikel'] as $article) {
			$result['artikel_ids'][] = 1;
		}

		assertType('array{artikel: Iterator, farben: null, artikel_ids: list<1>}', $result);
		assertType('list<1>', $result['artikel_ids']);

		if ($result['artikel_ids'] !== []) {
			$result['farben'] = new stdClass();
		}

		// $result['farben'] might be also null
		assertType('stdClass|null', $result['farben']);
		if ($result['farben'] instanceof stdClass) {
			echo '123';
		}
	}
}
