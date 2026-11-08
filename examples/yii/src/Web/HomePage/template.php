<?php

declare(strict_types=1);

use App\Shared\ApplicationParams;
use Yiisoft\Html\Html;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\Translator\TranslatorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var ApplicationParams $applicationParams
 * @var UrlGeneratorInterface $urlGenerator
 * @var TranslatorInterface $translator
 */

$this->setTitle($applicationParams->name);

// Try: go to the definition of 'home.greeting', which lands on its entry in
// both resources/messages/en/app.php and resources/messages/de/app.php, or
// hover it for the English text. The call names no category, so the ID is
// looked up in the translator's default one, 'app'. Clear the ID to complete
// the ones that category declares, or misspell it to see it reported.
?>

<div class="text-center">
    <h1><?= Html::encode($translator->translate('home.greeting')) ?></h1>

    <p>Let's start something great with <strong>Yii3</strong>!</p>

    <p>
        <a href="<?= $urlGenerator->generate('blog/index') ?>"><?= Html::encode($translator->translate('home.read_blog')) ?></a>
    </p>

    <p>
        <a href="https://yiisoft.github.io/docs/" target="_blank" rel="noopener">
            <i>Don't forget to check the guide.</i>
        </a>
    </p>
</div>
