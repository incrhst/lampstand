//! Free song databases a church can start its library from.
//!
//! Worship Leader App (worshipleaderapp.com) publishes its song database,
//! rebuilt daily, per language as OpenSongApp backups (`.osb`, a zip of
//! OpenSong song files), which `lampstand-import` reads. The database has
//! no copyright or CCLI fields, and many of its songs are under copyright:
//! projecting them is covered by the church's own licence (CCLI or
//! similar), as with any song typed in by hand. Nothing here is bundled
//! with Lampstand; the operator chooses to download.

use std::path::{Path, PathBuf};

use crate::FetchError;

/// One downloadable database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SongDatabase {
    /// The site's language code, as in its download URLs.
    pub code: &'static str,
    pub name: &'static str,
}

const fn db(code: &'static str, name: &'static str) -> SongDatabase {
    SongDatabase { code, name }
}

/// The per-language databases, as listed on
/// <https://worshipleaderapp.com/en/download-song-database-opensong-openlp-and-quelea>
/// (October 2026). The full multi-language database (72,000 songs) is only
/// offered as 7-Zip, RAR and Zip archives of loose files, so it is not here.
pub const WORSHIP_LEADER_APP: &[SongDatabase] = &[
    db("af", "Afrikaans"),
    db("sq", "Albanian"),
    db("ar", "Arabic"),
    db("ar-EN", "Arabic (Latin script)"),
    db("hy", "Armenian"),
    db("az", "Azerbaijani"),
    db("az-RU", "Azeri (Cyrillic script)"),
    db("be", "Belarusian"),
    db("bg", "Bulgarian"),
    db("zh", "Chinese"),
    db("zh-pn", "Chinese (Pinyin)"),
    db("cs", "Czech"),
    db("en", "English"),
    db("fi", "Finnish"),
    db("fr", "French"),
    db("de", "German"),
    db("cnh", "Hakha Chin"),
    db("ha", "Hausa"),
    db("hu", "Hungarian"),
    db("it", "Italian"),
    db("kaa", "Kara-Kalpak"),
    db("kk", "Kazakh"),
    db("kk-CN", "Kazakh (Arabic script)"),
    db("kk-EN", "Kazakh (Latin script)"),
    db("ko", "Korean"),
    db("kmr", "Kurdish"),
    db("kmr-AR", "Kurdish (Kurmanji, Arabic script)"),
    db("ky", "Kyrgyz"),
    db("ky-CN", "Kyrgyz (Arabic script)"),
    db("mn", "Mongolian"),
    db("mn-TR", "Mongolian (Traditional)"),
    db("no", "Norwegian"),
    db("pl", "Polish"),
    db("pt", "Portuguese"),
    db("ro", "Romanian"),
    db("ru", "Russian"),
    db("ru-ca", "Russian (Central Asia)"),
    db("sk", "Slovak"),
    db("es", "Spanish"),
    db("sw", "Swahili"),
    db("sv", "Swedish"),
    db("tg", "Tajik"),
    db("tt", "Tatar"),
    db("te", "Telugu"),
    db("th", "Thai"),
    db("aeb", "Tunisian Arabic (Tounsi)"),
    db("tr", "Turkish"),
    db("tr-RU", "Turkish (Cyrillic script)"),
    db("tk-AR", "Turkmen (Arabic script)"),
    db("tk", "Turkmen (Cyrillic script)"),
    db("tk-EN", "Turkmen (Latin script)"),
    db("udm", "Udmurt"),
    db("uk", "Ukrainian"),
    db("ug-CN", "Uyghur (Arabic script)"),
    db("ug", "Uyghur (Cyrillic script)"),
    db("uz", "Uzbek (Cyrillic script)"),
    db("uz-EN", "Uzbek (Latin script)"),
];

const BASE: &str = "https://worshipleaderapp.com/download/opensong";

/// The largest database is about 10 MB; anything far bigger is not one.
const MAX_BYTES: usize = 200 * 1024 * 1024;

/// Downloads one language's database into `dir` as `<code>.osb` and
/// returns its path. Only codes in [`WORSHIP_LEADER_APP`] are fetched.
pub async fn download_worship_leader_app(code: &str, dir: &Path) -> Result<PathBuf, FetchError> {
    download_from(BASE, code, dir).await
}

async fn download_from(base: &str, code: &str, dir: &Path) -> Result<PathBuf, FetchError> {
    if !WORSHIP_LEADER_APP.iter().any(|d| d.code == code) {
        return Err(FetchError::NotFound);
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("lampstand/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| FetchError::Network(e.to_string()))?;
    let response = client
        .get(format!("{base}/{code}.osb"))
        .send()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;
    match response.status().as_u16() {
        200 => {}
        401 | 403 => return Err(FetchError::Forbidden),
        404 => return Err(FetchError::NotFound),
        status => return Err(FetchError::Network(format!("HTTP {status}"))),
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(FetchError::Network("the download is too large".into()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| FetchError::Network(e.to_string()))?;
    if bytes.len() > MAX_BYTES {
        return Err(FetchError::Network("the download is too large".into()));
    }
    // A song database is a zip; an HTML error page is not.
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(FetchError::Network(
            "the download is not a song database".into(),
        ));
    }
    let io = |e: std::io::Error| FetchError::Network(e.to_string());
    std::fs::create_dir_all(dir).map_err(io)?;
    let path = dir.join(format!("{code}.osb"));
    let tmp = dir.join(format!("{code}.osb.part"));
    std::fs::write(&tmp, &bytes).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ZIP: &[u8] = b"PK\x03\x04 rest of a zip";

    #[tokio::test]
    async fn downloads_a_language_to_its_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/opensong/en.osb"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(ZIP))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let base = format!("{}/opensong", server.uri());
        let file = download_from(&base, "en", dir.path()).await.unwrap();
        assert_eq!(file, dir.path().join("en.osb"));
        assert_eq!(std::fs::read(file).unwrap(), ZIP);
    }

    #[tokio::test]
    async fn refuses_unknown_codes_html_and_missing_files() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/opensong/fr.osb"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>sorry</html>"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let base = format!("{}/opensong", server.uri());
        assert!(matches!(
            download_from(&base, "../etc", dir.path()).await,
            Err(FetchError::NotFound)
        ));
        assert!(matches!(
            download_from(&base, "fr", dir.path()).await,
            Err(FetchError::Network(_))
        ));
        assert!(matches!(
            download_from(&base, "de", dir.path()).await,
            Err(FetchError::NotFound)
        ));
        assert!(!dir.path().join("fr.osb").exists());
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<_> = WORSHIP_LEADER_APP.iter().map(|d| d.code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), WORSHIP_LEADER_APP.len());
    }
}
