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
 * Represents a cookie.
 *
 * @author Johannes M. Schmitt <schmittjoh@gmail.com>
 */
class Cookie
{
    public const SAMESITE_NONE = 'none';
    public const SAMESITE_LAX = 'lax';
    public const SAMESITE_STRICT = 'strict';

    protected int $expire;
    protected string $path;

    private ?string $sameSite = null;
    private bool $secureDefault = false;

    private const RESERVED_CHARS_LIST = "=,; \t\r\n\v\f";
    private const RESERVED_CHARS_FROM = ['=', ',', ';', ' ', "\t", "\r", "\n", "\v", "\f"];
    private const RESERVED_CHARS_TO = ['%3D', '%2C', '%3B', '%20', '%09', '%0D', '%0A', '%0B', '%0C'];

    // same list as above minus "=", which PHP allows in the path and domain attributes
    private const RESERVED_ATTR_CHARS_LIST = ",; \t\r\n\v\f";

    /**
     * Creates cookie from raw header string.
     */
    public static function fromString(string $cookie, bool $decode = false): static
    {}

    /**
     * @see self::__construct
     *
     * @param self::SAMESITE_*|''|null $sameSite
     */
    public static function create(string $name, ?string $value = null, int|string|\DateTimeInterface $expire = 0, ?string $path = '/', ?string $domain = null, ?bool $secure = null, bool $httpOnly = true, bool $raw = false, ?string $sameSite = self::SAMESITE_LAX, bool $partitioned = false): self
    {}

    /**
     * @param string                        $name     The name of the cookie
     * @param string|null                   $value    The value of the cookie
     * @param int|string|\DateTimeInterface $expire   The time the cookie expires
     * @param string|null                   $path     The path on the server in which the cookie will be available on
     * @param string|null                   $domain   The domain that the cookie is available to
     * @param bool|null                     $secure   Whether the client should send back the cookie only over HTTPS or null to auto-enable this when the request is already using HTTPS
     * @param bool                          $httpOnly Whether the cookie will be made accessible only through the HTTP protocol
     * @param bool                          $raw      Whether the cookie value should be sent with no url encoding
     * @param self::SAMESITE_*|''|null      $sameSite Whether the cookie will be available for cross-site requests
     *
     * @throws \InvalidArgumentException
     */
    public function __construct(
        protected string $name,
        protected ?string $value = null,
        int|string|\DateTimeInterface $expire = 0,
        ?string $path = '/',
        protected ?string $domain = null,
        protected ?bool $secure = null,
        protected bool $httpOnly = true,
        private bool $raw = false,
        ?string $sameSite = self::SAMESITE_LAX,
        private bool $partitioned = false,
    ) {}

    /**
     * Creates a cookie copy with a new value.
     */
    public function withValue(?string $value): static
    {}

    /**
     * Creates a cookie copy with a new domain that the cookie is available to.
     */
    public function withDomain(?string $domain): static
    {}

    /**
     * Creates a cookie copy with a new time the cookie expires.
     */
    public function withExpires(int|string|\DateTimeInterface $expire = 0): static
    {}

    /**
     * Converts expires formats to a unix timestamp.
     */
    private static function expiresTimestamp(int|string|\DateTimeInterface $expire = 0): int
    {}

    /**
     * Rejects the characters that PHP's setcookie() also rejects in the path and domain attributes.
     */
    private static function validateAttribute(string $attribute, ?string $value): void
    {}

    /**
     * Creates a cookie copy with a new path on the server in which the cookie will be available on.
     */
    public function withPath(string $path): static
    {}

    /**
     * Creates a cookie copy that only be transmitted over a secure HTTPS connection from the client.
     */
    public function withSecure(bool $secure = true): static
    {}

    /**
     * Creates a cookie copy that be accessible only through the HTTP protocol.
     */
    public function withHttpOnly(bool $httpOnly = true): static
    {}

    /**
     * Creates a cookie copy that uses no url encoding.
     */
    public function withRaw(bool $raw = true): static
    {}

    /**
     * Creates a cookie copy with SameSite attribute.
     *
     * @param self::SAMESITE_*|''|null $sameSite
     */
    public function withSameSite(?string $sameSite): static
    {}

    /**
     * Creates a cookie copy that is tied to the top-level site in cross-site context.
     */
    public function withPartitioned(bool $partitioned = true): static
    {}

    /**
     * Returns the cookie as a string.
     */
    public function __toString(): string
    {}

    /**
     * Gets the name of the cookie.
     */
    public function getName(): string
    {}

    /**
     * Gets the value of the cookie.
     */
    public function getValue(): ?string
    {}

    /**
     * Gets the domain that the cookie is available to.
     */
    public function getDomain(): ?string
    {}

    /**
     * Gets the time the cookie expires.
     */
    public function getExpiresTime(): int
    {}

    /**
     * Gets the max-age attribute.
     */
    public function getMaxAge(): int
    {}

    /**
     * Gets the path on the server in which the cookie will be available on.
     */
    public function getPath(): string
    {}

    /**
     * Checks whether the cookie should only be transmitted over a secure HTTPS connection from the client.
     */
    public function isSecure(): bool
    {}

    /**
     * Checks whether the cookie will be made accessible only through the HTTP protocol.
     */
    public function isHttpOnly(): bool
    {}

    /**
     * Whether this cookie is about to be cleared.
     */
    public function isCleared(): bool
    {}

    /**
     * Checks if the cookie value should be sent with no url encoding.
     */
    public function isRaw(): bool
    {}

    /**
     * Checks whether the cookie should be tied to the top-level site in cross-site context.
     */
    public function isPartitioned(): bool
    {}

    /**
     * @return self::SAMESITE_*|null
     */
    public function getSameSite(): ?string
    {}

    /**
     * @param bool $default The default value of the "secure" flag when it is set to null
     */
    public function setSecureDefault(bool $default): void
    {}
}
