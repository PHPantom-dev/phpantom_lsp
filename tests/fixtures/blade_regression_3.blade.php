@php
    /**
     * @var App\Models\Article $article
     */
@endphp
<?php
$schema = [
    '@context'         => 'http://schema.org',
    '@type'            => 'NewsArticle',
    'mainEntityOfPage' => [
        '@type' => 'WebPage',
        '@id'   => Illuminate\Support\Facades\URL::secure('/') . '/blog/' . $article->slug,
    ],
    'headline'      => $article->title,
    'datePublished' => ($article->created_at ?? now())->toIso8601String(),
];
?>
<script type="application/ld+json">
{{ json_encode($schema) }}
</script>
