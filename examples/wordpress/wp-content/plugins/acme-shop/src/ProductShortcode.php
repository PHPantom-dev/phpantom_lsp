<?php

declare(strict_types=1);

namespace Acme\Shop;

final class ProductShortcode
{
    public function __construct(private readonly ProductRepository $products) {}

    /**
     * The [acme_product id="42"] shortcode.
     *
     * @param array<string, string>|string $atts
     */
    public function render(array|string $atts): string
    {
        // Try: hover $atts after this line, or complete inside $atts['']. The
        // result has exactly the keys of the defaults. WordPress passes an
        // empty string when the shortcode has no attributes.
        $atts = shortcode_atts(['id' => '0', 'show_price' => 'yes'], (array) $atts, 'acme_product');

        $product = $this->products->find(absint($atts['id']));
        if ($product === null) {
            return '';
        }

        $html = '<span class="acme-product">' . esc_html($product->title) . '</span>';
        if ($atts['show_price'] === 'yes') {
            $html .= ' <span class="acme-price">' . esc_html(number_format_i18n($product->price, 2)) . '</span>';
        }

        return $html;
    }
}
