//! Rename of a class import's alias, which stays inside the file.

use crate::common::{
    apply_edits, create_test_backend, edits_for_uri, open_php, prepare_rename, rename,
    rename_result,
};
use tower_lsp_server::gen_lsp_types::*;

const USAGE: &str = concat!(
    "<?php\n",
    "namespace App;\n",
    "\n",
    "use Vendor\\Pkg\\Thing as Widget;\n",
    "\n",
    "/** @param Widget $w */\n",
    "function make(Widget $w): Widget {\n",
    "    return new Widget();\n",
    "}\n",
);

const RENAMED: &str = concat!(
    "<?php\n",
    "namespace App;\n",
    "\n",
    "use Vendor\\Pkg\\Thing as Gadget;\n",
    "\n",
    "/** @param Gadget $w */\n",
    "function make(Gadget $w): Gadget {\n",
    "    return new Gadget();\n",
    "}\n",
);

async fn renamed(text: &str, line: u32, character: u32, new_name: &str) -> String {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///usage.php").unwrap();
    open_php(&backend, &uri, text).await;
    let edit = rename(&backend, &uri, line, character, new_name)
        .await
        .expect("expected a rename");
    apply_edits(text, &edits_for_uri(&edit, &uri))
}

#[tokio::test]
async fn renames_from_the_as_clause() {
    // Cursor on `Widget` in `use Vendor\Pkg\Thing as Widget;`.
    assert_eq!(renamed(USAGE, 3, 28, "Gadget").await, RENAMED);
}

#[tokio::test]
async fn renames_from_a_name_written_through_the_alias() {
    // Cursor on `Widget` in `new Widget()`.
    assert_eq!(renamed(USAGE, 7, 16, "Gadget").await, RENAMED);
}

#[tokio::test]
async fn prepare_offers_only_the_alias() {
    let backend = create_test_backend();
    let uri = Uri::parse("file:///usage.php").unwrap();
    open_php(&backend, &uri, USAGE).await;
    for (line, character) in [(3, 28), (6, 15)] {
        let Some(PrepareRenameResult::PrepareRenamePlaceholder(result)) =
            prepare_rename(&backend, &uri, line, character).await
        else {
            panic!("expected a placeholder at {line}:{character}");
        };
        assert_eq!(result.placeholder, "Widget");
        let start = result.range.start.character as usize;
        let end = result.range.end.character as usize;
        let text = USAGE.lines().nth(result.range.start.line as usize).unwrap();
        assert_eq!(&text[start..end], "Widget");
    }
}

#[tokio::test]
async fn leaves_the_class_and_other_files_alone() {
    let backend = create_test_backend();
    let decl_uri = Uri::parse("file:///Thing.php").unwrap();
    let decl = "<?php\nnamespace Vendor\\Pkg;\n\nclass Thing {}\n";
    let uri = Uri::parse("file:///usage.php").unwrap();
    open_php(&backend, &decl_uri, decl).await;
    open_php(&backend, &uri, USAGE).await;

    let edit = rename(&backend, &uri, 7, 16, "Gadget").await.unwrap();
    assert!(edits_for_uri(&edit, &decl_uri).is_empty());
    assert_eq!(apply_edits(USAGE, &edits_for_uri(&edit, &uri)), RENAMED);
}

#[tokio::test]
async fn renaming_to_the_class_name_drops_the_alias() {
    let result = renamed(USAGE, 3, 28, "Thing").await;
    assert!(result.contains("use Vendor\\Pkg\\Thing;\n"), "{result}");
    assert!(result.contains("return new Thing();"), "{result}");
}

#[tokio::test]
async fn renames_the_namespace_alias_of_a_qualified_name() {
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Models as M;\n",
        "\n",
        "new M\\User();\n",
    );
    let result = renamed(text, 5, 4, "Models").await;
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Vendor\\Models;\n",
            "\n",
            "new Models\\User();\n",
        )
    );
}

#[tokio::test]
async fn refuses_a_name_the_file_already_binds() {
    let text = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Other\\Gadget;\n",
        "use Vendor\\Pkg\\Thing as Widget;\n",
        "\n",
        "new Widget();\n",
        "new Gadget();\n",
    );
    let backend = create_test_backend();
    let uri = Uri::parse("file:///usage.php").unwrap();
    open_php(&backend, &uri, text).await;
    let refusal = rename_result(&backend, &uri, 6, 5, "Gadget").await;
    assert!(refusal.is_err(), "expected a refusal, got {refusal:?}");
    let refusal = rename_result(&backend, &uri, 6, 5, "int").await;
    assert!(refusal.is_err(), "expected a refusal, got {refusal:?}");
}

#[tokio::test]
async fn only_renames_inside_the_namespace_block() {
    let text = concat!(
        "<?php\n",
        "namespace App {\n",
        "    use Vendor\\Pkg\\Thing as Widget;\n",
        "    new Widget();\n",
        "}\n",
        "namespace Other {\n",
        "    new Widget();\n",
        "}\n",
    );
    let result = renamed(text, 3, 9, "Gadget").await;
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App {\n",
            "    use Vendor\\Pkg\\Thing as Gadget;\n",
            "    new Gadget();\n",
            "}\n",
            "namespace Other {\n",
            "    new Widget();\n",
            "}\n",
        )
    );
}
