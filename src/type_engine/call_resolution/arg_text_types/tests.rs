use crate::Backend;
use crate::class_lookup::Fqn;
use crate::php_type::PhpType;
use crate::test_fixtures::make_backend;
use crate::type_engine::resolver::{FileCtx, ResolutionCtx};

#[test]
fn constant_argument_lookup_reads_imports_without_linking_classes() {
    let backend = make_backend();
    let dir = tempfile::tempdir().unwrap();
    let uri = crate::util::path_to_uri(&dir.path().join("constants.php"));
    let source = r"<?php
namespace { const ROUTE = '/global'; ROUTE; /* global */ }
namespace Provider { const PATH = '/books'; }
namespace Caller {
    use const Provider\PATH as ROUTE;
    class Unrelated {}
    ROUTE; /* imported */
}
namespace Local { const ROUTE = '/local'; ROUTE; /* local */ }
";
    backend.update_ast(&uri, source);
    let classes = backend.symbols.uri_classes_index.write();
    let worker_backend = backend.clone_for_blocking();
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let class_loader = |_: Fqn<'_>| None;
        let source_loader = |_: &str| None;
        let file = FileCtx::new(source, &[], &class_loader, &source_loader)
            .with_backend(Some(&worker_backend))
            .with_uri(Some(&uri));
        let types: Vec<_> = ["global", "imported", "local"]
            .into_iter()
            .map(|label| {
                let offset = source.find(&format!("ROUTE; /* {label} */")).unwrap() as u32;
                let context = ResolutionCtx::new(file, None, offset);
                Backend::resolve_arg_text_to_type("ROUTE", &context)
            })
            .collect();
        send.send(types).unwrap();
    });
    let types = receive.recv_timeout(std::time::Duration::from_secs(2));
    drop(classes);
    worker.join().unwrap();
    assert_eq!(
        types.expect("constant lookup must not wait for class linking"),
        ["'/global'", "'/books'", "'/local'"].map(|value| Some(PhpType::parse(value))),
    );
}
