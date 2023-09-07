pub use crate::stream::{LiveStreamOptions, NonLiveStreamOptions};

use crate::stream::{Stream as AsyncStream};
use crate::{block_async, VideoError};

pub struct Stream(AsyncStream);

impl Stream for NonLiveStream {
    pub fn new(options: StreamOptions) -> Result<Self, VideoError> {
        Ok(Self(AsyncStream::new(options)?))
    }
    fn chunk(&self) -> Result<Option<Vec<u8>>, VideoError> {
        Ok(block_async!(self.0.chunk())?)
    }

    fn content_length(&self) -> usize {
        self.0.content_length() as usize
    }
}

impl std::ops::Deref for Stream {
    type Target = AsyncNonLiveStream;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Stream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
