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

use Symfony\Component\HttpFoundation\Exception\BadRequestException;
use Symfony\Component\HttpFoundation\Exception\ConflictingHeadersException;
use Symfony\Component\HttpFoundation\Exception\JsonException;
use Symfony\Component\HttpFoundation\Exception\SessionNotFoundException;
use Symfony\Component\HttpFoundation\Exception\SuspiciousOperationException;
use Symfony\Component\HttpFoundation\Session\SessionInterface;

// Help opcache.preload discover always-needed symbols
class_exists(AcceptHeader::class);
class_exists(FileBag::class);
class_exists(HeaderBag::class);
class_exists(HeaderUtils::class);
class_exists(InputBag::class);
class_exists(ParameterBag::class);
class_exists(ServerBag::class);

/**
 * Request represents an HTTP request.
 *
 * The methods dealing with URL accept / return a raw path (% encoded):
 *   * getBasePath
 *   * getBaseUrl
 *   * getPathInfo
 *   * getRequestUri
 *   * getUri
 *   * getUriForPath
 *
 * @author Fabien Potencier <fabien@symfony.com>
 */
class Request
{
    public const HEADER_FORWARDED = 0b000001; // When using RFC 7239
    public const HEADER_X_FORWARDED_FOR = 0b000010;
    public const HEADER_X_FORWARDED_HOST = 0b000100;
    public const HEADER_X_FORWARDED_PROTO = 0b001000;
    public const HEADER_X_FORWARDED_PORT = 0b010000;
    public const HEADER_X_FORWARDED_PREFIX = 0b100000;

    public const HEADER_X_FORWARDED_AWS_ELB = 0b0011010; // AWS ELB doesn't send X-Forwarded-Host
    public const HEADER_X_FORWARDED_TRAEFIK = 0b0111110; // All "X-Forwarded-*" headers sent by Traefik reverse proxy

    public const METHOD_HEAD = 'HEAD';
    public const METHOD_GET = 'GET';
    public const METHOD_POST = 'POST';
    public const METHOD_PUT = 'PUT';
    public const METHOD_PATCH = 'PATCH';
    public const METHOD_DELETE = 'DELETE';
    public const METHOD_PURGE = 'PURGE';
    public const METHOD_OPTIONS = 'OPTIONS';
    public const METHOD_TRACE = 'TRACE';
    public const METHOD_CONNECT = 'CONNECT';
    public const METHOD_QUERY = 'QUERY';

    /**
     * @var string[]
     */
    protected static array $trustedProxies = [];

    /**
     * @var string[]
     */
    protected static array $trustedHostPatterns = [];

    /**
     * @var string[]
     */
    protected static array $trustedHosts = [];

    protected static bool $httpMethodParameterOverride = false;

    /**
     * The HTTP methods that can be overridden.
     *
     * @var uppercase-string[]|null
     */
    protected static ?array $allowedHttpMethodOverride = null;

    /**
     * Custom parameters.
     */
    public ParameterBag $attributes;

    /**
     * Request body parameters ($_POST).
     *
     * @see getPayload() for portability between content types
     */
    public InputBag $request;

    /**
     * Query string parameters ($_GET).
     *
     * @var InputBag<string>
     */
    public InputBag $query;

    /**
     * Server and execution environment parameters ($_SERVER).
     */
    public ServerBag $server;

    /**
     * Uploaded files ($_FILES).
     */
    public FileBag $files;

    /**
     * Cookies ($_COOKIE).
     *
     * @var InputBag<string>
     */
    public InputBag $cookies;

    /**
     * Headers (taken from the $_SERVER).
     */
    public HeaderBag $headers;

    /**
     * @var string|resource|false|null
     */
    protected $content;

    /**
     * @var string[]|null
     */
    protected ?array $languages = null;

    /**
     * @var string[]|null
     */
    protected ?array $charsets = null;

    /**
     * @var string[]|null
     */
    protected ?array $encodings = null;

    /**
     * @var string[]|null
     */
    protected ?array $acceptableContentTypes = null;

    protected ?string $pathInfo = null;
    protected ?string $requestUri = null;
    protected ?string $baseUrl = null;
    protected ?string $basePath = null;
    protected ?string $method = null;
    protected ?string $format = null;
    protected SessionInterface|\Closure|null $session = null;
    protected ?string $locale = null;
    protected string $defaultLocale = 'en';

    /**
     * @var array<string, string[]>|null
     */
    protected static ?array $formats = null;

    protected static ?\Closure $requestFactory = null;

    private ?string $preferredFormat = null;

    private bool $isHostValid = true;
    private bool $isForwardedValid = true;
    private bool $isSafeContentPreferred;

    private array $trustedValuesCache = [];

    private static int $trustedHeaderSet = -1;

    /**
     * @var array<string, true>
     */
    private static array $trustedHostsLiterals = [];

    /**
     * @var string[]
     */
    private static array $trustedHostsRegexps = [];

    private const FORWARDED_PARAMS = [
        self::HEADER_X_FORWARDED_FOR => 'for',
        self::HEADER_X_FORWARDED_HOST => 'host',
        self::HEADER_X_FORWARDED_PROTO => 'proto',
        self::HEADER_X_FORWARDED_PORT => 'host',
    ];

    /**
     * Names for headers that can be trusted when
     * using trusted proxies.
     *
     * The FORWARDED header is the standard as of rfc7239.
     *
     * The other headers are non-standard, but widely used
     * by popular reverse proxies (like Apache mod_proxy or Amazon EC2).
     */
    private const TRUSTED_HEADERS = [
        self::HEADER_FORWARDED => 'FORWARDED',
        self::HEADER_X_FORWARDED_FOR => 'X_FORWARDED_FOR',
        self::HEADER_X_FORWARDED_HOST => 'X_FORWARDED_HOST',
        self::HEADER_X_FORWARDED_PROTO => 'X_FORWARDED_PROTO',
        self::HEADER_X_FORWARDED_PORT => 'X_FORWARDED_PORT',
        self::HEADER_X_FORWARDED_PREFIX => 'X_FORWARDED_PREFIX',
    ];

    /**
     * This mapping is used when no exact MIME match is found in $formats.
     *
     * It enables mappings like application/soap+xml -> xml.
     *
     * @see https://datatracker.ietf.org/doc/html/rfc6839
     * @see https://datatracker.ietf.org/doc/html/rfc7303
     * @see https://www.iana.org/assignments/media-types/media-types.xhtml
     */
    private const STRUCTURED_SUFFIX_FORMATS = [
        'json' => 'json',
        'xml' => 'xml',
        'xhtml' => 'html',
        'cbor' => 'cbor',
        'zip' => 'zip',
        'ber' => 'asn1',
        'der' => 'asn1',
        'tlv' => 'tlv',
        'wbxml' => 'xml',
        'yaml' => 'yaml',
    ];

    private bool $isIisRewrite = false;

    /**
     * @param array                $query      The GET parameters
     * @param array                $request    The POST parameters
     * @param array                $attributes The request attributes (parameters parsed from the PATH_INFO, ...)
     * @param array                $cookies    The COOKIE parameters
     * @param array                $files      The FILES parameters
     * @param array                $server     The SERVER parameters
     * @param string|resource|null $content    The raw body data
     */
    public function __construct(array $query = [], array $request = [], array $attributes = [], array $cookies = [], array $files = [], array $server = [], $content = null)
    {}

    /**
     * Sets the parameters for this request.
     *
     * This method also re-initializes all properties.
     *
     * @param array                $query      The GET parameters
     * @param array                $request    The POST parameters
     * @param array                $attributes The request attributes (parameters parsed from the PATH_INFO, ...)
     * @param array                $cookies    The COOKIE parameters
     * @param array                $files      The FILES parameters
     * @param array                $server     The SERVER parameters
     * @param string|resource|null $content    The raw body data
     */
    public function initialize(array $query = [], array $request = [], array $attributes = [], array $cookies = [], array $files = [], array $server = [], $content = null): void
    {}

    /**
     * Creates a new request with values from PHP's super globals.
     */
    public static function createFromGlobals(): static
    {}

    /**
     * Creates a Request based on a given URI and configuration.
     *
     * The information contained in the URI always take precedence
     * over the other information (server and parameters).
     *
     * @param string               $uri        The URI
     * @param string               $method     The HTTP method
     * @param array                $parameters The query (GET) or request (POST) parameters
     * @param array                $cookies    The request cookies ($_COOKIE)
     * @param array                $files      The request files ($_FILES)
     * @param array                $server     The server parameters ($_SERVER)
     * @param string|resource|null $content    The raw body data
     *
     * @throws BadRequestException When the URI is invalid
     */
    public static function create(string $uri, string $method = 'GET', array $parameters = [], array $cookies = [], array $files = [], array $server = [], $content = null): static
    {}

    /**
     * Sets a callable able to create a Request instance.
     *
     * This is mainly useful when you need to override the Request class
     * to keep BC with an existing system. It should not be used for any
     * other purpose.
     */
    public static function setFactory(?callable $callable): void
    {}

    /**
     * Clones a request and overrides some of its parameters.
     *
     * @param array|null $query      The GET parameters
     * @param array|null $request    The POST parameters
     * @param array|null $attributes The request attributes (parameters parsed from the PATH_INFO, ...)
     * @param array|null $cookies    The COOKIE parameters
     * @param array|null $files      The FILES parameters
     * @param array|null $server     The SERVER parameters
     */
    public function duplicate(?array $query = null, ?array $request = null, ?array $attributes = null, ?array $cookies = null, ?array $files = null, ?array $server = null): static
    {}

    /**
     * Clones the current request.
     *
     * Note that the session is not cloned as duplicated requests
     * are most of the time sub-requests of the main one.
     */
    public function __clone()
    {}

    public function __toString(): string
    {}

    /**
     * Overrides the PHP global variables according to this request instance.
     *
     * It overrides $_GET, $_POST, $_REQUEST, $_SERVER, $_COOKIE.
     * $_FILES is never overridden, see rfc1867
     */
    public function overrideGlobals(): void
    {}

    /**
     * Sets a list of trusted proxies.
     *
     * You should only list the reverse proxies that you manage directly.
     *
     * @param array                          $proxies          A list of trusted proxies, the string 'REMOTE_ADDR' will be replaced with $_SERVER['REMOTE_ADDR'] and 'PRIVATE_SUBNETS' by IpUtils::PRIVATE_SUBNETS
     * @param int-mask-of<Request::HEADER_*> $trustedHeaderSet A bit field to set which headers to trust from your proxies
     */
    public static function setTrustedProxies(array $proxies, int $trustedHeaderSet): void
    {}

    /**
     * Gets the list of trusted proxies.
     *
     * @return string[]
     */
    public static function getTrustedProxies(): array
    {}

    /**
     * Gets the set of trusted headers from trusted proxies.
     *
     * @return int A bit field of Request::HEADER_* that defines which headers are trusted from your proxies
     */
    public static function getTrustedHeaderSet(): int
    {}

    /**
     * Sets a list of trusted host patterns.
     *
     * You should only list the hosts you manage using regexs.
     *
     * @param array $hostPatterns A list of trusted host patterns
     */
    public static function setTrustedHosts(array $hostPatterns): void
    {}

    /**
     * Gets the list of trusted host patterns.
     *
     * @return string[]
     */
    public static function getTrustedHosts(): array
    {}

    /**
     * Normalizes a query string.
     *
     * It builds a normalized query string, where keys/value pairs are alphabetized,
     * have consistent escaping and unneeded delimiters are removed.
     */
    public static function normalizeQueryString(?string $qs): string
    {}

    /**
     * Enables support for the _method request parameter to determine the intended HTTP method.
     *
     * Be warned that enabling this feature might lead to CSRF issues in your code.
     * Check that you are using CSRF tokens when required.
     * If the HTTP method parameter override is enabled, an html-form with method "POST" can be altered
     * and used to send a "PUT" or "DELETE" request via the _method request parameter.
     * If these methods are not protected against CSRF, this presents a possible vulnerability.
     *
     * The HTTP method can only be overridden when the real HTTP method is POST.
     */
    public static function enableHttpMethodParameterOverride(): void
    {}

    /**
     * Checks whether support for the _method request parameter is enabled.
     */
    public static function getHttpMethodParameterOverride(): bool
    {}

    /**
     * Sets the list of HTTP methods that can be overridden.
     *
     * Set to null to allow all methods to be overridden (default). Set to an
     * empty array to disallow overrides entirely. Otherwise, provide the list
     * of uppercased method names that are allowed.
     *
     * @param uppercase-string[]|null $methods
     */
    public static function setAllowedHttpMethodOverride(?array $methods): void
    {}

    /**
     * Gets the list of HTTP methods that can be overridden.
     *
     * @return uppercase-string[]|null
     */
    public static function getAllowedHttpMethodOverride(): ?array
    {}

    /**
     * Gets a "parameter" value from any bag.
     *
     * This method is mainly useful for libraries that want to provide some flexibility. If you don't need the
     * flexibility in controllers, it is better to explicitly get request parameters from the appropriate
     * public property instead (attributes, query, request).
     *
     * Order of precedence: PATH (routing placeholders or custom attributes), GET, POST
     *
     * @deprecated since Symfony 7.4, use properties `->attributes`, `query` or `request` directly instead
     */
    public function get(string $key, mixed $default = null): mixed
    {}

    /**
     * Gets the Session.
     *
     * @throws SessionNotFoundException When session is not set properly
     */
    public function getSession(): SessionInterface
    {}

    /**
     * Whether the request contains a Session which was started in one of the
     * previous requests.
     */
    public function hasPreviousSession(): bool
    {}

    /**
     * Whether the request contains a Session object.
     *
     * This method does not give any information about the state of the session object,
     * like whether the session is started or not. It is just a way to check if this Request
     * is associated with a Session instance.
     *
     * @param bool $skipIfUninitialized When true, ignores factories injected by `setSessionFactory`
     */
    public function hasSession(bool $skipIfUninitialized = false): bool
    {}

    public function setSession(SessionInterface $session): void
    {}

    /**
     * @internal
     *
     * @param callable(): SessionInterface $factory
     */
    public function setSessionFactory(callable $factory): void
    {}

    /**
     * Returns the client IP addresses.
     *
     * In the returned array the most trusted IP address is first, and the
     * least trusted one last. The "real" client IP address is the last one,
     * but this is also the least trusted one. Trusted proxies are stripped.
     *
     * Use this method carefully; you should use getClientIp() instead.
     *
     * @see getClientIp()
     */
    public function getClientIps(): array
    {}

    /**
     * Returns the client IP address.
     *
     * This method can read the client IP address from the "X-Forwarded-For" header
     * when trusted proxies were set via "setTrustedProxies()". The "X-Forwarded-For"
     * header value is a comma+space separated list of IP addresses, the left-most
     * being the original client, and each successive proxy that passed the request
     * adding the IP address where it received the request from.
     *
     * @see getClientIps()
     * @see https://wikipedia.org/wiki/X-Forwarded-For
     */
    public function getClientIp(): ?string
    {}

    /**
     * Returns current script name.
     */
    public function getScriptName(): string
    {}

    /**
     * Returns the path being requested relative to the executed script.
     *
     * The path info always starts with a /.
     *
     * Suppose this request is instantiated from /mysite on localhost:
     *
     *  * http://localhost/mysite              returns '/'
     *  * http://localhost/mysite/about        returns '/about'
     *  * http://localhost/mysite/enco%20ded   returns '/enco%20ded'
     *  * http://localhost/mysite/about?var=1  returns '/about'
     *
     * @return string The raw path (i.e. not urldecoded)
     */
    public function getPathInfo(): string
    {}

    /**
     * Returns the root path from which this request is executed.
     *
     * Suppose that an index.php file instantiates this request object:
     *
     *  * http://localhost/index.php         returns an empty string
     *  * http://localhost/index.php/page    returns an empty string
     *  * http://localhost/web/index.php     returns '/web'
     *  * http://localhost/we%20b/index.php  returns '/we%20b'
     *
     * @return string The raw path (i.e. not urldecoded)
     */
    public function getBasePath(): string
    {}

    /**
     * Returns the root URL from which this request is executed.
     *
     * The base URL never ends with a /.
     *
     * This is similar to getBasePath(), except that it also includes the
     * script filename (e.g. index.php) if one exists.
     *
     * @return string The raw URL (i.e. not urldecoded)
     */
    public function getBaseUrl(): string
    {}

    /**
     * Returns the real base URL received by the webserver from which this request is executed.
     * The URL does not include trusted reverse proxy prefix.
     *
     * @return string The raw URL (i.e. not urldecoded)
     */
    private function getBaseUrlReal(): string
    {}

    /**
     * Gets the request's scheme.
     */
    public function getScheme(): string
    {}

    /**
     * Returns the port on which the request is made.
     *
     * This method can read the client port from the "X-Forwarded-Port" header
     * when trusted proxies were set via "setTrustedProxies()".
     *
     * The "X-Forwarded-Port" header must contain the client port.
     *
     * @return int|string|null Can be a string if fetched from the server bag
     */
    public function getPort(): int|string|null
    {}

    /**
     * Returns the user.
     */
    public function getUser(): ?string
    {}

    /**
     * Returns the password.
     */
    public function getPassword(): ?string
    {}

    /**
     * Gets the user info.
     *
     * @return string|null A user name if any and, optionally, scheme-specific information about how to gain authorization to access the server
     */
    public function getUserInfo(): ?string
    {}

    /**
     * Returns the HTTP host being requested.
     *
     * The port name will be appended to the host if it's non-standard.
     */
    public function getHttpHost(): string
    {}

    /**
     * Returns the requested URI (path and query string).
     *
     * @return string The raw URI (i.e. not URI decoded)
     */
    public function getRequestUri(): string
    {}

    /**
     * Gets the scheme and HTTP host.
     *
     * If the URL was called with basic authentication, the user
     * and the password are not added to the generated string.
     */
    public function getSchemeAndHttpHost(): string
    {}

    /**
     * Generates a normalized URI (URL) for the Request.
     *
     * @see getQueryString()
     */
    public function getUri(): string
    {}

    /**
     * Generates a normalized URI for the given path.
     *
     * @param string $path A path to use instead of the current one
     */
    public function getUriForPath(string $path): string
    {}

    /**
     * Returns the path as relative reference from the current Request path.
     *
     * Only the URIs path component (no schema, host etc.) is relevant and must be given.
     * Both paths must be absolute and not contain relative parts.
     * Relative URLs from one resource to another are useful when generating self-contained downloadable document archives.
     * Furthermore, they can be used to reduce the link size in documents.
     *
     * Example target paths, given a base path of "/a/b/c/d":
     * - "/a/b/c/d"     -> ""
     * - "/a/b/c/"      -> "./"
     * - "/a/b/"        -> "../"
     * - "/a/b/c/other" -> "other"
     * - "/a/x/y"       -> "../../x/y"
     */
    public function getRelativeUriForPath(string $path): string
    {}

    /**
     * Generates the normalized query string for the Request.
     *
     * It builds a normalized query string, where keys/value pairs are alphabetized
     * and have consistent escaping.
     */
    public function getQueryString(): ?string
    {}

    /**
     * Checks whether the request is secure or not.
     *
     * This method can read the client protocol from the "X-Forwarded-Proto" header
     * when trusted proxies were set via "setTrustedProxies()".
     *
     * The "X-Forwarded-Proto" header must contain the protocol: "https" or "http".
     */
    public function isSecure(): bool
    {}

    /**
     * Returns the host name.
     *
     * This method can read the client host name from the "X-Forwarded-Host" header
     * when trusted proxies were set via "setTrustedProxies()".
     *
     * The "X-Forwarded-Host" header must contain the client host name.
     *
     * @throws SuspiciousOperationException when the host name is invalid or not trusted
     */
    public function getHost(): string
    {}

    /**
     * Sets the request method.
     */
    public function setMethod(string $method): void
    {}

    /**
     * Gets the request "intended" method.
     *
     * If the X-HTTP-Method-Override header is set, and if the method is a POST,
     * then it is used to determine the "real" intended HTTP method.
     *
     * The _method request parameter can also be used to determine the HTTP method,
     * but only if enableHttpMethodParameterOverride() has been called.
     *
     * The method is always an uppercased string.
     *
     * @see getRealMethod()
     */
    public function getMethod(): string
    {}

    /**
     * Gets the "real" request method.
     *
     * @see getMethod()
     */
    public function getRealMethod(): string
    {}

    /**
     * Gets the mime type associated with the format.
     */
    public function getMimeType(string $format): ?string
    {}

    /**
     * Gets the mime types associated with the format.
     *
     * @return string[]
     */
    public static function getMimeTypes(string $format): array
    {}

    /**
     * Gets the format associated with the mime type.
     *
     *  Resolution order:
     *   1) Exact match on the full MIME type (e.g. "application/json").
     *   2) Match on the canonical MIME type (i.e. before the first ";" parameter).
     *   3) If the type is "application/*+suffix", use the structured syntax suffix
     *      mapping (e.g. "application/foo+json" → "json"), when available.
     *   4) If $subtypeFallback is true and no match was found:
     *      - return the MIME subtype (without "x-" prefix), provided it does not
     *        contain a "+" (e.g. "application/x-yaml" → "yaml", "text/csv" → "csv").
     *
     * @param string|null $mimeType        The mime type to check
     * @param bool        $subtypeFallback Whether to fall back to the subtype if no exact match is found
     */
    public function getFormat(?string $mimeType/* , bool $subtypeFallback = false */): ?string
    {}

    /**
     * Associates a format with mime types.
     *
     * @param string          $format    The format to set
     * @param string|string[] $mimeTypes The associated mime types (the preferred one must be the first as it will be used as the content type)
     */
    public function setFormat(?string $format, string|array $mimeTypes): void
    {}

    /**
     * Gets the request format.
     *
     * Here is the process to determine the format:
     *
     *  * format defined by the user (with setRequestFormat())
     *  * _format request attribute
     *  * $default
     *
     * @see getPreferredFormat
     */
    public function getRequestFormat(?string $default = 'html'): ?string
    {}

    /**
     * Sets the request format.
     */
    public function setRequestFormat(?string $format): void
    {}

    /**
     * Gets the usual name of the format associated with the request's media type (provided in the Content-Type header).
     *
     * @see Request::$formats
     */
    public function getContentTypeFormat(): ?string
    {}

    /**
     * Sets the default locale.
     */
    public function setDefaultLocale(string $locale): void
    {}

    /**
     * Get the default locale.
     */
    public function getDefaultLocale(): string
    {}

    /**
     * Sets the locale.
     */
    public function setLocale(string $locale): void
    {}

    /**
     * Get the locale.
     */
    public function getLocale(): string
    {}

    /**
     * Checks if the request method is of specified type.
     *
     * @param string $method Uppercase request method (GET, POST etc)
     */
    public function isMethod(string $method): bool
    {}

    /**
     * Checks whether or not the method is safe.
     *
     * @see https://tools.ietf.org/html/rfc7231#section-4.2.1
     */
    public function isMethodSafe(): bool
    {}

    /**
     * Checks whether or not the method is idempotent.
     */
    public function isMethodIdempotent(): bool
    {}

    /**
     * Checks whether the method is cacheable or not.
     *
     * @see https://tools.ietf.org/html/rfc7231#section-4.2.3
     */
    public function isMethodCacheable(): bool
    {}

    /**
     * Returns the protocol version.
     *
     * If the application is behind a proxy, the protocol version used in the
     * requests between the client and the proxy and between the proxy and the
     * server might be different. This returns the former (from the "Via" header)
     * if the proxy is trusted (see "setTrustedProxies()"), otherwise it returns
     * the latter (from the "SERVER_PROTOCOL" server parameter).
     */
    public function getProtocolVersion(): ?string
    {}

    /**
     * Returns the request body content.
     *
     * @param bool $asResource If true, a resource will be returned
     *
     * @return string|resource
     *
     * @psalm-return ($asResource is true ? resource : string)
     */
    public function getContent(bool $asResource = false)
    {}

    /**
     * Gets the decoded form or json request body.
     *
     * @throws JsonException When the body cannot be decoded to an array
     */
    public function getPayload(): InputBag
    {}

    /**
     * Gets the request body decoded as array, typically from a JSON payload.
     *
     * @see getPayload() for portability between content types
     *
     * @throws JsonException When the body cannot be decoded to an array
     */
    public function toArray(): array
    {}

    /**
     * Gets the Etags.
     */
    public function getETags(): array
    {}

    public function isNoCache(): bool
    {}

    /**
     * Gets the preferred format for the response by inspecting, in the following order:
     *   * the request format set using setRequestFormat;
     *   * the values of the Accept HTTP header.
     *
     * Note that if you use this method, you should send the "Vary: Accept" header
     * in the response to prevent any issues with intermediary HTTP caches.
     */
    public function getPreferredFormat(?string $default = 'html'): ?string
    {}

    /**
     * Returns the preferred language.
     *
     * @param string[] $locales An array of ordered available locales
     */
    public function getPreferredLanguage(?array $locales = null): ?string
    {}

    /**
     * Gets a list of languages acceptable by the client browser ordered in the user browser preferences.
     *
     * @return string[]
     */
    public function getLanguages(): array
    {}

    /**
     * Strips the locale to only keep the canonicalized language value.
     *
     * Depending on the $locale value, this method can return values like :
     * - language_Script_REGION: "fr_Latn_FR", "zh_Hans_TW"
     * - language_Script: "fr_Latn", "zh_Hans"
     * - language_REGION: "fr_FR", "zh_TW"
     * - language: "fr", "zh"
     *
     * Invalid locale values are returned as is.
     *
     * @see https://wikipedia.org/wiki/IETF_language_tag
     * @see https://datatracker.ietf.org/doc/html/rfc5646
     */
    private static function formatLocale(string $locale): string
    {}

    /**
     * Returns an array of all possible combinations of the language components.
     *
     * For instance, if the locale is "fr_Latn_FR", this method will return:
     * - "fr_Latn_FR"
     * - "fr_Latn"
     * - "fr_FR"
     * - "fr"
     *
     * @return string[]
     */
    private static function getLanguageCombinations(string $locale): array
    {}

    /**
     * Returns an array with the language components of the locale.
     *
     * For example:
     * - If the locale is "fr_Latn_FR", this method will return "fr", "Latn", "FR"
     * - If the locale is "fr_FR", this method will return "fr", null, "FR"
     * - If the locale is "zh_Hans", this method will return "zh", "Hans", null
     *
     * @see https://wikipedia.org/wiki/IETF_language_tag
     * @see https://datatracker.ietf.org/doc/html/rfc5646
     *
     * @return array{string, string|null, string|null}
     */
    private static function getLanguageComponents(string $locale): array
    {}

    /**
     * Gets a list of charsets acceptable by the client browser in preferable order.
     *
     * @return string[]
     */
    public function getCharsets(): array
    {}

    /**
     * Gets a list of encodings acceptable by the client browser in preferable order.
     *
     * @return string[]
     */
    public function getEncodings(): array
    {}

    /**
     * Gets a list of content types acceptable by the client browser in preferable order.
     *
     * @return string[]
     */
    public function getAcceptableContentTypes(): array
    {}

    /**
     * Returns true if the request is an XMLHttpRequest.
     *
     * It works if your JavaScript library sets an X-Requested-With HTTP header.
     * It is known to work with common JavaScript frameworks:
     *
     * @see https://wikipedia.org/wiki/List_of_Ajax_frameworks#JavaScript
     */
    public function isXmlHttpRequest(): bool
    {}

    /**
     * Checks whether the client browser prefers safe content or not according to RFC8674.
     *
     * @see https://tools.ietf.org/html/rfc8674
     */
    public function preferSafeContent(): bool
    {}

    /*
     * The following methods are derived from code of the Zend Framework (1.10dev - 2010-01-24)
     *
     * Code subject to the new BSD license (https://framework.zend.com/license).
     *
     * Copyright (c) 2005-2010 Zend Technologies USA Inc. (https://www.zend.com/)
     */

    protected function prepareRequestUri(): string
    {}

    /**
     * Prepares the base URL.
     */
    protected function prepareBaseUrl(): string
    {}

    /**
     * Prepares the base path.
     */
    protected function prepareBasePath(): string
    {}

    /**
     * Prepares the path info.
     */
    protected function preparePathInfo(): string
    {}

    /**
     * Initializes HTTP request formats.
     */
    protected static function initializeFormats(): void
    {}

    private function setPhpDefaultLocale(string $locale): void
    {}

    /**
     * Returns the prefix as encoded in the string when the string starts with
     * the given prefix, null otherwise.
     */
    private function getUrlencodedPrefix(string $string, string $prefix): ?string
    {}

    private static function createRequestFromFactory(array $query = [], array $request = [], array $attributes = [], array $cookies = [], array $files = [], array $server = [], $content = null): static
    {}

    /**
     * Indicates whether this request originated from a trusted proxy.
     *
     * This can be useful to determine whether or not to trust the
     * contents of a proxy-specific header.
     */
    public function isFromTrustedProxy(): bool
    {}

    /**
     * This method is rather heavy because it splits and merges headers, and it's called by many other methods such as
     * getPort(), isSecure(), getHost(), getClientIps(), getBaseUrl() etc. Thus, we try to cache the results for
     * best performance.
     */
    private function getTrustedValues(int $type, ?string $ip = null): array
    {}

    private function normalizeAndFilterClientIps(array $clientIps, string $ip): array
    {}

    /**
     * Is this IIS with UrlRewriteModule?
     *
     * This method consumes, caches and removed the IIS_WasUrlRewritten env var,
     * so we don't inherit it to sub-requests.
     */
    private function isIisRewrite(): bool
    {}

    /**
     * See https://url.spec.whatwg.org/.
     */
    private static function isHostValid(string $host): bool
    {}

    /**
     * Combines host patterns into as few regexps as PCRE can compile.
     *
     * @return string[]
     */
    private static function compileHostPatterns(array $hostPatterns): array
    {}
}
