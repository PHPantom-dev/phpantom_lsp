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
        // Try: go to the definition of 'acme.settings' before the `:`, or
        // of 'notify_email' after it, which lands on its schema line.
        // Complete a key after the `:`.
        $form['notify_email'] = [
            '#type' => 'email',
            '#title' => $this->t('Notification email'),
            '#config_target' => 'acme.settings:notify_email',
        ];
        // Try: complete a key inside `get('')`, read from acme.schema.yml;
        // after `notifications.`, the keys under it are offered. Hover
        // `$settings`, whose type carries the name 'acme.settings'.
        $settings = $this->config('acme.settings');
        $form['notify_email']['#access'] = $settings->get('notifications.email') === true;

        return parent::buildForm($form, $form_state);
    }

    /**
     * @param array<string, mixed> $form
     */
    public function submitForm(array &$form, FormStateInterface $form_state): void
    {
        // Try: complete a key inside `set('')`. An ImmutableConfig, which
        // `\Drupal::config()` returns, offers none: it cannot be saved.
        $this->config('acme.settings')
            ->set('notifications.email', $form_state->getValue('notify_email') !== '')
            ->save();
        parent::submitForm($form, $form_state);
    }
}
