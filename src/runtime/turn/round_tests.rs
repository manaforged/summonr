use super::*;
use crate::provider::Image;
use crate::provider::scripted::{Reply, Scripted};

fn blind_session(workspace: &Path, home: &Path, provider: Scripted) -> Loop<Scripted> {
    let driver =
        super::super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    let mut loop_ = Loop::open(provider, "m", workspace, "system", driver).unwrap();
    loop_.set_images(false, None);
    loop_
}

#[tokio::test]
async fn an_image_on_a_blind_model_is_folded_to_text_not_sent() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let provider = Scripted::new([Reply::Text("ok")]);
    let sent = provider.sent();
    let mut loop_ = blind_session(workspace.path(), home.path(), provider);
    let input = UserInput {
        text: "here is a pic of me".to_string(),
        images: vec![Image::encode("image/png", b"not a real png")],
        source: None,
    };
    loop_.turn_with(input, |_| {}).await.unwrap();
    let sent = sent.lock().unwrap();
    assert!(
        sent.iter()
            .flatten()
            .all(|message| message.images.is_empty()),
        "a blind model received image content: {sent:?}"
    );
    assert!(
        sent.iter()
            .flatten()
            .any(|message| message.content.contains("image(s)")),
        "the fold note never reached the model: {sent:?}"
    );
}

#[tokio::test]
async fn a_switch_to_a_blind_model_folds_the_next_attached_image() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = blind_session(workspace.path(), home.path(), Scripted::new([]));
    loop_.set_images(true, None);
    let provider = Scripted::new([Reply::Text("ok")]);
    let sent = provider.sent();
    loop_.set_provider(provider, "glm-5.3");
    let input = UserInput {
        text: "what is in this?".to_string(),
        images: vec![Image::encode("image/png", b"not a real png")],
        source: None,
    };
    loop_.turn_with(input, |_| {}).await.unwrap();
    let sent = sent.lock().unwrap();
    assert!(
        sent.iter()
            .flatten()
            .all(|message| message.images.is_empty()),
        "the blind model received image content: {sent:?}"
    );
}
