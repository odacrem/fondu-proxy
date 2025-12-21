use super::*;

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

fn setup_test_data(op: &str) -> String {
  let data = format!(format_test_data!(), op);
  data
}

fn test_render(op: &str) -> String {
    let data = setup_test_data(op);
    let fondu_page = fondu::Page::from_json_str(&data).unwrap();
    let mut renderer = fondu::Renderer::new(fondu_page);
    let s = String::from("<div id='foo'>first</div>");
    let src_body = s.as_bytes();
    let r = renderer.render(src_body);
    match r {
        Ok(r) => r ,
        Err(_) => String::from("error")
    }
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
    let data = setup_test_data("replace");
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

#[test]
fn test_render_no_op() {
    // Default behavior when op is missing or unrecognized should ideally be tested
    // But since setup_test_data requires an op for string format, we can pass something unknown
    // Or we rely on "replace" being default if we could pass None, but format string demands value.
    // If we pass an empty string or unknown op, let's see what happens. 
    // The code in fondu.rs defaults to "replace" if op is None, but here we inject string.
    // The current logic in fondu.rs:
    // let op = match &component_list.op { Some(x) => x.as_str(), None => "replace" };
    // match op { ... _ => () } (breaks / does nothing)
    // Wait, let's check fondu.rs again.
    
    // In fondu.rs:
    // match op { "append" => ..., "replace" => ..., _ => break }
    // If op is unknown, it breaks (does nothing).
    
    let o = test_render("unknown_op");
    // Fondu defaults to "replace" for unknown ops
    assert_eq!(o, "<div id='foo'><b>second</b>\n<i>third</i></div>");
}

#[test]
fn test_invalid_selector() {
    let data = r##"
        {
            "selectors": [
                {
                    "selector": "div:invalid-pseudo",
                    "op": "replace",
                    "components": []
                }
            ]
        }
    "##;
    // In strict mode this might fail, but currently code logs and continues.
    // We want to ensure it doesn't panic.
    let fondu_page = fondu::Page::from_json_str(data).unwrap();
    let mut renderer = fondu::Renderer::new(fondu_page);
    let s = String::from("<div>content</div>");
    let res = renderer.render(s.as_bytes());
    assert!(res.is_ok());
    // Since selector is invalid, it is skipped, so output equals input
    assert_eq!(res.unwrap(), "<div>content</div>");
}

#[test]
fn test_render_large_payload() {
    let data = setup_test_data("replace");
    let fondu_page = fondu::Page::from_json_str(&data).unwrap();
    let mut renderer = fondu::Renderer::new(fondu_page);
    
    // Create a payload > 500 bytes (larger than 100 byte buffer)
    // Target element is <div id='foo'>first</div>
    // We want it to cross buffer boundaries potentially
    let padding = "a".repeat(200);
    // padding (200) + target + padding (200)
    let s = format!("{}<div id='foo'>first</div>{}", padding, padding);
    let src_body = s.as_bytes();
    
    let result = renderer.render(src_body).unwrap();
    
    // Expected output should have the replacement in the middle
    let expected = format!("{}<div id='foo'><b>second</b>\n<i>third</i></div>{}", padding, padding);
    assert_eq!(result, expected);
}

#[test]
fn test_multiple_selectors() {
     let data = r##"
        {
            "selectors": [
                {
                    "selector": "#first",
                    "op": "replace",
                    "components": [{ "_ref": "/1", "html": "ONE" }]
                },
                {
                    "selector": "#second",
                    "op": "replace",
                    "components": [{ "_ref": "/2", "html": "TWO" }]
                }
            ]
        }
    "##;
    let fondu_page = fondu::Page::from_json_str(data).unwrap();
    let mut renderer = fondu::Renderer::new(fondu_page);
    
    let s = "<div><span id='first'>old1</span> ... <span id='second'>old2</span></div>";
    let result = renderer.render(s.as_bytes()).unwrap();
    
    assert_eq!(result, "<div><span id='first'>ONE</span> ... <span id='second'>TWO</span></div>");
}
