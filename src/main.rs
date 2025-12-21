use fastly::http::{header, HeaderValue, Method, StatusCode};
use fastly::{ConfigStore, Error, Request, Response, Body};
use fastly::http::request::{PendingRequest, SendError};
use std::time::{Instant};
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

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    let path = String::from(req.get_path());
    // todo -- skip fondu requests for certain request patterns like .css or .jss or .jpg etc
	// todo -- alow configuration to exclude route patterns
	// for now only process requests with no extension
	if !RE.is_match(path.as_str()) {
    	rewrite(req)
	} else {
		println!("skipping {}", path);
		Ok(req.send(CONTENT_SOURCE_BACKEND)?)
	}
}
fn rewrite(mut req: Request) -> Result<Response, Error> {
    // capture these for logging later
    let method = String::from(req.get_method_str());
    let path = String::from(req.get_path());
    let config_dict = ConfigStore::open("config");
    let fondu_path  = config_dict.get("fondu_path");
    let fondu_path = match fondu_path {
        Some(path) => path,
        None => String::from("/")
    };
    // only allow GET and HEAD requests
    // in future we would proxy all requests to backend
    const VALID_METHODS: [Method; 2] = [Method::HEAD, Method::GET];
    if !(VALID_METHODS.contains(req.get_method())) {
        return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
            .with_body(Body::from("This method is not allowed")));
    }
    let mut fondu_req: Option<PendingRequest> = None;



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
            // if fondu_req is None
            // then that means no request was made to fondu
            // so just return the original content_source resp
            // otherwise lets parse the fondu response
            // and rewrite the html
            let mut content_source_resp = match fondu_req {
                Some(fondu_req) => {
                    // todo figure out how to poll for the
                    // fondu resp; ultimately we only want to wait N ms for the fondu response
                    // however, the backend config will have connect & time-between-bytes set
                    // that should be used to ensure that we don't wait more than an acceptable
                    // amount of time to fetch data
                    let start = Instant::now();
                    
                    // FAIL OPEN: If fondu fails (timeout, network), return original content
                    match fondu_req.wait() {
                        Ok(fondu_resp) => {
                            println!("Wait for fondu: {:?}", start.elapsed());
                            let fondu_resp_status = fondu_resp.get_status();
                            // lets check the the response code from fondu
                            // only proceed with an OK resonse
                            match fondu_resp_status {
                                StatusCode::OK => {
                                    // Use proper error handling, but here rewrite_response failure 
                                    // should implementation-wise probably also fail open?
                                    // Currently rewrite_response errors propagate.
                                    // For now, let's keep rewrite_response propogation as it's logic error not network.
                                    let mut rr = rewrite_response(content_source_resp, fondu_resp.into_body())?;
                                    rr.set_header("X-FONDU-REWRITE", HeaderValue::from_static("true"));
                                    rr
                                }
                                _ => {
                                    println!("error fetching:{}", fondu_resp_status);
                                    content_source_resp
                                },
                            }
                        }
                        Err(e) => {
                             println!("Failed to fetch fondu data (Fail Open): {}", e);
                             content_source_resp
                        }
                    }
                }
                None => content_source_resp,
            };
            // use faslty dynamic compression
            // see --> https://developer.fastly.com/learning/concepts/compression/
            content_source_resp.set_header("x-compress-hint", "on");
            // for demo lets make sure responses are not cached in any
            // upstream caches or the browser
            content_source_resp.set_header("Cache-Control",
                "private, max-age=0, no-cache, no-store, must-revalidate"
            );
            Ok(content_source_resp)
        },
        // otherwise just return unmodified resonse
        _ => {
            // use faslty dynamic compression
            // see --> https://developer.fastly.com/learning/concepts/compression/
            content_source_resp.set_header("x-compress-hint", "on");
            Ok(content_source_resp)
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

fn fetch_fondu_data_async(fondu_uri: String) -> Result<PendingRequest, SendError> {
    let mut fondu_req = Request::get(fondu_uri);
    // We leverage the backend's Cache-Control headers for SWR/SIE automatically
    // fondu_req.set_pass(true);
    // in theory this will auto decompress responses
    // https://developer.fastly.com/learning/concepts/compression/
    //fondu_req.set_auto_decompress_gzip(true);
    //TODO FIGURE OUT WHY THIS NOT WORK
    fondu_req.remove_header(header::ACCEPT_ENCODING);
    // send the request async so we can move on to requesting the content_source resource
    fondu_req.send_async(FONDU_BACKEND)
}

// given a content_source response and a fondu response
// rewrite the content_source response body
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
    // We keep the handle to the body to read from it
    let (content_source_response_handle, mut content_source_body_handle) = content_source_resp.into_handles();

    // Create a new empty body for the output
    let mut output_body = Body::new();
    
    // Pipe input -> rewriter -> output
    // We pass a mutable reference to content_source_body_handle so we don't consume it on error
    match fondu_renderer.pipe_through(&mut content_source_body_handle, &mut output_body) {
        Ok(_) => {
             //return the content_source page with fondu components inserted
             let modified_content_source_resp = Response::from_handles(content_source_response_handle, output_body.into_handle());
             Ok(modified_content_source_resp)
        },
        Err(e) => {
            println!("Rewriting failed, returning original content: {}", e);
            // Reconstruct original response from handles
            let original_resp = Response::from_handles(content_source_response_handle, content_source_body_handle);
            Ok(original_resp)
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
