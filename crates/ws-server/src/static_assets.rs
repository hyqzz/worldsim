//! Embedded viewer assets — compiled into the server binary so `worldsim` is a single-file distribution.

pub struct Asset { pub content_type: &'static str, pub body: &'static [u8] }

pub fn get(path: &str)->Option<Asset> {
    match path {
        "/" | "/viewer" | "/viewer/" | "/viewer/index.html" => Some(Asset{ content_type:"text/html; charset=utf-8", body: INDEX_HTML }),
        "/viewer/app.js" => Some(Asset{ content_type:"text/javascript; charset=utf-8", body: APP_JS }),
        "/viewer/style.css" => Some(Asset{ content_type:"text/css; charset=utf-8", body: STYLE_CSS }),
        "/favicon.svg" => Some(Asset{ content_type:"image/svg+xml", body: FAVICON }),
        _ => None,
    }
}

const INDEX_HTML: &[u8] = include_bytes!("../../../viewer/index.html");
const APP_JS: &[u8] = include_bytes!("../../../viewer/app.js");
const STYLE_CSS: &[u8] = include_bytes!("../../../viewer/style.css");
const FAVICON: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 32 32\"><rect width=\"32\" height=\"32\" rx=\"6\" fill=\"#0b0f17\"/><circle cx=\"16\" cy=\"16\" r=\"7\" fill=\"none\" stroke=\"#4f7cff\" stroke-width=\"2\"/><circle cx=\"16\" cy=\"16\" r=\"2.2\" fill=\"#4f7cff\"/></svg>";
