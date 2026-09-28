use phpantom_lsp::semantic_export::{
    DeclarationKind, ExportBatch, ExportDiagnosticKind, ExportDocument, OccurrenceKind,
    SemanticExportError, SemanticExporter, SourceDocument,
};

fn source(uri: &str, text: &str) -> SourceDocument {
    SourceDocument {
        uri: uri.to_string(),
        source: text.to_string(),
    }
}

fn document<'a>(batch: &'a ExportBatch, suffix: &str) -> &'a ExportDocument {
    batch
        .documents
        .iter()
        .find(|document| document.uri.ends_with(suffix))
        .unwrap()
}

#[test]
fn resolves_across_documents_and_keeps_owned_results() {
    let batch = SemanticExporter::new("/workspace")
        .export([
            source(
                "file:///workspace/use.php",
                "<?php namespace App; $user = new User(); $user->name();",
            ),
            source(
                "file:///workspace/User.php",
                "<?php namespace App; class User { public function name(): string {} }",
            ),
        ])
        .unwrap();

    let use_document = document(&batch, "/use.php");
    assert!(
        use_document
            .occurrences
            .iter()
            .any(|occurrence| occurrence.resolved_symbol.as_deref() == Some("App\\User::name"))
    );
    assert!(use_document.diagnostics.is_empty());
}

#[test]
fn inherited_members_resolve_to_the_class_that_declares_them() {
    let batch = SemanticExporter::new("/workspace")
        .export([
            source(
                "file:///workspace/Base.php",
                "<?php namespace App; class Base { public function shared(): void {} }",
            ),
            source(
                "file:///workspace/Child.php",
                "<?php namespace App; class Child extends Base {}",
            ),
            source(
                "file:///workspace/use.php",
                "<?php namespace App; (new Child())->shared();",
            ),
        ])
        .unwrap();

    let use_document = document(&batch, "/use.php");
    assert!(
        use_document
            .calls
            .iter()
            .any(|call| call.resolved_symbol.as_deref() == Some("App\\Base::shared"))
    );
    assert!(use_document.diagnostics.is_empty());
}

#[test]
fn a_trait_member_wins_over_the_same_name_in_a_parent() {
    let batch = SemanticExporter::new("/workspace")
        .export([
            source(
                "file:///workspace/Base.php",
                "<?php namespace App; class Base { public function run(): void {} }",
            ),
            source(
                "file:///workspace/Runner.php",
                "<?php namespace App; trait Runner { public function run(): void {} }",
            ),
            source(
                "file:///workspace/Child.php",
                "<?php namespace App; class Child extends Base { use Runner; }",
            ),
            source(
                "file:///workspace/use.php",
                "<?php namespace App; (new Child())->run();",
            ),
        ])
        .unwrap();

    assert!(
        document(&batch, "/use.php")
            .calls
            .iter()
            .any(|call| call.resolved_symbol.as_deref() == Some("App\\Runner::run"))
    );
}

#[test]
fn malformed_document_is_reported_without_stopping_the_batch() {
    let batch = SemanticExporter::new("/workspace")
        .export([
            source("file:///workspace/broken.php", "<?php class Broken {"),
            source("file:///workspace/good.php", "<?php class Good {}"),
        ])
        .unwrap();

    assert_eq!(batch.documents.len(), 2);
    assert!(
        batch.documents[0]
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == ExportDiagnosticKind::ParseError)
    );
    assert!(
        batch.documents[1]
            .declarations
            .iter()
            .any(|declaration| declaration.symbol == "Good")
    );
}

#[test]
fn batch_and_streaming_outputs_are_identical_and_sorted() {
    let exporter = SemanticExporter::new("/workspace");
    let sources = vec![
        source("file:///workspace/z.php", "<?php function zed() {}"),
        source("file:///workspace/a.php", "<?php function alpha() {}"),
    ];
    let batch = exporter.export(sources.clone()).unwrap();
    let mut streamed = Vec::new();
    exporter
        .export_stream(sources, |document| streamed.push(document))
        .unwrap();

    assert_eq!(batch.documents, streamed);
    assert_eq!(batch.documents[0].uri, "file:///workspace/a.php");
}

#[test]
fn repeated_exports_are_deterministic() {
    let exporter = SemanticExporter::new("/workspace");
    let sources = vec![
        source(
            "file:///workspace/a.php",
            "<?php namespace App; class A { public function go(): void {} }",
        ),
        source(
            "file:///workspace/b.php",
            "<?php namespace App; (new A())->go();",
        ),
    ];

    assert_eq!(
        exporter.export(sources.clone()).unwrap(),
        exporter.export(sources).unwrap()
    );
}

#[test]
fn duplicate_uris_are_rejected_before_streaming() {
    let exporter = SemanticExporter::new("/workspace");
    let sources = [
        source("file:///workspace/a.php", "<?php class First {}"),
        source("file:///workspace/a.php", "<?php class Second {}"),
    ];
    let mut streamed = Vec::new();

    let error = exporter
        .export_stream(sources, |document| streamed.push(document))
        .unwrap_err();

    assert_eq!(
        error,
        SemanticExportError::DuplicateUri("file:///workspace/a.php".to_string())
    );
    assert!(streamed.is_empty());
}

#[test]
fn named_argument_ranges_cover_only_expressions() {
    let source_text = "<?php function use_it($first, $second) {} $value = 1; use_it(first: $value, second: 'x:y');";
    let batch = SemanticExporter::new("/workspace")
        .export([source("file:///workspace/calls.php", source_text)])
        .unwrap();
    let call = batch.documents[0]
        .calls
        .iter()
        .find(|call| call.expression == "use_it")
        .unwrap();
    let argument_texts: Vec<_> = call
        .arguments
        .iter()
        .map(|range| &source_text[range.start as usize..range.end as usize])
        .collect();

    assert_eq!(argument_texts, ["$value", "'x:y'"]);
}

#[test]
fn duplicate_global_constants_are_exported_from_each_document() {
    let batch = SemanticExporter::new("/workspace")
        .export([
            source(
                "file:///workspace/a.php",
                "<?php namespace App; const SHARED = 1;",
            ),
            source("file:///workspace/b.php", "<?php define('SHARED', 2);"),
        ])
        .unwrap();

    assert!(batch.documents[0].declarations.iter().any(|declaration| {
        declaration.kind == DeclarationKind::Constant
            && declaration.name == "SHARED"
            && declaration.symbol == "App\\SHARED"
    }));
    assert!(batch.documents[1].declarations.iter().any(|declaration| {
        declaration.kind == DeclarationKind::Constant
            && declaration.name == "SHARED"
            && declaration.symbol == "SHARED"
    }));
}

#[test]
fn define_inside_namespace_stays_global() {
    let batch = SemanticExporter::new("/workspace")
        .export([source(
            "file:///workspace/constants.php",
            "<?php namespace App; define('GLOBAL_NAME', 1);",
        )])
        .unwrap();

    assert!(batch.documents[0].declarations.iter().any(|declaration| {
        declaration.kind == DeclarationKind::Constant && declaration.symbol == "GLOBAL_NAME"
    }));
    assert!(batch.documents[0].occurrences.iter().any(|occurrence| {
        occurrence.kind == OccurrenceKind::Constant
            && occurrence.is_definition
            && occurrence.resolved_symbol.as_deref() == Some("GLOBAL_NAME")
    }));
}
