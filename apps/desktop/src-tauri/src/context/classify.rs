//! The source classifier (docs/PLAN.md 4.5): which kind of place the user is
//! pointing in, so the prompt can steer the answer. Rules live in
//! `config/sources.json`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceHint {
    CodeEditor,
    Shopping,
    WorkTool,
    WebPage,
    OtherApp,
}

#[derive(Debug, Deserialize)]
pub struct Classifier {
    code_editors: Vec<String>,
    browsers: Vec<String>,
    hosts: Hosts,
}

#[derive(Debug, Deserialize)]
struct Hosts {
    shopping: Vec<String>,
    work_tool: Vec<String>,
}

impl Classifier {
    /// The rules shipped in `config/sources.json`.
    pub fn builtin() -> Self {
        serde_json::from_str(include_str!("../../../../../config/sources.json"))
            .expect("config/sources.json is valid")
    }

    pub fn is_browser(&self, bundle_id: Option<&str>) -> bool {
        bundle_id.is_some_and(|id| self.browsers.iter().any(|p| bundle_matches(p, id)))
    }

    pub fn classify(&self, bundle_id: Option<&str>, url: Option<&str>) -> SourceHint {
        let Some(bundle_id) = bundle_id else {
            return SourceHint::OtherApp;
        };
        let is = |patterns: &[String]| patterns.iter().any(|p| bundle_matches(p, bundle_id));
        if is(&self.code_editors) {
            return SourceHint::CodeEditor;
        }
        if !is(&self.browsers) {
            return SourceHint::OtherApp;
        }
        let Some(host) = url.and_then(host_of) else {
            return SourceHint::WebPage;
        };
        let on = |patterns: &[String]| patterns.iter().any(|p| host_matches(p, &host));
        if on(&self.hosts.shopping) {
            SourceHint::Shopping
        } else if on(&self.hosts.work_tool) {
            SourceHint::WorkTool
        } else {
            SourceHint::WebPage
        }
    }
}

fn bundle_matches(pattern: &str, bundle_id: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => bundle_id.starts_with(prefix),
        None => pattern == bundle_id,
    }
}

fn host_matches(pattern: &str, host: &str) -> bool {
    if let Some(domain) = pattern.strip_prefix("*.") {
        host.ends_with(&format!(".{domain}"))
    } else if let Some(name) = pattern.strip_suffix(".*") {
        // The name as any label but the last: amazon.com, www.amazon.co.uk.
        let labels: Vec<&str> = host.split('.').collect();
        labels[..labels.len() - 1].contains(&name)
    } else {
        host == pattern || host.ends_with(&format!(".{pattern}"))
    }
}

/// The lowercase host of an http(s) URL.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use SourceHint::*;

    fn classify(bundle_id: &str, url: Option<&str>) -> SourceHint {
        Classifier::builtin().classify(Some(bundle_id), url)
    }

    #[test]
    fn editors_and_terminals_are_code() {
        assert_eq!(classify("com.microsoft.VSCode", None), CodeEditor);
        assert_eq!(classify("com.jetbrains.WebStorm", None), CodeEditor);
        assert_eq!(classify("com.apple.Terminal", None), CodeEditor);
    }

    #[test]
    fn browsers_are_classified_by_host() {
        let chrome = "com.google.Chrome";
        assert_eq!(
            classify(chrome, Some("https://www.amazon.com/dp/B0")),
            Shopping
        );
        assert_eq!(
            classify(chrome, Some("https://www.amazon.co.uk/x")),
            Shopping
        );
        assert_eq!(
            classify(
                "com.apple.Safari",
                Some("https://acme.atlassian.net/browse/X-1")
            ),
            WorkTool
        );
        assert_eq!(
            classify(chrome, Some("https://linear.app/team/issue/1")),
            WorkTool
        );
        assert_eq!(
            classify(chrome, Some("https://en.wikipedia.org/wiki/X")),
            WebPage
        );
        assert_eq!(classify(chrome, None), WebPage);
    }

    #[test]
    fn lookalike_hosts_do_not_match() {
        let chrome = "com.google.Chrome";
        assert_eq!(classify(chrome, Some("https://notamazon.com/")), WebPage);
        assert_eq!(
            classify(chrome, Some("https://atlassian.net.evil.io/")),
            WebPage
        );
        assert_eq!(classify(chrome, Some("https://mygithub.com/")), WebPage);
    }

    #[test]
    fn browsers_are_known_by_bundle_id() {
        let classifier = Classifier::builtin();
        assert!(classifier.is_browser(Some("com.apple.Safari")));
        assert!(!classifier.is_browser(Some("com.microsoft.VSCode")));
        assert!(!classifier.is_browser(None));
    }

    #[test]
    fn everything_else_is_another_app() {
        assert_eq!(classify("com.apple.finder", None), OtherApp);
        assert_eq!(Classifier::builtin().classify(None, None), OtherApp);
    }

    #[test]
    fn hosts_are_parsed_from_urls() {
        assert_eq!(
            host_of("https://User@Example.COM:8080/a?b#c").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("http://localhost").as_deref(), Some("localhost"));
        assert_eq!(host_of("file:///etc/hosts"), None);
    }
}
