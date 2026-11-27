//! # Query parameters
//!
//! This module contains structs and enums used as input parameters
//! for the functions in RustyPipe.

mod locale;
mod stream_filter;

pub mod search_filter;

pub use locale::{Country, Language, COUNTRIES, LANGUAGES};
pub(crate) use stream_filter::cmp_bitrate;
pub use stream_filter::StreamFilter;

/// Category of trending videos
///
/// YouTube removed its general trending page in 2025. These are the
/// country-specific destination pages that replaced it.
///
/// Not included because they are unavailable without signing in or hold no
/// videos: Shopping and Courses (redirect to the start page), Movies & TV
/// (paid titles only) and Playables (games).
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TrendingCategory {
    /// Trending podcast episodes (default, the broadest mix of topics)
    #[default]
    Podcasts,
    /// Trending gaming videos
    Gaming,
    /// Top news stories
    News,
    /// Music videos and live performances
    Music,
    /// Sports highlights and live games
    Sports,
    /// Educational videos
    Learning,
    /// Fashion and beauty videos
    Fashion,
    /// Current and recent livestreams
    Live,
}

impl TrendingCategory {
    /// Browse ID and params of the destination page
    pub(crate) const fn browse_params(self) -> (&'static str, Option<&'static str>) {
        match self {
            TrendingCategory::Podcasts => ("FEpodcasts_destination", Some("qgcCCAM%3D")),
            TrendingCategory::Gaming => ("UCOpNcN46UbXVtpKMrmU4Abg", Some("Egh0cmVuZGluZw%3D%3D")),
            TrendingCategory::News => ("UCYfdidRxbB8Qhf0Nx7ioOYw", Some("EgDSBwIIAQ%3D%3D")),
            TrendingCategory::Music => ("UC-9-kyTW8ZkZNDHQJ6FgpwQ", None),
            TrendingCategory::Sports => ("UCEgdi0XIXXZ-qJOFPf4JSKw", None),
            TrendingCategory::Learning => ("UCtFRv9O2AHqOZjjynzrv-xg", None),
            TrendingCategory::Fashion => ("UCrpQ4p1Ql_hG8rKXIKM1MOQ", None),
            TrendingCategory::Live => ("UC4R8DWoMoI7CAwX8_LjQHig", None),
        }
    }
}

/// Channel video tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelVideoTab {
    /// Regular videos
    Videos,
    /// Short videos
    Shorts,
    /// Livestreams
    Live,
}

/// Sort order for channel videos
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelOrder {
    /// Order videos with the latest upload date first (default)
    #[default]
    Latest, // videos/shorts 3=4; live 3=12
    /// Order videos with the highest number of views first
    Popular, // videos/shorts 3=2; live 3=14
    /// Order videos with the earliest upload date first
    Oldest, // videos/shorts 3=5; live 3=13
}

impl ChannelVideoTab {
    /// Get the tab ID used to create ordered continuation tokens
    pub(crate) const fn order_ctoken_id(self) -> u32 {
        match self {
            ChannelVideoTab::Videos => 15,
            ChannelVideoTab::Shorts => 10,
            ChannelVideoTab::Live => 14,
        }
    }
}

/// Sort order for YTM artist albums
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumOrder {
    /// Sort albums by release date
    Recency = 1,
    /// Sort albums by popularity
    Popularity = 2,
    /// Sort albums by their name
    Alphabetical = 3,
}

/// Filter for YTM artist albums
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumFilter {
    /// Only show albums
    Albums = 1,
    /// Only show singles
    Singles = 2,
}
