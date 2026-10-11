use super::joined;
use crate::provider::{Image, UserInput};

#[test]
fn steering_waiting_messages_become_one_next_turn_in_the_order_sent() {
    let image = Image::encode("image/png", &[1, 2, 3]);
    let mut with_image = UserInput::from("look at this");
    with_image.images.push(image.clone());
    let next = joined([
        UserInput::from("check the build"),
        with_image,
        UserInput::from("  "),
        UserInput::from("actually, stop and run the tests"),
    ])
    .expect("one next turn");
    assert_eq!(
        next.text,
        "check the build\n\nlook at this\n\nactually, stop and run the tests"
    );
    assert_eq!(next.images, vec![image]);
}
