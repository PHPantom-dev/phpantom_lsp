<?php

declare(strict_types=1);

namespace App;

use Attribute;
use Tempest\Router\Prefix;
use Tempest\Router\Route;
use Tempest\Router\RouteDecorator;

#[Attribute(Attribute::TARGET_CLASS | Attribute::TARGET_METHOD)]
final class Admin implements RouteDecorator
{
    public function decorate(Route $route): Route
    {
        return new Prefix('/admin')->decorate($route);
    }
}
