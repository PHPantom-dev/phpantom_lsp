<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\acme\Entity\NoteInterface;

/**
 * Sums a note up from its fields.
 */
final class NoteSummary
{
    public function summarize(NoteInterface $note): string
    {
        // Try: complete after `$note->`. The base fields of acme_note are
        // offered: `title` from Note, `id` and `uuid` from ContentEntityBase,
        // and `uid` from core's owner trait. Hover `$title`, the list of
        // StringItem items the `string` field type keeps, and `$author`, an
        // entity reference list that also names `User`, the class of the
        // entity type it points to. Go to the definition of `title`, which
        // lands on its `$fields['title']` line in Note, or of `uid`, which
        // lands in core's EntityOwnerTrait.
        $title = $note->title;
        $author = $note->uid;
        // Try: `field_tags` is a configurable field, which the site's
        // exported configuration declares in
        // `config/sync/field.storage.acme_note.field_tags.yml`. It completes
        // after `$note->` with the base fields, `$tags` is an entity
        // reference list that names `Term`, the class of the taxonomy_term
        // entity type its `target_type` gives, and go-to-definition on
        // `field_tags` lands in that file.
        $tags = $note->field_tags;
        // Try: a field reads the properties of its first item, the ones its
        // item class declares in `propertyDefinitions()`. Complete after
        // `$note->field_tags->`: `target_id` and `entity` come from
        // EntityReferenceItem. Hover `$firstTag`, a `?Term`, null when the
        // field is empty, and `$authorAccount`, a `?User`; `->label()` after
        // either navigates. `$titleText` is `mixed`, as every stored value
        // is: the database gives it as it is.
        $firstTag = $note->field_tags->entity;
        $authorAccount = $note->uid->entity;
        $titleText = $title->value;

        return sprintf('%s (%d, %d tags, first %s, by %s)', (string) $titleText, count($author), count($tags), (string) $firstTag?->label(), (string) $authorAccount?->label());
    }
}
