use super::{Cause, DicomwebFrameClient, DicomwebTileSource, Error, Kind};
use crate::net::dicomweb::{
    CauseClass, Client, ClientError, FramePart, InstanceRef, RetrieveOptions,
};
use crate::net::ul::Ctx;
use crate::pixel::Metadata;
use std::collections::HashMap;
pub struct DicomwebClientFrames<'a> {
    pub client: &'a Client,
    pub ctx: Ctx,
}
fn canceled(err: &ClientError) -> bool {
    match err {
        ClientError::Web(e) => e
            .err
            .as_ref()
            .is_some_and(|c| c.class == CauseClass::Canceled),
        ClientError::BulkDataTooLarge(Some(inner)) => canceled(inner),
        ClientError::Joined(errors) => errors.iter().any(canceled),
        _ => false,
    }
}
impl DicomwebFrameClient for DicomwebClientFrames<'_> {
    fn retrieve_frames(
        &self,
        instance: &InstanceRef,
        frames: &[i64],
        options: &RetrieveOptions,
    ) -> Result<Vec<FramePart>, Error> {
        self.client
            .retrieve_frames(&self.ctx, instance, frames, options)
            .map_err(|e| {
                let cause = canceled(&e).then_some(Cause::Canceled);
                Error::with_cause(Kind::Other, cause, e.to_string())
            })
    }
}
impl<'a> DicomwebTileSource<DicomwebClientFrames<'a>> {
    #[doc(hidden)]
    pub fn over_client(
        client: &'a Client,
        ctx: Ctx,
        metadata: HashMap<String, Metadata>,
        options: RetrieveOptions,
    ) -> Self {
        DicomwebTileSource {
            client: Some(DicomwebClientFrames { client, ctx }),
            metadata,
            options,
        }
    }
}
