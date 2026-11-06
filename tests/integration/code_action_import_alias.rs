//! Integration tests for the "Alias import" and "Remove alias" code
//! actions on a `use` statement.

use crate::common::{
    apply_workspace_edit, create_test_backend, get_code_actions_in_range, position_of,
};
use tower_lsp_server::gen_lsp_types::*;

fn alias_action(content: &str, needle: &str) -> Option<CodeAction> {
    let backend = create_test_backend();
    let uri = "file:///test.php";
    backend.update_ast(uri, content);
    let pos = position_of(content, needle);
    get_code_actions_in_range(&backend, uri, content, Range::new(pos, pos))
        .into_iter()
        .find_map(|action| match action {
            CodeActionResponse::CodeAction(action)
                if action.title.starts_with("Alias `")
                    || action.title.starts_with("Remove alias `") =>
            {
                Some(action)
            }
            _ => None,
        })
}

fn apply(content: &str, needle: &str) -> (String, String) {
    let action = alias_action(content, needle).expect("expected an alias action");
    let result = apply_workspace_edit(content, action.edit.as_ref().unwrap());
    (action.title, result)
}

#[test]
fn aliases_an_import_and_every_reference() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Illuminate\\Database\\Eloquent\\Collection;\n",
        "\n",
        "class Repo {\n",
        "    /** @param Collection<int, string> $items */\n",
        "    public function take(Collection $items): Collection {\n",
        "        if ($items instanceof Collection) {}\n",
        "        $class = Collection::class;\n",
        "        Collection::make();\n",
        "        return new Collection();\n",
        "    }\n",
        "}\n",
    );
    let (title, result) = apply(src, "Eloquent\\Collection;");
    assert_eq!(title, "Alias `Collection` as `EloquentCollection`");
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Illuminate\\Database\\Eloquent\\Collection as EloquentCollection;\n",
            "\n",
            "class Repo {\n",
            "    /** @param EloquentCollection<int, string> $items */\n",
            "    public function take(EloquentCollection $items): EloquentCollection {\n",
            "        if ($items instanceof EloquentCollection) {}\n",
            "        $class = EloquentCollection::class;\n",
            "        EloquentCollection::make();\n",
            "        return new EloquentCollection();\n",
            "    }\n",
            "}\n",
        )
    );
}

#[test]
fn offered_from_the_use_keyword() {
    let src = "<?php\nnamespace App;\n\nuse Vendor\\Pkg\\Thing;\n\nnew Thing();\n";
    let (title, _) = apply(src, "use Vendor");
    assert_eq!(title, "Alias `Thing` as `PkgThing`");
}

#[test]
fn rewrites_names_qualified_through_a_namespace_import() {
    let src = "<?php\nnamespace App;\n\nuse Vendor\\Models;\n\nnew Models\\User();\n";
    let (_, result) = apply(src, "Models;");
    assert_eq!(
        result,
        "<?php\nnamespace App;\n\nuse Vendor\\Models as VendorModels;\n\nnew VendorModels\\User();\n"
    );
}

#[test]
fn leaves_fully_qualified_and_unrelated_names_alone() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\Thing;\n",
        "\n",
        "new Thing();\n",
        "new \\Thing();\n",
        "new Thingamajig();\n",
    );
    let (_, result) = apply(src, "Thing;");
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Vendor\\Pkg\\Thing as PkgThing;\n",
            "\n",
            "new PkgThing();\n",
            "new \\Thing();\n",
            "new Thingamajig();\n",
        )
    );
}

#[test]
fn alias_skips_a_name_the_file_already_uses() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\Thing;\n",
        "\n",
        "new Thing();\n",
        "new PkgThing();\n",
    );
    let (title, _) = apply(src, "Thing;");
    assert_eq!(title, "Alias `Thing` as `PkgThing2`");
}

#[test]
fn aliases_one_member_of_a_group_import() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\{Thing, Other};\n",
        "\n",
        "new Thing();\n",
        "new Other();\n",
    );
    let (title, result) = apply(src, "Other}");
    assert_eq!(title, "Alias `Other` as `PkgOther`");
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Vendor\\Pkg\\{Thing, Other as PkgOther};\n",
            "\n",
            "new Thing();\n",
            "new PkgOther();\n",
        )
    );
}

#[test]
fn removes_an_alias() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\Thing as Widget;\n",
        "\n",
        "/** @var Widget $w */\n",
        "$w = new Widget();\n",
        "Widget::make();\n",
    );
    let (title, result) = apply(src, "Widget;");
    assert_eq!(title, "Remove alias `Widget`");
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App;\n",
            "\n",
            "use Vendor\\Pkg\\Thing;\n",
            "\n",
            "/** @var Thing $w */\n",
            "$w = new Thing();\n",
            "Thing::make();\n",
        )
    );
}

#[test]
fn no_remove_alias_when_the_short_name_is_imported() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Other\\Thing;\n",
        "use Vendor\\Pkg\\Thing as Widget;\n",
        "\n",
        "new Widget();\n",
    );
    assert!(alias_action(src, "Widget;").is_none());
}

#[test]
fn no_remove_alias_when_the_short_name_is_declared_in_the_file() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\Thing as Widget;\n",
        "\n",
        "class Thing {}\n",
        "new Widget();\n",
    );
    assert!(alias_action(src, "Widget;").is_none());
}

#[test]
fn no_remove_alias_when_the_short_name_reaches_another_class() {
    let src = concat!(
        "<?php\n",
        "namespace App;\n",
        "\n",
        "use Vendor\\Pkg\\Thing as Widget;\n",
        "\n",
        "new Widget();\n",
        "new Thing();\n",
    );
    assert!(alias_action(src, "Widget;").is_none());
}

#[test]
fn only_rewrites_the_namespace_block_of_the_import() {
    let src = concat!(
        "<?php\n",
        "namespace App {\n",
        "    use Vendor\\Pkg\\Thing;\n",
        "    new Thing();\n",
        "}\n",
        "namespace Other {\n",
        "    new Thing();\n",
        "}\n",
    );
    let (_, result) = apply(src, "Thing;");
    assert_eq!(
        result,
        concat!(
            "<?php\n",
            "namespace App {\n",
            "    use Vendor\\Pkg\\Thing as PkgThing;\n",
            "    new PkgThing();\n",
            "}\n",
            "namespace Other {\n",
            "    new Thing();\n",
            "}\n",
        )
    );
}

#[test]
fn not_offered_for_function_imports_or_away_from_imports() {
    let src = "<?php\nnamespace App;\n\nuse function Vendor\\Pkg\\helper;\n\nhelper();\n";
    assert!(alias_action(src, "helper;").is_none());
    assert!(alias_action(src, "helper();").is_none());
}
