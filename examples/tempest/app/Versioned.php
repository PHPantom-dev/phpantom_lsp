<?php

declare(strict_types=1);

namespace App;

use Attribute;
use Tempest\Router\Route;
use Tempest\Router\RouteDecorator;

#[Attribute(Attribute::TARGET_METHOD)]
final class Versioned implements RouteDecorator
{
    public function decorate(Route $route): Route
    {
        $route->uri = '/v1/' . $route->uri;

        return $route;
    }
}
