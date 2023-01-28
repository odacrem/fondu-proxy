use fastly::Error;
//use lol_html::errors::RewritingError
use lol_html::html_content::{ContentType, Element};
use lol_html::{ElementContentHandlers, HtmlRewriter, Selector, Settings};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::io::Read;

// the css selector to find and replace with fondu component data
macro_rules! component_selector_format {
    () => {
        "component-list[list={}]"
    };
}

// fondu page json structure is
// {
// "top": [
//          {
//            _ref: "_component/foo",
//            html: "<div></div>",
//            data: {..}
//          }
//        ],
// "bottom": [ { "foo" ... } ],
// }
pub struct Page {
    pub name: String,
    pub component_lists: HashMap<String, ComponentList>,
}

impl Page {
    // create an empty page struct
    pub fn new(_name: String) -> Page {
        Page {
            name: _name,
            component_lists: HashMap::new(),
        }
    }
    // given fondu page json string
    // parse into Page struct
    pub fn from_json_str(json: &str) -> Result<Page, serde_json::error::Error> {
        let parsed: Value = serde_json::from_str(json)?;
        let mut page = Page::new(String::from("page"));
        // if for some reason the resulting json is not an object
        // then bail out
        if !parsed.is_object() {
            return Ok(page);
        }
        // loop through all the keys e.g "top", "bottom" etc
        // the value should be an array of components
        let obj: Map<String, Value> = parsed.as_object().unwrap().clone();
        for key in obj.keys() {
            // assume any arrays are component lists
            if obj.get(key).unwrap().is_array() {
                let list = obj.get(key).unwrap().as_array().unwrap();
                // create a ComponentList struct for each
                let mut component_list = ComponentList::new(String::from(key));
                // loop through each and create a Component struct
                for c in list {
                    let m = c.as_object().unwrap();
                    let dc = Component {
                        _ref: String::from(m.get("_ref").unwrap().as_str().unwrap()),
                        html: String::from(m.get("html").unwrap().as_str().unwrap()),
                    };
                    component_list.components.push(dc);
                }
                page.component_lists
                    .insert(String::from(key), component_list);
            }
        }
        Ok(page)
    }
}
// holds a list of components
pub struct ComponentList {
    pub name: String,
    pub components: Vec<Component>,
}

impl ComponentList {
    pub fn new(_name: String) -> ComponentList {
        ComponentList {
            name: _name,
            components: Vec::new(),
        }
    }
}

// represent a component
pub struct Component {
    pub _ref: String,
    pub html: String,
}

// hold parsed fondu page data
// and element handlers that will be used
// to rewrite an html response body
pub struct Renderer {
    fondu_page: Page,
    element_handlers: Vec<ElementHandler>,
}

impl Renderer {
    pub fn new(fondu_page: Page) -> Renderer {
        Renderer {
            fondu_page,
            element_handlers: Vec::new(),
        }
    }

    // set up the element handlers for each component list
    // in the fondu_page data
    fn setup_element_handlers(&mut self) -> Vec<(&Selector, ElementContentHandlers)> {
        for (key, component_list) in self.fondu_page.component_lists.iter() {
            // this is the selector we will be looking to replace
            // ie <component-list list='top' />
            let name = format!(component_selector_format!(), key);
            let selector: Selector = name.parse().unwrap();

            // gather up the html data for each component
            // in this list
            let mut string_list = vec![];
            let components = component_list.components.as_slice();
            for component in components {
                string_list.push(component.html.to_string());
            }
            let html = string_list.join("\n");

            // create the ElementHandler; store it on a vec
            // owned by the Rendered so its lifetime will extend long enough
            // got to be a better way to do this...
            self.element_handlers
                .push(ElementHandler { selector, html });
        }
        // loop through all the handlers
        // and create the Tuples that lol_html::Settings.element_content_handlers expects
        let handlers: Vec<(&Selector, ElementContentHandlers)> = self
            .element_handlers
            .iter()
            .map(|handler| {
                let closure = move |el: &mut Element| {
                    el.set_inner_content(&handler.html, ContentType::Html);
                    Ok(())
                };
                (
                    &handler.selector,
                    ElementContentHandlers::default().element(closure),
                )
            })
            .collect();
        handlers
    }

    // given a handle to html body
    // rewrite the html, inserting components
    pub fn render(&mut self, mut src_body: fastly::handle::BodyHandle) -> Result<String, Error> {
        // set up the element handlers
        let element_content_handlers = self.setup_element_handlers();
        // buffer to hold the rewrite output
        let mut output = vec![];
        // ok, create the rewriter and assign the element_handlers
        let mut rewriter = HtmlRewriter::try_new(
            Settings {
                element_content_handlers,
                ..Settings::default()
            },
            |c: &[u8]| output.extend_from_slice(c),
        )
        .unwrap();
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

// hold a css selector and the html string we want to replace its content with
struct ElementHandler {
    selector: Selector,
    html: String,
}
