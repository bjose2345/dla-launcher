use dla_application::catalog_artwork::CatalogArtworkError;
use percent_encoding::percent_decode_str;
use tauri::{
    AppHandle, Manager,
    http::{
        Method, Request, Response, StatusCode,
        header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE},
    },
};

use crate::commands::AppState;

pub fn respond(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match response(app, &request) {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

fn response(
    app: &AppHandle,
    request: &Request<Vec<u8>>,
) -> Result<Response<Vec<u8>>, CatalogArtworkError> {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return Err(CatalogArtworkError::InvalidSource);
    }
    let source_url = parse_source_url(request)?;
    let state = app.state::<AppState>();
    let asset = state.cover_cache.load(&source_url)?;
    let builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, asset.media_type.content_type())
        .header(CONTENT_LENGTH, asset.bytes.len())
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "private, max-age=3600")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("X-DLA-Cover-Cache", asset.cache_status.as_str());
    if request.method() == Method::HEAD {
        return builder
            .body(Vec::new())
            .map_err(|error| CatalogArtworkError::Storage(error.to_string()));
    }
    builder
        .body(asset.bytes)
        .map_err(|error| CatalogArtworkError::Storage(error.to_string()))
}

fn parse_source_url(request: &Request<Vec<u8>>) -> Result<String, CatalogArtworkError> {
    if request.uri().query().is_some() {
        return Err(CatalogArtworkError::InvalidSource);
    }
    let encoded = request.uri().path().trim_start_matches('/');
    if encoded.is_empty() || encoded.contains('/') {
        return Err(CatalogArtworkError::InvalidSource);
    }
    percent_decode_str(encoded)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| CatalogArtworkError::InvalidSource)
}

fn error_response(error: CatalogArtworkError) -> Response<Vec<u8>> {
    let status = match error {
        CatalogArtworkError::InvalidSource => StatusCode::BAD_REQUEST,
        CatalogArtworkError::SourceNotAllowed => StatusCode::FORBIDDEN,
        CatalogArtworkError::NotFound => StatusCode::NOT_FOUND,
        CatalogArtworkError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        CatalogArtworkError::UnsupportedImage => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        CatalogArtworkError::SourceUnavailable(_) => StatusCode::BAD_GATEWAY,
        CatalogArtworkError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    log::warn!(target: "dla::cover_cache", "event=cover_request_failed status={}", status.as_u16());
    Response::builder()
        .status(status)
        .header(CONTENT_LENGTH, 0)
        .header(CACHE_CONTROL, "no-store")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header("X-Content-Type-Options", "nosniff")
        .body(Vec::new())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use dla_application::catalog_artwork::CatalogArtworkCacheStatus;

    use super::*;

    #[test]
    fn decodes_the_opaque_source_locator() {
        let request = Request::builder()
            .uri("dla-cover://localhost/https%3A%2F%2Fimg.dlsite.jp%2Fcover.webp%3Fv%3D2")
            .body(Vec::new())
            .unwrap();

        assert_eq!(
            parse_source_url(&request).unwrap(),
            "https://img.dlsite.jp/cover.webp?v=2"
        );
    }

    #[test]
    fn rejects_extra_protocol_path_segments_and_queries() {
        for uri in [
            "dla-cover://localhost/encoded/extra",
            "dla-cover://localhost/encoded?source=other",
        ] {
            let request = Request::builder().uri(uri).body(Vec::new()).unwrap();
            assert!(matches!(
                parse_source_url(&request),
                Err(CatalogArtworkError::InvalidSource)
            ));
        }
    }

    #[test]
    fn cache_status_header_values_are_stable() {
        assert_eq!(CatalogArtworkCacheStatus::Hit.as_str(), "hit");
        assert_eq!(CatalogArtworkCacheStatus::Miss.as_str(), "miss");
        assert_eq!(
            CatalogArtworkCacheStatus::Revalidated.as_str(),
            "revalidated"
        );
        assert_eq!(CatalogArtworkCacheStatus::Stale.as_str(), "stale");
    }
}
