<?php

namespace Bug7224;


use function PHPStan\Testing\assertType;

/**
 * @param array{id: string, name?: string, team_name?: string|null} $roleUpdates
 */
function processUpdates(array $roleUpdates): void {
	assertType('array{id: string, name?: string, team_name?: string|null}', $roleUpdates);
	if (!isset($roleUpdates['team_name']) && !isset($roleUpdates['name'])) {
		return;
	}

	assertType('array{id: string, name?: string, team_name?: string|null}', $roleUpdates);

	$fieldUpdates = ['updated_at' => new \DateTime()];

	if (\array_key_exists('team_name', $roleUpdates)) {
		$fieldUpdates['team_name'] = $roleUpdates['team_name'];
	}

	if (isset($roleUpdates['name'])) {
		$fieldUpdates['name'] = $roleUpdates['name'];
	}

	saveUpdates($roleUpdates['id'], $fieldUpdates);
}

/**
 * @param array{id: string, name?: string, team_name?: string|null} $roleUpdates
 */
function processUpdates2(array $roleUpdates): void {
	assertType('array{id: string, name?: string, team_name?: string|null}', $roleUpdates);
	if (!isset($roleUpdates['team_name'])) {

	}

	assertType('array{id: string, name?: string, team_name?: string|null}', $roleUpdates);

	$fieldUpdates = ['updated_at' => new \DateTime()];

	if (\array_key_exists('team_name', $roleUpdates)) {
		$fieldUpdates['team_name'] = $roleUpdates['team_name'];
	}

	if (isset($roleUpdates['name'])) {
		$fieldUpdates['name'] = $roleUpdates['name'];
	}

	saveUpdates($roleUpdates['id'], $fieldUpdates);
}

/**
 * @param array<string, mixed> $updatedFields
 */
function saveUpdates(string $id, array $updatedFields): void {
	// ...
}
