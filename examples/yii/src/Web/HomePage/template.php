<?php

declare(strict_types=1);

use App\Shared\ApplicationParams;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var ApplicationParams $applicationParams
 * @var UrlGeneratorInterface $urlGenerator
 */

$this->setTitle($applicationParams->name);
?>

<div class="text-center">
    <h1>Hello!</h1>

    <p>Let's start something great with <strong>Yii3</strong>!</p>

    <p>
        <a href="<?= $urlGenerator->generate('blog/index') ?>">Read the blog.</a>
    </p>

    <p>
        <a href="https://yiisoft.github.io/docs/" target="_blank" rel="noopener">
            <i>Don't forget to check the guide.</i>
        </a>
    </p>
</div>
