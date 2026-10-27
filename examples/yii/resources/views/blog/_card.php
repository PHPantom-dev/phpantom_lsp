<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\Html\Html;

/**
 * @var Post $post
 * @var string $url
 */
?>

<article>
    <h2><?= Html::a(Html::encode($post->title), $url) ?></h2>
</article>
