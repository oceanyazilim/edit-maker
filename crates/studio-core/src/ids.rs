//! Identifier helpers. IDs are plain strings in serialized form so the
//! planner can generate deterministic ids (reproducible plans) while
//! user operations use random UUIDs.

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// Produce a safe file-name stem from user input (no separators, no traversal).
pub fn sanitize_file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .trim_matches('.')
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "untitled".into()
    } else {
        s.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitizes() {
        assert_eq!(sanitize_file_stem("a/b\\c:..d"), "a_b_c___d");
        assert_eq!(sanitize_file_stem("..."), "untitled");
        assert!(new_id("clip").starts_with("clip_"));
    }
}
