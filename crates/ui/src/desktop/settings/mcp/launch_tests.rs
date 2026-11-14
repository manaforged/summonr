use super::launch;
use summonr_agent::mcp::McpLaunch;

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn the_form_gives_a_command_its_environment() {
    let Ok(McpLaunch::Stdio { command, args, env }) =
        launch("npx -y server", "API_KEY=abc, NAME=two words")
    else {
        panic!("a command is a stdio server");
    };
    assert_eq!(
        (command.as_str(), args),
        ("npx", vec!["-y".to_owned(), "server".to_owned()])
    );
    assert_eq!(env, pairs(&[("API_KEY", "abc"), ("NAME", "two words")]));
}

#[test]
fn the_form_gives_a_url_its_headers() {
    let Ok(McpLaunch::Http { url, headers, .. }) =
        launch("https://example.com/mcp", "Authorization=Bearer abc")
    else {
        panic!("a URL is an HTTP server");
    };
    assert_eq!(url, "https://example.com/mcp");
    assert_eq!(headers, pairs(&[("Authorization", "Bearer abc")]));
}

#[test]
fn the_form_refuses_a_word_with_no_value() {
    let Err(error) = launch("npx server", "API_KEY") else {
        panic!("a word with no value is an error");
    };
    assert!(error.contains("KEY=VALUE"), "{error}");
}
