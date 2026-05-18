use once_cell::sync::Lazy;
use regex::Regex;

static TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[a-zA-Z_][a-zA-Z0-9_]*").unwrap());

pub fn split_identifier(token: &str) -> Vec<String> {
    let lower = token.to_lowercase();
    let parts: Vec<String> = if token.contains('_') {
        lower
            .split('_')
            .filter(|p| !p.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        split_camel_case(token)
    };
    if parts.len() >= 2 {
        std::iter::once(lower).chain(parts).collect()
    } else {
        vec![lower]
    }
}

fn split_camel_case(token: &str) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.is_empty() {
        return vec![];
    }

    let mut parts = Vec::new();
    let mut start = 0usize;

    for i in 1..chars.len() {
        let prev = chars[i - 1];
        let curr = chars[i];
        let next = chars.get(i + 1).copied();

        let boundary = (prev.is_lowercase() && curr.is_uppercase())
            || (prev.is_uppercase()
                && curr.is_uppercase()
                && next.is_some_and(|n| n.is_lowercase()));

        if boundary {
            parts.push(chars[start..i].iter().collect::<String>().to_lowercase());
            start = i;
        }
    }

    parts.push(chars[start..].iter().collect::<String>().to_lowercase());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

pub fn tokenize(text: &str) -> Vec<String> {
    TOKEN_RE
        .find_iter(text)
        .flat_map(|m| split_identifier(m.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{split_identifier, tokenize};

    #[test]
    fn splits_camel_case_and_snake_case() {
        assert_eq!(
            split_identifier("HandlerStack"),
            vec!["handlerstack", "handler", "stack"]
        );
        assert_eq!(split_identifier("my_func"), vec!["my_func", "my", "func"]);
        assert_eq!(split_identifier("simple"), vec!["simple"]);
    }

    #[test]
    fn tokenizes_identifier_shapes() {
        let tokens = tokenize("HandlerStack my_func");
        assert!(tokens.contains(&"handlerstack".to_string()));
        assert!(tokens.contains(&"handler".to_string()));
        assert!(tokens.contains(&"stack".to_string()));
        assert!(tokens.contains(&"my_func".to_string()));
    }
}
