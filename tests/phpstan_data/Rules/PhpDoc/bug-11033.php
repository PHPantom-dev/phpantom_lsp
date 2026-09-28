<?php // lint >= 8.1

namespace Bug11033;

use function PHPStan\Testing\assertType;

/**
 * @phpstan-template T of GotoTarget
 *
 * @phpstan-type CandidateNotesData array{candidate: string, note: string}
 * @phpstan-type PersonalmanagerData array{token: string}
 *
 * @phpstan-type GotoRouteDataTypes array{
 *     'personalmanager': PersonalmanagerData,
 *     'my-pending-reviews': null,
 *     'candidate-notes': CandidateNotesData,
 *     'dashboard': null,
 * }
 */
final class GotoRoute
{
	/**
	 * @phpstan-var T
	 */
	public GotoTarget $target;

	/**
	 * @phpstan-var GotoRouteDataTypes[value-of<T>]
	 */
	public ?array $data = null;
}

enum GotoTarget: string
{
	case PERSONALMANAGER = 'personalmanager';
	case MY_PENDING_REVIEWS = 'my-pending-reviews';
	case CANDIDATE_NOTES = 'candidate-notes';
	case DASHBOARD = 'dashboard';
}

function (): void {
	/** @var GotoRoute<GotoTarget::PERSONALMANAGER> $goto */
	$goto = new GotoRoute();
	assertType('array{token: string}', $goto->data);
};

function (): void {
	/** @var GotoRoute<GotoTarget::DASHBOARD> $goto */
	$goto = new GotoRoute();
	assertType('null', $goto->data);
};
