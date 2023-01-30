use fastly::Error;
//use lol_html::errors::RewritingError
use lol_html::html_content::{ContentType, Element};
use lol_html::{ElementContentHandlers, HtmlRewriter, Selector, Settings};
use serde::{Serialize, Deserialize};
use std::io::Read;
use std::borrow::Cow;


#[derive(Serialize, Deserialize)]
pub struct Page {
    pub selectors: Vec<ComponentList>,
}

impl Page {
    // given fondu page json string
    // parse into Page struct
    pub fn from_json_str(json: &str) -> Result<Page, serde_json::error::Error> {
        let page:Page = serde_json::from_str(json)?;
        Ok(page)
    }
}
// holds a list of components
#[derive(Serialize, Deserialize)]
pub struct ComponentList {
    pub selector: String,
    pub op: Option<String>,
    pub components: Vec<Component>,
}


// represent a component
#[derive(Serialize, Deserialize)]
pub struct Component {
    pub _ref: String,
    pub html: String,
}

// hold parsed fondu page data
// and element handlers that will be used
// to rewrite an html response body
pub struct Renderer {
    fondu_page: Page,
}

impl Renderer {
    pub fn new(fondu_page: Page) -> Renderer {
        Renderer {
            fondu_page,
        }
    }

    // set up the element handlers for each component list
    // in the fondu_page data
    fn setup_element_handlers(&mut self) -> Vec<(Cow<Selector>, ElementContentHandlers)> {
        let mut handlers: Vec<(Cow<Selector>, ElementContentHandlers)> = Vec::new();
        for component_list in self.fondu_page.selectors.iter() {
            println!("setting up: {}", component_list.selector);
            let selector : Cow<Selector>  =  Cow::Owned(component_list.selector.parse().unwrap());
            let components = component_list.components.as_slice();
            let op = match &component_list.op {
                Some(x) => {
                    String::from(x)
                },
                None => String::from(""),
            };
            let closure = move |el: &mut Element| {
                let mut string_list = vec![];
                for component in components {
                    string_list.push(component.html.to_string());
                }
                let html = string_list.join("\n");
                match op.as_str() {
                  "append" =>  el.append(&html, ContentType::Html),
                  "prepend" =>  el.prepend(&html, ContentType::Html),
                  "after" =>  el.after(&html, ContentType::Html),
                  "before" =>  el.before(&html, ContentType::Html),
                  _  =>  el.set_inner_content(&html, ContentType::Html),
                }
                Ok(())
            };
            let element_handler = ElementContentHandlers::default().element(closure);
            handlers.push((selector, element_handler))
        }
        handlers
    }

    // given a handle to html body
    // rewrite the html, inserting components
    pub fn render(&mut self, mut src_body: impl Read) -> Result<String, Error> {
        // set up the element handlers
        let element_content_handlers = self.setup_element_handlers();
        // buffer to hold the rewrite output
        let mut output = vec![];
        // ok, create the rewriter and assign the element_handlers
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers,
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        );
        // set up a buffer for the src_body html
        let mut buffer = [0; 100];
        // read through the src_body html and rewrite
        while let Ok(bytes_read) = src_body.read(&mut buffer) {
            if bytes_read == 0 {
                break;
            }
            // todo handle lol_html::errors::RewritingError
            match rewriter.write(&buffer[..bytes_read]) {
                Ok(_) => (),
                Err(_) => (),
            }
        }
        // finish this up and return rewritten string
        rewriter.end().unwrap();
        let out = String::from_utf8(output).unwrap();
        Ok(out)
    }
}

