mod fixtures;

use search_common::extraction::*;

// ============================================================================
// Title extraction tests
// ============================================================================

#[test]
fn title_from_title_tag() {
    let html = "<html><head><title>My Page Title</title></head><body></body></html>";
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("My Page Title".to_string()));
}

#[test]
fn title_from_og_title() {
    let html =
        r#"<html><head><meta property="og:title" content="OG Title"></head><body></body></html>"#;
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("OG Title".to_string()));
}

#[test]
fn title_from_application_name() {
    let html =
        r#"<html><head><meta name="application-name" content="My App"></head><body></body></html>"#;
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("My App".to_string()));
}

#[test]
fn title_from_h1() {
    let html = "<html><body><h1>Heading Title</h1></body></html>";
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("Heading Title".to_string()));
}

#[test]
fn title_priority_title_over_og() {
    let html = r#"<html><head><title>Title Tag</title><meta property="og:title" content="OG Title"></head></html>"#;
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("Title Tag".to_string()));
}

#[test]
fn title_priority_og_over_h1() {
    let html = r#"<html><head><meta property="og:title" content="OG Title"></head><body><h1>H1 Title</h1></body></html>"#;
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("OG Title".to_string()));
}

#[test]
fn title_none_when_empty() {
    let html = "<html><head></head><body><p>No title here</p></body></html>";
    let result = extract_title_from_html(html);
    assert_eq!(result, None);
}

#[test]
fn title_strips_whitespace() {
    let html = "<html><head><title>  Spaced Title  </title></head></html>";
    let result = extract_title_from_html(html);
    assert_eq!(result, Some("Spaced Title".to_string()));
}

#[test]
fn title_handles_nested_tags() {
    let html = "<html><head><title>Hello <b>World</b></title></head></html>";
    let result = extract_title_from_html(html);
    // Should extract just the text content
    assert_eq!(result, Some("Hello World".to_string()));
}

// ============================================================================
// Description extraction tests
// ============================================================================

#[test]
fn description_from_meta() {
    let html =
        r#"<html><head><meta name="description" content="A test description"></head></html>"#;
    let result = extract_description_from_html(html);
    assert_eq!(result, Some("A test description".to_string()));
}

#[test]
fn description_from_og() {
    let html =
        r#"<html><head><meta property="og:description" content="OG Description"></head></html>"#;
    let result = extract_description_from_html(html);
    assert_eq!(result, Some("OG Description".to_string()));
}

#[test]
fn description_priority() {
    let html = r#"<html><head>
        <meta name="description" content="Meta Description">
        <meta property="og:description" content="OG Description">
    </head></html>"#;
    let result = extract_description_from_html(html);
    assert_eq!(result, Some("Meta Description".to_string()));
}

#[test]
fn description_none_when_empty() {
    let html = "<html><head></head><body></body></html>";
    let result = extract_description_from_html(html);
    assert_eq!(result, None);
}

// ============================================================================
// Snippet extraction tests
// ============================================================================

#[test]
fn snippet_strips_scripts() {
    let html = "<html><body><p>Hello</p><script>alert('xss')</script><p>World</p></body></html>";
    let result = extract_snippet(html, 1000);
    assert!(!result.contains("alert"));
    assert!(result.contains("Hello"));
    assert!(result.contains("World"));
}

#[test]
fn snippet_strips_styles() {
    let html =
        "<html><head><style>body { color: red; }</style></head><body><p>Visible</p></body></html>";
    let result = extract_snippet(html, 1000);
    assert!(!result.contains("color"));
    assert!(result.contains("Visible"));
}

#[test]
fn snippet_strips_tags() {
    let html = "<html><body><p>Hello <b>bold</b> <a href='#'>link</a></p></body></html>";
    let result = extract_snippet(html, 1000);
    assert!(!result.contains('<'));
    assert!(!result.contains('>'));
    assert!(result.contains("Hello"));
    assert!(result.contains("bold"));
    assert!(result.contains("link"));
}

#[test]
fn snippet_respects_max_chars() {
    let html = "<html><body><p>This is a long paragraph that should be truncated at the limit</p></body></html>";
    let result = extract_snippet(html, 20);
    assert!(result.len() <= 20);
}

#[test]
fn snippet_collapses_whitespace() {
    let html = "<html><body><p>Hello    \n\t   World</p></body></html>";
    let result = extract_snippet(html, 1000);
    // Should collapse multiple whitespace into single space
    assert!(result.contains("Hello World") || result.contains("Hello world"));
    assert!(!result.contains("    "));
}

// ============================================================================
// Version extraction from state
// ============================================================================

#[test]
fn version_from_valid_state() {
    let container =
        fixtures::make_web_container_with_metadata("<html><body>test</body></html>", 42);
    let version = extract_version_from_state(&container);
    assert_eq!(version, Some(42));
}
