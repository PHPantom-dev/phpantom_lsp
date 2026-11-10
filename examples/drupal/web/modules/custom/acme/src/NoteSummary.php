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

        return sprintf('%s (%d)', (string) $title->value, count($author));
    }
}
