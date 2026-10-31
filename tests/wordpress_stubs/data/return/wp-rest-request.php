<?php

declare(strict_types=1);

namespace PhpStubs\WordPress\Core\Tests;

use function PHPStan\Testing\assertType;

/**
 * @var \WP_REST_Request<array> $request
 */
$request = Faker::wpRestRequest();

assertType('mixed', $request->get_param('maybeParam')); // SKIP: `T[K]` on a class template is not resolved
assertType('mixed', $request['maybeParam']); // SKIP: `T[K]` on a class template is not resolved
assertType('array', $request->get_params());
assertType('bool', $request->has_param('maybeParam'));

/**
 * @var \WP_REST_Request<array<string, string>> $request
 */
$request = Faker::wpRestRequest();

assertType('string|null', $request->get_param('maybeParam')); // SKIP: the same call as an argument earlier in the file keeps the type of `$request`'s first `@var`
assertType('string|null', $request['maybeParam']); // SKIP: `T[K]` on a class template is not resolved
assertType('array<string, string>', $request->get_params());
assertType('bool', $request->has_param('maybeParam'));

/**
 * @var \WP_REST_Request<array{
 *      stringParam: string,
 *      intParam: int,
 *      boolParam: bool
 * }> $request
 */
$request = Faker::wpRestRequest();

assertType('string|null', $request->get_param('stringParam'));
assertType('int|null', $request->get_param('intParam'));
assertType('bool|null', $request->get_param('boolParam'));
assertType('bool|int|string|null', $request->get_param('nonExistentParam')); // SKIP: `T[K]` on a class template is not resolved

assertType('string|null', $request['stringParam']); // SKIP: `T[K]` on a class template is not resolved
assertType('int|null', $request['intParam']); // SKIP: `T[K]` on a class template is not resolved
assertType('bool|null', $request['boolParam']); // SKIP: `T[K]` on a class template is not resolved
assertType('bool|int|string|null', $request['nonExistentParam']); // SKIP: `T[K]` on a class template is not resolved

assertType('array{stringParam: string, intParam: int, boolParam: bool}', $request->get_params());

assertType('bool', $request->has_param('stringParam'));
assertType('bool', $request->has_param('intParam'));
assertType('bool', $request->has_param('boolParam'));
assertType('bool', $request->has_param('nonExistentParam'));
