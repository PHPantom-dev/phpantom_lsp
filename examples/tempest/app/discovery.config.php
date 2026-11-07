<?php

declare(strict_types=1);

use Tempest\Discovery\DiscoveryConfig;
use Tempest\Discovery\DiscoveryLocation;

$directory = dirname(__DIR__) . '/resources';

return new DiscoveryConfig(locations: [
    new DiscoveryLocation('App\\', $directory, ignore: [$directory . '/private']),
]);
