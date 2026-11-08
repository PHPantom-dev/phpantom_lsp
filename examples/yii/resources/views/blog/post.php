<?php

declare(strict_types=1);

use App\Blog\Post;
use App\Web\Blog\CommentForm;
use Yiisoft\FormModel\Field;
use Yiisoft\Html\Html;
use Yiisoft\Router\UrlGeneratorInterface;
use Yiisoft\Translator\TranslatorInterface;
use Yiisoft\View\WebView;

/**
 * @var WebView $this
 * @var UrlGeneratorInterface $urlGenerator
 * @var TranslatorInterface $translator
 * @var Post $post
 * @var CommentForm $comment
 */

$this->setTitle($post->title);

// Try: go to the definition of 'back-link' in class(), which lands on its
// selector in assets/main/site.css, or hover it.  Clear the string to
// complete the classes the project's stylesheets declare.
//
// Try: go to the definition of 'blog.all_posts', which lands on its entries
// in resources/messages/*/blog.php: the call names the 'blog' category.
//
// Try: go to the definition of 'author' in Field::text(), which lands on the
// property in src/Web/Blog/CommentForm.php. Clear the string to complete
// the form's properties, misspell it to see it reported, or rename the
// property and both fields follow.
?>

<h1><?= Html::encode($post->title) ?></h1>

<p><?= Html::encode($post->body) ?></p>

<p><?= Html::a($translator->translate('blog.all_posts', category: 'blog'), $urlGenerator->generate('blog/index'))->class('back-link') ?></p>

<?= Html::form()->post($urlGenerator->generate('blog/comment', ['slug' => $post->slug]))->open() ?>
<?= Field::text($comment, 'author') ?>
<?= Field::textarea($comment, 'message') ?>
<?= Field::submitButton('Comment') ?>
<?= Html::form()->close() ?>
