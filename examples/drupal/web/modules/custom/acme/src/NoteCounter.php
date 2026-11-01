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
        // core's core.services.yml. Complete a service ID inside
        // `$container->get('')`.
        return new static($container->get('entity_type.manager'));
    }

    public function first(): ?Note
    {
        // Try: hover `load()`, typed as `Note`, the class that declares the
        // acme_note entity type, or go to the definition of 'acme_note',
        // which lands on the `id` in Note's attribute. Complete an entity
        // type ID inside `getStorage('')`.
        return $this->entityTypeManager->getStorage('acme_note')->load(1);
    }

    public function count(): int
    {
        return (int) $this->entityTypeManager
            ->getStorage('acme_note')
            ->getQuery()
            ->accessCheck(false)
            ->count()
            ->execute();
    }
}
