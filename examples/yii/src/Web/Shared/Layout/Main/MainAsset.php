<?php

declare(strict_types=1);

namespace App\Web\Shared\Layout\Main;

use Yiisoft\Assets\AssetBundle;

final class MainAsset extends AssetBundle
{
    // Try: go to the definition of '@assetsSource', declared in
    // config/common/aliases.php, or follow the link on '/main' to the
    // assets/main directory it names. Type '@' in a string to complete the
    // aliases.
    public ?string $basePath = '@assets/main';
    public ?string $baseUrl = '@assetsUrl/main';
    public ?string $sourcePath = '@assetsSource/main';

    // Try: go to the definition of 'site.css', which opens
    // assets/main/site.css under $sourcePath. Clear it to complete the files
    // there, or misspell it to see it reported.
    public array $css = [
        'site.css',
    ];
}
