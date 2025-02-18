use log::warn;
use xmlparser::{Tokenizer, Token};
use std::collections::HashMap;

fn unescape_xml(value: &str) -> String {
    value.replace("&quot;", "\"")
         .replace("&apos;", "'")
         .replace("&lt;", "<")
         .replace("&gt;", ">")
         .replace("&amp;", "&")
}

pub fn parse_single_xml(src: &str) -> HashMap<String, String> {
    // Ensure the string starts with < and ends with />
    if !src.starts_with('<') || !src.ends_with("/>") {
        warn!("Invalid XML string: {}", src);
    }

    let mut attributes = HashMap::new();
    let tokenizer = Tokenizer::from(src);
    
    for token in tokenizer {
        // If attribute, add to attributes
        if let Ok(token) = token {
            match token {
                Token::Attribute { prefix, local, value, span: _ } => {
                    let key = if prefix.is_empty() {
                        local.to_string()
                    } else {
                        format!("{}:{}", prefix, local)
                    };
                    attributes.insert(key, unescape_xml(value.as_str()));
                }

                _default => { }
            }
        }
    }

    attributes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xml_test() {
        let xml = r#"<xml name="value" can="lens" />"#;
        let attributes = parse_single_xml(xml);

        assert_eq!(attributes.len(), 2);

        // Check solution
        assert_eq!(attributes.get("name").unwrap(), "value");
        assert_eq!(attributes.get("can").unwrap(), "lens");
    }

    #[test]
    fn parse_escaped_xml_test() {
        let xml = r#"<xml name="value" can="&apos;&lt;&lt;&gt;&amp;lens&quot;" escaped="true&quot;" />"#;
        let attributes = parse_single_xml(xml);

        assert_eq!(attributes.len(), 3);

        // Check solution
        assert_eq!(attributes.get("name").unwrap(), "value");
        assert_eq!(attributes.get("can").unwrap(), "'<<>&lens\"");
        assert_eq!(attributes.get("escaped").unwrap(), "true\"");
    }
}