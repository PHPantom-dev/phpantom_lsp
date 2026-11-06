<?php

declare(strict_types=1);

namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;
use Drupal\Core\StringTranslation\TranslatableMarkup;

/**
 * A short note, declared by attribute the way Drupal 11 core declares
 * its own entity types.
 */
#[ContentEntityType(
    id: 'acme_note',
    label: new TranslatableMarkup('Note'),
    entity_keys: [
        'id' => 'id',
        'uuid' => 'uuid',
        'label' => 'title',
    ],
    base_table: 'acme_note',
    // Try: complete a permission inside `admin_permission: ''`.
    admin_permission: 'administer acme',
)]
final class Note extends ContentEntityBase
{
    /**
     * @return array<string, \Drupal\Core\Field\FieldDefinitionInterface>
     */
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type): array
    {
        $fields = parent::baseFieldDefinitions($entity_type);

        // Try: complete after `BaseFieldDefinition::create('string')->`. Each
        // setter returns the definition, so the chain keeps completing.
        $fields['title'] = BaseFieldDefinition::create('string')
            ->setLabel(new TranslatableMarkup('Title'))
            ->setRequired(true)
            ->setSetting('max_length', 255);

        return $fields;
    }
}
