use fastly::http::{header, HeaderValue, Method, StatusCode};
use fastly::{ConfigStore, Error, Request, Response, Body};
use fastly::http::request::SendErrorCause;
use fastly::handle::{BodyHandle, PendingRequestHandle};
use fastly_sys::fastly_async_io;
use std::time::{Duration, Instant};
use regex::Regex;
// include fondu module src/fondu.rs
mod fondu;

// fondu fastly backend
const FONDU_BACKEND: &str = "fondu";

// content_source backend
const CONTENT_SOURCE_BACKEND: &str = "content";

// derive the fondu Resource Uri from the request url
// rather than from the x-fondu-resource response header
const FONDU_RESOURCE_MODE: FonduResourceMode = FonduResourceMode::Uri;

lazy_static::lazy_static! {
    static ref RE: Regex = Regex::new(r"\.[\w]+$").unwrap();
}

// not using #[fastly::main] because rewritten responses are streamed
// to the client rather than returned as a buffered Response
fn main() {
    let req = Request::from_client();
    // errors returned here happen before anything is sent to the client
    if let Err(e) = handle(req) {
        println!("Request failed: {}", e);
        Response::from_status(StatusCode::INTERNAL_SERVER_ERROR).send_to_client();
    }
}

fn handle(req: Request) -> Result<(), Error> {
    let path = String::from(req.get_path());
    // todo -- skip fondu requests for certain request patterns like .css or .jss or .jpg etc
    // todo -- alow configuration to exclude route patterns
    // for now only process requests with no extension
    if !RE.is_match(path.as_str()) {
        rewrite(req)
    } else {
        println!("skipping {}", path);
        req.send(CONTENT_SOURCE_BACKEND)?.send_to_client();
        Ok(())
    }
}

fn rewrite(mut req: Request) -> Result<(), Error> {
    // the fondu time budget is counted from here
    let request_start = Instant::now();
    // capture these for logging later
    let method = String::from(req.get_method_str());
    let path = String::from(req.get_path());
    let config_dict = ConfigStore::open("config");
    let fondu_path  = config_dict.get("fondu_path");
    let fondu_path = match fondu_path {
        Some(path) => path,
        None => String::from("/")
    };
    // how long to wait for fondu before sending the original content;
    // if unset, wait as long as the fondu backend takes
    let fondu_deadline = match config_dict.get("fondu_budget_ms") {
        Some(ms) => match ms.parse::<u64>() {
            Ok(ms) => Some(request_start + Duration::from_millis(ms)),
            Err(_) => {
                println!("Invalid fondu_budget_ms: {}", ms);
                None
            }
        },
        None => None,
    };
    // only allow GET and HEAD requests
    // in future we would proxy all requests to backend
    const VALID_METHODS: [Method; 2] = [Method::HEAD, Method::GET];
    if !(VALID_METHODS.contains(req.get_method())) {
        Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
            .with_body(Body::from("This method is not allowed"))
            .send_to_client();
        return Ok(());
    }
    let mut fondu_req: Option<PendingRequestHandle> = None;



    // if configured with fonduResourceMode::Uri
    // then we are going to determine the fondu resource to fetch
    // from the content_source request uri
    // this means we can kick off the request to fondu without
    // having to wait for content_source response
    if FONDU_RESOURCE_MODE == FonduResourceMode::Uri {
        // todo: the fondu resource should be derived from the content_source request
        // (.e.g) /_pages/<...content_source uri ...>
        // dummy host foo.bar will be overidden by backend config
        // option to override origin host
        let fondu_uri = format!("https://{}{}", "foo.bar", fondu_path);
        match fetch_fondu_data_async(fondu_uri) {
            Ok(req) => fondu_req = Some(req),
            Err(e) => println!("Failed to fetch fondu data: {}", e),
        }
    }


    // request the base page from content_source
    // in theory this will auto decompress responses
    // https://developer.fastly.com/learning/concepts/compression/
    //TODO FIGURE OUT WHY THIS NOT WORK
    //req.set_auto_decompress_gzip(true);
    req.remove_header(header::ACCEPT_ENCODING);
    // send the request to content_source backend
    let csr_start = Instant::now();
    let mut content_source_resp = req.send(CONTENT_SOURCE_BACKEND)?;
    println!("Wait for content: {:?}", csr_start.elapsed());
    // examine the response
    // only text/html responses are to be rewritten
    // text/* responses can be compressed
    // others just returned unmodified
    let content_source_content_type = header_val(content_source_resp.get_header(header::CONTENT_TYPE))
        .split(';')
        .collect::<Vec<&str>>()[0];
    match content_source_content_type {
        "text/html" => {
            println!("Rewriting {} {} {}", method, path, content_source_content_type);
            // if the fetch mode is configured to "Header"
            // then lets look for an X-fondu-Resource header
            // in the content_source response and use that to fetch
            // data from fondu
            if FONDU_RESOURCE_MODE == FonduResourceMode::Header {
                let fondu_resource = header_val(content_source_resp.get_header("X-FONDU-RESOURCE"));
                if !fondu_resource.is_empty() {
                    // dummy host foo.bar will be overidden by backend config
                    // option to override origin host
                    let fondu_uri = format!("https://{}{}", "foo.bar", fondu_path);
                    match fetch_fondu_data_async(fondu_uri) {
                        Ok(req) => fondu_req = Some(req),
                        Err(e) => println!("Failed to fetch fondu data: {}", e),
                    }
                }
            }
            // headers must be set before the response starts streaming
            // use faslty dynamic compression
            // see --> https://developer.fastly.com/learning/concepts/compression/
            content_source_resp.set_header("x-compress-hint", "on");
            // for demo lets make sure responses are not cached in any
            // upstream caches or the browser
            content_source_resp.set_header("Cache-Control",
                "private, max-age=0, no-cache, no-store, must-revalidate"
            );
            // if fondu_req is None
            // then that means no request was made to fondu
            // so just return the original content_source resp
            // otherwise lets parse the fondu response
            // and stream the rewritten html
            match fondu_req {
                Some(fondu_req) => {
                    let start = Instant::now();

                    // FAIL OPEN: If fondu fails (timeout, network), return original content
                    match wait_for_fondu(fondu_req, fondu_deadline) {
                        None => {
                            println!("Fondu not ready within budget (Fail Open): {:?}", request_start.elapsed());
                            content_source_resp.send_to_client();
                        }
                        Some(Ok(fondu_resp)) => {
                            println!("Wait for fondu: {:?}", start.elapsed());
                            let fondu_resp_status = fondu_resp.get_status();
                            // lets check the the response code from fondu
                            // only proceed with an OK resonse
                            match fondu_resp_status {
                                StatusCode::OK => {
                                    stream_rewritten_response(content_source_resp, fondu_resp.into_body())
                                }
                                _ => {
                                    println!("error fetching:{}", fondu_resp_status);
                                    content_source_resp.send_to_client();
                                },
                            }
                        }
                        Some(Err(e)) => {
                             println!("Failed to fetch fondu data (Fail Open): {}", e);
                             content_source_resp.send_to_client();
                        }
                    }
                }
                None => content_source_resp.send_to_client(),
            };
            Ok(())
        },
        // otherwise just return unmodified resonse
        _ => {
            // use faslty dynamic compression
            // see --> https://developer.fastly.com/learning/concepts/compression/
            content_source_resp.set_header("x-compress-hint", "on");
            content_source_resp.send_to_client();
            Ok(())
        },
    }
}

//extract value of header or return blank string
fn header_val(header: Option<&HeaderValue>) -> &str {
    match header {
        Some(h) => h.to_str().unwrap_or(""),
        None => "",
    }
}

fn fetch_fondu_data_async(fondu_uri: String) -> Result<PendingRequestHandle, SendErrorCause> {
    let mut fondu_req = Request::get(fondu_uri);
    // We leverage the backend's Cache-Control headers for SWR/SIE automatically
    // fondu_req.set_pass(true);
    // in theory this will auto decompress responses
    // https://developer.fastly.com/learning/concepts/compression/
    //fondu_req.set_auto_decompress_gzip(true);
    //TODO FIGURE OUT WHY THIS NOT WORK
    fondu_req.remove_header(header::ACCEPT_ENCODING);
    // send the request async so we can move on to requesting the content_source resource;
    // uses the handle API because PendingRequest doesn't expose the raw handle
    // that wait_for_fondu needs to wait with a timeout
    let (req_handle, body_handle) = fondu_req.into_handles();
    req_handle.send_async(body_handle.unwrap_or_else(BodyHandle::new), FONDU_BACKEND)
}

// wait for the fondu response until the deadline, or indefinitely if there is none
// returns None if fondu wasn't ready in time
fn wait_for_fondu(
    pending: PendingRequestHandle,
    deadline: Option<Instant>,
) -> Option<Result<Response, SendErrorCause>> {
    if let Some(deadline) = deadline {
        if !is_ready_by(&pending, deadline) {
            return None;
        }
    }
    Some(pending.wait().map(|(resp_handle, body_handle)| Response::from_handles(resp_handle, body_handle)))
}

// block until the pending request is ready or the deadline passes
// based on RequestPromise::wait_timeout in fastly::experimental::reusable_sessions
fn is_ready_by(pending: &PendingRequestHandle, deadline: Instant) -> bool {
    let handle = pending.as_u32();
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        // select treats a zero timeout as "wait forever", so just check readiness
        let mut ready = 0u32;
        let status = unsafe { fastly_async_io::is_ready(handle, &mut ready) };
        status.is_ok() && ready != 0
    } else {
        // round sub-millisecond waits up to 1ms
        let millis = remaining.as_millis().try_into().unwrap_or(u32::MAX).max(1);
        // select sets this to u32::MAX on timeout
        let mut done_index = u32::MAX;
        let status = unsafe { fastly_async_io::select(&handle, 1, millis, &mut done_index) };
        status.is_ok() && done_index == 0
    }
}

// given a content_source response and a fondu response
// stream the content_source response body to the client,
// rewriting it with the components from the fondu as it goes
fn stream_rewritten_response(
    mut content_source_resp: Response,
    fondu_resp_body: Body,
) {
    // parse the fondu response
    let fondu_page = fondu::Page::from_json_str(fondu_resp_body.into_string().as_str());
    // if we encounter an error here
    // send the original content_source_resp
    let fondu_page = match fondu_page {
        Ok(fondu_page) => fondu_page,
        Err(e) => {
            println!("Invalid fondu data (Fail Open): {}", e);
            content_source_resp.send_to_client();
            return;
        }
    };
    // set up a fondu page renderer
    let mut fondu_renderer = fondu::Renderer::new(fondu_page);
    let content_source_body = content_source_resp.take_body();
    // the rewritten body length differs from the origin's
    content_source_resp.remove_header(header::CONTENT_LENGTH);
    content_source_resp.set_header("X-FONDU-REWRITE", HeaderValue::from_static("true"));
    // headers go out now; body bytes go out as they are rewritten
    let mut client_body = content_source_resp.stream_to_client();
    match fondu_renderer.pipe_through(content_source_body, &mut client_body) {
        Ok(_) => {
            if let Err(e) = client_body.finish() {
                println!("Failed to finish client response: {}", e);
            }
        },
        Err(e) => {
            // part of the page has already been sent, so we can't fall back
            // to the original content; dropping client_body without finish()
            // aborts the response so the client sees it as incomplete
            println!("Rewriting failed mid-stream, aborting response: {}", e);
            drop(client_body);
        }
    }
}

// todo needs a better name
// designates how to determine the fondu resource to fetch
// Uri: determine the fondu resource from the request uri
// Header: determine the fondu resource from the x-fondu-resource response header
#[derive(PartialEq)]
enum FonduResourceMode {
    Uri,
    Header,
}

#[cfg(test)]
mod tests;
