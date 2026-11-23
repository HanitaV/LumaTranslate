# Cloud Translation Providers (Vietnamese Target Only)

LumaTranslate translates recognized foreign-language dialogue exclusively into **Vietnamese (`vi`)**. No local LLM or local machine translation model is used.

## Privacy & User Consent

- **Only recognized OCR text strings** are transmitted over HTTPS to the user's selected translation provider.
- **Captured screen frames and cropped images never leave your PC.**
- Online translation remains disabled until the user explicitly grants consent via the Vietnamese privacy banner on the main screen or the checkbox in **Cài đặt** (`SettingsPage`).
- API keys are stored locally in `%LOCALAPPDATA%\LumaTranslate\data\luma_translate.db` and are never committed to Git.

## Supported Providers

### 1. MyMemory (Default — No API Key Required)
- **Endpoint**: `https://api.mymemory.translated.net/get`
- **Setup**: Works out-of-the-box for initial testing. Providing an optional contact email in **Cài đặt** increases the daily free character quota per MyMemory's official policy.
- **Safeguards**: Automatically chunks long dialogue below the 500-byte request limit, infers script language (`ja`, `ko`, `zh-CN`, `en`) when region source language is set to `auto`, and surfaces HTTP 429 / daily quota exhaustion clearly in Vietnamese.

### 2. Microsoft Azure Translator (Official REST v3.0 — Supports F0 Free Tier)
- **Endpoint**: `https://api.cognitive.microsofttranslator.com/translate?api-version=3.0&to=vi`
- **Setup**:
  1. Create an **Azure AI Translator** resource in the Azure Portal (select the **F0 Free** pricing tier for 2M characters/month free).
  2. Copy **Key 1** and the **Location/Region** (e.g., `southeastasia` or `eastus`; leave blank only for global resources).
  3. Paste them into **Cài đặt → Dịch đám mây** and click **Kiểm tra kết nối**.
- **Features**: Supports automatic source-language detection when a region is set to `Tự động`.

### 3. Google Cloud Translation (Official Basic REST API v2)
- **Endpoint**: `https://translation.googleapis.com/language/translate/v2`
- **Setup**:
  1. Enable the **Cloud Translation API** in Google Cloud Console (includes monthly free character credit).
  2. Restrict an API key to Cloud Translation API and enter it in **Cài đặt → Dịch đám mây**.
  3. Click **Kiểm tra kết nối** to verify latency and credentials.

## Fallback & Caching Behavior

- **Authorized Fallback Only**: Fallback providers are tried on network/quota failure **only** if the user explicitly checked the corresponding fallback box in **Cài đặt** and supplied valid credentials.
- **Two-Tier Persistent Cache**: Translations are cached in a bounded in-memory LRU (`1,024` entries) backed by SQLite (`%LOCALAPPDATA%\LumaTranslate\data\luma_translate.db`), keyed by `(source_text, source_language, "vi", provider, config_hash)`. Repeated game dialogue is displayed and spoken immediately with `0 ms` network cost.
