<?php

declare(strict_types=1);

namespace Drupal\acme;

use Drupal\Core\Access\AccessResult;
use Drupal\Core\Access\AccessResultInterface;
use Drupal\Core\Session\AccountInterface;

/**
 * Decides who may read and manage notes.
 */
final class NoteAccess
{
    public function canManage(AccountInterface $account): bool
    {
        // Try: hover 'administer acme' to read its title and description from
        // acme.permissions.yml, go to its definition, or complete a permission
        // inside `hasPermission('')`; core's are offered too.
        return $account->hasPermission('administer acme');
    }

    public function view(AccountInterface $account): AccessResultInterface
    {
        // Try: go to the definition of 'access content', which lands in core's
        // system.permissions.yml.
        return AccessResult::allowedIfHasPermissions($account, ['access content', 'view acme notes'], 'OR');
    }
}
