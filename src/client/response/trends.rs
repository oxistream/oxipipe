use serde::Deserialize;
use serde_with::{serde_as, DefaultOnError};

use super::{video_item::YouTubeListRenderer, Tab, TwoColumnBrowseResults};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Trending {
    pub contents: Contents,
}

type Contents = TwoColumnBrowseResults<Tab<TrendingContent>>;

/// Some destination pages (e.g. music) send an empty section list next to the rich grid
#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrendingContent {
    #[serde_as(as = "DefaultOnError")]
    #[serde(default)]
    pub rich_grid_renderer: Option<YouTubeListRenderer>,
    #[serde_as(as = "DefaultOnError")]
    #[serde(default)]
    pub section_list_renderer: Option<YouTubeListRenderer>,
}
