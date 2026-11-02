//! Asking the editor to re-pull a feature it caches: code lenses, inlay
//! hints, and semantic tokens.  Each is a server-to-client request that the
//! editor only accepts once it has advertised the matching capability, so
//! every caller used to spell the same flag check and client unwrap.
//!
//! Diagnostics have their own owner in
//! [`request_diagnostic_refresh`](crate::Backend::request_diagnostic_refresh),
//! which goes through a pump because it can be signalled from cancellable
//! request futures.  The three here are only awaited from spawned tasks and
//! notification handlers that live until the answer arrives.

use crate::Backend;

impl Backend {
    /// Ask the editor to re-pull every code lens, if it said it can.
    pub(crate) async fn request_code_lens_refresh(&self) {
        if self.caps().code_lens_refresh
            && let Some(client) = &self.client
        {
            let _ = client.code_lens_refresh().await;
        }
    }

    /// Ask the editor to re-pull every inlay hint, if it said it can.
    pub(crate) async fn request_inlay_hint_refresh(&self) {
        if self.caps().inlay_hint_refresh
            && let Some(client) = &self.client
        {
            let _ = client.inlay_hint_refresh().await;
        }
    }

    /// [`request_inlay_hint_refresh`](Self::request_inlay_hint_refresh)
    /// from synchronous code, such as a config reload: the request goes
    /// out on a spawned task rather than being awaited.
    pub(crate) fn request_inlay_hint_refresh_detached(&self) {
        if self.caps().inlay_hint_refresh
            && let Some(client) = &self.client
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let client = client.clone();
            runtime.spawn(async move {
                let _ = client.inlay_hint_refresh().await;
            });
        }
    }

    /// Ask the editor to re-pull every file's semantic tokens, if it said
    /// it can.
    pub(crate) async fn request_semantic_tokens_refresh(&self) {
        if self.caps().semantic_tokens_refresh
            && let Some(client) = &self.client
        {
            let _ = client.semantic_tokens_refresh().await;
        }
    }
}
