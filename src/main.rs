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

#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    let path = String::from(req.get_path());
    // todo -- skip fondu requests for certain request patterns like .css or .jss or .jpg etc
	// todo -- alow configuration to exclude route patterns
	// for now only process requests with no extension
	let re = Regex::new(r"\.[\w]+$").unwrap();
	if !re.is_match(path.as_str()) {
    	return rewrite(req)
	} else {
		println!("skipping {}", path);
		return Ok(req.send(CONTENT_SOURCE_BACKEND)?);
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
        fondu_req = Some(fetch_fondu_data_async(fondu_uri).unwrap());
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
                    // however, the backend config will have connect & time-between-bytes set
                    // that should be used to ensure that we don't wait more than an acceptable
                    // amount of time to fetch data
                    let start = Instant::now();
                    let fondu_resp = fondu_req.wait()?;
                    println!("Wait for fondu: {:?}", start.elapsed());
                    let fondu_resp_status = fondu_resp.get_status();
                    // lets check the the response code from fondu
                    // only proceed with an OK resonse
                    match fondu_resp_status {
                        StatusCode::OK => {
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
    // for this demo let's make sure not to cache responses from fondu
    // in future we would leverage sensible cache policy
    fondu_req.set_pass(true);
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
    let (content_source_response_handle, content_source_body_handle) = content_source_resp.into_handles();

    // todo handle this error; ideally we can return the original content_source resp
    let modified_content_source_body = fondu_renderer.render(content_source_body_handle)?;

    // create a new response body from the transformed html
    let modified_content_source_body = Body::from(modified_content_source_body).into_handle();


    //return the content_source page with fondu components inserted
    let modified_content_source_resp = Response::from_handles(content_source_response_handle, modified_content_source_body).unwrap();

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

/***************************
// YE OLDE TEST SUITE
****************************/

macro_rules! format_test_data {
    () => {
        "{{
            \"selectors\": [
                {{
                    \"selector\": \"#foo\",
                    \"op\":\"{}\",
                    \"components\": [
                        {{
                            \"_ref\": \"/components/foo\",
                            \"html\": \"<b>second</b>\"
                        }},
                        {{
                            \"_ref\": \"/components/bar\",
                            \"html\": \"<i>third</i>\"
                        }}
                    ]

                }}
            ]
        }}"
    };
}



#[allow(dead_code)]
fn setup_test_data(op: String) -> String {
  let data = format!(format_test_data!(), op);
  data
}

#[allow(dead_code)]
fn test_render(op: &str) -> String {
    let data = setup_test_data(String::from(op));
    let fondu_page = fondu::Page::from_json_str(&data).unwrap();
    let mut renderer = fondu::Renderer::new(fondu_page);
    let s = String::from("<div id='foo'>first</div>");
    let src_body = s.as_bytes();
    let r = renderer.render(src_body);
    let o = match r {
        Ok(r) => r ,
        Err(_) => String::from("error")
    };
    o
}

#[test]
fn test_render_replace() {
    let o = test_render("replace");
    println!("{}",o);
    assert_eq!(o,"<div id='foo'><b>second</b>\n<i>third</i></div>");
}

#[test]
fn test_render_append() {
    let o = test_render("append");
    println!("{}",o);
    assert_eq!(o,"<div id='foo'>first<b>second</b>\n<i>third</i></div>");
}

#[test]
fn test_render_prepend() {
    let o = test_render("prepend");
    println!("{}",o);
    assert_eq!(o,"<div id='foo'><b>second</b>\n<i>third</i>first</div>");
}

#[test]
fn test_render_before() {
    let o = test_render("before");
    println!("{}",o);
    assert_eq!(o,"<b>second</b>\n<i>third</i><div id='foo'>first</div>");
}

#[test]
fn test_render_after() {
    let o = test_render("after");
    println!("{}",o);
    assert_eq!(o,"<div id='foo'>first</div><b>second</b>\n<i>third</i>");
}

#[test]
fn test_parse_json() {
    let data = setup_test_data(String::from("replace"));
    let fondu_page = fondu::Page::from_json_str(&data);
    let fondu_page = match fondu_page {
        Ok(fondu_page) => fondu_page,
        Err(_) => {
            assert!(false);
            return
        },
    };
    assert_eq!(1,fondu_page.selectors.len());
    assert_eq!(2,fondu_page.selectors[0].components.len());
    assert_eq!("/components/foo",fondu_page.selectors[0].components[0]._ref)
}
#[test]
fn test_parse_bad_json() {
    let data = r##"
        {
            "selectors": [{
        }
    "##;
    let fondu_page = fondu::Page::from_json_str(data);
    assert!(fondu_page.is_err())
}
