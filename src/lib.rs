//! IMVDb (<https://imvdb.com>, the Internet Music Video Database) metadata
//! provider extension for LisekMovie.
//!
//! Music videos live in film libraries ("Artist - Song.mkv"), so this
//! provider answers film questions only; series get nothing. A free API key
//! (`https://imvdb.com/developers`) goes in Admin → Plugins and is sent as
//! the `IMVDB-APP-KEY` header.
//!
//! - search: `GET /api/v1/search/videos?q={query}`
//! - details: `GET /api/v1/video/{id}?include=credits`
//!
//! A match fills `imvdb_id`, the title as `Artist - Song`, the year, the
//! genre "Music Video", the directors' names (`fields.directors`) and the
//! video's still as poster and backdrop.

use std::time::Duration;

use async_trait::async_trait;
use lisek_plugin_api::{
    MediaKind, MetadataProvider, MovieQuery, ProviderCandidate, ProviderError, ProviderMatch,
    SeriesQuery, normalize_query_title,
};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://imvdb.com/api/v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default, Deserialize)]
struct RawArtist {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawImage {
    /// Original size.
    o: Option<String>,
    /// Large.
    l: Option<String>,
    /// Small.
    s: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawCrew {
    #[serde(default)]
    entity_name: String,
    #[serde(default)]
    position_name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawCredits {
    #[serde(default)]
    crew: Vec<RawCrew>,
}

/// One music video.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ImvdbVideo {
    pub id: u64,
    #[serde(default)]
    song_title: String,
    year: Option<i32>,
    #[serde(default)]
    artists: Vec<RawArtist>,
    #[serde(default)]
    image: RawImage,
    #[serde(default)]
    version_name: Option<String>,
    #[serde(default)]
    credits: Option<RawCredits>,
}

impl ImvdbVideo {
    /// `Artist - Song` (`Artist - Song (Version)` for alternate cuts).
    pub fn display_title(&self) -> String {
        let artists: Vec<&str> = self
            .artists
            .iter()
            .map(|a| a.name.trim())
            .filter(|a| !a.is_empty())
            .collect();
        let mut title = if artists.is_empty() {
            self.song_title.trim().to_string()
        } else {
            format!("{} - {}", artists.join(", "), self.song_title.trim())
        };
        if let Some(version) = self
            .version_name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            title.push_str(&format!(" ({version})"));
        }
        title
    }

    fn directors(&self) -> Vec<String> {
        self.credits
            .iter()
            .flat_map(|c| c.crew.iter())
            .filter(|c| c.position_name.eq_ignore_ascii_case("director"))
            .map(|c| c.entity_name.clone())
            .collect()
    }

    fn to_match(&self, with_title: bool) -> ProviderMatch {
        let mut fields = serde_json::json!({
            "imvdb_id": self.id,
            "overview": "",
            "genres": ["Music Video"],
            "directors": self.directors(),
        });
        if with_title {
            fields["title"] = serde_json::json!(self.display_title());
            fields["year"] = serde_json::json!(self.year);
        }
        let still = self.image.o.clone().or_else(|| self.image.l.clone());
        ProviderMatch {
            poster_url: self.image.l.clone().or_else(|| still.clone()),
            backdrop_url: still,
            fields,
            ..ProviderMatch::default()
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<ImvdbVideo>,
}

/// HTTP client of the IMVDb API. `base_url` is injectable for tests.
pub struct ImvdbClient {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl ImvdbClient {
    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, ProviderError> {
        let response = self
            .http
            .get(format!("{}{path}", self.base_url))
            .header("IMVDB-APP-KEY", &self.api_key)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await
            .map_err(|err| ProviderError::Request(err.to_string()))?;
        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                return Err(ProviderError::Unauthorized);
            }
            reqwest::StatusCode::NOT_FOUND => return Ok(None),
            _ => {}
        }
        response
            .error_for_status()
            .map_err(|err| ProviderError::Request(err.to_string()))?
            .json()
            .await
            .map(Some)
            .map_err(|err| ProviderError::UnexpectedResponse(err.to_string()))
    }

    pub async fn search(&self, query: &str) -> Result<Vec<ImvdbVideo>, ProviderError> {
        let found: Option<SearchResponse> = self.get("/search/videos", &[("q", query)]).await?;
        Ok(found.map(|f| f.results).unwrap_or_default())
    }

    pub async fn video(&self, id: u64) -> Result<Option<ImvdbVideo>, ProviderError> {
        self.get(&format!("/video/{id}"), &[("include", "credits")])
            .await
    }
}

/// The [`MetadataProvider`] for `"imvdb"`.
pub struct ImvdbProvider {
    api_key: String,
    base_url: String,
}

impl ImvdbProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_base_url(api_key, DEFAULT_BASE_URL)
    }

    pub fn with_base_url(api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: base_url.into(),
        }
    }

    fn client(&self) -> ImvdbClient {
        ImvdbClient::with_base_url(self.api_key.clone(), self.base_url.clone())
    }
}

#[async_trait]
impl MetadataProvider for ImvdbProvider {
    fn provider_key(&self) -> &'static str {
        "imvdb"
    }

    /// Strict: the file name (`Artist - Song`) must equal the video's
    /// title after normalization, so a feature film never becomes a music
    /// video.
    async fn find_movie(
        &self,
        query: &MovieQuery<'_>,
        _lang: &str,
    ) -> Result<Option<ProviderMatch>, ProviderError> {
        let wanted = normalize_query_title(query.title);
        let found = self.client().search(query.title).await?;
        let Some(video) = found.into_iter().find(|v| {
            normalize_query_title(&v.display_title()) == wanted
                && query.year.is_none_or(|y| v.year == Some(y))
        }) else {
            return Ok(None);
        };
        Ok(Some(video.to_match(false)))
    }

    async fn find_series(
        &self,
        _query: &SeriesQuery<'_>,
        _lang: &str,
    ) -> Result<Option<ProviderMatch>, ProviderError> {
        Ok(None)
    }

    async fn search(
        &self,
        kind: MediaKind,
        title: &str,
        year: Option<i32>,
        _lang: &str,
    ) -> Result<Vec<ProviderCandidate>, ProviderError> {
        if kind != MediaKind::Movie {
            return Ok(Vec::new());
        }
        Ok(self
            .client()
            .search(title)
            .await?
            .into_iter()
            .filter(|v| year.is_none_or(|y| v.year == Some(y)))
            .take(10)
            .map(|v| ProviderCandidate {
                provider_id: v.id.to_string(),
                title: v.display_title(),
                year: v.year,
                overview: String::new(),
                poster_url: v.image.s.clone().or_else(|| v.image.l.clone()),
            })
            .collect())
    }

    async fn lookup(
        &self,
        kind: MediaKind,
        provider_id: &str,
        _lang: &str,
    ) -> Result<Option<ProviderMatch>, ProviderError> {
        if kind != MediaKind::Movie {
            return Ok(None);
        }
        let Ok(id) = provider_id.trim().parse::<u64>() else {
            return Ok(None);
        };
        Ok(self.client().video(id).await?.map(|v| v.to_match(true)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lisek_plugin_api::DirectId;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn video() -> serde_json::Value {
        serde_json::json!({
            "id": 121779770452u64, "song_title": "Shake It Off", "year": 2014,
            "artists": [{ "name": "Taylor Swift" }],
            "image": { "o": "https://img/o.jpg", "l": "https://img/l.jpg", "s": "https://img/s.jpg" },
            "credits": { "crew": [{ "entity_name": "Mark Romanek", "position_name": "Director" },
                                  { "entity_name": "Someone", "position_name": "Producer" }] }
        })
    }

    #[tokio::test]
    async fn search_lookup_and_strict_match() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/videos"))
            .and(header("IMVDB-APP-KEY", "k"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "results": [video()] })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/video/121779770452"))
            .and(query_param("include", "credits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(video()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/video/1"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let provider = ImvdbProvider::with_base_url("k", server.uri());

        let found = provider
            .search(MediaKind::Movie, "shake it off", None, "en")
            .await
            .unwrap();
        assert_eq!(
            (found[0].title.as_str(), found[0].year),
            ("Taylor Swift - Shake It Off", Some(2014))
        );
        assert!(
            provider
                .search(MediaKind::Series, "shake it off", None, "en")
                .await
                .unwrap()
                .is_empty()
        );

        let matched = provider
            .find_movie(
                &MovieQuery {
                    title: "Taylor Swift - Shake It Off",
                    year: None,
                },
                "en",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(matched.fields["imvdb_id"], 121779770452u64);
        assert!(
            provider
                .find_movie(
                    &MovieQuery {
                        title: "Shake",
                        year: None
                    },
                    "en"
                )
                .await
                .unwrap()
                .is_none()
        );

        let candidate = provider
            .lookup_by_id(
                MediaKind::Movie,
                &DirectId::Numeric("121779770452".into()),
                "en",
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(candidate.title, "Taylor Swift - Shake It Off");
        let details = provider
            .lookup(MediaKind::Movie, "121779770452", "en")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            details.fields["directors"],
            serde_json::json!(["Mark Romanek"])
        );
        assert!(
            provider
                .lookup(MediaKind::Movie, "1", "en")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_rejected_key_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let provider = ImvdbProvider::with_base_url("bad", server.uri());
        assert!(matches!(
            provider.search(MediaKind::Movie, "x", None, "en").await,
            Err(ProviderError::Unauthorized)
        ));
    }

    #[test]
    fn provider_key_is_imvdb() {
        assert_eq!(ImvdbProvider::new("k").provider_key(), "imvdb");
    }
}
