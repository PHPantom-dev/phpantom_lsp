<?php

namespace QueryResult\CreateQuery;

use Doctrine\ORM\EntityManager;
use QueryResult\Entities\Bug245Episode;
use QueryResult\Entities\Bug245Segment;
use function PHPStan\Testing\assertType;

class Foo
{

	/** @var EntityManager */
	private $em;

	public function doFoo(): void
	{
		$result = $this->em->createQueryBuilder()
			->select('episode.id')
			->from(Bug245Episode::class, 'episode', 'episode.id')
			->where('episode.segmentsComplete = false')
			->andWhere('0 != SIZE(episode.segments)')
			->andWhere(
				'0 < (SELECT COUNT(last_segment.id) FROM ' . Bug245Segment::class . ' as last_segment
              WHERE last_segment.episode = episode.id AND last_segment.isLastSegment = true)'
			)
			->andWhere(
				"0 = (SELECT COUNT(segment.id) FROM " . Bug245Segment::class . " as segment
              WHERE segment.episode = episode.id AND segment.state != 'distributed')"
			)->getQuery()->getResult();
		// Upstream expects `mixed`: its conditions name fields the entities do
		// not map, which fails the whole query. PHPantom types the rows by what
		// the query selects, and reports the unmapped fields where they are
		// written.
		assertType('array<string, array{id: string}>', $result);
	}

}
