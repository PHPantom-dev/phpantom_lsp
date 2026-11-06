<?php

declare(strict_types=1);

namespace Drupal\acme\Controller;

use Drupal\Core\Controller\ControllerBase;
use Drupal\Core\Entity\EntityInterface;

/**
 * Shows a note for the acme.note route.
 */
final class NoteController extends ControllerBase
{
    /**
     * @return array<string, mixed>
     */
    public function view(EntityInterface $acme_note): array
    {
        // Try: hover $acme_note. The parameter is named after the acme_note
        // entity type and hinted with an interface Note implements, so Drupal
        // upcasts the ID in the path to the Note itself.
        return ['#markup' => $acme_note->label()];
    }

    /**
     * The title of the note page.
     */
    public function title($acme_note): string
    {
        // Try: hover $acme_note. The title callback receives the same Note,
        // though nothing types it here.
        return (string) $acme_note->label();
    }
}
