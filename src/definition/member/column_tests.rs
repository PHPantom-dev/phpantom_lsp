use crate::Backend;

#[test]
fn missing_column_declarations_terminate_across_shared_and_unresolved_traits() {
    let backend = Backend::new_test();
    let source = r#"<?php
namespace App;
trait SharedColumns {}
trait First { use SharedColumns; }
trait Second { use SharedColumns; }
class Base {}
class Customer extends Base { use First, Second, Unavailable; }
"#;
    backend.update_ast("file:///app/Models.php", source);
    let model = backend.find_or_load_class("App\\Customer").unwrap();
    assert!(
        backend
            .model_column_declaration_location(&model, "email")
            .is_none()
    );
}

#[test]
fn missing_column_declarations_terminate_on_cyclic_inheritance() {
    let backend = Backend::new_test();
    backend.update_ast(
        "file:///app/Models.php",
        "<?php class First extends Second {} class Second extends First {}",
    );
    let model = backend.find_or_load_class("First").unwrap();
    assert!(
        backend
            .model_column_declaration_location(&model, "email")
            .is_none()
    );
}
