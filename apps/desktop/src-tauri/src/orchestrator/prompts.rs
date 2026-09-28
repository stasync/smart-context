//! What the model is told (docs/PLAN.md 4.6): one shared system prompt, a
//! block per source hint and effort, and the context pack as the first
//! message. Changing these means running the eval before and after (13.2).

use crate::context::{ContextPack, SourceHint};
use crate::engines::{Block, Effort, Message};
use crate::platform::ElementInfo;

pub const DEFAULT_QUESTION: &str = "What is this, and why is it here?";

const SHARED: &str = "\
You are Context, an assistant built into the user's computer. The user held a hotkey and pointed at something on their screen. Explain what they pointed at, and why it's there, in the context of the app or page they're in.

You get:
- Image 1: the area inside the lens, where the user pointed.
- Image 2: the whole window. The red rectangle is the lens and the red dot is the cursor.
- Text read from the screen: the element under the pointer and its ancestors, the text under the lens, the app, the window and, in browsers, the page URL.

How to answer:
- Answer the implicit question: what is this, and why is it here, in this specific context? Never give a generic dictionary answer when the context says more.
- Treat the lens as the focus and the window as the surroundings.
- For names, numbers and spelling, trust the text you're given over what the pixels seem to say.
- Start with one line: TARGET: <what the user pointed at and where, in 12 words or fewer>. Then a blank line, then the answer in Markdown.
- If you're unsure what the target is, name the most likely one in the TARGET line and answer that. The user can correct it.
- Use tools, including web search, only when the screen doesn't answer the question.";

pub fn system(source: SourceHint, effort: Effort, language: &str) -> String {
    let length = match effort {
        Effort::Low => "Answer in 2–4 sentences. No headings, no lists.",
        Effort::Medium => {
            "Go into more depth, in about 150–250 words. Short sections with headings and lists are fine. Cite the files and links you used."
        }
        Effort::High => {
            "Give a thorough answer, in about 300–450 words. Short sections with headings and lists are fine. Cite the files and links you used."
        }
        Effort::Max => {
            "Give the most complete answer you can. Short sections with headings and lists are fine. Cite the files and links you used."
        }
    };
    format!(
        "{SHARED}\n- {length}\n- Write the answer in the language with the BCP 47 tag \"{language}\". Keep the \"TARGET:\" prefix itself in English.\n\n{}",
        source_guidance(source, effort)
    )
}

fn source_guidance(source: SourceHint, effort: Effort) -> String {
    let deeper = effort > Effort::Low;
    match source {
        SourceHint::CodeEditor => "The user is in a code editor or terminal. Explain what the thing is, why it's in this project, and where it's used. Prefer reading the manifest, the README and searching for usages. For packages, look up the package's description.".into(),
        SourceHint::Shopping if deeper => "The user is on a shopping site. Say what the product is and what it's for, in plain words. Then give its key specs, what reviewers commonly complain about, and good alternatives, using web search.".into(),
        SourceHint::Shopping => "The user is on a shopping site. Say what the product is and what it's for, in plain words.".into(),
        SourceHint::WorkTool => "The user is in a work tool (an issue tracker, code host or docs). Say what the item asks or means, with its jargon explained. Relate it to what's visible.".into(),
        SourceHint::WebPage | SourceHint::OtherApp => "Explain the term, element, chart or diagram as it's used here.".into(),
    }
}

/// The first message: the screenshots first (they work best before text),
/// then everything read from the screen, then the question.
pub fn first_message(
    pack: &ContextPack,
    question: Option<&str>,
    correction: Option<&str>,
) -> Message {
    let mut blocks = Vec::new();
    if let Some(image) = &pack.lens_image {
        blocks.push(Block::Text("Image 1: inside the lens.".into()));
        blocks.push(Block::Image {
            media_type: image.media_type.clone(),
            data: image.bytes.clone(),
        });
    }
    if let Some(image) = &pack.window_image {
        blocks.push(Block::Text(
            "Image 2: the whole window. The red rectangle is the lens; the red dot is the cursor."
                .into(),
        ));
        blocks.push(Block::Image {
            media_type: image.media_type.clone(),
            data: image.bytes.clone(),
        });
    }

    let mut text = screen_text(pack);
    if let Some(correction) = correction {
        text.push_str(&format!("\nThe user says they pointed at: {correction}\n"));
    }
    text.push_str(&format!(
        "\nQuestion: {}",
        question.unwrap_or(DEFAULT_QUESTION)
    ));
    blocks.push(Block::Text(text));
    Message::user(blocks)
}

pub fn go_deeper() -> Message {
    Message::user_text("Go deeper.")
}

/// Everything the pack says about the screen, as plain text.
fn screen_text(pack: &ContextPack) -> String {
    let mut lines = Vec::new();
    if let Some(window) = &pack.window {
        let app = &window.app;
        match &app.bundle_id {
            Some(id) => lines.push(format!("App: {} ({id})", app.name)),
            None => lines.push(format!("App: {}", app.name)),
        }
        if let Some(title) = &window.title {
            lines.push(format!("Window title: {title}"));
        }
    }
    if let Some(url) = &pack.url {
        lines.push(format!("Page URL: {url}"));
    }
    if let Some(focus) = &pack.focus {
        lines.push(format!("Element under the pointer: {}", describe(focus)));
    }
    if !pack.ancestors.is_empty() {
        lines.push("Its ancestors, nearest first:".into());
        for ancestor in &pack.ancestors {
            lines.push(format!("- {}", describe(ancestor)));
        }
    }
    if let Some(selection) = &pack.selection {
        lines.push(format!(
            "Selected text:\n<selection>\n{selection}\n</selection>"
        ));
    }
    if !pack.nearby_text.is_empty() {
        lines.push(format!(
            "Text under the lens:\n<screen_text>\n{}\n</screen_text>",
            pack.nearby_text
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// "text field: “visa_docs”", from whatever the element offers.
fn describe(element: &ElementInfo) -> String {
    let kind = element
        .role_description
        .clone()
        .or_else(|| element.role.clone())
        .unwrap_or_else(|| "element".into());
    let texts: Vec<&str> = [&element.title, &element.value, &element.description]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    if texts.is_empty() {
        kind
    } else {
        format!("{kind}: “{}”", texts.join("” / “"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{Capture, Classifier};
    use crate::platform::{AppInfo, Inspection, Point, Rect, Screenshots, WindowInfo};

    fn pack() -> ContextPack {
        ContextPack::build(
            Capture {
                cursor: Point { x: 10.0, y: 10.0 },
                lens: Rect::centered_at(Point { x: 10.0, y: 10.0 }, 20.0, 10.0),
                window: Some(WindowInfo {
                    id: 1,
                    title: Some("Echo Dot".into()),
                    bounds: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                    app: AppInfo {
                        name: "Google Chrome".into(),
                        bundle_id: Some("com.google.Chrome".into()),
                        pid: 1,
                    },
                }),
                inspection: Inspection {
                    chain: vec![
                        ElementInfo {
                            role: Some("AXStaticText".into()),
                            role_description: Some("text".into()),
                            value: Some("Echo Dot (5th Gen)".into()),
                            ..Default::default()
                        },
                        ElementInfo {
                            role: Some("AXHeading".into()),
                            title: Some("Product title".into()),
                            ..Default::default()
                        },
                    ],
                    nearby_text: "Echo Dot (5th Gen)\n4.7 out of 5 stars".into(),
                    selection: None,
                    url: Some("https://www.amazon.com/dp/B0".into()),
                },
                focus_level: 0,
                screenshots: Screenshots {
                    window: Some(image::RgbaImage::new(50, 50)),
                    lens: Some(image::RgbaImage::new(40, 20)),
                    scale: 2.0,
                },
            },
            &Classifier::builtin(),
        )
    }

    #[test]
    fn the_first_message_puts_images_before_text() {
        let message = first_message(&pack(), None, None);
        let kinds: Vec<&str> = message
            .content
            .iter()
            .map(|b| match b {
                Block::Text(_) => "text",
                Block::Image { .. } => "image",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["text", "image", "text", "image", "text"]);

        let text = message.text();
        assert!(text.contains("App: Google Chrome (com.google.Chrome)"));
        assert!(text.contains("Page URL: https://www.amazon.com/dp/B0"));
        assert!(text.contains("Element under the pointer: text: “Echo Dot (5th Gen)”"));
        assert!(text.contains("- AXHeading: “Product title”"));
        assert!(
            text.contains("<screen_text>\nEcho Dot (5th Gen)\n4.7 out of 5 stars\n</screen_text>")
        );
        assert!(text.ends_with(&format!("Question: {DEFAULT_QUESTION}")));
    }

    #[test]
    fn a_typed_question_and_a_correction_replace_the_defaults() {
        let text = first_message(&pack(), Some("Is it waterproof?"), Some("the price")).text();
        assert!(text.contains("The user says they pointed at: the price"));
        assert!(text.ends_with("Question: Is it waterproof?"));
    }

    #[test]
    fn the_system_prompt_follows_effort_source_and_language() {
        let low = system(SourceHint::Shopping, Effort::Low, "de-DE");
        assert!(low.contains("2–4 sentences"));
        assert!(low.contains("\"de-DE\""));
        assert!(!low.contains("reviewers"));
        assert!(low.contains("TARGET:"));

        let medium = system(SourceHint::Shopping, Effort::Medium, "en");
        assert!(medium.contains("reviewers commonly complain"));
        assert!(medium.contains("Cite the files and links"));
    }

    #[test]
    fn prompts_never_name_a_vendor_or_model() {
        for source in [
            SourceHint::CodeEditor,
            SourceHint::Shopping,
            SourceHint::WorkTool,
            SourceHint::WebPage,
            SourceHint::OtherApp,
        ] {
            for effort in Effort::ALL {
                let prompt = system(source, effort, "en").to_lowercase();
                for name in [
                    "claude",
                    "anthropic",
                    "haiku",
                    "sonnet",
                    "opus",
                    "gpt",
                    "gemini",
                ] {
                    assert!(!prompt.contains(name), "{name} in {source:?}/{effort:?}");
                }
            }
        }
    }
}
