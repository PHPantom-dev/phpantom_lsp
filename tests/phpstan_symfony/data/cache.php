<?php declare(strict_types = 1);

namespace PHPStan\Type\Symfony\cache;

use function PHPStan\Testing\assertType;

function testCacheCallable(\Symfony\Contracts\Cache\CacheInterface  $cache): void {
	$result = $cache->get('foo', function (): string {
		return '';
	});

	assertType('string', $result);
};

/**
 * @param callable():string $fn
 */
function testNonScalarCacheCallable(\Symfony\Contracts\Cache\CacheInterface $cache, callable $fn): void {
	$result = $cache->get('foo', $fn);

	assertType('string', $result);
};


/**
 * @param callable():non-empty-string $fn
 */
function testCacheCallableReturnTypeGeneralization(\Symfony\Contracts\Cache\CacheInterface $cache, callable $fn): void {
	$result = $cache->get('foo', $fn);

	// The callback returns a non-empty-string, and so does get(); upstream
	// generalizes it to string.
	assertType('non-empty-string', $result);
};


/**
 * @param \Symfony\Contracts\Cache\CallbackInterface<\stdClass> $cb
 */
 function testCacheCallbackInterface(\Symfony\Contracts\Cache\CacheInterface  $cache, \Symfony\Contracts\Cache\CallbackInterface $cb): void {
	$result = $cache->get('foo',$cb);

	assertType('stdClass', $result);
};
