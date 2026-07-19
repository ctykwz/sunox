use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::clip::Clip;

#[derive(Debug, Deserialize, Serialize)]
pub struct FeedResponse {
    #[serde(default)]
    pub clips: Vec<Clip>,
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct FeedV3Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<FeedFilters>,
}

#[derive(Debug, Serialize)]
pub struct FeedFilters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<IdsFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "searchText")]
    pub search_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disliked: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "fullSong")]
    pub full_song: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "fromStudioProject")]
    pub from_studio_project: Option<FilterPresence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem: Option<FilterPresence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<FilterPresence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extend: Option<FilterPresence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<UserFilter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<FeedSort>,
}

#[derive(Debug, Serialize)]
pub struct FilterPresence {
    pub presence: String,
}

#[derive(Debug, Serialize)]
pub struct IdsFilter {
    pub presence: String,
    #[serde(rename = "clipIds")]
    pub clip_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceFilter {
    pub presence: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
}

#[derive(Debug, Serialize)]
pub struct UserFilter {
    pub presence: String,
    #[serde(rename = "userId")]
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct FeedSort {
    #[serde(rename = "sortBy")]
    pub sort_by: String,
    #[serde(rename = "sortDirection")]
    pub sort_direction: String,
}

impl FeedFilters {
    pub fn trashed() -> Self {
        Self {
            ids: None,
            search_text: None,
            disliked: None,
            liked: None,
            public: None,
            upload: None,
            trashed: Some("True".to_string()),
            full_song: None,
            from_studio_project: None,
            stem: None,
            cover: None,
            extend: None,
            workspace: None,
            user: None,
            sort: None,
        }
    }

    pub fn default_workspace() -> Self {
        Self {
            ids: None,
            search_text: None,
            disliked: Some("False".to_string()),
            liked: None,
            public: None,
            upload: None,
            trashed: Some("False".to_string()),
            full_song: None,
            from_studio_project: Some(FilterPresence::absent()),
            stem: Some(FilterPresence::absent()),
            cover: None,
            extend: None,
            workspace: Some(WorkspaceFilter::default_workspace()),
            user: None,
            sort: None,
        }
    }

    pub fn search(query: &str) -> Self {
        Self {
            search_text: Some(query.to_string()),
            ..Self::default_workspace()
        }
    }

    /// Exact-ID filter used by the current Web client for batched generation
    /// polling and other multi-clip reads.
    pub fn ids(ids: &[String]) -> Self {
        Self {
            ids: Some(IdsFilter {
                presence: "True".to_string(),
                clip_ids: ids.to_vec(),
            }),
            search_text: None,
            disliked: None,
            liked: None,
            public: None,
            upload: None,
            trashed: None,
            full_song: None,
            from_studio_project: None,
            stem: None,
            cover: None,
            extend: None,
            workspace: None,
            user: None,
            sort: None,
        }
    }

    pub fn with_public(mut self) -> Self {
        self.public = Some("True".to_string());
        self
    }

    pub fn with_liked(mut self) -> Self {
        self.liked = Some("True".to_string());
        self.disliked = None;
        self
    }

    pub fn with_upload(mut self) -> Self {
        self.upload = Some("True".to_string());
        self
    }

    pub fn with_cover(mut self) -> Self {
        self.cover = Some(FilterPresence::present());
        self
    }

    pub fn with_extend(mut self) -> Self {
        self.extend = Some(FilterPresence::present());
        self
    }

    pub fn with_popular_sort(mut self) -> Self {
        self.sort = Some(FeedSort::upvote_count_desc());
        self
    }
}

impl FilterPresence {
    pub fn present() -> Self {
        Self {
            presence: "True".to_string(),
        }
    }

    pub fn absent() -> Self {
        Self {
            presence: "False".to_string(),
        }
    }
}

impl WorkspaceFilter {
    pub fn default_workspace() -> Self {
        Self {
            presence: "True".to_string(),
            workspace_id: "default".to_string(),
        }
    }
}

impl FeedSort {
    pub fn upvote_count_desc() -> Self {
        Self {
            sort_by: "upvote_count".to_string(),
            sort_direction: "desc".to_string(),
        }
    }
}

#[cfg(test)]
impl UserFilter {
    pub fn for_user(user_id: impl Into<String>) -> Self {
        Self {
            presence: "True".to_string(),
            user_id: user_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FeedFilters, FeedV3Request, UserFilter};

    #[test]
    fn default_feed_matches_create_page_workspace_filter() {
        let req = FeedV3Request {
            cursor: None,
            limit: Some(20),
            filters: Some(FeedFilters::default_workspace()),
        };

        let json = serde_json::to_value(req).expect("serialize feed request");

        assert_eq!(json["cursor"], serde_json::Value::Null);
        assert_eq!(json["limit"], 20);
        assert_eq!(json["filters"]["disliked"], "False");
        assert_eq!(json["filters"]["trashed"], "False");
        assert!(json["filters"].get("liked").is_none());
        assert!(json["filters"].get("public").is_none());
        assert!(json["filters"].get("upload").is_none());
        assert!(json["filters"].get("cover").is_none());
        assert!(json["filters"].get("extend").is_none());
        assert_eq!(json["filters"]["fromStudioProject"]["presence"], "False");
        assert_eq!(json["filters"]["stem"]["presence"], "False");
        assert_eq!(json["filters"]["workspace"]["presence"], "True");
        assert_eq!(json["filters"]["workspace"]["workspaceId"], "default");
    }

    #[test]
    fn user_feed_filter_matches_me_page_shape() {
        let mut filters = FeedFilters::default_workspace();
        filters.workspace = None;
        filters.user = Some(UserFilter::for_user("user-123"));

        let req = FeedV3Request {
            cursor: None,
            limit: Some(20),
            filters: Some(filters),
        };

        let json = serde_json::to_value(req).expect("serialize user feed request");

        assert_eq!(json["filters"]["user"]["presence"], "True");
        assert_eq!(json["filters"]["user"]["userId"], "user-123");
        assert!(json["filters"].get("workspace").is_none());
    }

    #[test]
    fn feed_filters_match_public_liked_upload_cover_extend_popular_web_shape() {
        let filters = FeedFilters::default_workspace()
            .with_public()
            .with_liked()
            .with_upload()
            .with_cover()
            .with_extend()
            .with_popular_sort();

        let json = serde_json::to_value(FeedV3Request {
            cursor: None,
            limit: Some(20),
            filters: Some(filters),
        })
        .expect("serialize feed request");

        assert_eq!(json["filters"]["liked"], "True");
        assert_eq!(json["filters"]["public"], "True");
        assert_eq!(json["filters"]["upload"], "True");
        assert!(json["filters"].get("disliked").is_none());
        assert_eq!(json["filters"]["cover"]["presence"], "True");
        assert_eq!(json["filters"]["extend"]["presence"], "True");
        assert_eq!(json["filters"]["sort"]["sortBy"], "upvote_count");
        assert_eq!(json["filters"]["sort"]["sortDirection"], "desc");
    }

    #[test]
    fn trashed_feed_filter_overrides_the_default_active_library_filter() {
        let filters = FeedFilters::trashed();
        let json = serde_json::to_value(filters).expect("feed filters json");

        assert_eq!(json, serde_json::json!({ "trashed": "True" }));
    }
}
