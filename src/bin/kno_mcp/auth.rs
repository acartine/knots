use std::fs;
use std::path::Path;

pub fn read_token(token_file: Option<&Path>, env_name: &str) -> Result<String, std::io::Error> {
    if let Some(path) = token_file {
        return fs::read_to_string(path).map(|token| token.trim().to_string());
    }
    Ok(std::env::var(env_name).unwrap_or_default())
}

pub fn bearer_token_matches(header: Option<&str>, expected: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let Some(supplied) = header.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(supplied.as_bytes(), expected.as_bytes()) && !expected.is_empty()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let max_len = left.len().max(right.len());
    let mut diff = left.len() ^ right.len();
    for i in 0..max_len {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_missing_empty_short_or_wrong_tokens() {
        assert!(!bearer_token_matches(None, "secret"));
        assert!(!bearer_token_matches(Some("Bearer secret"), ""));
        assert!(!bearer_token_matches(Some("Basic secret"), "secret"));
        assert!(!bearer_token_matches(Some("Bearer sec"), "secret"));
        assert!(!bearer_token_matches(Some("Bearer wrong"), "secret"));
        assert!(bearer_token_matches(Some("Bearer secret"), "secret"));
    }

    #[test]
    fn reads_token_file_or_environment() {
        let token_ws = knots_test_support::workspace("knots-kno-mcp-auth-token");
        let token_file = token_ws.path().join("token");
        fs::write(&token_file, " file-secret \n").expect("write token file");
        assert_eq!(
            read_token(Some(&token_file), "KNO_MCP_AUTH_UNUSED").expect("token file"),
            "file-secret"
        );

        std::env::set_var("KNO_MCP_AUTH_TEST_TOKEN", "env-secret");
        assert_eq!(
            read_token(None, "KNO_MCP_AUTH_TEST_TOKEN").expect("env token"),
            "env-secret"
        );
        std::env::remove_var("KNO_MCP_AUTH_TEST_TOKEN");

        assert_eq!(
            read_token(None, "KNO_MCP_AUTH_TEST_MISSING").expect("missing env"),
            ""
        );
    }
}
