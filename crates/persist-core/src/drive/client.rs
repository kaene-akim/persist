//! A focused Google Drive API v3 client.
//!
//! Only the operations Persist needs are implemented. Everything is built around
//! the real characteristics of the Drive API rather than a POSIX mindset:
//!
//! * Drive is a flat namespace of files with parent references, so directories
//!   are found or created by name + parent and their ids are cached.
//! * Listing is paginated.
//! * Uploads use multipart for small files and resumable uploads for large ones.
//! * Transient failures (429/5xx, network errors) are retried with exponential
//!   backoff, honouring `Retry-After`.
//!
//! Files Persist creates are tagged with an `appProperties.persist` marker so
//! they can be identified reliably.

use crate::error::{Error, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub const API: &str = "https://www.googleapis.com/drive/v3";
pub const UPLOAD_API: &str = "https://www.googleapis.com/upload/drive/v3";
pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

/// Supplies access tokens to the Drive client.
#[async_trait]
pub trait TokenProvider: Send + Sync {
    async fn access_token(&self) -> Result<String>;
}

/// File metadata as returned by the Drive API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DriveFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: String,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default)]
    pub md5_checksum: Option<String>,
}

impl DriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }
    pub fn size_u64(&self) -> u64 {
        self.size
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }
}

/// Storage quota from `about.get`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DriveQuota {
    #[serde(default)]
    pub limit: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(default, rename = "usageInDrive")]
    pub usage_in_drive: Option<String>,
    #[serde(default, rename = "usageInDriveTrash")]
    pub usage_in_drive_trash: Option<String>,
}

/// Drive API client.
#[derive(Clone)]
pub struct DriveClient {
    http: reqwest::Client,
    tokens: std::sync::Arc<dyn TokenProvider>,
    max_retries: u32,
    chunk_size: usize,
    /// Files at or above this size use resumable uploads.
    resumable_threshold: u64,
}

impl DriveClient {
    pub fn new(tokens: std::sync::Arc<dyn TokenProvider>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("persist/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            tokens,
            max_retries: 5,
            chunk_size: 8 * 1024 * 1024,
            resumable_threshold: 8 * 1024 * 1024,
        }
    }

    pub fn with_chunk_size(mut self, bytes: usize) -> Self {
        self.chunk_size = bytes.max(256 * 1024);
        self
    }

    /// Files at or above this size use a resumable upload.
    pub fn resumable_threshold(&self) -> u64 {
        self.resumable_threshold
    }

    /// Send a request built by `build`, retrying transient failures.
    async fn send_with_retry<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(&str) -> reqwest::RequestBuilder,
    {
        let mut attempt = 0u32;
        loop {
            let token = self.tokens.access_token().await?;
            let resp = match build(&token).send().await {
                Ok(r) => r,
                Err(e) if e.is_timeout() || e.is_connect() => {
                    if attempt >= self.max_retries {
                        return Err(Error::Network(format!("request failed: {e}")));
                    }
                    self.backoff(attempt, None).await;
                    attempt += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let status = resp.status();
            if status.is_success() {
                return Ok(resp);
            }
            let retry_after = parse_retry_after(&resp);
            let body = resp.text().await.unwrap_or_default();
            let transient = status.as_u16() == 429
                || status.as_u16() == 403 && is_rate_limit(&body)
                || status.is_server_error();
            if transient && attempt < self.max_retries {
                self.backoff(attempt, retry_after).await;
                attempt += 1;
                continue;
            }
            if status.as_u16() == 429 || (status.as_u16() == 403 && is_rate_limit(&body)) {
                return Err(Error::RateLimited {
                    retry_after_secs: retry_after,
                });
            }
            if status.as_u16() == 401 {
                return Err(Error::AuthRequired(body));
            }
            return Err(Error::Drive {
                status: status.as_u16(),
                message: body,
            });
        }
    }

    async fn backoff(&self, attempt: u32, retry_after: Option<u64>) {
        let base = retry_after.unwrap_or(0).max(1);
        let delay = base.max(1 << attempt.min(6)); // 1,2,4,...,64s
        tokio::time::sleep(Duration::from_secs(delay.min(64))).await;
    }

    // -- Files --------------------------------------------------------------

    /// Find the single child of `parent` with the given name, if any.
    pub async fn find_child(&self, parent: &str, name: &str) -> Result<Option<DriveFile>> {
        let q = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            escape_query(name),
            parent
        );
        let url = format!("{API}/files");
        let resp = self
            .send_with_retry(|token| {
                self.http
                    .get(&url)
                    .bearer_auth(token)
                    .query(&[
                        ("q", q.as_str()),
                        ("fields", "files(id,name,mimeType,size,trashed)"),
                        ("pageSize", "10"),
                        ("spaces", "drive"),
                    ])
            })
            .await?;
        let body: ListResponse = resp.json().await?;
        Ok(body.files.into_iter().next())
    }

    /// Find a folder child of `parent`, or create it.
    pub async fn find_or_create_folder(&self, parent: &str, name: &str) -> Result<DriveFile> {
        if let Some(f) = self.find_child(parent, name).await? {
            if f.is_folder() {
                return Ok(f);
            }
            return Err(Error::InvalidRepository(format!(
                "a non-folder item named {name:?} already exists in the repository"
            )));
        }
        self.create_folder(parent, name).await
    }

    /// Create a folder.
    pub async fn create_folder(&self, parent: &str, name: &str) -> Result<DriveFile> {
        let url = format!("{API}/files?fields=id,name,mimeType");
        let body = serde_json::json!({
            "name": name,
            "mimeType": FOLDER_MIME,
            "parents": [parent],
            "appProperties": { "persist": "folder" }
        });
        let resp = self
            .send_with_retry(|token| self.http.post(&url).bearer_auth(token).json(&body))
            .await?;
        Ok(resp.json().await?)
    }

    /// Get a file's metadata by id.
    pub async fn get_file(&self, id: &str) -> Result<DriveFile> {
        let url = format!("{API}/files/{id}?fields=id,name,mimeType,size,trashed,md5Checksum");
        let resp = self
            .send_with_retry(|token| self.http.get(&url).bearer_auth(token))
            .await?;
        Ok(resp.json().await?)
    }

    /// List all children of a folder, following pagination.
    pub async fn list_children(&self, parent: &str) -> Result<Vec<DriveFile>> {
        let q = format!("'{}' in parents and trashed = false", parent);
        let mut out = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let url = format!("{API}/files");
            let mut query: Vec<(&str, String)> = vec![
                ("q", q.clone()),
                (
                    "fields",
                    "nextPageToken,files(id,name,mimeType,size,trashed)".to_string(),
                ),
                ("pageSize", "1000".to_string()),
                ("spaces", "drive".to_string()),
            ];
            if let Some(t) = &page_token {
                query.push(("pageToken", t.clone()));
            }
            let resp = self
                .send_with_retry(|token| {
                    let req = self.http.get(&url).bearer_auth(token);
                    req.query(&query)
                })
                .await?;
            let body: ListResponse = resp.json().await?;
            out.extend(body.files);
            match body.next_page_token {
                Some(t) if !t.is_empty() => page_token = Some(t),
                _ => break,
            }
        }
        Ok(out)
    }

    /// Delete a file by id. Missing files are not an error.
    pub async fn delete_file(&self, id: &str) -> Result<()> {
        let url = format!("{API}/files/{id}");
        match self
            .send_with_retry(|token| self.http.delete(&url).bearer_auth(token))
            .await
        {
            Ok(_) => Ok(()),
            Err(Error::Drive { status: 404, .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Download a file's contents into memory.
    pub async fn download(&self, id: &str) -> Result<Vec<u8>> {
        let url = format!("{API}/files/{id}?alt=media");
        let resp = self
            .send_with_retry(|token| self.http.get(&url).bearer_auth(token))
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Download a file to a local path, streaming to avoid buffering.
    pub async fn download_to_file(&self, id: &str, dest: &Path) -> Result<()> {
        let url = format!("{API}/files/{id}?alt=media");
        let resp = self
            .send_with_retry(|token| self.http.get(&url).bearer_auth(token))
            .await?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(dest).await?;
        let mut stream = resp.bytes_stream();
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk: Bytes = chunk?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        Ok(())
    }

    // -- Uploads ------------------------------------------------------------

    /// Upload small content using a multipart request, creating a new file.
    pub async fn upload_new(
        &self,
        parent: &str,
        name: &str,
        content_type: &str,
        data: &[u8],
        app_properties: serde_json::Value,
    ) -> Result<DriveFile> {
        let boundary = format!("persist{}", crate::util::new_id());
        let metadata = serde_json::json!({
            "name": name,
            "parents": [parent],
            "appProperties": app_properties,
        });
        let mut body = Vec::with_capacity(data.len() + 512);
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Type: application/json; charset=UTF-8\r\n\r\n");
        body.extend_from_slice(serde_json::to_string(&metadata)?.as_bytes());
        body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());

        let url = format!("{UPLOAD_API}/files?uploadType=multipart&fields=id,name,mimeType,size,md5Checksum");
        let ct = format!("multipart/related; boundary={boundary}");
        let resp = self
            .send_with_retry(|token| {
                self.http
                    .post(&url)
                    .bearer_auth(token)
                    .header("Content-Type", &ct)
                    .body(body.clone())
            })
            .await?;
        Ok(resp.json().await?)
    }

    /// Replace the contents of an existing file.
    pub async fn update_content(&self, id: &str, content_type: &str, data: &[u8]) -> Result<()> {
        let url = format!("{UPLOAD_API}/files/{id}?uploadType=media&fields=id,size,md5Checksum");
        self.send_with_retry(|token| {
            self.http
                .patch(&url)
                .bearer_auth(token)
                .header("Content-Type", content_type)
                .body(data.to_vec())
        })
        .await?;
        Ok(())
    }

    /// Upload a large file using a resumable session, streaming from disk.
    pub async fn upload_resumable(
        &self,
        parent: &str,
        name: &str,
        content_type: &str,
        file: &Path,
        app_properties: serde_json::Value,
    ) -> Result<DriveFile> {
        let size = tokio::fs::metadata(file).await?.len();
        let metadata = serde_json::json!({
            "name": name,
            "parents": [parent],
            "appProperties": app_properties,
        });
        let init_url = format!("{UPLOAD_API}/files?uploadType=resumable&fields=id,name,mimeType,size,md5Checksum");
        let token = self.tokens.access_token().await?;
        let init = self
            .http
            .post(&init_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("X-Upload-Content-Type", content_type)
            .header("X-Upload-Content-Length", size.to_string())
            .json(&metadata)
            .send()
            .await?;
        if !init.status().is_success() {
            let status = init.status().as_u16();
            let body = init.text().await.unwrap_or_default();
            return Err(Error::Drive { status, message: body });
        }
        let location = init
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Error::Drive {
                status: 500,
                message: "resumable upload did not return a session URI".into(),
            })?
            .to_string();

        // Stream the file in chunks.
        use tokio::io::AsyncReadExt;
        let mut f = tokio::fs::File::open(file).await?;
        let mut offset: u64 = 0;
        let mut buf = vec![0u8; self.chunk_size];
        loop {
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let start = offset;
            let end = offset + n as u64 - 1;
            let total = size;
            let content_range = format!("bytes {start}-{end}/{total}");
            let is_last = end + 1 >= total;
            let resp = self
                .put_chunk(&location, &buf[..n], &content_range, is_last)
                .await?;
            offset = end + 1;
            if is_last {
                let status = resp.status();
                let text = resp.text().await?;
                if !status.is_success() {
                    return Err(Error::Drive {
                        status: status.as_u16(),
                        message: text,
                    });
                }
                return serde_json::from_str(&text).map_err(|e| Error::Drive {
                    status: 200,
                    message: format!("malformed upload response: {e}"),
                });
            }
        }
        // Zero-length file.
        let resp = self
            .put_chunk(&location, &[], &format!("bytes */{size}"), true)
            .await?;
        let text = resp.text().await?;
        serde_json::from_str(&text).map_err(|e| Error::Drive {
            status: 200,
            message: format!("malformed upload response: {e}"),
        })
    }

    async fn put_chunk(
        &self,
        location: &str,
        chunk: &[u8],
        content_range: &str,
        _is_last: bool,
    ) -> Result<reqwest::Response> {
        let mut attempt = 0u32;
        loop {
            let token = self.tokens.access_token().await?;
            let resp = self
                .http
                .put(location)
                .bearer_auth(&token)
                .header("Content-Range", content_range)
                .body(chunk.to_vec())
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status().as_u16();
                    if status == 308 || r.status().is_success() {
                        return Ok(r);
                    }
                    if (status == 429 || r.status().is_server_error()) && attempt < self.max_retries {
                        self.backoff(attempt, None).await;
                        attempt += 1;
                        continue;
                    }
                    let body = r.text().await.unwrap_or_default();
                    return Err(Error::Drive {
                        status,
                        message: body,
                    });
                }
                Err(e) if attempt < self.max_retries && (e.is_timeout() || e.is_connect()) => {
                    self.backoff(attempt, None).await;
                    attempt += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    // -- About --------------------------------------------------------------

    /// Fetch storage quota information.
    pub async fn about_quota(&self) -> Result<DriveQuota> {
        let url = format!("{API}/about?fields=storageQuota");
        let resp = self
            .send_with_retry(|token| self.http.get(&url).bearer_auth(token))
            .await?;
        #[derive(Deserialize)]
        struct About {
            #[serde(default, rename = "storageQuota")]
            storage_quota: Option<DriveQuota>,
        }
        let about: About = resp.json().await?;
        about.storage_quota.ok_or_else(|| Error::Drive {
            status: 200,
            message: "about.get returned no storageQuota".into(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct ListResponse {
    #[serde(default)]
    files: Vec<DriveFile>,
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
}

fn escape_query(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

fn parse_retry_after(resp: &reqwest::Response) -> Option<u64> {
    resp.headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
}

fn is_rate_limit(body: &str) -> bool {
    body.contains("rateLimitExceeded")
        || body.contains("userRateLimitExceeded")
        || body.contains("quotaExceeded")
        || body.contains("dailyLimitExceeded")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_escaping() {
        assert_eq!(escape_query("it's"), "it\\'s");
    }

    #[test]
    fn drive_file_helpers() {
        let f = DriveFile {
            id: "1".into(),
            name: "x".into(),
            mime_type: FOLDER_MIME.into(),
            size: Some("10".into()),
            trashed: Some(false),
            md5_checksum: None,
        };
        assert!(f.is_folder());
        assert_eq!(f.size_u64(), 10);
    }
}
