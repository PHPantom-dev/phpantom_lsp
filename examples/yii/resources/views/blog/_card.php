<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\Html\Html;

// Try: the lens above the first line lists the render in index.php that
// names this template as '_card'.

/**
 * @var Post $post
 * @var string $url
 */
?>

<article>
    <h2><?= Html::a(Html::encode($post->title), $url) ?></h2>
</article>
