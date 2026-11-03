<?php

declare(strict_types=1);

get_header();

if (have_posts()) {
    while (have_posts()) {
        the_post();
        get_template_part('template-parts/content', get_post_type());
    }

    // Try: hover acme_theme_last_page(), or go to its definition in
    // functions.php.
    if (acme_theme_last_page() > 1) {
        the_posts_pagination();
    }
} else {
    get_template_part('template-parts/content', 'none');
}

get_footer();
