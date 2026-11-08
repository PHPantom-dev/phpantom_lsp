<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\Html\Html;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\Translator\TranslatorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var UrlGeneratorInterface $urlGenerator
 * @var TranslatorInterface $translator
 * @var Post $post
 */

$this->setTitle($post->title);

// Try: go to the definition of 'back-link' in class(), which lands on its
// selector in assets/main/site.css, or hover it.  Clear the string to
// complete the classes the project's stylesheets declare.
//
// Try: go to the definition of 'blog.all_posts', which lands on its entries
// in resources/messages/*/blog.php: the call names the 'blog' category.
?>

<h1><?= Html::encode($post->title) ?></h1>

<p><?= Html::encode($post->body) ?></p>

<p><?= Html::a($translator->translate('blog.all_posts', category: 'blog'), $urlGenerator->generate('blog/index'))->class('back-link') ?></p>
