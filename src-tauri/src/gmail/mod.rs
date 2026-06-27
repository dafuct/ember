pub mod types;

use std::collections::HashMap;
use types::{
    AttachmentMeta, FullMessage, HistoryResponse, Label, LabelColor, MessageList, MessagePart,
    MessagePreview, ModifiedMessage, Profile, RawMessage, ReplyContext,
};
use types::{DraftContent, DraftRef};

use crate::error::{AppError, Result};

const DEFAULT_BASE: &str = "https://gmail.googleapis.com";

pub struct GmailClient {
    base_url: String,
    access_token: String,
    http: reqwest::Client,
}

#[derive(Debug, Default, PartialEq)]
pub struct HistoryDelta {
    pub added_ids: Vec<String>,
    pub removed_ids: Vec<String>,
    pub new_history_id: Option<String>,
    pub too_old: bool,
}

pub struct RawBody {
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

fn decode_b64url_bytes(data: &str) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.trim().trim_end_matches('='))
        .ok()
}

fn decode_b64url(data: &str) -> Option<String> {
    decode_b64url_bytes(data).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn collect_body(part: &MessagePart, html: &mut Option<String>, text: &mut Option<String>) {
    match part.mime_type.as_str() {
        "text/html" if html.is_none() => *html = decode_b64url(&part.body.data),
        "text/plain" if text.is_none() => *text = decode_b64url(&part.body.data),
        _ => {}
    }
    for child in &part.parts {
        collect_body(child, html, text);
    }
}

fn collect_attachments(part: &MessagePart, out: &mut Vec<AttachmentMeta>) {
    if !part.filename.is_empty() {
        if let Some(id) = &part.body.attachment_id {
            out.push(AttachmentMeta {
                filename: part.filename.clone(),
                mime_type: part.mime_type.clone(),
                size: part.body.size,
                attachment_id: id.clone(),
            });
        }
    }
    for child in &part.parts {
        collect_attachments(child, out);
    }
}

impl GmailClient {
    pub fn new(access_token: String) -> Self {
        Self {
            base_url: DEFAULT_BASE.to_string(),
            access_token,
            http: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(access_token: String, base_url: String) -> Self {
        Self {
            base_url,
            access_token,
            http: reqwest::Client::new(),
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    async fn post_no_body(&self, url: &str) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::CONTENT_LENGTH, 0)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn delete_no_body(&self, url: &str) -> Result<()> {
        self.http
            .delete(url)
            .bearer_auth(&self.access_token)
            .header(reqwest::header::CONTENT_LENGTH, 0)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    async fn post_json_no_response<B: serde::Serialize>(&self, url: &str, body: &B) -> Result<()> {
        self.http
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn put_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &B,
    ) -> Result<T> {
        let resp = self
            .http
            .put(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(resp.json::<T>().await?)
    }

    pub async fn get_profile(&self) -> Result<Profile> {
        let url = format!("{}/gmail/v1/users/me/profile", self.base_url);
        self.get_json(&url).await
    }

    pub async fn list_inbox_message_ids(&self, max: u32) -> Result<Vec<String>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages?maxResults={}&labelIds=INBOX",
            self.base_url, max
        );
        let list: MessageList = self.get_json(&url).await?;
        Ok(list.messages.into_iter().map(|m| m.id).collect())
    }

    pub async fn get_message_preview(&self, id: &str) -> Result<MessagePreview> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}?format=metadata\
             &metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date\
             &metadataHeaders=To&metadataHeaders=List-Id&metadataHeaders=List-Unsubscribe",
            self.base_url, id
        );
        let raw: RawMessage = self.get_json(&url).await?;
        let header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        let has_header = |name: &str| {
            raw.payload
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case(name))
        };
        let from = header("From");
        let subject = header("Subject");
        let date = header("Date");
        let to_addr = header("To");
        let has_list_unsubscribe = has_header("List-Unsubscribe");
        let has_list_id = has_header("List-Id");
        let internal_date = raw.internal_date.parse::<i64>().unwrap_or(0);
        Ok(MessagePreview {
            id: raw.id,
            thread_id: raw.thread_id,
            from,
            subject,
            date,
            snippet: raw.snippet,
            internal_date,
            label_ids: raw.label_ids,
            to_addr,
            has_list_unsubscribe,
            has_list_id,
            category: String::new(),
            draft_id: None,
        })
    }

    pub async fn list_message_ids(
        &self,
        label: Option<&str>,
        query: &str,
        max_total: u32,
        include_spam_trash: bool,
    ) -> Result<Vec<String>> {
        let encoded_q: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let mut ids = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/messages?maxResults=100&q={}",
                self.base_url, encoded_q
            );
            if let Some(l) = label {
                url.push_str(&format!("&labelIds={l}"));
            }
            if include_spam_trash {
                url.push_str("&includeSpamTrash=true");
            }
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={token}"));
            }
            let list: MessageList = self.get_json(&url).await?;
            for m in list.messages {
                ids.push(m.id);
                if ids.len() >= max_total as usize {
                    return Ok(ids);
                }
            }
            match list.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }
        Ok(ids)
    }

    pub async fn list_inbox_message_ids_paged(
        &self,
        query: &str,
        max_total: u32,
    ) -> Result<Vec<String>> {
        self.list_message_ids(Some("INBOX"), query, max_total, false).await
    }

    pub async fn search_message_ids(&self, query: &str, max_total: u32) -> Result<Vec<String>> {
        self.list_message_ids(None, query, max_total, false).await
    }

    pub async fn list_history(&self, start_history_id: &str) -> Result<HistoryDelta> {
        let mut state: HashMap<String, bool> = HashMap::new();
        let mut page_token: Option<String> = None;
        let mut latest_history_id: Option<String> = None;

        loop {
            let mut url = format!(
                "{}/gmail/v1/users/me/history?startHistoryId={}&labelId=INBOX&maxResults=500\
                 &historyTypes=messageAdded&historyTypes=messageDeleted\
                 &historyTypes=labelAdded&historyTypes=labelRemoved",
                self.base_url, start_history_id
            );
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={token}"));
            }

            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.access_token)
                .send()
                .await?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(HistoryDelta {
                    too_old: true,
                    ..Default::default()
                });
            }
            let resp = resp.error_for_status()?;
            let page: HistoryResponse = resp.json().await?;

            for record in page.history {
                for m in record.messages_added {
                    state.insert(m.message.id, true);
                }
                for c in record.labels_added {
                    if c.label_ids.iter().any(|l| l == "INBOX") {
                        state.insert(c.message.id, true);
                    }
                }
                for m in record.messages_deleted {
                    state.insert(m.message.id, false);
                }
                for c in record.labels_removed {
                    if c.label_ids.iter().any(|l| l == "INBOX") {
                        state.insert(c.message.id, false);
                    }
                }
            }
            if !page.history_id.is_empty() {
                latest_history_id = Some(page.history_id);
            }
            match page.next_page_token {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }

        let added_ids = state
            .iter()
            .filter(|(_, &present)| present)
            .map(|(id, _)| id.clone())
            .collect();
        let removed_ids = state
            .iter()
            .filter(|(_, &present)| !present)
            .map(|(id, _)| id.clone())
            .collect();
        Ok(HistoryDelta {
            added_ids,
            removed_ids,
            new_history_id: latest_history_id,
            too_old: false,
        })
    }

    pub async fn get_message_previews(
        &self,
        ids: &[String],
        concurrency: usize,
    ) -> Result<Vec<MessagePreview>> {
        use futures::stream::StreamExt;
        let results = futures::stream::iter(ids.iter().cloned())
            .map(|id| async move { self.get_message_preview(&id).await })
            .buffer_unordered(concurrency)
            .collect::<Vec<Result<MessagePreview>>>()
            .await;
        Ok(results.into_iter().filter_map(|r| r.ok()).collect())
    }

    pub async fn get_message_body(&self, id: &str) -> Result<RawBody> {
        let url = format!("{}/gmail/v1/users/me/messages/{}?format=full", self.base_url, id);
        let full: FullMessage = self.get_json(&url).await?;
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        let mut attachments = Vec::new();
        collect_attachments(&full.payload, &mut attachments);
        Ok(RawBody { html, text, attachments })
    }

    pub async fn get_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let url = format!(
            "{}/gmail/v1/users/me/messages/{}/attachments/{}",
            self.base_url, message_id, attachment_id
        );
        let resp: AttachmentResponse = self.get_json(&url).await?;
        decode_b64url_bytes(&resp.data)
            .ok_or_else(|| AppError::Other("attachment data was empty or not valid base64url".into()))
    }

    pub async fn get_reply_context(&self, id: &str) -> Result<ReplyContext> {
        let url = format!("{}/gmail/v1/users/me/messages/{}?format=full", self.base_url, id);
        let full: FullMessage = self.get_json(&url).await?;
        let header = |name: &str| {
            full.payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
                .unwrap_or_default()
        };
        let message_id = header("Message-ID");
        let references = header("References");
        let to = header("To");
        let cc = header("Cc");
        let mut html = None;
        let mut text = None;
        collect_body(&full.payload, &mut html, &mut text);
        let mut attachments = Vec::new();
        collect_attachments(&full.payload, &mut attachments);
        Ok(ReplyContext {
            message_id,
            references,
            quoted_text: text.unwrap_or_default(),
            to,
            cc,
            attachments,
        })
    }

    pub async fn send_message(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()> {
        let raw = encode_raw(raw_rfc822);
        #[derive(serde::Serialize)]
        struct SendRequest<'a> {
            raw: String,
            #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
            thread_id: Option<&'a str>,
        }
        let url = format!("{}/gmail/v1/users/me/messages/send", self.base_url);
        let body = SendRequest { raw, thread_id };
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    pub async fn trash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/trash", self.base_url, id);
        self.post_no_body(&url).await
    }

    pub async fn untrash_message(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}/untrash", self.base_url, id);
        self.post_no_body(&url).await
    }

    pub async fn delete_message_forever(&self, id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/messages/{}", self.base_url, id);
        self.delete_no_body(&url).await
    }

    pub async fn modify_message(
        &self,
        id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> Result<ModifiedMessage> {
        #[derive(serde::Serialize)]
        struct ModifyRequest<'a> {
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/{}/modify", self.base_url, id);
        let body = ModifyRequest {
            add_label_ids: add,
            remove_label_ids: remove,
        };
        self.post_json(&url, &body).await
    }

    pub async fn create_draft(&self, raw_rfc822: &str, thread_id: Option<&str>) -> Result<String> {
        let url = format!("{}/gmail/v1/users/me/drafts", self.base_url);
        let body = DraftWriteBody {
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        let resp: DraftIdResponse = self.post_json(&url, &body).await?;
        Ok(resp.id)
    }

    pub async fn update_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<String> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}", self.base_url, draft_id);
        let body = DraftWriteBody {
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        let resp: DraftIdResponse = self.put_json(&url, &body).await?;
        Ok(resp.id)
    }

    pub async fn list_drafts(&self, max: u32) -> Result<Vec<DraftRef>> {
        let url = format!("{}/gmail/v1/users/me/drafts?maxResults={}", self.base_url, max);
        let resp: DraftListResponse = self.get_json(&url).await?;
        Ok(resp
            .drafts
            .into_iter()
            .map(|d| DraftRef { id: d.id, message_id: d.message.id })
            .collect())
    }

    pub async fn get_draft(&self, draft_id: &str) -> Result<DraftContent> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}?format=full", self.base_url, draft_id);
        let resp: DraftGetResponse = self.get_json(&url).await?;
        let header = |name: &str| {
            resp.message
                .payload
                .headers
                .iter()
                .find(|h| h.name.eq_ignore_ascii_case(name))
                .map(|h| h.value.clone())
        };
        let mut html = None;
        let mut text = None;
        collect_body(&resp.message.payload, &mut html, &mut text);
        let thread_id = if resp.message.thread_id.is_empty() {
            None
        } else {
            Some(resp.message.thread_id)
        };
        Ok(DraftContent {
            draft_id: resp.id,
            to: header("To").unwrap_or_default(),
            cc: header("Cc").unwrap_or_default(),
            subject: header("Subject").unwrap_or_default(),
            body: text.unwrap_or_default(),
            in_reply_to: header("In-Reply-To"),
            references: header("References"),
            thread_id,
        })
    }

    pub async fn send_draft(&self, draft_id: &str, raw_rfc822: &str, thread_id: Option<&str>) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/drafts/send", self.base_url);
        let body = DraftSendBody {
            id: draft_id,
            message: DraftWriteMessage { raw: encode_raw(raw_rfc822), thread_id },
        };
        let _: serde_json::Value = self.post_json(&url, &body).await?;
        Ok(())
    }

    pub async fn delete_draft(&self, draft_id: &str) -> Result<()> {
        let url = format!("{}/gmail/v1/users/me/drafts/{}", self.base_url, draft_id);
        self.delete_no_body(&url).await
    }

    pub async fn batch_modify(&self, ids: &[String], add: &[&str], remove: &[&str]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct BatchModifyRequest<'a> {
            ids: &'a [String],
            #[serde(rename = "addLabelIds")]
            add_label_ids: &'a [&'a str],
            #[serde(rename = "removeLabelIds")]
            remove_label_ids: &'a [&'a str],
        }
        let url = format!("{}/gmail/v1/users/me/messages/batchModify", self.base_url);
        let body = BatchModifyRequest { ids, add_label_ids: add, remove_label_ids: remove };
        self.post_json_no_response(&url, &body).await
    }

    pub async fn batch_delete(&self, ids: &[String]) -> Result<()> {
        #[derive(serde::Serialize)]
        struct BatchDeleteRequest<'a> {
            ids: &'a [String],
        }
        let url = format!("{}/gmail/v1/users/me/messages/batchDelete", self.base_url);
        self.post_json_no_response(&url, &BatchDeleteRequest { ids }).await
    }

    pub async fn list_labels(&self) -> Result<Vec<Label>> {
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let resp: LabelsListResponse = self.get_json(&url).await?;
        Ok(resp
            .labels
            .into_iter()
            .filter(|l| l.label_type == "user")
            .map(|l| Label { id: l.id, name: l.name, color: l.color })
            .collect())
    }

    pub async fn create_label(&self, name: &str) -> Result<Label> {
        #[derive(serde::Serialize)]
        struct CreateLabelRequest<'a> {
            name: &'a str,
            #[serde(rename = "labelListVisibility")]
            label_list_visibility: &'a str,
            #[serde(rename = "messageListVisibility")]
            message_list_visibility: &'a str,
        }
        let url = format!("{}/gmail/v1/users/me/labels", self.base_url);
        let body = CreateLabelRequest {
            name,
            label_list_visibility: "labelShow",
            message_list_visibility: "show",
        };
        let raw: RawLabel = self.post_json(&url, &body).await?;
        Ok(Label { id: raw.id, name: raw.name, color: raw.color })
    }
}

fn encode_raw(raw_rfc822: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw_rfc822.as_bytes())
}

#[derive(serde::Serialize)]
struct DraftWriteBody<'a> {
    message: DraftWriteMessage<'a>,
}

#[derive(serde::Serialize)]
struct DraftSendBody<'a> {
    id: &'a str,
    message: DraftWriteMessage<'a>,
}

#[derive(serde::Serialize)]
struct DraftWriteMessage<'a> {
    raw: String,
    #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
    thread_id: Option<&'a str>,
}

#[derive(serde::Deserialize)]
struct DraftIdResponse {
    id: String,
}

#[derive(serde::Deserialize)]
struct AttachmentResponse {
    #[serde(default)]
    data: String,
}

#[derive(serde::Deserialize)]
struct DraftListResponse {
    #[serde(default)]
    drafts: Vec<DraftListItem>,
}
#[derive(serde::Deserialize)]
struct DraftListItem {
    id: String,
    message: DraftMsgRef,
}
#[derive(serde::Deserialize)]
struct DraftMsgRef {
    id: String,
}

#[derive(serde::Deserialize)]
struct DraftGetResponse {
    id: String,
    message: DraftGetMessage,
}
#[derive(serde::Deserialize)]
struct DraftGetMessage {
    #[serde(rename = "threadId", default)]
    thread_id: String,
    payload: MessagePart,
}

#[derive(serde::Deserialize)]
struct LabelsListResponse {
    #[serde(default)]
    labels: Vec<RawLabel>,
}
#[derive(serde::Deserialize)]
struct RawLabel {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    label_type: String,
    #[serde(default)]
    color: Option<LabelColor>,
}
