#[cfg(test)]
mod tests {
    use crate::common::{create_test_backend, goto_definition_at};
    use tower_lsp::LanguageServer;
    use tower_lsp::lsp_types::*;

    #[tokio::test]
    async fn test_blade_syntax_error_resilience() {
        let backend = create_test_backend();
        let blade_uri = Url::parse("file:///complex_view.blade.php").unwrap();
        let blade_text = r#"
@extends('layouts.admin')
@section('header')
    @include('partials.header')
@stop
@php ($field_blade = 'admin.page._preview_field')

@section('content')
    <div class="panel panel-default min-height">
        <div class="panel-heading col-md-12">
            <h3>Preview Import</h3>
            <div class="col-md-4">
                <span class="text-original">Normal</span> is original data<br>
                If changed, <span class="text-modified">Bold</span> is modified data
                <div class="h4">
                    <span class="text-success">Total {{ count($import_rows) }} records</span>
                    @if ($row_errors->isNotEmpty())
                        <span class="text-danger">Found {{ $row_errors->count() }} errors</span>
                    @endif
                    @if ($row_warnings->isNotEmpty())
                        <span class="text-warning">Found {{ $row_warnings->count() }} warnings</span>
                    @endif
                </div>
            </div>
        </div>
    </div>
@stop
"#;

        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: blade_uri.clone(),
                    language_id: "blade".to_string(),
                    version: 1,
                    text: blade_text.to_string(),
                },
            })
            .await;

        let result = goto_definition_at(&backend, &blade_uri, 5, 10).await;
        let _ = result;
    }
}
