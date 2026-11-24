//! Cloud translation providers targeting Vietnamese (`vi`) exclusively:
//! MyMemory, Azure Translator (official REST v3.0), and Google Cloud Translation (official REST v2),
//! plus privacy consent enforcement, in-flight deduplication, cache integration, and stale protection.

use async_trait::async_trait;
use luma_core::{AppSettings, ProviderCredentials, TranslationProviderKind};
use luma_storage::{CacheKey, LumaStore};
use parking_lot::Mutex;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::broadcast;

pub const DESTINATION_LANGUAGE_VI: &str = "vi";
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(8);
const MYMEMORY_MAX_QUERY_CHARS: usize = 480;

#[derive(Debug, Error, Clone)]
pub enum TranslationError {
    #[error("Online translation requires user privacy consent before sending OCR text")]
    ConsentRequired,
    #[error("Source text is empty")]
    EmptyInput,
    #[error("Provider '{0}' is missing required API credentials")]
    MissingCredentials(String),
    #[error("Provider '{0}' quota or rate limit exceeded: {1}")]
    QuotaExceeded(String, String),
    #[error("Request timed out contacting provider '{0}'")]
    Timeout(String),
    #[error("Network error contacting provider '{0}': {1}")]
    Network(String, String),
    #[error("Invalid response from provider '{0}': {1}")]
    InvalidResponse(String, String),
    #[error("Translation request cancelled")]
    Cancelled,
}

impl TranslationError {
    fn short_detail(detail: &str) -> String {
        let one_line: String = detail
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if one_line.chars().count() > 140 {
            let truncated: String = one_line.chars().take(140).collect();
            format!("{truncated}...")
        } else {
            one_line
        }
    }

    pub fn user_message_vi(&self) -> String {
        match self {
            Self::ConsentRequired => {
                "Vui lòng xác nhận đồng ý gửi văn bản OCR lên dịch vụ đám mây trước khi dịch.".to_string()
            }
            Self::EmptyInput => "Không có văn bản nguồn để dịch.".to_string(),
            Self::MissingCredentials(p) => {
                if p.contains("Azure") {
                    format!("Chưa cấu hình khóa API hoặc Vùng (Region, ví dụ: southeastasia) hợp lệ cho {p}.")
                } else {
                    format!("Chưa cấu hình khóa API hợp lệ cho {p}.")
                }
            }
            Self::QuotaExceeded(p, detail) => {
                let extra = Self::short_detail(detail);
                if p.contains("MyMemory") {
                    format!(
                        "Đã chạm giới hạn tốc độ/hạn mức miễn phí trên {p}{}. Hãy điền Email liên hệ MyMemory trong Cài đặt để nâng hạn mức hoặc cấu hình Azure/Google.",
                        if extra.is_empty() { String::new() } else { format!(" ({extra})") }
                    )
                } else {
                    format!(
                        "Đã hết hạn mức hoặc bị giới hạn tốc độ trên {p}{}.",
                        if extra.is_empty() { String::new() } else { format!(" ({extra})") }
                    )
                }
            }
            Self::Timeout(p) => format!("Quá thời gian chờ phản hồi từ {p}."),
            Self::Network(p, detail) => {
                let extra = Self::short_detail(detail);
                if extra.is_empty() {
                    format!("Không thể kết nối tới {p}. Vui lòng kiểm tra kết nối Internet.")
                } else {
                    format!("Không thể kết nối tới {p}: {extra}")
                }
            }
            Self::InvalidResponse(p, detail) => {
                let extra = Self::short_detail(detail);
                if extra.is_empty() {
                    format!("Phản hồi không hợp lệ từ nhà cung cấp {p}.")
                } else {
                    format!("Phản hồi lỗi từ {p}: {extra}")
                }
            }
            Self::Cancelled => "Yêu cầu dịch đã bị hủy.".to_string(),
        }
    }
}

#[async_trait]
pub trait TranslationProvider: Send + Sync {
    async fn translate(
        &self,
        source_text: &str,
        source_language: Option<&str>,
    ) -> Result<String, TranslationError>;
}

/// Provider A — MyMemory REST API (`https://api.mymemory.translated.net/get`)
pub struct MyMemoryProvider {
    client: Client,
    base_url: String,
    contact_email: Option<String>,
}

impl MyMemoryProvider {
    pub fn new(client: Client, contact_email: Option<String>) -> Self {
        Self::with_base_url(
            client,
            "https://api.mymemory.translated.net/get".to_string(),
            contact_email,
        )
    }

    pub fn with_base_url(client: Client, base_url: String, contact_email: Option<String>) -> Self {
        Self {
            client,
            base_url,
            contact_email: contact_email
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }

    fn normalize_langpair_source(source_language: Option<&str>) -> &'static str {
        match source_language
            .map(str::trim)
            .unwrap_or("auto")
            .to_lowercase()
            .as_str()
        {
            "en" | "eng" | "english" => "en",
            "ja" | "jp" | "japan" | "japanese" => "ja",
            "zh" | "zh-cn" | "zh-hans" | "ch" | "chinese" => "zh-CN",
            "zh-tw" | "zh-hant" | "chinese_cht" => "zh-TW",
            "ko" | "kor" | "korean" => "ko",
            "fr" => "fr",
            "de" => "de",
            "es" => "es",
            "it" => "it",
            "pt" => "pt",
            "ru" => "ru",
            "latin" => "autodetect",
            "auto" | "" => "auto",
            _ => "autodetect",
        }
    }

    fn infer_script_lang(text: &str) -> &'static str {
        let mut has_kana = false;
        let mut has_hangul = false;
        let mut has_han = false;
        let mut has_non_ascii_latin = false;
        for ch in text.chars() {
            let code = ch as u32;
            if (0x3040..=0x30FF).contains(&code) || (0x31F0..=0x31FF).contains(&code) {
                has_kana = true;
            } else if (0xAC00..=0xD7AF).contains(&code) || (0x1100..=0x11FF).contains(&code) {
                has_hangul = true;
            } else if (0x4E00..=0x9FFF).contains(&code) {
                has_han = true;
            } else if (0x00C0..=0x024F).contains(&code) {
                has_non_ascii_latin = true;
            }
        }
        if has_kana {
            "ja"
        } else if has_hangul {
            "ko"
        } else if has_han {
            "zh-CN"
        } else if has_non_ascii_latin {
            "autodetect"
        } else {
            "en"
        }
    }

    fn chunk_text_respecting_limit(text: &str) -> Vec<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if trimmed.chars().count() <= MYMEMORY_MAX_QUERY_CHARS && trimmed.len() <= 490 {
            return vec![trimmed.to_string()];
        }
        let mut chunks = Vec::new();
        let mut current = String::new();
        for ch in trimmed.chars() {
            if current.chars().count() + 1 > MYMEMORY_MAX_QUERY_CHARS
                || current.len() + ch.len_utf8() > 480
            {
                let part = current.trim();
                if !part.is_empty() {
                    chunks.push(part.to_string());
                }
                current.clear();
            }
            current.push(ch);
            if matches!(ch, '.' | '!' | '?' | '。' | '！' | '？' | '\n') && current.len() >= 180 {
                let part = current.trim();
                if !part.is_empty() {
                    chunks.push(part.to_string());
                }
                current.clear();
            }
        }
        let tail = current.trim();
        if !tail.is_empty() {
            chunks.push(tail.to_string());
        }
        chunks
    }
}

#[derive(Debug, Deserialize)]
struct MyMemoryResponse {
    #[serde(rename = "responseData")]
    response_data: Option<MyMemoryResponseData>,
    #[serde(rename = "responseStatus")]
    response_status: Option<serde_json::Value>,
    #[serde(rename = "responseDetails")]
    response_details: Option<String>,
    #[serde(rename = "quotaFinished")]
    quota_finished: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MyMemoryResponseData {
    #[serde(rename = "translatedText")]
    translated_text: Option<String>,
}

#[async_trait]
impl TranslationProvider for MyMemoryProvider {
    async fn translate(
        &self,
        source_text: &str,
        source_language: Option<&str>,
    ) -> Result<String, TranslationError> {
        let trimmed = source_text.trim();
        if trimmed.is_empty() {
            return Err(TranslationError::EmptyInput);
        }

        let mapped_src = match Self::normalize_langpair_source(source_language) {
            "auto" => Self::infer_script_lang(trimmed),
            other => other,
        };
        let langpair = format!("{mapped_src}|{DESTINATION_LANGUAGE_VI}");
        let chunks = Self::chunk_text_respecting_limit(trimmed);
        let mut translated_parts = Vec::with_capacity(chunks.len());

        for chunk in chunks {
            let mut query_params = vec![("q", chunk.as_str()), ("langpair", langpair.as_str())];
            if let Some(ref email) = self.contact_email {
                query_params.push(("de", email.as_str()));
            }

            let resp = self
                .client
                .get(&self.base_url)
                .query(&query_params)
                .send()
                .await
                .map_err(|e| map_reqwest_err("MyMemory", e))?;

            let status = resp.status();
            if status.as_u16() == 429 {
                return Err(TranslationError::QuotaExceeded(
                    "MyMemory".into(),
                    "HTTP 429 Too Many Requests".into(),
                ));
            }
            if !status.is_success() {
                return Err(TranslationError::InvalidResponse(
                    "MyMemory".into(),
                    format!("HTTP {status}"),
                ));
            }

            let parsed: MyMemoryResponse = resp.json().await.map_err(|e| {
                TranslationError::InvalidResponse("MyMemory".into(), e.to_string())
            })?;

            if parsed.quota_finished == Some(true) {
                return Err(TranslationError::QuotaExceeded(
                    "MyMemory".into(),
                    parsed
                        .response_details
                        .unwrap_or_else(|| "Daily free quota exhausted".into()),
                ));
            }

            if let Some(code_val) = &parsed.response_status {
                let code_num = code_val
                    .as_i64()
                    .or_else(|| code_val.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .unwrap_or(200);
                if code_num == 429 || code_num == 403 {
                    return Err(TranslationError::QuotaExceeded(
                        "MyMemory".into(),
                        parsed
                            .response_details
                            .unwrap_or_else(|| format!("Status {code_num}")),
                    ));
                }
                if code_num != 200 {
                    return Err(TranslationError::InvalidResponse(
                        "MyMemory".into(),
                        parsed
                            .response_details
                            .unwrap_or_else(|| format!("Status {code_num}")),
                    ));
                }
            }

            let out = parsed
                .response_data
                .and_then(|d| d.translated_text)
                .map(|s| decode_basic_html_entities(s.trim()))
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    TranslationError::InvalidResponse(
                        "MyMemory".into(),
                        "Empty translatedText".into(),
                    )
                })?;

            if out.to_uppercase().contains("MYMEMORY WARNING:") {
                return Err(TranslationError::QuotaExceeded("MyMemory".into(), out));
            }

            translated_parts.push(out);
        }

        Ok(translated_parts.join(" "))
    }
}

/// Provider B — Official Microsoft Azure Translator REST API v3.0 (supports F0 Free tier)
pub struct AzureTranslatorProvider {
    client: Client,
    endpoint: String,
    api_key: String,
    region: Option<String>,
}

impl AzureTranslatorProvider {
    pub fn new(
        client: Client,
        api_key: String,
        region: Option<String>,
        endpoint: Option<String>,
    ) -> Self {
        let base = endpoint
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://api.cognitive.microsofttranslator.com".to_string());
        Self {
            client,
            endpoint: base,
            api_key: api_key.trim().to_string(),
            region: region
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        }
    }

    fn normalize_source_code(source_language: Option<&str>) -> Option<&'static str> {
        match source_language
            .map(str::trim)
            .unwrap_or("auto")
            .to_lowercase()
            .as_str()
        {
            "" | "auto" => None,
            "en" | "eng" | "english" => Some("en"),
            "ja" | "jp" | "japan" | "japanese" => Some("ja"),
            "zh" | "zh-cn" | "zh-hans" | "ch" | "chinese" => Some("zh-Hans"),
            "zh-tw" | "zh-hant" | "chinese_cht" => Some("zh-Hant"),
            "ko" | "kor" | "korean" => Some("ko"),
            "fr" => Some("fr"),
            "de" => Some("de"),
            "es" => Some("es"),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AzureTranslateItem {
    translations: Vec<AzureTranslationEntry>,
}

#[derive(Debug, Deserialize)]
struct AzureTranslationEntry {
    text: String,
}

#[async_trait]
impl TranslationProvider for AzureTranslatorProvider {
    async fn translate(
        &self,
        source_text: &str,
        source_language: Option<&str>,
    ) -> Result<String, TranslationError> {
        let trimmed = source_text.trim();
        if trimmed.is_empty() {
            return Err(TranslationError::EmptyInput);
        }
        if self.api_key.is_empty() {
            return Err(TranslationError::MissingCredentials(
                "Azure Translator".into(),
            ));
        }

        let url = format!("{}/translate", self.endpoint);
        let mut query = vec![("api-version", "3.0"), ("to", DESTINATION_LANGUAGE_VI)];
        if let Some(from_code) = Self::normalize_source_code(source_language) {
            query.push(("from", from_code));
        }

        let mut req = self
            .client
            .post(&url)
            .query(&query)
            .header("Ocp-Apim-Subscription-Key", &self.api_key)
            .json(&serde_json::json!([{ "Text": trimmed }]));

        if let Some(ref reg) = self.region {
            req = req.header("Ocp-Apim-Subscription-Region", reg);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| map_reqwest_err("Azure Translator", e))?;

        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("403000") || body.contains("403001") || body.to_lowercase().contains("quota") {
                return Err(TranslationError::QuotaExceeded("Azure Translator".into(), body));
            }
            return Err(TranslationError::MissingCredentials("Azure Translator".into()));
        }
        if status.as_u16() == 429 {
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslationError::QuotaExceeded("Azure Translator".into(), body));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslationError::InvalidResponse(
                "Azure Translator".into(),
                format!("HTTP {status}: {body}"),
            ));
        }

        let items: Vec<AzureTranslateItem> = resp.json().await.map_err(|e| {
            TranslationError::InvalidResponse("Azure Translator".into(), e.to_string())
        })?;

        items
            .into_iter()
            .next()
            .and_then(|it| it.translations.into_iter().next())
            .map(|t| t.text.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                TranslationError::InvalidResponse(
                    "Azure Translator".into(),
                    "Missing translation text".into(),
                )
            })
    }
}

/// Provider C — Official Google Cloud Translation Basic REST API v2
pub struct GoogleCloudProvider {
    client: Client,
    endpoint: String,
    api_key: String,
}

impl GoogleCloudProvider {
    pub fn new(client: Client, api_key: String) -> Self {
        Self::with_endpoint(
            client,
            "https://translation.googleapis.com/language/translate/v2".to_string(),
            api_key,
        )
    }

    pub fn with_endpoint(client: Client, endpoint: String, api_key: String) -> Self {
        Self {
            client,
            endpoint,
            api_key: api_key.trim().to_string(),
        }
    }

    fn normalize_source_code(source_language: Option<&str>) -> Option<&'static str> {
        match source_language
            .map(str::trim)
            .unwrap_or("auto")
            .to_lowercase()
            .as_str()
        {
            "" | "auto" => None,
            "en" | "eng" | "english" => Some("en"),
            "ja" | "jp" | "japan" | "japanese" => Some("ja"),
            "zh" | "zh-cn" | "zh-hans" | "ch" | "chinese" => Some("zh-CN"),
            "zh-tw" | "zh-hant" | "chinese_cht" => Some("zh-TW"),
            "ko" | "kor" | "korean" => Some("ko"),
            "fr" => Some("fr"),
            "de" => Some("de"),
            "es" => Some("es"),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GoogleTranslateResponse {
    data: GoogleTranslateData,
}

#[derive(Debug, Deserialize)]
struct GoogleTranslateData {
    translations: Vec<GoogleTranslateEntry>,
}

#[derive(Debug, Deserialize)]
struct GoogleTranslateEntry {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

#[async_trait]
impl TranslationProvider for GoogleCloudProvider {
    async fn translate(
        &self,
        source_text: &str,
        source_language: Option<&str>,
    ) -> Result<String, TranslationError> {
        let trimmed = source_text.trim();
        if trimmed.is_empty() {
            return Err(TranslationError::EmptyInput);
        }
        if self.api_key.is_empty() {
            return Err(TranslationError::MissingCredentials(
                "Google Cloud Translation".into(),
            ));
        }

        let mut payload = serde_json::json!({
            "q": trimmed,
            "target": DESTINATION_LANGUAGE_VI,
            "format": "text",
        });
        if let Some(src) = Self::normalize_source_code(source_language) {
            payload["source"] = serde_json::Value::String(src.to_string());
        }

        let resp = self
            .client
            .post(&self.endpoint)
            .header("X-Goog-Api-Key", &self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| map_reqwest_err("Google Cloud Translation", e))?;

        let status = resp.status();
        if status.as_u16() == 429 {
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslationError::QuotaExceeded(
                "Google Cloud Translation".into(),
                body,
            ));
        }
        if status.as_u16() == 403 {
            let body = resp.text().await.unwrap_or_default();
            if body.to_lowercase().contains("limit") || body.to_lowercase().contains("quota") {
                return Err(TranslationError::QuotaExceeded(
                    "Google Cloud Translation".into(),
                    body,
                ));
            }
            return Err(TranslationError::MissingCredentials(
                "Google Cloud Translation".into(),
            ));
        }
        if status.as_u16() == 400 || status.as_u16() == 401 {
            let body = resp.text().await.unwrap_or_default();
            let lower = body.to_lowercase();
            if status.as_u16() == 401 || lower.contains("api_key_invalid") || lower.contains("keyinvalid") || lower.contains("api key not valid") {
                return Err(TranslationError::MissingCredentials(
                    "Google Cloud Translation".into(),
                ));
            }
            return Err(TranslationError::InvalidResponse(
                "Google Cloud Translation".into(),
                body,
            ));
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(TranslationError::InvalidResponse(
                "Google Cloud Translation".into(),
                format!("HTTP {status}: {body}"),
            ));
        }

        let parsed: GoogleTranslateResponse = resp.json().await.map_err(|e| {
            TranslationError::InvalidResponse("Google Cloud Translation".into(), e.to_string())
        })?;

        parsed
            .data
            .translations
            .into_iter()
            .next()
            .map(|t| t.translated_text.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                TranslationError::InvalidResponse(
                    "Google Cloud Translation".into(),
                    "Empty translations array".into(),
                )
            })
    }
}

fn map_reqwest_err(provider: &str, err: reqwest::Error) -> TranslationError {
    if err.is_timeout() {
        TranslationError::Timeout(provider.to_string())
    } else {
        TranslationError::Network(provider.to_string(), err.to_string())
    }
}

fn decode_basic_html_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[derive(Debug, Clone)]
pub struct TranslationOutcome {
    pub vietnamese_text: String,
    pub provider_used: String,
    pub from_cache: bool,
    pub latency_ms: u64,
}

/// High-level Cloud Translation Manager coordinating privacy consent, persistent SQLite + LRU cache,
/// in-flight request deduplication, authorized fallback providers, and sequence ordering.
pub struct CloudTranslationManager {
    http_client: Client,
    store: Arc<LumaStore>,
    inflight: Mutex<HashMap<CacheKey, broadcast::Sender<Result<String, TranslationError>>>>,
    custom_providers: Mutex<HashMap<TranslationProviderKind, Arc<dyn TranslationProvider>>>,
}

impl CloudTranslationManager {
    pub fn new(store: Arc<LumaStore>) -> Self {
        let http_client = Client::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .user_agent("LumaTranslate/0.1")
            .build()
            .expect("Failed to construct reqwest Client");

        Self {
            http_client,
            store,
            inflight: Mutex::new(HashMap::new()),
            custom_providers: Mutex::new(HashMap::new()),
        }
    }

    /// Inject mock or test provider implementations for deterministic offline integration tests.
    pub fn register_custom_provider(
        &self,
        kind: TranslationProviderKind,
        provider: Arc<dyn TranslationProvider>,
    ) {
        self.custom_providers.lock().insert(kind, provider);
    }

    fn build_provider(
        &self,
        kind: TranslationProviderKind,
        creds: &ProviderCredentials,
    ) -> Result<(Arc<dyn TranslationProvider>, String), TranslationError> {
        if let Some(custom) = self.custom_providers.lock().get(&kind) {
            return Ok((Arc::clone(custom), format!("custom-{}", kind.as_str())));
        }

        match kind {
            TranslationProviderKind::MyMemory => {
                let tag = format!(
                    "mymemory:v1:{}",
                    creds.mymemory_email.as_deref().unwrap_or("anon")
                );
                Ok((
                    Arc::new(MyMemoryProvider::new(
                        self.http_client.clone(),
                        creds.mymemory_email.clone(),
                    )),
                    tag,
                ))
            }
            TranslationProviderKind::Azure => {
                let key = creds
                    .azure_api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        TranslationError::MissingCredentials("Azure Translator".into())
                    })?;
                let tag = format!(
                    "azure:v3:{}",
                    creds.azure_region.as_deref().unwrap_or("global")
                );
                Ok((
                    Arc::new(AzureTranslatorProvider::new(
                        self.http_client.clone(),
                        key,
                        creds.azure_region.clone(),
                        creds.azure_endpoint.clone(),
                    )),
                    tag,
                ))
            }
            TranslationProviderKind::Google => {
                let key = creds
                    .google_api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        TranslationError::MissingCredentials("Google Cloud Translation".into())
                    })?;
                let tag = "google:v2".to_string();
                Ok((
                    Arc::new(GoogleCloudProvider::new(self.http_client.clone(), key)),
                    tag,
                ))
            }
        }
    }

    fn is_provider_configured(kind: TranslationProviderKind, creds: &ProviderCredentials) -> bool {
        match kind {
            TranslationProviderKind::MyMemory => true,
            TranslationProviderKind::Azure => creds
                .azure_api_key
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
            TranslationProviderKind::Google => creds
                .google_api_key
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
        }
    }

    pub async fn translate_with_cache(
        &self,
        source_text: &str,
        source_language: Option<&str>,
        settings: &AppSettings,
        cancel_token: &tokio_util::sync::CancellationToken,
    ) -> Result<TranslationOutcome, TranslationError> {
        let trimmed = source_text.trim();
        if trimmed.is_empty() {
            return Err(TranslationError::EmptyInput);
        }
        if !settings.cloud_consent_granted {
            return Err(TranslationError::ConsentRequired);
        }

        // Build ordered chain: active provider + only user-authorized & configured fallback providers.
        let mut chain = vec![settings.active_provider];
        for &fb in &settings.fallback_providers {
            if !chain.contains(&fb)
                && (Self::is_provider_configured(fb, &settings.credentials)
                    || self.custom_providers.lock().contains_key(&fb))
            {
                chain.push(fb);
            }
        }

        let mut last_err: Option<TranslationError> = None;

        for provider_kind in chain {
            if cancel_token.is_cancelled() {
                return Err(TranslationError::Cancelled);
            }

            let (provider, cfg_tag) = match self.build_provider(provider_kind, &settings.credentials)
            {
                Ok(pair) => pair,
                Err(err) => {
                    last_err = Some(err);
                    continue;
                }
            };

            let cache_key = CacheKey::new(
                trimmed,
                source_language,
                provider_kind.as_str(),
                &cfg_tag,
            );

            let t0 = Instant::now();
            if let Ok(Some(cached)) = self.store.get_cached_translation(&cache_key) {
                return Ok(TranslationOutcome {
                    vietnamese_text: cached,
                    provider_used: provider_kind.display_name().to_string(),
                    from_cache: true,
                    latency_ms: t0.elapsed().as_millis() as u64,
                });
            }

            // Coalesce identical concurrent in-flight requests so rapid multi-region duplicates
            // never hammer cloud APIs.
            let existing_rx = {
                let mut map = self.inflight.lock();
                if let Some(tx) = map.get(&cache_key) {
                    Some(tx.subscribe())
                } else {
                    let (tx, _) = broadcast::channel(4);
                    map.insert(cache_key.clone(), tx);
                    None
                }
            };

            let result = if let Some(mut rx) = existing_rx {
                tokio::select! {
                    _ = cancel_token.cancelled() => return Err(TranslationError::Cancelled),
                    recv = rx.recv() => match recv {
                        Ok(res) => res,
                        Err(_) => provider.translate(trimmed, source_language).await,
                    }
                }
            } else {
                let res = tokio::select! {
                    _ = cancel_token.cancelled() => Err(TranslationError::Cancelled),
                    out = provider.translate(trimmed, source_language) => out,
                };
                if let Some(tx) = self.inflight.lock().remove(&cache_key) {
                    let _ = tx.send(res.clone());
                }
                res
            };

            match result {
                Ok(vi_text) => {
                    let _ = self.store.put_cached_translation(cache_key, &vi_text);
                    return Ok(TranslationOutcome {
                        vietnamese_text: vi_text,
                        provider_used: provider_kind.display_name().to_string(),
                        from_cache: false,
                        latency_ms: t0.elapsed().as_millis().max(1) as u64,
                    });
                }
                Err(TranslationError::Cancelled) => return Err(TranslationError::Cancelled),
                Err(err) => {
                    tracing::warn!(
                        provider = provider_kind.display_name(),
                        error = %err,
                        "Translation attempt failed; checking authorized fallback"
                    );
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            TranslationError::InvalidResponse("None".into(), "No translation provider succeeded".into())
        }))
    }

    pub async fn test_provider_connection(
        &self,
        kind: TranslationProviderKind,
        creds: &ProviderCredentials,
    ) -> Result<(String, u64), TranslationError> {
        let (provider, _) = self.build_provider(kind, creds)?;
        let t0 = Instant::now();
        let vi = provider
            .translate("Hello! Nice to meet you.", Some("en"))
            .await?;
        Ok((vi, t0.elapsed().as_millis().max(1) as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio_util::sync::CancellationToken;

    struct CountingMockProvider {
        calls: Arc<AtomicUsize>,
        delay_ms: u64,
        fail_quota: bool,
    }

    #[async_trait]
    impl TranslationProvider for CountingMockProvider {
        async fn translate(
            &self,
            source_text: &str,
            _source_language: Option<&str>,
        ) -> Result<String, TranslationError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            if self.fail_quota {
                return Err(TranslationError::QuotaExceeded(
                    "MockPrimary".into(),
                    "Quota reached".into(),
                ));
            }
            Ok(format!("VI: {source_text}"))
        }
    }

    fn temp_store(name: &str) -> Arc<LumaStore> {
        let path = std::env::temp_dir().join(format!(
            "luma_trans_test_{}_{}.db",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Arc::new(LumaStore::open_at_path(path, 64).unwrap())
    }

    #[tokio::test]
    async fn enforces_user_consent_and_caches_and_deduplicates_inflight_requests() {
        let store = temp_store("cache_dedup");
        let mgr = Arc::new(CloudTranslationManager::new(store));
        let calls = Arc::new(AtomicUsize::new(0));
        mgr.register_custom_provider(
            TranslationProviderKind::MyMemory,
            Arc::new(CountingMockProvider {
                calls: Arc::clone(&calls),
                delay_ms: 40,
                fail_quota: false,
            }),
        );

        let token = CancellationToken::new();
        let mut settings = AppSettings::default();
        assert!(matches!(
            mgr.translate_with_cache("Hello", Some("en"), &settings, &token)
                .await,
            Err(TranslationError::ConsentRequired)
        ));

        settings.cloud_consent_granted = true;

        // Fire two concurrent identical requests -> must coalesce into 1 provider call!
        let m1 = Arc::clone(&mgr);
        let m2 = Arc::clone(&mgr);
        let s1 = settings.clone();
        let s2 = settings.clone();
        let t1 = token.clone();
        let t2 = token.clone();
        let (r1, r2) = tokio::join!(
            async move { m1.translate_with_cache("また会えて嬉しい。", Some("ja"), &s1, &t1).await },
            async move { m2.translate_with_cache("また会えて嬉しい。", Some("ja"), &s2, &t2).await }
        );
        assert_eq!(r1.unwrap().vietnamese_text, "VI: また会えて嬉しい。");
        assert_eq!(r2.unwrap().vietnamese_text, "VI: また会えて嬉しい。");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Subsequent call hits persistent cache without network call
        let cached = mgr
            .translate_with_cache("また会えて嬉しい。", Some("ja"), &settings, &token)
            .await
            .unwrap();
        assert!(cached.from_cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn falls_back_only_to_authorized_providers_and_supports_cancellation() {
        let store = temp_store("fallback_cancel");
        let mgr = CloudTranslationManager::new(store);
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let azure_calls = Arc::new(AtomicUsize::new(0));

        mgr.register_custom_provider(
            TranslationProviderKind::MyMemory,
            Arc::new(CountingMockProvider {
                calls: Arc::clone(&primary_calls),
                delay_ms: 0,
                fail_quota: true,
            }),
        );
        mgr.register_custom_provider(
            TranslationProviderKind::Azure,
            Arc::new(CountingMockProvider {
                calls: Arc::clone(&azure_calls),
                delay_ms: 80,
                fail_quota: false,
            }),
        );

        let mut settings = AppSettings {
            cloud_consent_granted: true,
            active_provider: TranslationProviderKind::MyMemory,
            fallback_providers: Vec::new(), // Not authorized yet
            ..Default::default()
        };

        let token = CancellationToken::new();
        assert!(matches!(
            mgr.translate_with_cache("Line 1", Some("en"), &settings, &token)
                .await,
            Err(TranslationError::QuotaExceeded(_, _))
        ));
        assert_eq!(azure_calls.load(Ordering::SeqCst), 0);

        // Authorize Azure fallback
        settings
            .fallback_providers
            .push(TranslationProviderKind::Azure);
        let ok = mgr
            .translate_with_cache("Line 1", Some("en"), &settings, &token)
            .await
            .unwrap();
        assert_eq!(ok.provider_used, "Azure Translator");
        assert_eq!(ok.vietnamese_text, "VI: Line 1");

        // Test cancellation mid-request
        let cancel_tok = CancellationToken::new();
        let cancel_clone = cancel_tok.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            cancel_clone.cancel();
        });
        let cancelled = mgr
            .translate_with_cache("Line 2", Some("en"), &settings, &cancel_tok)
            .await;
        assert!(matches!(cancelled, Err(TranslationError::Cancelled)));
    }
}
