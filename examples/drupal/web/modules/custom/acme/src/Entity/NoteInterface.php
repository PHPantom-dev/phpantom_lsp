<?php

declare(strict_types=1);

namespace Drupal\acme\Entity;

use Drupal\Core\Entity\ContentEntityInterface;
use Drupal\user\EntityOwnerInterface;

/**
 * What code that receives a note types it as, the way core types a node
 * as `NodeInterface`.
 */
interface NoteInterface extends ContentEntityInterface, EntityOwnerInterface {}
