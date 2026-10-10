//! Fondu Proxy: stitches components from the fondu backend into HTML pages
//! from the content backend, streaming the result to the client.

mod config;
mod fondu;
mod pending;

use std::time::Instant;

use fastly::handle::{BodyHandle, PendingRequestHandle};
use fastly::http::request::SendErrorCause;
use fastly::http::{header, Method, StatusCode};
use fastly::{mime, Error, Request, Response};

use config::Config;
use fondu::Page;

const CONTENT_BACKEND: &str = "content";
const FONDU_BACKEND: &str = "fondu";

/// Host for fondu requests; replaced by the fondu backend's host override.
const FONDU_PLACEHOLDER_HOST: &str = "foo.bar";

// not #[fastly::main]: rewritten pages are streamed to the client
// rather than returned as a buffered Response
fn main() {
    let req = Request::from_client();
    // errors returned here happen before anything is sent to the client
    if let Err(e) = handle(req) {
        println!("Request failed: {e}");
        Response::from_status(StatusCode::INTERNAL_SERVER_ERROR).send_to_client();
    }
}

fn handle(req: Request) -> Result<(), Error> {
    // only pages are rewritten, so requests for files like .css or .jpg skip fondu
    // todo: make the excluded routes configurable
    if has_file_extension(req.get_path()) {
        println!("Skipping {}", req.get_path());
        req.send(CONTENT_BACKEND)?.send_to_client();
        return Ok(());
    }
    if !matches!(*req.get_method(), Method::GET | Method::HEAD) {
        Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
            .with_body("This method is not allowed")
            .send_to_client();
        return Ok(());
    }
    rewrite(req)
}

fn rewrite(mut req: Request) -> Result<(), Error> {
    let start = Instant::now();
    let config = Config::load();
    let deadline = config.fondu_budget.map(|budget| start + budget);

    // start the fondu request first so it runs while we wait for the content
    // todo: derive the fondu path from the request path so each page gets its own directives
    let fondu = send_fondu_request(&config.fondu_path)
        .inspect_err(|e| println!("Failed to send fondu request: {e}"))
        .ok();

    let label = format!("{} {}", req.get_method_str(), req.get_path());
    // ask for an uncompressed page so it can be rewritten
    req.remove_header(header::ACCEPT_ENCODING);
    let mut content = req.send(CONTENT_BACKEND)?;
    println!("Wait for content: {:?}", start.elapsed());

    // let Fastly compress the response on the way out
    // see https://developer.fastly.com/learning/concepts/compression/
    content.set_header("x-compress-hint", "on");

    if !is_html(&content) {
        content.send_to_client();
        return Ok(());
    }
    println!("Rewriting {label}");

    // for the demo, keep pages out of downstream caches and the browser
    content.set_header(
        header::CACHE_CONTROL,
        "private, max-age=0, no-cache, no-store, must-revalidate",
    );

    // fail open: without usable directives, send the page unchanged
    match fondu.and_then(|pending| receive_page(pending, deadline)) {
        Some(page) => stream_rewritten(content, &page),
        None => content.send_to_client(),
    }
    Ok(())
}

fn send_fondu_request(path: &str) -> Result<PendingRequestHandle, SendErrorCause> {
    // fondu responses are cached according to the backend's Cache-Control, including SWR/SIE
    let mut req = Request::get(format!("https://{FONDU_PLACEHOLDER_HOST}{path}"));
    req.remove_header(header::ACCEPT_ENCODING);
    // sent with the handle API because pending::wait_until needs the raw handle
    let (req_handle, body_handle) = req.into_handles();
    req_handle.send_async(body_handle.unwrap_or_else(BodyHandle::new), FONDU_BACKEND)
}

/// Waits for the fondu response and parses its directives.
/// Returns `None`, logging why, when the page should be sent unchanged.
fn receive_page(pending: PendingRequestHandle, deadline: Option<Instant>) -> Option<Page> {
    let waiting = Instant::now();
    let resp = pending::wait_until(pending, deadline)
        .inspect_err(|e| println!("No fondu response (fail open): {e}"))
        .ok()?;
    println!("Wait for fondu: {:?}", waiting.elapsed());

    let status = resp.get_status();
    if status != StatusCode::OK {
        println!("Fondu responded {status} (fail open)");
        return None;
    }
    Page::from_json(&resp.into_body_bytes())
        .inspect_err(|e| println!("Invalid fondu data (fail open): {e}"))
        .ok()
}

/// Streams the page to the client, applying the fondu directives as it goes.
fn stream_rewritten(mut content: Response, page: &Page) {
    let body = content.take_body();
    // the rewritten body length differs from the origin's
    content.remove_header(header::CONTENT_LENGTH);
    content.set_header("x-fondu-rewrite", "true");

    let mut client_body = content.stream_to_client();
    match page.rewrite(body, &mut client_body) {
        Ok(()) => {
            if let Err(e) = client_body.finish() {
                println!("Failed to finish client response: {e}");
            }
        }
        Err(e) => {
            // part of the page is already sent, so it's too late to fall back to the
            // original; dropping the body without finish() aborts the response so the
            // client sees it as incomplete
            println!("Rewriting failed mid-stream, aborting response: {e}");
            drop(client_body);
        }
    }
}

/// Whether the path ends in a file extension like `.css` (i.e. matches `\.\w+$`).
fn has_file_extension(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        !ext.is_empty() && ext.chars().all(|c| c.is_alphanumeric() || c == '_')
    })
}

fn is_html(resp: &Response) -> bool {
    resp.get_content_type().is_some_and(|content_type| {
        content_type.type_() == mime::TEXT && content_type.subtype() == mime::HTML
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_file_extensions() {
        for path in ["/style.css", "/img/logo.png", "/a.b_c", "/archive.tar.gz"] {
            assert!(has_file_extension(path), "{path}");
        }
        for path in ["/", "/about", "/v1.2/page", "/file.", "/a.b-c"] {
            assert!(!has_file_extension(path), "{path}");
        }
    }
}
