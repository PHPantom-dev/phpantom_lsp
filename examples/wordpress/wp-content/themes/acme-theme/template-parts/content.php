<?php

declare(strict_types=1);

// Try: hover $post. get_post() with no argument reads the current post of
// the loop.
$post = get_post();
if ($post === null) {
    return;
}

?>
<article id="post-<?php the_ID(); ?>" <?php post_class(); ?>>
    <h2><a href="<?php the_permalink(); ?>"><?php the_title(); ?></a></h2>
    <?php if (get_post_type($post) === 'acme_product') { ?>
        <p class="price"><?php echo esc_html(acme_theme_price($post->ID)); ?></p>
    <?php } ?>
    <?php echo acme_theme_first_category_link($post); ?>
    <?php the_excerpt(); ?>
</article>
