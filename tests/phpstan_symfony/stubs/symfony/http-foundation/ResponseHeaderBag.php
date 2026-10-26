<?php

/*
 * This file is part of the Symfony package.
 *
 * (c) Fabien Potencier <fabien@symfony.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 */

namespace Symfony\Component\HttpFoundation;

/**
 * ResponseHeaderBag is a container for Response HTTP headers.
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class ResponseHeaderBag extends HeaderBag
{
    public const COOKIES_FLAT = 'flat';
    public const COOKIES_ARRAY = 'array';

    public const DISPOSITION_ATTACHMENT = 'attachment';
    public const DISPOSITION_INLINE = 'inline';

    protected array $computedCacheControl = [];
    protected array $cookies = [];
    protected array $headerNames = [];

    public function __construct(array $headers = [])
    {}

    /**
     * Returns the headers, with original capitalizations.
     */
    public function allPreserveCase(): array
    {}

    public function allPreserveCaseWithoutCookies(): array
    {}

    public function replace(array $headers = []): void
    {}

    public function all(?string $key = null): array
    {}

    public function set(string $key, string|array|null $values, bool $replace = true): void
    {}

    public function remove(string $key): void
    {}

    public function hasCacheControlDirective(string $key): bool
    {}

    public function getCacheControlDirective(string $key): bool|string|null
    {}

    public function setCookie(Cookie $cookie): void
    {}

    /**
     * Removes a cookie from the array, but does not unset it in the browser.
     */
    public function removeCookie(string $name, ?string $path = '/', ?string $domain = null): void
    {}

    /**
     * Returns an array with all cookies.
     *
     * @return Cookie[]
     *
     * @throws \InvalidArgumentException When the $format is invalid
     */
    public function getCookies(string $format = self::COOKIES_FLAT): array
    {}

    /**
     * Clears a cookie in the browser.
     *
     * @param bool $partitioned
     */
    public function clearCookie(string $name, ?string $path = '/', ?string $domain = null, bool $secure = false, bool $httpOnly = true, ?string $sameSite = null /* , bool $partitioned = false */): void
    {}

    /**
     * @see HeaderUtils::makeDisposition()
     */
    public function makeDisposition(string $disposition, string $filename, string $filenameFallback = ''): string
    {}

    /**
     * Returns the calculated value of the cache-control header.
     *
     * This considers several other headers and calculates or modifies the
     * cache-control header to a sensible, conservative value.
     */
    protected function computeCacheControlValue(): string
    {}

    private function initDate(): void
    {}
}
