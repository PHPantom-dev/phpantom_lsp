<?php

declare(strict_types=1);

namespace Drupal\acme\Form;

use Drupal\Core\Form\ConfigFormBase;
use Drupal\Core\Form\FormStateInterface;

/**
 * Edits the acme.settings configuration object.
 */
final class SettingsForm extends ConfigFormBase
{
    public function getFormId(): string
    {
        return 'acme_settings';
    }

    /**
     * @return array<int, string>
     */
    protected function getEditableConfigNames(): array
    {
        // Try: go to the definition of 'acme.settings', which lists its
        // default configuration and its schema entry, or complete a
        // configuration name inside `['']`; core's are offered too.
        return ['acme.settings'];
    }

    /**
     * @param array<string, mixed> $form
     * @return array<string, mixed>
     */
    public function buildForm(array $form, FormStateInterface $form_state): array
    {
        // Try: go to the definition of 'acme.settings' before the `:`.
        $form['notify_email'] = [
            '#type' => 'email',
            '#title' => $this->t('Notification email'),
            '#config_target' => 'acme.settings:notify_email',
        ];

        return parent::buildForm($form, $form_state);
    }
}
