/// MCP server name for the Chrome DevTools Protocol server.
pub const CDP_SERVER: &str = "chrome-devtools";

/// Parse a CDP snapshot and find elements whose text contains the target.
/// Returns a vec of (uid, matched_line) tuples.
///
/// The snapshot from chrome-devtools `take_snapshot` is a text representation
/// of the accessibility tree where each element has a UID. Format:
/// ```text
/// [uid="e1"] button "Submit"
/// [uid="e2"] link "Friends"
/// ```
pub fn find_elements_in_snapshot(snapshot_text: &str, target: &str) -> Vec<(String, String)> {
    let target_lower = target.to_lowercase();
    let mut matches = Vec::new();
    for line in snapshot_text.lines() {
        if let Some(uid_start) = line.find("uid=\"") {
            let uid_rest = &line[uid_start + 5..];
            if let Some(uid_end) = uid_rest.find('"') {
                let uid = &uid_rest[..uid_end];
                if line.to_lowercase().contains(&target_lower) {
                    matches.push((uid.to_string(), line.trim().to_string()));
                }
            }
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::find_elements_in_snapshot;

    const SNAPSHOT: &str = r#"
[uid="e1"] button "Submit"
[uid="e2"] link "Friends"
[uid="e3"] heading "Settings"
[uid="e4"] button "Submit Form"
"#;

    #[test]
    fn single_match() {
        let matches = find_elements_in_snapshot(SNAPSHOT, "Friends");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "e2");
    }

    #[test]
    fn multiple_matches() {
        let matches = find_elements_in_snapshot(SNAPSHOT, "Submit");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0, "e1");
        assert_eq!(matches[1].0, "e4");
    }

    #[test]
    fn case_insensitive() {
        let matches = find_elements_in_snapshot(SNAPSHOT, "settings");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "e3");
    }

    #[test]
    fn no_matches() {
        let matches = find_elements_in_snapshot(SNAPSHOT, "Nonexistent");
        assert!(matches.is_empty());
    }

    #[test]
    fn empty_snapshot() {
        let matches = find_elements_in_snapshot("", "Submit");
        assert!(matches.is_empty());
    }
}
