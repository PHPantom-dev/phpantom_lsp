<?php

declare(strict_types=1);

namespace App\Web\Blog;

use Yiisoft\FormModel\FormModel;

/**
 * The comment form under a post. resources/views/blog/post.php renders a
 * field for each property, and PostController::comment() fills it from the
 * posted data.
 */
final class CommentForm extends FormModel
{
    public string $author = '';

    public string $message = '';
}
