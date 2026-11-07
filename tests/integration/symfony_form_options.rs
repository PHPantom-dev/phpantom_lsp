//! Symfony form options: the keys of the options array a form builder's
//! `add()` or a controller's `createForm()` passes, checked against and
//! completed from the options the form type, its parents, and the type
//! extensions declare.

use crate::common::{
    completion_response, create_psr4_workspace, offset_position, open_document, response_labels,
};
use phpantom_lsp::Backend;
use tower_lsp_server::LanguageServer;
use tower_lsp_server::gen_lsp_types::*;

const COMPOSER: &str = r#"{
    "require": { "symfony/framework-bundle": "^7.3" },
    "autoload": { "psr-4": {
        "App\\": "src/",
        "Symfony\\Component\\Form\\": "stubs/form/",
        "Symfony\\Component\\OptionsResolver\\": "stubs/options-resolver/",
        "Symfony\\Bundle\\FrameworkBundle\\": "stubs/framework-bundle/"
    } }
}"#;

const STUBS: &[(&str, &str)] = &[
    (
        "stubs/options-resolver/OptionsResolver.php",
        "<?php\nnamespace Symfony\\Component\\OptionsResolver;\nclass OptionsResolver {\n    public function setDefaults(array $defaults): static { return $this; }\n    public function setDefault(string $option, mixed $value): static { return $this; }\n    public function setDefined(string|array $optionNames): static { return $this; }\n    public function setAllowedTypes(string $option, string|array $allowedTypes): static { return $this; }\n}\n",
    ),
    (
        "stubs/form/FormBuilderInterface.php",
        "<?php\nnamespace Symfony\\Component\\Form;\ninterface FormBuilderInterface {\n    public function add(string $child, ?string $type = null, array $options = []): static;\n}\n",
    ),
    (
        "stubs/form/FormTypeExtensionInterface.php",
        "<?php\nnamespace Symfony\\Component\\Form;\ninterface FormTypeExtensionInterface {\n    public static function getExtendedTypes(): iterable;\n}\n",
    ),
    (
        "stubs/form/AbstractTypeExtension.php",
        "<?php\nnamespace Symfony\\Component\\Form;\nuse Symfony\\Component\\OptionsResolver\\OptionsResolver;\nabstract class AbstractTypeExtension implements FormTypeExtensionInterface {\n    public function configureOptions(OptionsResolver $resolver): void {}\n}\n",
    ),
    (
        "stubs/form/AbstractType.php",
        "<?php\nnamespace Symfony\\Component\\Form;\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\FormType;\nuse Symfony\\Component\\OptionsResolver\\OptionsResolver;\nabstract class AbstractType {\n    public function buildForm(FormBuilderInterface $builder, array $options): void {}\n    public function configureOptions(OptionsResolver $resolver): void {}\n    public function getParent(): ?string\n    {\n        return FormType::class;\n    }\n}\n",
    ),
    (
        "stubs/form/Extension/Core/Type/FormType.php",
        "<?php\nnamespace Symfony\\Component\\Form\\Extension\\Core\\Type;\nuse Symfony\\Component\\Form\\AbstractType;\nuse Symfony\\Component\\OptionsResolver\\OptionsResolver;\nclass FormType extends AbstractType {\n    public function configureOptions(OptionsResolver $resolver): void\n    {\n        $resolver->setDefaults([\n            'data_class' => null,\n            'required' => true,\n            'label' => null,\n        ]);\n    }\n    public function getParent(): ?string\n    {\n        return null;\n    }\n}\n",
    ),
    (
        "stubs/form/Extension/Core/Type/TextType.php",
        "<?php\nnamespace Symfony\\Component\\Form\\Extension\\Core\\Type;\nuse Symfony\\Component\\Form\\AbstractType;\nuse Symfony\\Component\\OptionsResolver\\OptionsResolver;\nclass TextType extends AbstractType {\n    public function configureOptions(OptionsResolver $resolver): void\n    {\n        $resolver->setDefault('trim', true);\n    }\n}\n",
    ),
    (
        "stubs/form/Extension/Core/Type/EmailType.php",
        "<?php\nnamespace Symfony\\Component\\Form\\Extension\\Core\\Type;\nuse Symfony\\Component\\Form\\AbstractType;\nclass EmailType extends AbstractType {\n    public function getParent(): ?string\n    {\n        return TextType::class;\n    }\n}\n",
    ),
    (
        "stubs/framework-bundle/Controller/AbstractController.php",
        "<?php\nnamespace Symfony\\Bundle\\FrameworkBundle\\Controller;\nabstract class AbstractController {\n    protected function createForm(string $type, mixed $data = null, array $options = []): object { return new \\stdClass(); }\n}\n",
    ),
];

/// A type extension of the project's own, which gives every form an
/// option.
const EXTENSION_PHP: &str = r#"<?php
namespace App\Form;

use Symfony\Component\Form\AbstractTypeExtension;
use Symfony\Component\Form\Extension\Core\Type\FormType;
use Symfony\Component\OptionsResolver\OptionsResolver;

final class HelpIconExtension extends AbstractTypeExtension
{
    public function configureOptions(OptionsResolver $resolver): void
    {
        $resolver->setDefined('help_icon');
    }

    public static function getExtendedTypes(): iterable
    {
        return [FormType::class];
    }
}
"#;

const FORM_PHP: &str = r#"<?php
namespace App\Form;

use Symfony\Component\Form\AbstractType;
use Symfony\Component\Form\Extension\Core\Type\EmailType;
use Symfony\Component\Form\FormBuilderInterface;
use Symfony\Component\OptionsResolver\OptionsResolver;

final class RegistrationType extends AbstractType
{
    public function buildForm(FormBuilderInterface $builder, array $options): void
    {
        $builder
            ->add('email', EmailType::class, ['trim' => false, 'requird' => true, 'help_icon' => 'mail'])
            ->add('name', null, ['anything' => 1])
            ->add('nickname', EmailType::class, ['required' => false, '']);
    }

    public function configureOptions(OptionsResolver $resolver): void
    {
        parent::configureOptions($resolver);
        $resolver->setDefaults(['newsletter' => false]);
    }
}
"#;

const CONTROLLER_PHP: &str = r#"<?php
namespace App\Controller;

use App\Form\RegistrationType;
use App\Form\LooseType;
use Symfony\Bundle\FrameworkBundle\Controller\AbstractController;

final class SignupController extends AbstractController
{
    public function signup(): void
    {
        $this->createForm(RegistrationType::class, null, ['newsletter' => true, 'newsleter' => true, 'label' => 'Sign up']);
        $this->createForm(LooseType::class, null, ['whatever' => true]);
    }
}
"#;

/// A type whose options a helper declares, which are not read.
const LOOSE_PHP: &str = r#"<?php
namespace App\Form;

use Symfony\Component\Form\AbstractType;
use Symfony\Component\OptionsResolver\OptionsResolver;

final class LooseType extends AbstractType
{
    public function configureOptions(OptionsResolver $resolver): void
    {
        $this->configureShared($resolver);
    }

    private function configureShared(OptionsResolver $resolver): void
    {
        $resolver->setDefined('whatever');
    }
}
"#;

async fn workspace(files: &[(&str, &str)]) -> (Backend, tempfile::TempDir) {
    let all: Vec<(&str, &str)> = STUBS.iter().chain(files).copied().collect();
    let (backend, dir) = create_psr4_workspace(COMPOSER, &all);
    for (path, content) in files {
        open_document(&backend, &uri_for(&dir, path), "php", content).await;
    }
    (backend, dir)
}

fn uri_for(dir: &tempfile::TempDir, rel: &str) -> Uri {
    Uri::from_file_path(dir.path().join(rel)).unwrap()
}

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/Form/HelpIconExtension.php", EXTENSION_PHP),
        ("src/Form/RegistrationType.php", FORM_PHP),
        ("src/Form/LooseType.php", LOOSE_PHP),
        ("src/Controller/SignupController.php", CONTROLLER_PHP),
    ]
}

/// The unknown-option messages reported in the file at `rel`.
fn unknown_options(
    backend: &Backend,
    dir: &tempfile::TempDir,
    rel: &str,
    content: &str,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    backend.collect_slow_diagnostics(uri_for(dir, rel).as_str(), content, &mut diagnostics);
    diagnostics
        .into_iter()
        .filter(|d| d.code == Some(Code::String("invalid_symfony_form_option".to_string())))
        .map(|d| crate::common::message_text(&d.message).to_string())
        .collect()
}

#[tokio::test]
async fn options_no_type_declares_are_reported() {
    let (backend, dir) = workspace(&files()).await;
    assert_eq!(
        unknown_options(&backend, &dir, "src/Form/RegistrationType.php", FORM_PHP),
        vec!["Unknown form option 'requird' for Symfony\\Component\\Form\\Extension\\Core\\Type\\EmailType".to_string()],
    );
    assert_eq!(
        unknown_options(
            &backend,
            &dir,
            "src/Controller/SignupController.php",
            CONTROLLER_PHP
        ),
        vec!["Unknown form option 'newsleter' for App\\Form\\RegistrationType".to_string()],
    );
}

#[tokio::test]
async fn options_complete_from_the_type_its_parents_and_extensions() {
    let (backend, dir) = workspace(&files()).await;
    let uri = uri_for(&dir, "src/Form/RegistrationType.php");
    let offset = FORM_PHP.find("'']").unwrap() + 1;
    let position = offset_position(FORM_PHP, offset);
    let labels = response_labels(
        completion_response(&backend, &uri, position.line, position.character, None).await,
    );
    // `required` is already set in the array.
    assert_eq!(labels, vec!["data_class", "help_icon", "label", "trim"]);
}

#[tokio::test]
async fn an_option_goes_to_its_declaration() {
    let (backend, dir) = workspace(&files()).await;
    let uri = uri_for(&dir, "src/Form/RegistrationType.php");
    let offset = FORM_PHP.find("'trim'").unwrap() + 2;
    let response = backend
        .goto_definition(DefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: offset_position(FORM_PHP, offset),
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let Some(DefinitionResponse::Definition(Definition::Location(location))) = response else {
        panic!("expected one location, got {response:?}");
    };
    assert_eq!(
        location.uri,
        uri_for(&dir, "stubs/form/Extension/Core/Type/TextType.php")
    );
    assert_eq!(location.range.start, Position::new(7, 31));
    assert_eq!(location.range.end, Position::new(7, 35));
}
