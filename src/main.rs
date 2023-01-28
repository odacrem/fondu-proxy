use fastly::http::{header, HeaderValue, Method, StatusCode};
use fastly::{Error, Request, Response, Body};
use fastly::http::request::{PendingRequest, SendError};
//use url::{Url, ParseError};

// use flate2 for zlib/deflate compression
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

// include fondu module src/fondu.rs
mod fondu;

// fondu fastly backend
const FONDU_BACKEND: &str = "";
// fondu hostname
const FONDU_BACKEND_HOST: &str = "";
// for demo, force this to be the fondu resource
const FONDU_RESOURCE: &str = "";

// content_source backend (can be any site that returns <component-list> markup
//const content_source_BACKEND: &str = "content_source-demo";
//const content_source_BACKEND_HOST: &str = "dummy-server-m22kxdvy6a-uk.a.run.app";
const CONTENT_SOURCE_BACKEND: &str = "";
const CONTENT_SOURCE_BACKEND_HOST: &str = "";

// derive the fondu Resource Uri from the request url
// rather than from the x-fondu-resource response header
const FONDU_RESOURCE_MODE: FonduResourceMode = FonduResourceMode::Uri;

#[fastly::main]
fn main(mut req: Request) -> Result<Response, Error> {
    // only allow GET and HEAD requests
    // in future we would proxy all requests to backend
    const VALID_METHODS: [Method; 2] = [Method::HEAD, Method::GET];
    if !(VALID_METHODS.contains(req.get_method())) {
        return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
            .with_body(Body::from("This method is not allowed")));
    }

    // todo -- skip fondu requests certain request patterns like .css or .jss or .jpg etc

    let mut fondu_req: Option<PendingRequest> = None;

    // if configured with fonduResourceMode::Uri
    // then we are going to determine the fondu resource to fetch
    // from the content_source request uri
    // this means we can kick off the request to fondu without
    // having to wait for content_source response
    if FONDU_RESOURCE_MODE == FonduResourceMode::Uri {
        // todo: the fondu resource should be derived from the content_source request
        // (.e.g) /_pages/<...content_source uri ...>
        let fondu_uri = format!("https://{}{}", FONDU_BACKEND_HOST, FONDU_RESOURCE);
        fondu_req = Some(fetch_fondu_data_async(fondu_uri).unwrap());
    }

    // request the base page from content_source
    // remove accept-encoding to ensure no gzip
    // since we will be parsing the html response
    req.set_header("Host", HeaderValue::from_static(CONTENT_SOURCE_BACKEND_HOST));
    req.remove_header(header::ACCEPT_ENCODING);

    // send the request to content_source backend
    let content_source_resp = req.send(CONTENT_SOURCE_BACKEND)?;

    // examine the response
    // only text/html responses are to be rewritten
    // text/* responses can be compressed
    // others just returned unmodified
    let content_source_content_type = header_val(content_source_resp.get_header(header::CONTENT_TYPE))
        .split(';')
        .collect::<Vec<&str>>()[0];
    match content_source_content_type {
        "text/html" => {
            // if the fetch mode is configured to "Header"
            // then lets look for an X-fondu-Resource header
            // in the content_source response and use that to fetch
            // data from fondu
            if FONDU_RESOURCE_MODE == FonduResourceMode::Header {
                let fondu_resource = header_val(content_source_resp.get_header("X-FONDU-RESOURCE"));
                if !fondu_resource.is_empty() {
                    let fondu_uri = format!("https://{}{}", FONDU_BACKEND_HOST, fondu_resource);
                    fondu_req = Some(fetch_fondu_data_async(fondu_uri).unwrap());
                }
            }
            // if fondu_req is None
            // then that means no request was made to fondu
            // so just return the original content_source resp
            // otherwise lets parse the fondu response
            // and rewrite the html
            let mut content_source_resp = match fondu_req {
                Some(fondu_req) => {
                    // todo figure out how to poll for the
                    // fondu resp; ultimately we only want to wait N ms for the fondu response
                    let fondu_resp = fondu_req.wait()?;
                    let fondu_resp_status = fondu_resp.get_status();
                    // lets check the the response code from fondu
                    // only proceed with an OK resonse
                    match fondu_resp_status {
                        StatusCode::OK => rewrite_response(content_source_resp, fondu_resp.into_body())?,
                        _ => content_source_resp,
                    }
                }
                None => content_source_resp,
            };
            // for demo lets make sure responses are not cached in any
            // upstream caches or the browser
            content_source_resp.set_header(
                "Cache-Control",
                HeaderValue::from_static("private, max-age=0, no-cache, no-store, must-revalidate"),
            );
            // all good, send along the response
            Ok(compress_response(content_source_resp)?)
        }
        // if text response then compress
        "text/css" | "text/javscript" | "application/javascript" | "application/json" => {
            Ok(compress_response(content_source_resp)?)
        }
        // otherwise just return unmodified resonse
        _ => Ok(content_source_resp),
    }
}

//extract value of header or return blank string
fn header_val(header: Option<&HeaderValue>) -> &str {
    match header {
        Some(h) => h.to_str().unwrap_or(""),
        None => "",
    }
}

// compress a response and set headers
// assumes "accept-encoding: deflate"
// todo handle other compression types, etc
fn compress_response(mut resp: Response) -> Result<Response, Error> {
    let body = resp.take_body();
    let gzip_body = compress_body(body).unwrap();
    let mut modified_resp = Response::from_body(gzip_body);
    modified_resp
        .set_header("CONTENT-ENCODING", HeaderValue::from_static("deflate"));
    Ok(modified_resp)
}

// compress the body using flate2/zlib
fn compress_body(body: fastly::http::body::Body) -> Result<fastly::http::body::Body, Error> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(&body.into_bytes())?;
    let gzip_body = Body::from(e.finish().unwrap());
    Ok(gzip_body)
}

fn fetch_fondu_data_async(fondu_uri: String) -> Result<PendingRequest, SendError> {
    let mut fondu_req = Request::get(fondu_uri);
    fondu_req.set_pass(true);
    fondu_req.remove_header(header::ACCEPT_ENCODING);
    // for this demo let's make sure not to cache responses from fondu
    // in future we would leverage sensible cache policy
    // redundant given the fastly backend config matches
    fondu_req.set_header("Host", HeaderValue::from_static(FONDU_BACKEND_HOST));
    // send the request async so we can move on to requesting the content_source resource
    fondu_req.send_async(FONDU_BACKEND)
}

// given a content_source response and a fondu response
// rewrite the content_source response body
// replacing the contents of <component-list> tags in the content_source body
// with the components from the fondu
fn rewrite_response(
    content_source_resp: Response,
    fondu_resp_body: Body,
) -> Result<Response, Error> {
    // parse the fondu response
    let fondu_page = fondu::Page::from_json_str(fondu_resp_body.into_string().as_str());
    // if we encounter an error here
    // return the original content_source_resp
    let fondu_page = match fondu_page {
        Ok(fondu_page) => fondu_page,
        Err(_) => return Ok(content_source_resp),
    };
    // set up a fondu page renderer
    let mut fondu_renderer = fondu::Renderer::new(fondu_page);
    // break the content_source response into header/body parts
    let (content_source_response_handle, content_source_body_handle) = content_source_resp.into_handles();

    // todo handle this error; ideally we can return the original content_source resp
    let modified_content_source_body = fondu_renderer.render(content_source_body_handle)?;

    // create a new response body from the transformed html
    let modified_content_source_body = Body::from(modified_content_source_body).into_handle();


    //return the content_source page with fondu components inserted
    let mut modified_content_source_resp = Response::from_handles(content_source_response_handle, modified_content_source_body).unwrap();

    // indicate that this page was modified by fondu
    modified_content_source_resp.set_header("X-FONDU-REWRITE", HeaderValue::from_static("true"));
    Ok(modified_content_source_resp)
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
