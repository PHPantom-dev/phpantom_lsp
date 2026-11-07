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

    public array $css = [
        'site.css',
    ];
}
