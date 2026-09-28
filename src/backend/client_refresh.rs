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

use std::sync::atomic::Ordering;

use crate::Backend;

impl Backend {
    /// Ask the editor to re-pull every code lens, if it said it can.
    pub(crate) async fn request_code_lens_refresh(&self) {
        if self.supports_code_lens_refresh.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.code_lens_refresh().await;
        }
    }

    /// Ask the editor to re-pull every inlay hint, if it said it can.
    pub(crate) async fn request_inlay_hint_refresh(&self) {
        if self.supports_inlay_hint_refresh.load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.inlay_hint_refresh().await;
        }
    }

    /// Ask the editor to re-pull every file's semantic tokens, if it said
    /// it can.
    pub(crate) async fn request_semantic_tokens_refresh(&self) {
        if self
            .supports_semantic_tokens_refresh
            .load(Ordering::Acquire)
            && let Some(client) = &self.client
        {
            let _ = client.semantic_tokens_refresh().await;
        }
    }
}
