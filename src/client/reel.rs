use std::fmt::Debug;

use serde::Serialize;

use crate::{
    error::{Error, ExtractionError},
    model::{
        paginator::{ContinuationEndpoint, Paginator},
        ShortDetails, ShortItem, VideoId,
    },
    serializer::MapResult,
    util::{self, ProtoBuilder},
};

use super::{response, ClientType, MapRespCtx, MapRespOptions, MapResponse, RustyPipeQuery};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QReelSequence<'a> {
    sequence_params: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QReelItem<'a> {
    player_request: QReelPlayerRequest<'a>,
    params: &'a str,
    disable_player_response: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QReelPlayerRequest<'a> {
    video_id: &'a str,
}

impl RustyPipeQuery {
    /// Get the shorts YouTube's shorts player plays after the given short
    ///
    /// The seed short itself is not included.
    #[tracing::instrument(skip(self), level = "error")]
    pub async fn reel_sequence<S: AsRef<str> + Debug>(
        &self,
        video_id: S,
    ) -> Result<Paginator<ShortItem>, Error> {
        let video_id = video_id.as_ref();
        let params = sequence_params(video_id);
        self.reel_sequence_request(video_id, &params, None).await
    }

    /// Get more shorts from the shorts player sequence using the given continuation token
    #[tracing::instrument(skip(self), level = "error")]
    pub async fn reel_sequence_continuation<S: AsRef<str> + Debug>(
        &self,
        ctoken: S,
        visitor_data: Option<&str>,
    ) -> Result<Paginator<ShortItem>, Error> {
        let ctoken = ctoken.as_ref();
        self.reel_sequence_request(ctoken, ctoken, visitor_data)
            .await
    }

    async fn reel_sequence_request(
        &self,
        id: &str,
        sequence_params: &str,
        visitor_data: Option<&str>,
    ) -> Result<Paginator<ShortItem>, Error> {
        self.execute_request_ctx::<response::ReelSequence, _, _>(
            ClientType::Desktop,
            "reel_sequence",
            id,
            ContinuationEndpoint::ReelSequence.as_str(),
            &QReelSequence { sequence_params },
            MapRespOptions {
                visitor_data,
                ..Default::default()
            },
        )
        .await
    }

    /// Get the details the shorts player shows over a short, including a linked full video
    #[tracing::instrument(skip(self), level = "error")]
    pub async fn short_details<S: AsRef<str> + Debug>(
        &self,
        video_id: S,
    ) -> Result<ShortDetails, Error> {
        let video_id = video_id.as_ref();
        let request_body = QReelItem {
            player_request: QReelPlayerRequest { video_id },
            params: "CAUwAg%3D%3D",
            disable_player_response: true,
        };

        self.execute_request::<response::ReelItem, _, _>(
            ClientType::Desktop,
            "short_details",
            video_id,
            "reel/reel_item_watch",
            &request_body,
        )
        .await
    }
}

fn sequence_params(video_id: &str) -> String {
    let mut inner = ProtoBuilder::new();
    inner.varint(3, 5);
    let mut pb = ProtoBuilder::new();
    pb.string(1, video_id);
    pb.embedded(5, inner);
    pb.varint(10, 25);
    pb.varint(13, 0);
    pb.to_base64()
}

/// The uploader's channel ID, which the endpoint params carry in field 23
fn channel_id_from_params(params: &str) -> Option<String> {
    let params = urlencoding::decode(params).ok()?;
    let bytes = util::b64_decode(params.replace('+', "-").replace('/', "_").as_bytes()).ok()?;
    util::string_from_pb(bytes, 23).filter(|id| util::CHANNEL_ID_REGEX.is_match(id))
}

impl MapResponse<Paginator<ShortItem>> for response::ReelSequence {
    fn map_response(
        self,
        ctx: &MapRespCtx<'_>,
    ) -> Result<MapResult<Paginator<ShortItem>>, ExtractionError> {
        let mut seen = std::collections::HashSet::new();
        let items = self
            .entries
            .c
            .into_iter()
            .map(|entry| entry.command.reel_watch_endpoint)
            .filter(|ep| seen.insert(ep.video_id.clone()))
            .map(|ep| ShortItem {
                channel_id: ep.params.as_deref().and_then(channel_id_from_params),
                id: ep.video_id,
                thumbnail: ep.thumbnail.into(),
            })
            .collect::<Vec<_>>();
        let ctoken = if items.is_empty() {
            None
        } else {
            self.continuation_endpoint.and_then(|ep| ep.into_token())
        };

        Ok(MapResult {
            c: Paginator::new_ext(
                None,
                items,
                ctoken,
                ctx.visitor_data.map(str::to_owned),
                ContinuationEndpoint::ReelSequence,
                false,
            ),
            warnings: self.entries.warnings,
        })
    }
}

impl MapResponse<ShortDetails> for response::ReelItem {
    fn map_response(
        self,
        ctx: &MapRespCtx<'_>,
    ) -> Result<MapResult<ShortDetails>, ExtractionError> {
        let metapanel = self
            .overlay
            .and_then(|o| o.reel_player_overlay_renderer.player_overlay)
            .and_then(|o| o.reel_player_overlay_view_model.metapanel)
            .ok_or_else(|| ExtractionError::NotFound {
                id: ctx.id.to_owned(),
                msg: "no metapanel".into(),
            })?;

        let mut name = None;
        let mut linked_video = None;
        for item in metapanel.reel_metapanel_view_model.metadata_items {
            match item {
                response::reel::MetadataItem::ShortsVideoTitleViewModel(title) => {
                    name = Some(title.text).filter(|t| !t.is_empty());
                }
                response::reel::MetadataItem::ReelCarouselViewModel(carousel) => {
                    linked_video = linked_video.or_else(|| {
                        carousel.button_view_models.into_iter().find_map(|b| {
                            let button = b
                                .reel_carousel_button_view_model
                                .button_view_model
                                .button_view_model;
                            let id = button.on_tap?.innertube_command.watch_endpoint.video_id;
                            let name = button.title_formatted.or(button.accessibility_text)?;
                            (id != ctx.id).then_some(VideoId { id, name })
                        })
                    });
                }
            }
        }

        Ok(MapResult {
            c: ShortDetails {
                id: ctx.id.to_owned(),
                name,
                linked_video,
            },
            warnings: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::BufReader};

    use path_macro::path;
    use rstest::rstest;

    use super::*;
    use crate::util::tests::TESTFILES;

    #[test]
    fn t_sequence_params() {
        assert_eq!(
            sequence_params("sHpEqeSByPI"),
            "CgtzSHBFcWVTQnlQSSoCGAVQGWgA"
        );
    }

    #[test]
    fn t_channel_id_from_params() {
        let mut pb = ProtoBuilder::new();
        pb.varint(1, 5);
        pb.string(5, "");
        pb.string(23, "UCSO9gir6NjbOGMHVW_nbGkA");
        pb.bytes(26, &[0, 136, 77]);
        assert_eq!(
            channel_id_from_params(&pb.to_base64()).as_deref(),
            Some("UCSO9gir6NjbOGMHVW_nbGkA")
        );
        assert_eq!(channel_id_from_params("CAUwAg%3D%3D"), None);
    }

    #[rstest]
    #[case::start("start")]
    #[case::continuation("continuation")]
    fn map_reel_sequence(#[case] name: &str) {
        let json_path = path!(*TESTFILES / "reel" / format!("sequence_{name}.json"));
        let json_file = File::open(json_path).unwrap();

        let resp: response::ReelSequence =
            serde_json::from_reader(BufReader::new(json_file)).unwrap();
        let map_res: MapResult<Paginator<ShortItem>> =
            resp.map_response(&MapRespCtx::test("")).unwrap();

        assert!(
            map_res.warnings.is_empty(),
            "deserialization/mapping warnings: {:?}",
            map_res.warnings
        );
        assert!(map_res.c.ctoken.is_some(), "no continuation");
        assert!(
            map_res.c.items.iter().all(|s| s.channel_id.is_some()),
            "missing channel IDs"
        );
        insta::assert_ron_snapshot!(format!("map_reel_sequence_{name}"), map_res.c);
    }

    #[rstest]
    #[case::linked("linked", "sHpEqeSByPI")]
    #[case::plain("plain", "kQO5P-qsyiA")]
    fn map_short_details(#[case] name: &str, #[case] id: &str) {
        let json_path = path!(*TESTFILES / "reel" / format!("item_{name}.json"));
        let json_file = File::open(json_path).unwrap();

        let resp: response::ReelItem = serde_json::from_reader(BufReader::new(json_file)).unwrap();
        let map_res: MapResult<ShortDetails> = resp.map_response(&MapRespCtx::test(id)).unwrap();

        assert!(
            map_res.warnings.is_empty(),
            "deserialization/mapping warnings: {:?}",
            map_res.warnings
        );
        assert_eq!(map_res.c.linked_video.is_some(), name == "linked");
        insta::assert_ron_snapshot!(format!("map_short_details_{name}"), map_res.c);
    }
}
