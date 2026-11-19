<?php

declare(strict_types=1);

use App\ConfiguredViews;
use Tempest\View\ViewComponent;
use Tempest\View\ViewConfig;

use function Tempest\Support\Filesystem\read_file;

$config = new ViewConfig();
$config->addViewComponents(
    __DIR__ . '/components/x-card.view.php',
    __DIR__ . '/components/x-book.view.php',
    __DIR__ . '/components/x-dynamic-book.view.php',
    __DIR__ . '/../vendor/tempest/view/src/Components/x-component.view.php',
);
$notice = new ViewComponent(
    name: 'book-notice',
    contents: read_file(ConfiguredViews::NOTICE),
    file: ConfiguredViews::NOTICE,
    isVendorComponent: false,
);
$config->addViewComponent($notice);

return $config;
