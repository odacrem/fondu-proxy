use super::*;
use serde_json::json;

const TARGET: &str = "<div id='foo'>first</div>";
const COMPONENTS: &str = "<b>second</b>\n<i>third</i>";

/// Two components targeting `#foo`, with `op` set as given (or left out).
fn page(op: Option<&str>) -> Page {
    let mut directive = json!({
        "selector": "#foo",
        "components": [
            { "_ref": "/components/foo", "html": "<b>second</b>" },
            { "_ref": "/components/bar", "html": "<i>third</i>" },
        ],
    });
    if let Some(op) = op {
        directive["op"] = json!(op);
    }
    parse(&json!({ "selectors": [directive] }))
}

fn parse(json: &serde_json::Value) -> Page {
    Page::from_json(json.to_string().as_bytes()).unwrap()
}

fn render(page: &Page, html: &str) -> String {
    let mut output = Vec::new();
    page.rewrite(html.as_bytes(), &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn replace() {
    let expected = format!("<div id='foo'>{COMPONENTS}</div>");
    assert_eq!(render(&page(Some("replace")), TARGET), expected);
}

#[test]
fn append() {
    let expected = format!("<div id='foo'>first{COMPONENTS}</div>");
    assert_eq!(render(&page(Some("append")), TARGET), expected);
}

#[test]
fn prepend() {
    let expected = format!("<div id='foo'>{COMPONENTS}first</div>");
    assert_eq!(render(&page(Some("prepend")), TARGET), expected);
}

#[test]
fn before() {
    let expected = format!("{COMPONENTS}{TARGET}");
    assert_eq!(render(&page(Some("before")), TARGET), expected);
}

#[test]
fn after() {
    let expected = format!("{TARGET}{COMPONENTS}");
    assert_eq!(render(&page(Some("after")), TARGET), expected);
}

#[test]
fn missing_null_or_unknown_op_replaces() {
    assert_eq!(page(None).selectors[0].op, Op::Replace);
    assert_eq!(page(Some("unknown_op")).selectors[0].op, Op::Replace);
    let null_op = parse(&json!({
        "selectors": [{ "selector": "#foo", "op": null, "components": [] }],
    }));
    assert_eq!(null_op.selectors[0].op, Op::Replace);
}

#[test]
fn parses_json() {
    let page = page(Some("replace"));
    assert_eq!(page.selectors.len(), 1);
    assert_eq!(page.selectors[0].components.len(), 2);
    assert_eq!(page.selectors[0].components[0].reference, "/components/foo");
}

#[test]
fn rejects_malformed_json() {
    assert!(Page::from_json(br#"{ "selectors": [{ }"#).is_err());
}

#[test]
fn skips_invalid_selector() {
    let page = parse(&json!({
        "selectors": [{ "selector": "div:invalid-pseudo", "op": "replace", "components": [] }],
    }));
    assert_eq!(render(&page, "<div>content</div>"), "<div>content</div>");
}

#[test]
fn rewrites_element_split_across_chunks() {
    // the target element starts just before the first chunk boundary
    let padding = "a".repeat(CHUNK_SIZE - 5);
    let html = format!("{padding}{TARGET}{padding}");
    let expected = format!("{padding}<div id='foo'>{COMPONENTS}</div>{padding}");
    assert_eq!(render(&page(Some("replace")), &html), expected);
}

#[test]
fn applies_multiple_selectors() {
    let page = parse(&json!({
        "selectors": [
            { "selector": "#first", "components": [{ "_ref": "/1", "html": "ONE" }] },
            { "selector": "#second", "components": [{ "_ref": "/2", "html": "TWO" }] },
        ],
    }));
    let html = "<div><span id='first'>old1</span> ... <span id='second'>old2</span></div>";
    let expected = "<div><span id='first'>ONE</span> ... <span id='second'>TWO</span></div>";
    assert_eq!(render(&page, html), expected);
}

struct Broken;

impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("connection reset"))
    }
}

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("connection reset"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn reports_read_errors() {
    let result = page(None).rewrite(Broken, Vec::new());
    assert!(matches!(result, Err(RewriteError::Read(_))));
}

#[test]
fn reports_write_errors() {
    let result = page(None).rewrite(TARGET.as_bytes(), Broken);
    assert!(matches!(result, Err(RewriteError::Write(_))));
}
