//! Fondu directives and the streaming HTML rewriter that applies them.

use std::borrow::Cow;
use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};

use lol_html::errors::RewritingError;
use lol_html::html_content::{ContentType, Element};
use lol_html::{ElementContentHandlers, HtmlRewriter, Selector, Settings};
use serde::Deserialize;

/// Size of each chunk read from the source HTML. Output is flushed after each chunk.
const CHUNK_SIZE: usize = 4096;

/// Directives returned by the fondu backend.
#[derive(Debug, Deserialize)]
pub struct Page {
    pub selectors: Vec<Directive>,
}

/// Components to insert at every element matching `selector`.
#[derive(Debug, Deserialize)]
pub struct Directive {
    pub selector: String,
    #[serde(default)]
    pub op: Op,
    pub components: Vec<Component>,
}

/// An HTML snippet to insert.
#[derive(Debug, Deserialize)]
pub struct Component {
    /// Identifies the component in the fondu API; not used for rendering.
    #[serde(rename = "_ref")]
    #[allow(dead_code)]
    pub reference: String,
    pub html: String,
}

/// Where components are inserted relative to a matched element.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(from = "Option<String>")]
pub enum Op {
    /// Replace the element's inner content. Also used when the op is missing or unknown.
    #[default]
    Replace,
    /// Insert inside the element, after its content.
    Append,
    /// Insert inside the element, before its content.
    Prepend,
    /// Insert immediately before the element.
    Before,
    /// Insert immediately after the element.
    After,
}

impl From<Option<String>> for Op {
    fn from(op: Option<String>) -> Self {
        match op.as_deref() {
            Some("append") => Op::Append,
            Some("prepend") => Op::Prepend,
            Some("before") => Op::Before,
            Some("after") => Op::After,
            _ => Op::Replace,
        }
    }
}

impl Op {
    fn apply(self, el: &mut Element, html: &str) {
        match self {
            Op::Replace => el.set_inner_content(html, ContentType::Html),
            Op::Append => el.append(html, ContentType::Html),
            Op::Prepend => el.prepend(html, ContentType::Html),
            Op::Before => el.before(html, ContentType::Html),
            Op::After => el.after(html, ContentType::Html),
        }
    }
}

impl Page {
    pub fn from_json(json: &[u8]) -> serde_json::Result<Page> {
        serde_json::from_slice(json)
    }

    /// Streams `input` to `output`, applying the directives as the HTML passes through.
    ///
    /// Output is flushed after each input chunk so it reaches the client as the source arrives.
    pub fn rewrite(&self, mut input: impl Read, output: impl Write) -> Result<(), RewriteError> {
        let sink = RefCell::new(Sink::new(output));
        let mut rewriter = HtmlRewriter::new(self.settings(), |chunk: &[u8]| {
            sink.borrow_mut().write(chunk);
        });

        let mut buffer = [0; CHUNK_SIZE];
        loop {
            let len = match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(len) => len,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(RewriteError::Read(e)),
            };
            rewriter.write(&buffer[..len])?;
            sink.borrow_mut().flush()?;
        }
        rewriter.end()?;

        sink.into_inner().flush()
    }

    /// Rewriter settings with a handler for each directive, skipping (and logging) invalid
    /// selectors.
    fn settings(&self) -> Settings<'_, '_> {
        self.selectors
            .iter()
            .filter_map(|directive| {
                let selector: Selector = match directive.selector.parse() {
                    Ok(selector) => selector,
                    Err(e) => {
                        println!("Skipping invalid selector {:?}: {}", directive.selector, e);
                        return None;
                    }
                };
                let html = directive
                    .components
                    .iter()
                    .map(|component| component.html.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let op = directive.op;
                let handlers =
                    ElementContentHandlers::default().element(move |el: &mut Element| {
                        op.apply(el, &html);
                        Ok(())
                    });
                Some((Cow::Owned(selector), handlers))
            })
            .fold(Settings::new(), Settings::append_element_content_handler)
    }
}

/// Receives rewriter output. `lol_html`'s output sink can't return errors,
/// so the first write error is kept and reported by the next `flush`.
struct Sink<W> {
    writer: W,
    error: Option<io::Error>,
}

impl<W: Write> Sink<W> {
    fn new(writer: W) -> Self {
        Sink {
            writer,
            error: None,
        }
    }

    fn write(&mut self, chunk: &[u8]) {
        if self.error.is_none() {
            self.error = self.writer.write_all(chunk).err();
        }
    }

    fn flush(&mut self) -> Result<(), RewriteError> {
        match self.error.take() {
            Some(e) => Err(RewriteError::Write(e)),
            None => self.writer.flush().map_err(RewriteError::Write),
        }
    }
}

/// Why rewriting stopped before the end of the page.
#[derive(Debug)]
pub enum RewriteError {
    /// Reading the source HTML failed, e.g. the content backend connection dropped.
    Read(io::Error),
    /// Writing the output failed, e.g. the client disconnected.
    Write(io::Error),
    /// `lol_html` couldn't process the HTML.
    Html(RewritingError),
}

impl fmt::Display for RewriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewriteError::Read(e) => write!(f, "reading source HTML: {e}"),
            RewriteError::Write(e) => write!(f, "writing output: {e}"),
            RewriteError::Html(e) => write!(f, "rewriting HTML: {e}"),
        }
    }
}

impl Error for RewriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RewriteError::Read(e) | RewriteError::Write(e) => Some(e),
            RewriteError::Html(e) => Some(e),
        }
    }
}

impl From<RewritingError> for RewriteError {
    fn from(e: RewritingError) -> Self {
        RewriteError::Html(e)
    }
}

#[cfg(test)]
mod tests;
