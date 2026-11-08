<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\Html\Html;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var UrlGeneratorInterface $urlGenerator
 * @var Post $post
 */

$this->setTitle($post->title);

// Try: go to the definition of 'back-link' in class(), which lands on its
// selector in assets/main/site.css, or hover it.  Clear the string to
// complete the classes the project's stylesheets declare.
?>

<h1><?= Html::encode($post->title) ?></h1>

<p><?= Html::encode($post->body) ?></p>

<p><?= Html::a('All posts', $urlGenerator->generate('blog/index'))->class('back-link') ?></p>
