use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;
use crate::endpoint::{Endpoint, Wire};

#[tokio::test]
async fn picker_discovers_models_from_completions_endpoint() -> crate::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 2048];
        let count = stream.read(&mut request)?;
        assert!(String::from_utf8_lossy(&request[..count]).starts_with("GET /models "));
        let body = r#"{"data":[{"id":"glm-5.3"},{"id":"glm-5.3-flash"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    });
    let endpoint = Endpoint {
        name: "glm".to_string(),
        model: "glm-5.3".to_string(),
        base_url: Some(format!("http://{address}")),
        api_key: Some("key".to_string()),
        oauth: None,
        body: None,
        context_window: None,
        env: Default::default(),
        headers: Default::default(),
        wire: Wire::Completions,
    };

    let models = discover(vec![endpoint], std::time::Duration::from_secs(60)).await;

    assert_eq!(models, ["glm\tglm-5.3", "glm\tglm-5.3-flash"]);
    server
        .join()
        .map_err(|_panic| crate::Error::Config("model fixture panicked".to_string()))??;
    Ok(())
}
