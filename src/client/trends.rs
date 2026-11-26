use std::{borrow::Cow, collections::HashSet};

use crate::{
    error::{Error, ExtractionError},
    model::VideoItem,
    param::TrendingCategory,
    serializer::MapResult,
};

use super::{response, ClientType, MapRespCtx, MapResponse, QBrowseParams, RustyPipeQuery};

impl RustyPipeQuery {
    /// Get trending videos of the default category ([`TrendingCategory::Podcasts`])
    /// for the query's country
    #[tracing::instrument(skip(self), level = "error")]
    pub async fn trending(&self) -> Result<Vec<VideoItem>, Error> {
        self.trending_category(TrendingCategory::default()).await
    }

    /// Get trending videos of the given category for the query's country
    #[tracing::instrument(skip(self), level = "error")]
    pub async fn trending_category(
        &self,
        category: TrendingCategory,
    ) -> Result<Vec<VideoItem>, Error> {
        let (browse_id, params) = category.browse_params();
        let request_body = QBrowseParams { browse_id, params };

        self.execute_request::<response::Trending, _, _>(
            ClientType::Desktop,
            "trends",
            browse_id,
            "browse",
            &request_body,
        )
        .await
    }
}

impl MapResponse<Vec<VideoItem>> for response::Trending {
    fn map_response(
        self,
        ctx: &MapRespCtx<'_>,
    ) -> Result<MapResult<Vec<VideoItem>>, ExtractionError> {
        let items = self
            .contents
            .two_column_browse_results_renderer
            .contents
            .into_iter()
            .next()
            .ok_or(ExtractionError::InvalidData(Cow::Borrowed("no contents")))?
            .tab_renderer
            .content
            .section_list_renderer
            .contents;

        let mut mapper = response::YouTubeListMapper::<VideoItem>::new(ctx.lang);
        mapper.map_response(items);

        // News shelves may list the same video more than once
        let mut seen = HashSet::new();
        let mut videos = mapper.items;
        videos.retain(|v| seen.insert(v.id.clone()));

        Ok(MapResult {
            c: videos,
            warnings: mapper.warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs::File, io::BufReader};

    use path_macro::path;
    use rstest::rstest;

    use crate::{
        client::{response, MapRespCtx, MapResponse},
        model::VideoItem,
        serializer::MapResult,
        util::tests::TESTFILES,
    };

    #[rstest]
    #[case::podcasts("podcasts")]
    #[case::gaming("gaming")]
    #[case::news("news")]
    fn map_trending(#[case] name: &str) {
        let json_path = path!(*TESTFILES / "trends" / format!("trending_{name}.json"));
        let json_file = File::open(json_path).unwrap();

        let trending: response::Trending =
            serde_json::from_reader(BufReader::new(json_file)).unwrap();
        let map_res: MapResult<Vec<VideoItem>> =
            trending.map_response(&MapRespCtx::test("")).unwrap();

        assert!(
            map_res.warnings.is_empty(),
            "deserialization/mapping warnings: {:?}",
            map_res.warnings
        );
        assert!(map_res.c.len() >= 15, "got {} items", map_res.c.len());
        let ids = map_res.c.iter().map(|v| &v.id).collect::<HashSet<_>>();
        assert_eq!(ids.len(), map_res.c.len(), "duplicate videos");

        insta::assert_ron_snapshot!(format!("map_trending_{name}"), map_res.c, {
            "[].publish_date" => "[date]",
        });
    }
}
