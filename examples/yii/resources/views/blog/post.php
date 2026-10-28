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
?>

<h1><?= Html::encode($post->title) ?></h1>

<p><?= Html::encode($post->body) ?></p>

<p><?= Html::a('All posts', $urlGenerator->generate('blog/index')) ?></p>
