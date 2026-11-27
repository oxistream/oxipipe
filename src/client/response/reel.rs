use serde::Deserialize;
use serde_with::{serde_as, DefaultOnError, VecSkipError};

use super::{video_details::StructuredDescriptionContent, ContinuationEndpoint, Thumbnails};
use crate::serializer::{text::AttributedText, MapResult};

/// Response of `reel/reel_watch_sequence`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelSequence {
    #[serde(default)]
    pub entries: MapResult<Vec<ReelSequenceEntry>>,
    pub continuation_endpoint: Option<ContinuationEndpoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelSequenceEntry {
    pub command: ReelSequenceCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelSequenceCommand {
    pub reel_watch_endpoint: ReelWatchEndpoint,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelWatchEndpoint {
    pub video_id: String,
    #[serde(default)]
    pub thumbnail: Thumbnails,
    /// Protobuf with the channel ID in field 23
    pub params: Option<String>,
}

/// Response of `reel/reel_item_watch`
#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelItem {
    pub overlay: Option<ReelItemOverlay>,
    #[serde(default)]
    #[serde_as(as = "VecSkipError<_>")]
    pub engagement_panels: Vec<ReelEngagementPanel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelEngagementPanel {
    pub engagement_panel_section_list_renderer: ReelEngagementPanelRenderer,
}

/// Only the structured description is used, other panels have no parseable content
#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelEngagementPanelRenderer {
    #[serde(default)]
    #[serde_as(as = "DefaultOnError")]
    pub content: Option<StructuredDescriptionContent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelItemOverlay {
    pub reel_player_overlay_renderer: ReelPlayerOverlayRenderer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelPlayerOverlayRenderer {
    pub player_overlay: Option<PlayerOverlay>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerOverlay {
    pub reel_player_overlay_view_model: ReelPlayerOverlayViewModel,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelPlayerOverlayViewModel {
    pub metapanel: Option<Metapanel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Metapanel {
    pub reel_metapanel_view_model: ReelMetapanelViewModel,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelMetapanelViewModel {
    #[serde(default)]
    #[serde_as(as = "VecSkipError<_>")]
    pub metadata_items: Vec<MetadataItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MetadataItem {
    ShortsVideoTitleViewModel(ShortsVideoTitleViewModel),
    ReelCarouselViewModel(ReelCarouselViewModel),
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShortsVideoTitleViewModel {
    #[serde_as(as = "AttributedText")]
    pub text: String,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelCarouselViewModel {
    #[serde(default)]
    #[serde_as(as = "VecSkipError<_>")]
    pub button_view_models: Vec<ReelCarouselButton>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelCarouselButton {
    pub reel_carousel_button_view_model: ReelCarouselButtonViewModel,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReelCarouselButtonViewModel {
    pub button_view_model: CarouselButtonWrap,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CarouselButtonWrap {
    pub button_view_model: CarouselButton,
}

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CarouselButton {
    #[serde_as(as = "DefaultOnError")]
    #[serde(default)]
    pub on_tap: Option<CarouselButtonTap>,
    #[serde_as(as = "Option<AttributedText>")]
    #[serde(default)]
    pub title_formatted: Option<String>,
    pub accessibility_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CarouselButtonTap {
    pub innertube_command: WatchCommand,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WatchCommand {
    pub watch_endpoint: WatchEndpoint,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WatchEndpoint {
    pub video_id: String,
}
