use super::*;
use crate::parser::with_parsed_program;

/// The offset of the closing brace of the declaration whose name token
/// starts where `needle` first occurs in `php`.
fn close_offset(php: &str, needle: &str) -> Option<u32> {
    let name_offset = php.find(needle).expect("needle in source") as u32;
    with_parsed_program(php, "out_param_test", |program, _| {
        body_close_offset(program, name_offset)
    })
}

#[test]
fn finds_the_closing_brace_of_a_namespaced_function() {
    let php = "<?php\nnamespace App;\nfunction helper(): int\n{\n    return 1;\n}\n";
    let offset = close_offset(php, "helper").expect("function found");
    assert_eq!(&php[offset as usize..offset as usize + 1], "}");
}

#[test]
fn finds_the_closing_brace_of_a_method_nested_in_a_namespace_block() {
    let php = "<?php\nnamespace App {\n    class Box\n    {\n        public function size(): int\n        {\n            return 1;\n        }\n    }\n}\n";
    let offset = close_offset(php, "size").expect("method found");
    assert_eq!(&php[offset as usize..offset as usize + 1], "}");
    // The method's brace, not the class's or the namespace block's.
    assert!(offset < php.rfind('}').unwrap() as u32);
}

#[test]
fn an_abstract_method_has_no_body_to_read() {
    let php = "<?php\nabstract class Box\n{\n    abstract public function size(): int;\n}\n";
    assert!(close_offset(php, "size").is_none());
}

#[test]
fn an_interface_method_has_no_body_to_read() {
    let php = "<?php\ninterface Sized\n{\n    public function size(): int;\n}\n";
    assert!(close_offset(php, "size").is_none());
}

/// A `name_offset` recorded against content that has since changed names
/// nothing in the file it is read back against. The walk must give up
/// rather than pick whichever declaration happens to come first.
#[test]
fn an_offset_that_names_nothing_finds_no_body() {
    let php = "<?php\nclass Box\n{\n    public function size(): int\n    {\n        return 1;\n    }\n}\n";
    assert!(
        with_parsed_program(php, "out_param_test", |program, _| body_close_offset(
            program,
            php.len() as u32 - 1
        ))
        .is_none()
    );
}
