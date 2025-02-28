use pulldown_cmark::Event::{self, Code, End, HardBreak, Rule, SoftBreak, Start, Text};
use pulldown_cmark::{html, CowStr, Options, Parser, Tag};

/// The visitor trait to allow customize html rendering.
///
/// All methods return a [`Visiting`], the default behavior is [`Visiting::NotChanged`].
#[allow(unused_variables)]
pub trait MarkdownVisitor<'a> {
    fn visit_start_tag(&mut self, tag: &Tag<'a>) -> Visiting {
        Visiting::NotChanged
    }

    fn visit_end_tag(&mut self, tag: &Tag<'a>) -> Visiting {
        Visiting::NotChanged
    }

    fn visit_text(&mut self, text: &CowStr<'a>) -> Visiting {
        Visiting::NotChanged
    }

    fn visit_code(&mut self, code: &CowStr<'a>) -> Visiting {
        Visiting::NotChanged
    }
}

/// The markdown visit result.
pub enum Visiting {
    /// A new event should be rendered.
    Event(Event<'static>),
    /// Nothing changed, still render the origin event.
    NotChanged,
    /// Don't render this event.
    Ignore,
}

impl Visiting {
    fn resolve<'a, F>(self, not_changed: F) -> Option<Event<'a>>
    where
        F: FnOnce() -> Event<'a>,
    {
        match self {
            Visiting::Event(event) => Some(event),
            Visiting::NotChanged => Some(not_changed()),
            Visiting::Ignore => None,
        }
    }
}

/// Render markdown to HTML.
pub fn markdown_to_html<'a>(markdown: &'a str, mut v: impl MarkdownVisitor<'a>) -> String {
    let parser_events_iter = Parser::new_ext(markdown, Options::all()).into_offset_iter();
    let events = parser_events_iter
        .into_iter()
        .filter_map(|(event, _)| match event {
            Event::Start(tag) => v.visit_start_tag(&tag).resolve(|| Event::Start(tag)),
            Event::End(tag) => v.visit_end_tag(&tag).resolve(|| Event::End(tag)),
            Event::Code(code) => v.visit_code(&code).resolve(|| Event::Code(code)),
            Event::Text(text) => v
                .visit_text(&text)
                // Not a code block inside text, or the code block's fenced is unsupported.
                // We still need record this text event.
                .resolve(|| Event::Text(text)),
            _ => Some(event),
        });

    let mut html = String::new();
    html::push_html(&mut html, events);
    html
}

/// Extract the description from markdown content.
///
/// The strategy is extract the first meaningful line,
/// and only take at most 200 plain chars from this line.
pub fn extract_description(markdown: &str) -> String {
    markdown
        .lines()
        .find_map(|line| {
            // Ignore heading, image line.
            let line = line.trim();
            if line.is_empty() || line.starts_with(&['#', '!']) {
                None
            } else {
                let raw = strip_markdown(line);
                // If the stripped raw text is empty, we step to next one.
                if raw == "\n" || raw.is_empty() {
                    None
                } else {
                    // No more than 200 chars.
                    // Also, replace double quote to single quote.
                    Some(raw.chars().take(200).collect::<String>().replace('"', "'"))
                }
            }
        })
        .unwrap_or_default()
}

/// Convert markdown into plain text.
#[must_use]
pub fn strip_markdown(markdown: &str) -> String {
    // GFM tables and tasks lists are not enabled.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(markdown, options);
    let mut buffer = String::new();

    // For each event we push into the buffer to produce the 'stripped' version.
    for event in parser {
        match event {
            // The start and end events don't contain the text inside the tag.
            // That's handled by the `Event::Text` arm.
            Start(tag) => start_tag(&tag, &mut buffer),
            End(tag) => end_tag(&tag, &mut buffer),
            Text(text) => {
                // FIXME: img alt text shouldn't be treated as a text?
                buffer.push_str(&text);
            }
            Code(code) => buffer.push_str(&code),
            SoftBreak | HardBreak | Rule => fresh_line(&mut buffer),
            _ => (),
        }
    }
    buffer
}

fn start_tag(tag: &Tag, buffer: &mut String) {
    match tag {
        Tag::CodeBlock(_) | Tag::List(_) => fresh_line(buffer),
        Tag::Link(_, _, title) => {
            if !title.is_empty() {
                buffer.push_str(title);
            }
        }
        _ => (),
    }
}

fn end_tag(tag: &Tag, buffer: &mut String) {
    match tag {
        Tag::Table(_)
        | Tag::TableHead
        | Tag::TableRow
        | Tag::Heading(..)
        | Tag::BlockQuote
        | Tag::CodeBlock(_)
        | Tag::Item => fresh_line(buffer),
        _ => (),
    }
}

fn fresh_line(buffer: &mut String) {
    buffer.push('\n');
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use test_case::test_case;

    #[test]
    fn test_markdown_visitor() {
        struct NopVisitor;
        impl<'a> MarkdownVisitor<'a> for NopVisitor {}

        let html = markdown_to_html("![](image.png)", NopVisitor);
        assert_eq!("<p><img src=\"image.png\" alt=\"\" /></p>\n", html);

        struct DummyVisitor;
        impl<'a> MarkdownVisitor<'a> for DummyVisitor {
            fn visit_start_tag(&mut self, tag: &Tag<'a>) -> Visiting {
                if let Tag::BlockQuote = tag {
                    Visiting::Ignore
                } else {
                    Visiting::NotChanged
                }
            }

            fn visit_end_tag(&mut self, tag: &Tag<'a>) -> Visiting {
                if let Tag::BlockQuote = tag {
                    Visiting::Ignore
                } else {
                    Visiting::NotChanged
                }
            }

            fn visit_code(&mut self, code: &CowStr<'a>) -> Visiting {
                if let Some(username) = code.strip_prefix('@') {
                    return Visiting::Event(Event::Html(
                        format!("<a href=\"https://github.com/{username}\">{code}</a>").into(),
                    ));
                }
                Visiting::NotChanged
            }
        }

        // Test Visiting::Event and Visiting::NotChanged
        let html = markdown_to_html("`@zineland`", DummyVisitor);
        assert_eq!(
            "<p><a href=\"https://github.com/zineland\">@zineland</a></p>\n",
            html
        );
        let html = markdown_to_html("`DummyVisitor`", DummyVisitor);
        assert_eq!("<p><code>DummyVisitor</code></p>\n", html);
        // Test Visiting::Ignore case
        let html = markdown_to_html("> DummyVisitor", DummyVisitor);
        assert_eq!("<p>DummyVisitor</p>\n", html);
    }

    #[test_case("aaaa"; "case1")]
    fn test_extract_decription1(markdown: &str) {
        assert_eq!("aaaa", extract_description(markdown));
    }

    #[test_case("

    aaaa"; "case0")]
    #[test_case("
    # h1
    aaaa"; "case1")]
    #[test_case("
    ![](img.png)
    aaaa"; "case2")]
    fn test_extract_decription2(markdown: &str) {
        assert_eq!("aaaa", extract_description(markdown));
    }

    #[test_case("a \"aa\" a"; "quote replace")]
    fn test_extract_decription3(markdown: &str) {
        assert_eq!("a 'aa' a", extract_description(markdown));
    }

    #[test]
    fn test_extract_decription_at_most_1_paragraphs() {
        let base = iter::repeat('a').take(10).collect::<String>();
        let mut p1 = base.clone();
        p1.push('\n');
        p1.push_str(&base.clone());
        assert_eq!(base, extract_description(&p1));
    }

    #[test]
    fn test_extract_decription_at_most_200_chars() {
        let p1 = iter::repeat('a').take(400).collect::<String>();

        let p2 = p1.clone();
        // Never extract more than 200 chars.
        assert_eq!(p1[..200], extract_description(&p2));
    }

    #[test]
    fn basic_inline_strong() {
        let markdown = r#"**Hello**"#;
        let expected = "Hello";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn basic_inline_emphasis() {
        let markdown = r#"_Hello_"#;
        let expected = "Hello";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn basic_header() {
        let markdown = r#"# Header"#;
        let expected = "Header\n";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn alt_header() {
        let markdown = r#"
Header
======
"#;
        let expected = "Header\n";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn strong_emphasis() {
        let markdown = r#"**asterisks and _underscores_**"#;
        let expected = "asterisks and underscores";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn strikethrough() {
        let markdown = r#"~~strikethrough~~"#;
        let expected = "strikethrough";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn mixed_list() {
        let markdown = r#"
1. First ordered list item
2. Another item
1. Actual numbers don't matter, just that it's a number
  1. Ordered sub-list
4. And another item.
"#;

        let expected = r#"
First ordered list item
Another item
Actual numbers don't matter, just that it's a number
Ordered sub-list
And another item.
"#;
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn basic_list() {
        let markdown = r#"
* alpha
* beta
"#;
        let expected = r#"
alpha
beta
"#;
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn list_with_header() {
        let markdown = r#"# Title
* alpha
* beta
"#;
        let expected = r#"Title

alpha
beta
"#;
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn basic_link() {
        let markdown = "[I'm an inline-style link](https://www.google.com)";
        let expected = "I'm an inline-style link";
        assert_eq!(strip_markdown(markdown), expected)
    }

    #[ignore]
    #[test]
    fn link_with_itself() {
        let markdown = "[https://www.google.com]";
        let expected = "https://www.google.com";
        assert_eq!(strip_markdown(markdown), expected)
    }

    #[test]
    fn basic_image() {
        let markdown = "![alt text](https://github.com/adam-p/markdown-here/raw/master/src/common/images/icon48.png)";
        let expected = "alt text";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn inline_code() {
        let markdown = "`inline code`";
        let expected = "inline code";
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn code_block() {
        let markdown = r#"
```javascript
var s = "JavaScript syntax highlighting";
alert(s);
```"#;
        let expected = r#"
var s = "JavaScript syntax highlighting";
alert(s);

"#;
        assert_eq!(strip_markdown(markdown), expected);
    }

    #[test]
    fn block_quote() {
        let markdown = r#"> Blockquotes are very handy in email to emulate reply text.
> This line is part of the same quote."#;
        let expected = "Blockquotes are very handy in email to emulate reply text.
This line is part of the same quote.\n";
        assert_eq!(strip_markdown(markdown), expected);
    }
}
