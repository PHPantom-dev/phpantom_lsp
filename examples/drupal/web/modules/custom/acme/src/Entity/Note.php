<?php

declare(strict_types=1);

namespace Drupal\acme\Entity;

use Drupal\Core\Entity\Attribute\ContentEntityType;
use Drupal\Core\Entity\ContentEntityBase;
use Drupal\Core\Entity\EntityTypeInterface;
use Drupal\Core\Field\BaseFieldDefinition;
use Drupal\Core\StringTranslation\TranslatableMarkup;
use Drupal\user\EntityOwnerTrait;

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
        'owner' => 'uid',
    ],
    base_table: 'acme_note',
    // Try: complete a permission inside `admin_permission: ''`.
    admin_permission: 'administer acme',
)]
final class Note extends ContentEntityBase implements NoteInterface
{
    use EntityOwnerTrait;

    /**
     * @return array<string, \Drupal\Core\Field\FieldDefinitionInterface>
     */
    public static function baseFieldDefinitions(EntityTypeInterface $entity_type): array
    {
        // Try: in `src/NoteSummary.php`, `$note->` offers the fields this
        // method declares, its parent's and the owner trait's: `id` and
        // `uuid` come from ContentEntityBase under the names the attribute
        // gives the keys, and `uid` from core's EntityOwnerTrait.
        $fields = parent::baseFieldDefinitions($entity_type);
        $fields += static::ownerBaseFieldDefinitions($entity_type);

        // Try: complete after `BaseFieldDefinition::create('string')->`. Each
        // setter returns the definition, so the chain keeps completing. Go to
        // the definition of 'string', the field type, of 'string_textfield',
        // the widget, or of 'UniqueField', the constraint, which land on
        // core's plugin classes, or complete one inside its quotes. Misspell
        // 'string' as 'strin' and a warning reports a field type no plugin
        // declares.
        $fields['title'] = BaseFieldDefinition::create('string')
            ->setLabel(new TranslatableMarkup('Title'))
            ->setRequired(true)
            ->setSetting('max_length', 255)
            ->setDisplayOptions('form', ['type' => 'string_textfield'])
            ->addConstraint('UniqueField');

        return $fields;
    }
}
