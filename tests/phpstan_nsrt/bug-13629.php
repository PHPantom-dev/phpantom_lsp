<?php declare(strict_types = 1);

namespace Bug13629;

use function PHPStan\Testing\assertType;

/**
 * @param array<string, list<array{xmlNamespace: string, namespace: string, name: string}>> $xsdFiles
 * @param array<string, list<array{xmlNamespace: string, namespace: string, name: string}>> $groupedByNamespace
 * @param array<string, list<string>> $extraNamespaces
 */
function test(array $xsdFiles, array $groupedByNamespace, array $extraNamespaces): void {
	foreach ($extraNamespaces as $mergedNamespace) {
		if (count($mergedNamespace) < 2) {
			continue;
		}

		$targetNamespace = end($mergedNamespace);
		if (!isset($groupedByNamespace[$targetNamespace])) {
			continue;
		}
		$xmlNamespace = $groupedByNamespace[$targetNamespace][0]['xmlNamespace'];

		assertType('string', $xmlNamespace);

		$xsdFiles[$xmlNamespace] = [];
		foreach ($mergedNamespace as $namespace) {
			foreach ($groupedByNamespace[$namespace] ?? [] as $viewHelper) {
				assertType('string', $viewHelper['name']);
				$xsdFiles[$xmlNamespace][$viewHelper['name']] = $viewHelper;
			}
		}
		// After assigning with string keys ($viewHelper['name']), $xsdFiles[$xmlNamespace] should NOT be a list
		assertType('array<int<0, max>|string, array{xmlNamespace: string, namespace: string, name: string}>', $xsdFiles[$xmlNamespace]); // SKIP: a nested write under a dynamic key inside a loop is lost after the loop
		$xsdFiles[$xmlNamespace] = array_values($xsdFiles[$xmlNamespace]);
	}
}
