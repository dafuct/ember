use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://people.googleapis.com";

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PersonHit {
    pub name: String,
    pub email: String,
    pub photo_url: Option<String>,
}

pub struct PeopleClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

impl PeopleClient {
    pub fn new(access_token: String) -> Self {
        Self { base_url: DEFAULT_BASE.to_string(), access_token, http: reqwest::Client::new() }
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self { base_url, access_token, http: reqwest::Client::new() }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self.http.get(url).bearer_auth(&self.access_token).send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::Auth("People access not granted — reconnect Google to enable it.".into()));
        }
        let resp = resp.error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    /// Org directory search (Workspace). Errors on personal accounts.
    pub async fn search_directory(&self, query: &str) -> Result<Vec<PersonHit>> {
        let q = enc(query);
        let url = format!(
            "{}/v1/people:searchDirectoryPeople?query={}&readMask=names,emailAddresses,photos\
             &sources=DIRECTORY_SOURCE_TYPE_DOMAIN_PROFILE&sources=DIRECTORY_SOURCE_TYPE_DOMAIN_CONTACT",
            self.base_url, q
        );
        let resp: DirectoryResp = self.get_json(&url).await?;
        Ok(resp.people.into_iter().filter_map(person_to_hit).collect())
    }

    async fn search_contacts(&self, query: &str) -> Result<Vec<PersonHit>> {
        let q = enc(query);
        let url1 = format!("{}/v1/people:searchContacts?query={}&readMask=names,emailAddresses,photos", self.base_url, q);
        let url2 = format!("{}/v1/otherContacts:search?query={}&readMask=names,emailAddresses,photos", self.base_url, q);
        let mut out = Vec::new();
        if let Ok(r) = self.get_json::<SearchResp>(&url1).await {
            out.extend(r.results.into_iter().filter_map(|x| person_to_hit(x.person)));
        }
        if let Ok(r) = self.get_json::<SearchResp>(&url2).await {
            out.extend(r.results.into_iter().filter_map(|x| person_to_hit(x.person)));
        }
        Ok(out)
    }

    /// Merge directory + contacts; swallow errors (missing scope / personal account)
    /// so autocomplete degrades to "no matches" instead of failing hard.
    pub async fn search(&self, query: &str) -> Vec<PersonHit> {
        let mut hits = Vec::new();
        if let Ok(d) = self.search_directory(query).await {
            hits.extend(d);
        }
        if let Ok(c) = self.search_contacts(query).await {
            hits.extend(c);
        }
        dedupe_by_email(hits)
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn person_to_hit(p: GPerson) -> Option<PersonHit> {
    let email = p.emails.into_iter().find_map(|e| e.value)?;
    let name = p.names.into_iter().find_map(|n| n.display_name).unwrap_or_else(|| email.clone());
    let photo_url = p.photos.into_iter().find_map(|ph| ph.url);
    Some(PersonHit { name, email, photo_url })
}

fn dedupe_by_email(hits: Vec<PersonHit>) -> Vec<PersonHit> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for h in hits {
        if seen.insert(h.email.to_lowercase()) {
            out.push(h);
        }
        if out.len() >= 10 {
            break;
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct DirectoryResp {
    #[serde(default)]
    people: Vec<GPerson>,
}
#[derive(serde::Deserialize)]
struct SearchResp {
    #[serde(default)]
    results: Vec<SearchResult>,
}
#[derive(serde::Deserialize)]
struct SearchResult {
    person: GPerson,
}
#[derive(serde::Deserialize, Default)]
struct GPerson {
    #[serde(default)]
    names: Vec<GName>,
    #[serde(rename = "emailAddresses", default)]
    emails: Vec<GEmail>,
    #[serde(default)]
    photos: Vec<GPhoto>,
}
#[derive(serde::Deserialize)]
struct GName {
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
}
#[derive(serde::Deserialize)]
struct GEmail {
    #[serde(default)]
    value: Option<String>,
}
#[derive(serde::Deserialize)]
struct GPhoto {
    #[serde(default)]
    url: Option<String>,
}
