use fastly::Error;
//use lol_html::errors::RewritingError
use lol_html::html_content::{ContentType, Element};
use lol_html::{ElementContentHandlers, HtmlRewriter, Selector, Settings};
use serde::{Serialize, Deserialize};
use std::io::Read;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};


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
    fn setup_element_handlers(&mut self) -> Vec<(Cow<'_, Selector>, ElementContentHandlers<'_>)> {
        let mut handlers: Vec<(Cow<'_, Selector>, ElementContentHandlers<'_>)> = Vec::new();
        for component_list in self.fondu_page.selectors.iter() {
            let selector: Cow<Selector> = match component_list.selector.parse() {
                Ok(s) => Cow::Owned(s),
                Err(_) => {
                    println!("Invalid selector: {}", component_list.selector);
                    continue;
                }
            };
            let components = component_list.components.as_slice();
            // check to see if an "op" operation value is set;
            // default to 'replace'
            let op = match &component_list.op {
                Some(x) => x.as_str(),
                None => "replace",
            };
            println!("element handler: selector: {} op: {}", component_list.selector, op);
            let closure = move |el: &mut Element| {
                let mut string_list = vec![];
                // loop through all the component html
                // and combine them into one big string
                for component in components {
                    string_list.push(component.html.to_string());
                }
                let html = string_list.join("\n");

                // if no op is set then replace the inner contents
                match op {
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

    // parse the fondu response and stream the rewritten HTML to the output writer
    pub fn pipe_through(&mut self, mut src_body: impl Read, output_writer: impl std::io::Write) -> Result<(), Error> {
        // set up the element handlers
        let element_content_handlers = self.setup_element_handlers();

        // shared with the rewriter's output sink so we can flush
        // and check for write errors between input chunks
        let output_writer = RefCell::new(output_writer);
        let write_err: Cell<Option<std::io::Error>> = Cell::new(None);

        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers,
                ..Settings::default()
            },
            |c: &[u8]| {
                if let Err(e) = output_writer.borrow_mut().write_all(c) {
                    write_err.set(Some(e));
                }
            },
        );

        // set up a buffer for the src_body html
        // Increase buffer size for better throughput
        let mut buffer = [0; 4096];

        // read through the src_body html and rewrite
        loop {
            let bytes_read = match src_body.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::msg(format!("Input read error: {}", e))),
            };
            if let Err(e) = rewriter.write(&buffer[..bytes_read]) {
                return Err(Error::msg(e.to_string()));
            }
            // send what we have so far rather than waiting for the
            // writer's buffer to fill; stop early if the output is gone
            if let Err(e) = output_writer.borrow_mut().flush() {
                write_err.set(Some(e));
            }
            if let Some(e) = write_err.take() {
                return Err(Error::msg(format!("Output write error: {}", e)));
            }
        }

        rewriter.end().map_err(|e| Error::msg(e.to_string()))?;

        if let Some(e) = write_err.take() {
            return Err(Error::msg(format!("Output write error: {}", e)));
        }

        Ok(())
    }
}

