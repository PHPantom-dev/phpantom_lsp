<?php

declare(strict_types=1);

namespace App;

use Tempest\Container\Container;

use function Tempest\Container\invoke;

final class ContainerCalls
{
    public function run(Container $container): mixed
    {
        // Try: complete path/limit and follow either name to BookImporter::import().
        return $container->invoke([BookImporter::class, 'import'], path: '/books', limit: 2);
    }

    public function invokeClass(Container $container): mixed
    {
        // Try: follow path/limit to BookImporter's __invoke() parameters.
        return $container->invoke(BookImporter::class, path: '/books', limit: 3);
    }

    public function invokeHelper(): mixed
    {
        // Try: complete and follow arguments through the imported helper too.
        return invoke(BookImporter::class, path: '/books', limit: 4);
    }
}
