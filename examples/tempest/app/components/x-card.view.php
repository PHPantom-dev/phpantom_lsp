<?php

/** @var string $title */
/** @var bool $showTitle */
?>

<article>
    <!-- Try: hover $title to see string. -->
    <h2 :if="$showTitle ?? true">{{ $title }}</h2>
    <x-slot />
</article>
