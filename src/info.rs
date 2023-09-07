use std::sync::Arc;

use scraper::{Html, Selector};

use crate::constants::BASE_URL;
use crate::info_extras::get_media;
use crate::stream::{Stream, StreamOptions};
use crate::structs::{VideoError, VideoInfo, VideoOptions};

use crate::utils::{
    choose_format, clean_video_details, get_functions, get_html, get_html5player,
    get_video_id, is_play_error, is_private_video,
    is_rental, parse_video_formats, sort_formats,
};

#[derive(Clone, derive_more::Display, derivative::Derivative)]
#[display(fmt = "Video({video_id})")]
#[derivative(Debug, PartialEq, Eq)]
pub struct Video {
    video_id: String,
    options: VideoOptions,
    #[derivative(PartialEq = "ignore")]
    client: reqwest_middleware::ClientWithMiddleware,
}

impl Video {
    /// Crate [`Video`] struct to get info or download with default [`VideoOptions`]
    pub fn new(url_or_id: impl Into<String>) -> Result<Self, VideoError> {
        let id = get_video_id(&url_or_id.into());

        if id.is_none() {
            return Err(VideoError::VideoNotFound);
        }

        let client = reqwest::Client::builder()
            .build()
            .map_err(VideoError::Reqwest)?;

        let retry_policy = reqwest_retry::policies::ExponentialBackoff::builder()
            .retry_bounds(
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(10000),
            )
            .build_with_max_retries(3);
        let client = reqwest_middleware::ClientBuilder::new(client)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                retry_policy,
            ))
            .build();

        Ok(Self {
            video_id: id.unwrap(),
            options: VideoOptions::default(),
            client,
        })
    }

    /// Crate [`Video`] struct to get info or download with custom [`VideoOptions`]
    pub fn new_with_options(
        url_or_id: impl Into<String>,
        options: VideoOptions,
    ) -> Result<Self, VideoError> {
        let id = get_video_id(&url_or_id.into());

        if id.is_none() {
            return Err(VideoError::VideoNotFound);
        }

        let mut client = reqwest::Client::builder();

        if options.request_options.proxy.is_some() {
            client = client.proxy(options.request_options.proxy.as_ref().unwrap().clone());
        }

        if options.request_options.cookies.is_some() {
            let cookie = options.request_options.cookies.as_ref().unwrap();
            let host = "https://youtube.com".parse::<url::Url>().unwrap();

            let jar = reqwest::cookie::Jar::default();
            jar.add_cookie_str(cookie.as_str(), &host);

            client = client.cookie_provider(Arc::new(jar));
        }

        let client = client.build().map_err(VideoError::Reqwest)?;

        let retry_policy = reqwest_retry::policies::ExponentialBackoff::builder()
            .retry_bounds(
                std::time::Duration::from_millis(500),
                std::time::Duration::from_millis(10000),
            )
            .build_with_max_retries(3);
        let client = reqwest_middleware::ClientBuilder::new(client)
            .with(reqwest_retry::RetryTransientMiddleware::new_with_policy(
                retry_policy,
            ))
            .build();

        Ok(Self {
            video_id: id.unwrap(),
            options,
            client,
        })
    }

    /// Try to get basic information about video
    /// - `HLS` and `DashMPD` formats excluded!
    pub async fn get_basic_info(&self) -> Result<VideoInfo, VideoError> {
        let client = &self.client;

        let url_parsed =
            url::Url::parse_with_params(self.get_video_url().as_str(), &[("hl", "en")]);
        if url_parsed.is_err() {
            return Err(VideoError::URLParseError(url_parsed.err().unwrap()));
        }

        let response = get_html(client, url_parsed.unwrap().as_str(), None).await?;

        let (player_response, initial_response): (serde_json::Value, serde_json::Value) = {
            let document = Html::parse_document(&response);
            let scripts_selector = Selector::parse("script").unwrap();
            let mut player_response_string = document
                .select(&scripts_selector)
                .filter(|x| x.inner_html().contains("var ytInitialPlayerResponse ="))
                .map(|x| x.inner_html().replace("var ytInitialPlayerResponse =", ""))
                .next()
                .unwrap_or(String::from(""))
                .trim()
                .to_string();
            let mut initial_response_string = document
                .select(&scripts_selector)
                .filter(|x| x.inner_html().contains("var ytInitialData ="))
                .map(|x| x.inner_html().replace("var ytInitialData =", ""))
                .next()
                .unwrap_or(String::from(""))
                .trim()
                .to_string();

            // remove json objects' last element (;)
            player_response_string.pop();
            initial_response_string.pop();

            let player_response: serde_json::Value =
                serde_json::from_str(&player_response_string).unwrap();
            let initial_response: serde_json::Value =
                serde_json::from_str(&initial_response_string).unwrap();

            (player_response, initial_response)
        };

        if is_play_error(&player_response, ["ERROR"].to_vec()) {
            return Err(VideoError::VideoNotFound);
        }

        if is_private_video(&player_response) {
            return Err(VideoError::VideoIsPrivate);
        }

        if player_response.get("streamingData").is_none()
            || is_rental(&player_response)
        {
            return Err(VideoError::VideoSourceNotFound);
        }

        let video_details = clean_video_details(
            &initial_response,
            &player_response,
            get_media(&initial_response).unwrap(),
            self.video_id.clone(),
        );

        Ok(VideoInfo {
            formats: parse_video_formats(
                &player_response,
                get_functions(get_html5player(response.as_str()).unwrap(), client).await?,
            )
            .unwrap_or(vec![]),
            video_details,
        })
    }

    /// Try to get full information about video
    /// - `HLS` and `DashMPD` formats included!
    pub async fn get_info(&self) -> Result<VideoInfo, VideoError> {
        let mut info = self.get_basic_info().await?;

        // Last sort formats
        info.formats.sort_by(sort_formats);
        Ok(info)
    }

    /// Try to turn [`Stream`] implemented [`LiveStream`] or [`NonLiveStream`] depend on the video.
    /// If function successfully return can download video chunk by chunk
    /// # Example
    /// ```ignore
    ///     let video_url = "https://www.youtube.com/watch?v=FZ8BxMU3BYc";
    ///
    ///     let video = Video::new(video_url).unwrap();
    ///
    ///     let stream = video.stream().await.unwrap();
    ///
    ///     while let Some(chunk) = stream.chunk().await.unwrap() {
    ///           println!("{:#?}", chunk);
    ///     }
    /// ```
    pub async fn stream(&self) -> Result<Stream, VideoError> {
        let client = &self.client;

        let info = self.get_info().await?;
        let format = choose_format(&info.formats, &self.options)
            .map_err(|_op| VideoError::VideoSourceNotFound)?;

        let link = format.url;

        if link.is_empty() {
            return Err(VideoError::VideoSourceNotFound);
        }

        let dl_chunk_size = if self.options.download_options.dl_chunk_size.is_some() {
            self.options.download_options.dl_chunk_size.unwrap()
        } else {
            1024 * 1024 * 10_u64 // -> Default is 10MB to avoid Youtube throttle (Bigger than this value can be throttle by Youtube)
        };

        let start = 0;
        let end = start + dl_chunk_size;

        let mut content_length = format
            .content_length
            .unwrap_or("0".to_string())
            .parse::<u64>()
            .unwrap_or(0);

        // Get content length from source url if content_length is 0
        if content_length == 0 {
            let content_length_response = client
                .get(&link)
                .send()
                .await
                .map_err(VideoError::ReqwestMiddleware)?
                .content_length();

            if content_length_response.is_none() {
                return Err(VideoError::VideoNotFound);
            }

            content_length = content_length_response.unwrap();
        }

        let stream = Stream::new(StreamOptions {
            client: Some(client.clone()),
            link,
            content_length,
            dl_chunk_size,
            start,
            end,
        });

        if stream.is_err() {
            return Err(stream.err().unwrap());
        }

        Ok(stream.unwrap())
    }

    /// Get video URL
    pub fn get_video_url(&self) -> String {
        format!("{}{}", BASE_URL, &self.video_id)
    }

    /// Get video id
    pub fn get_video_id(&self) -> String {
        self.video_id.clone()
    }

    pub(crate) fn get_client(&self) -> &reqwest_middleware::ClientWithMiddleware {
        &self.client
    }

    pub(crate) fn get_options(&self) -> VideoOptions {
        self.options.clone()
    }
}

