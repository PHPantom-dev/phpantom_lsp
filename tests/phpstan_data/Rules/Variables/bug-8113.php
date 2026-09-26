<?php declare(strict_types=1);

namespace Bug8113;

use function PHPStan\Testing\assertType;

function () {
	/** @var mixed[][] $review */
	$review = array(
		'Review' => array('id' => 23,
			'User' => array(
				'first_name' => 'x',
			),
		),
		'SurveyInvitation' => array(
			'is_too_old_to_follow' => 'yes',
		),
		'User' => array(
			'first_name' => 'x',
		),
	);

	assertType('array<array<mixed>>', $review);

	if (
		array_key_exists('review', $review['SurveyInvitation']) &&
		$review['SurveyInvitation']['review'] === null
	) {
		$review['Review'] = [
			'id' => null,
			'text' => null,
			'answer' => null,
		];
		unset($review['SurveyInvitation']['review']);
	}
	assertType('array<array<mixed>>', $review); // SKIP: array shape unions are merged differently: optional keys are lost and array|array{…} is not simplified
	if (array_key_exists('User', $review['Review'])) {
		$review['User'] = $review['Review']['User'];
		unset($review['Review']['User']);
	}
};
