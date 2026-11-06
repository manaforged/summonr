use std::path::Path;

use image::RgbImage;
use tempfile::TempDir;

use crate::endpoint::{Endpoint, Wire, resolve};
use crate::prompt::compose;
use crate::provider::{Client, Image, ToolCall, ToolFunction, ToolKind, UserInput};
use crate::runtime::Loop;
use crate::workflow::Role;
use crate::{Error, Requirement, Result, Sources};

use super::http::{Server, server};

const VISION: &str = "gpt-6.1-sol";
const BLIND: &str = "glm-5.3";
const REPLY: &str = "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\"}]}]}}\n\n";

fn receiver(base: &str, model: &str, images: bool) -> Result<Client> {
    let mut endpoint: Endpoint = resolve(Some("openai"), Some(model), Requirement::Config)?;
    endpoint.base_url = Some(base.to_owned());
    endpoint.api_key = Some(summonr_core::secret::Secret::new("offline-fixture"));
    endpoint.oauth = None;
    endpoint.wire = Wire::Responses;
    endpoint.images = Some(images);
    Client::connect(&endpoint)
}

fn read_reply(workspace: &Path) -> String {
    let call = serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[{
        "type":"function_call","id":"fc-image","call_id":"image-read","name":"read",
        "arguments":serde_json::json!({"op":"image","path":workspace.join("shot.png")}).to_string()
    }]}});
    format!("data: {call}\n\n")
}

async fn parent(
    startup_images: bool,
    images: bool,
    custom: Option<&str>,
) -> Result<(Loop<Client>, Server, TempDir)> {
    let workspace = tempfile::tempdir()?;
    RgbImage::new(8, 8)
        .save(workspace.path().join("shot.png"))
        .map_err(|error| Error::Tool(format!("save screenshot fixture: {error}")))?;
    let (base, server) = server([read_reply(workspace.path()), REPLY.to_owned()]).await?;
    let prompt = match custom {
        Some(text) => text.to_owned(),
        None => compose(
            workspace.path(),
            None,
            Sources {
                images: startup_images,
                ..Default::default()
            },
        ),
    };
    let startup = if startup_images { VISION } else { BLIND };
    let active = if images { VISION } else { BLIND };
    let mut parent = Loop::new(
        receiver(&base, startup, startup_images)?,
        startup,
        workspace.path(),
        prompt,
    )?;
    parent.set_images(startup_images, None);
    parent.set_role_model(
        Role::Implement,
        receiver(&base, active, images)?,
        active,
        None,
        None,
        None,
    );
    Ok((parent, server, workspace))
}

fn screenshot(workspace: &Path) -> Result<UserInput> {
    Ok(UserInput {
        text: "Review this screenshot, and read the image file too".to_owned(),
        images: vec![Image::encode(
            "image/png",
            &std::fs::read(workspace.join("shot.png"))?,
        )],
        source: None,
    })
}

fn assert_received(bodies: &[String], uploaded: bool, images: bool) {
    assert_eq!(bodies.len(), 2, "image read did not finish: {bodies:?}");
    for (index, body) in bodies.iter().enumerate() {
        let model = if images { VISION } else { BLIND };
        assert!(body.contains(model), "wrong receiver: {body}");
        assert_eq!(
            body.contains("The model does not accept image input."),
            !images,
            "guidance contradicts receiver: {body}"
        );
        let count = if images {
            index + usize::from(uploaded)
        } else {
            0
        };
        assert_eq!(
            body.matches("data:image/png;base64,").count(),
            count,
            "wrong image admission: {body}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vision_workflow_receives_user_and_read_images_from_a_text_only_startup() -> Result<()> {
    for startup in [false, true] {
        let (mut parent, server, dirs) = parent(startup, true, None).await?;
        let original = parent.system_prompt.clone();
        let turn = parent.turn_with(screenshot(dirs.path())?, |_| {}).await;
        let bodies = server
            .await
            .map_err(|error| Error::Tool(format!("offline backend task: {error}")))??;
        assert_eq!(turn?.text, "done");
        assert_eq!(
            parent.system_prompt, original,
            "request rewrote saved guidance"
        );
        assert_received(&bodies, true, true);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_only_workflow_drops_images_and_replaces_a_vision_startup_hint() -> Result<()> {
    let (mut parent, server, dirs) = parent(true, false, None).await?;
    let turn = parent.turn_with(screenshot(dirs.path())?, |_| {}).await;
    let bodies = server
        .await
        .map_err(|error| Error::Tool(format!("offline backend task: {error}")))??;
    assert_eq!(turn?.text, "done");
    assert_received(&bodies, true, false);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_the_default_receiver_refreshes_guidance_without_changing_history() -> Result<()>
{
    let (mut parent, server, dirs) = parent(false, true, None).await?;
    let selected = parent.selection(Role::Implement).0.clone();
    parent.clear_role_model(Role::Implement);
    parent.set_provider(selected, VISION.to_owned());
    let original = parent.system_prompt.clone();
    let turn = parent.turn_with(screenshot(dirs.path())?, |_| {}).await;
    let bodies = server
        .await
        .map_err(|error| Error::Tool(format!("offline backend task: {error}")))??;
    assert_eq!(turn?.text, "done");
    assert_eq!(parent.system_prompt, original);
    assert_received(&bodies, true, true);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_vision_child_receives_images_without_its_parents_blindness_instruction() -> Result<()> {
    let (mut parent, server, _dirs) = parent(false, true, None).await?;
    let call = ToolCall {
        id: "review-image".to_owned(), kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(), name: "task".to_owned(),
            arguments: serde_json::json!({"assignment":"Review the screenshot","access":"research","background":false}).to_string(),
        }, continuation: None,
    };
    let result = parent
        .task_execute(&call, &mut Vec::new(), &mut |_| {})
        .await;
    let bodies = server
        .await
        .map_err(|error| Error::Tool(format!("offline backend task: {error}")))??;
    let result = result?;
    assert!(result.output.success, "{}", result.output.text);
    assert!(
        result.output.text.contains("done"),
        "{}",
        result.output.text
    );
    assert_received(&bodies, false, true);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_system_instructions_are_not_rewritten_as_model_guidance() -> Result<()> {
    let instructions =
        "Custom policy: The model does not accept image input. Keep this quoted instruction.";
    let (mut parent, server, dirs) = parent(true, true, Some(instructions)).await?;
    let turn = parent.turn_with(screenshot(dirs.path())?, |_| {}).await;
    let bodies = server
        .await
        .map_err(|error| Error::Tool(format!("offline backend task: {error}")))??;
    assert_eq!(turn?.text, "done");
    assert_eq!(bodies.len(), 2);
    for (index, body) in bodies.iter().enumerate() {
        assert!(
            body.contains(instructions),
            "custom policy was changed: {body}"
        );
        assert_eq!(body.matches("data:image/png;base64,").count(), index + 1);
    }
    Ok(())
}
