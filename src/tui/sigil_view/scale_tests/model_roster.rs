use super::{State, busy_info, frame_with};
use crate::child::Access;
use crate::tools::Event;
use crate::tui::{Fact, Msg};

#[test]
fn the_seven_agent_roster_shows_each_selected_model() {
    let roles = [
        ("Decode worker", "gpt-6.1-terra"),
        ("Prefill worker", "gpt-6.1-terra"),
        ("MoE worker", "gpt-6.1-terra"),
        ("Attention worker", "gpt-6.1-terra"),
        ("Projection worker", "gpt-6.1-terra"),
        ("Resource monitor", "gpt-6.1-terra"),
        ("Research assistant", "gpt-6.1-sol"),
    ];
    let shown = frame_with(busy_info(), 164, 60, State::Busy, |app| {
        for (index, (name, model)) in roles.iter().enumerate() {
            app.update(Msg::Fact(Fact::Event(Event::Child {
                id: format!("{:032x}", index + 1),
                event: Box::new(Event::Spawned {
                    assignment: name.to_string(),
                    name: name.to_string(),
                    model: model.to_string(),
                    access: Access::Research,
                    call: String::new(),
                    background: true,
                    workspace: String::new(),
                    agent: String::new(),
                }),
            })));
        }
        app.toggle_agents();
    })
    .join("\n");
    for (name, _) in roles {
        assert!(shown.contains(name), "missing {name}: {shown}");
    }
    assert_eq!(shown.matches("gpt-6.1-terra").count(), 6, "{shown}");
    assert_eq!(shown.matches("gpt-6.1-sol").count(), 1, "{shown}");
}
