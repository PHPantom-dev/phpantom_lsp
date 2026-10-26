use crate::common::{completion_response, create_test_backend, open_php};
use tower_lsp_server::gen_lsp_types::*;

/// `$this->` inside a method of a class declared within a top-level `if`
/// version guard must resolve to the class's own members. Such classes are a
/// common polyfill/compat pattern (e.g. defining a class differently for
/// different runtime versions), and the enclosing-class detection must descend
/// into the conditional block to associate the cursor with the class.
#[tokio::test]
async fn test_completion_this_inside_conditional_class() {
    let backend = create_test_backend();

    let uri = Uri::parse("file:///conditional_class_this.php").unwrap();
    let text = concat!(
        "<?php\n",
        "if (\\PHP_VERSION_ID >= 80000) {\n",
        "    class Compat {\n",
        "        public string $label;\n",
        "        public function describe(): string { return ''; }\n",
        "        public function run() {\n",
        "            $this->\n",
        "        }\n",
        "    }\n",
        "}\n",
    );

    open_php(&backend, &uri, text).await;

    let result = completion_response(&backend, &uri, 6, 19, None).await;

    let items = match result.expect("should return completion results") {
        CompletionResponse::CompletionItemList(items) => items,
        CompletionResponse::CompletionList(list) => list.items,
    };

    let labels: Vec<&str> = items
        .iter()
        .filter_map(|i| i.filter_text.as_deref())
        .collect();

    assert!(
        labels.contains(&"describe"),
        "method of conditionally-defined class should complete, got {labels:?}",
    );
    assert!(
        labels.contains(&"label"),
        "property of conditionally-defined class should complete, got {labels:?}",
    );
}
