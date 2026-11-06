<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\acme\Entity\Note;
use Drupal\Core\DependencyInjection\ContainerInjectionInterface;
use Drupal\Core\Entity\EntityTypeManagerInterface;
use Symfony\Component\DependencyInjection\ContainerInterface;

/**
 * Counts the notes, built the way most Drupal classes get their services.
 */
final class NoteCounter implements ContainerInjectionInterface
{
    public function __construct(
        private readonly EntityTypeManagerInterface $entityTypeManager,
    ) {}

    public static function create(ContainerInterface $container): static
    {
        // Try: go to the definition of 'entity_type.manager', which lands in
        // core's core.services.yml, or hover `$manager`, typed by the class
        // core declares for it. Complete a service ID inside
        // `$container->get('')`.
        $manager = $container->get('entity_type.manager');

        return new static($manager);
    }

    public function first(): ?Note
    {
        // Try: hover `$storage`, the storage of the acme_note entity type,
        // and `$note`, typed as `Note`, the class that declares it. Go to the
        // definition of 'acme_note', which lands on the `id` in Note's
        // attribute, or complete an entity type ID inside `getStorage('')`.
        $storage = $this->entityTypeManager->getStorage('acme_note');
        $note = $storage->load(1);

        return $note;
    }

    public function count(): int
    {
        // Try: hover `$count`. An entity query that counts gives the number
        // of matches, and keeps counting through the methods it chains.
        $count = $this->entityTypeManager
            ->getStorage('acme_note')
            ->getQuery()
            ->accessCheck(false)
            ->count()
            ->execute();

        return $count;
    }

    /**
     * @return array<int, string>
     */
    public function ids(): array
    {
        // Try: hover `$ids`, the IDs of the notes, which a content entity
        // query gives as strings under integer keys. Go to the definition of
        // 'acme_note', or complete an entity type ID inside
        // `\Drupal::entityQuery('')`.
        $ids = \Drupal::entityQuery('acme_note')->accessCheck(false)->execute();

        return $ids;
    }
}
