<?php

declare(strict_types=1);

use App\Blog\Post;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var UrlGeneratorInterface $urlGenerator
 * @var list<Post> $posts
 */

$this->setTitle('Blog');
?>

<h1>Blog</h1>

<?php foreach ($posts as $post): ?>
    <?php // Try: hover $post, or complete after $post->. The list is typed by the @var above. ?>
    <?php // Try: go-to-definition on 'blog/post', which lands on ->name('post') in config/common/routes.php under its group's blog/ prefix. Complete the name, or the 'slug' key, which is the placeholder the route's path declares. ?>
    <?= $this->render('_card', ['post' => $post, 'url' => $urlGenerator->generate('blog/post', ['slug' => $post->slug])]) ?>
<?php endforeach ?>
