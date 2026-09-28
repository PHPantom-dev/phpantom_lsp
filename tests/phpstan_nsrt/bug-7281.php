<?php

namespace Bug7281;

use function PHPStan\Testing\assertType;

class Percentage {}

/**
 * @template T
 */
final class Timeline {}

/**
 * @template K of array-key
 * @template T
 * @template U
 *
 * @param array<K, T> $array
 * @param (callable(T, K): U) $fn
 *
 * @return array<K, U>
 */
function map(array $array, callable $fn): array
{
	/** @phpstan-ignore-next-line */
	return array_map($fn, $array);
}

function (): void {
	/**
	 * @var array<int, Timeline<Percentage>> $timelines
	 */
	$timelines = [];

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', map(
		$timelines,
		static function (Timeline $timeline): Timeline {
			return $timeline;
		},
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', map(
		$timelines,
		static function ($timeline) {
			return $timeline;
		},
	));

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', map(
		$timelines,
		static fn (Timeline $timeline): Timeline => $timeline,
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', map(
		$timelines,
		static fn ($timeline) => $timeline,
	));

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static function (Timeline $timeline): Timeline {
			return $timeline;
		},
		$timelines,
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static function ($timeline) {
			return $timeline;
		},
		$timelines,
	));

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static fn (Timeline $timeline): Timeline => $timeline,
		$timelines,
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static fn ($timeline) => $timeline,
		$timelines,
	));

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static function (Timeline $timeline) {
			return $timeline;
		},
		$timelines,
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static function ($timeline): Timeline {
			return $timeline;
		},
		$timelines,
	));

	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static fn (Timeline $timeline) => $timeline,
		$timelines,
	));
	assertType('array<int, Bug7281\\Timeline<Bug7281\\Percentage>>', array_map(
		static fn ($timeline): Timeline => $timeline,
		$timelines,
	));
};
