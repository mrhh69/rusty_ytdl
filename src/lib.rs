#![recursion_limit = "256"]

mod info;
mod info_extras;
mod structs;
mod utils;

pub mod constants;

pub use info::Video;
pub use structs::{
    Author, Chapter, ColorInfo, DownloadOptions, Embed, RangeObject, RelatedVideo, RequestOptions,
    StoryBoard, Thumbnail, VideoDetails, VideoError, VideoFormat, VideoInfo, VideoOptions,
    VideoQuality, VideoSearchOptions,
};
pub use utils::{choose_format, get_video_id};
// export to access proxy feature
pub use reqwest;
