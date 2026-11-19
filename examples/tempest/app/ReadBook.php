<?php

declare(strict_types=1);

namespace App;

use Attribute;
use Tempest\Http\Method;
use Tempest\Router\HttpMiddleware;
use Tempest\Router\Route;

#[Attribute(Attribute::TARGET_METHOD)]
final class ReadBook implements Route
{
    public Method $method = Method::GET;

    /**
     * @param class-string<HttpMiddleware>[] $middleware
     * @param class-string<HttpMiddleware>[] $without
     */
    public function __construct(
        public string $uri,
        public array $middleware = [],
        public array $without = [],
    ) {}
}
