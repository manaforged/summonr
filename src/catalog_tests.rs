use super::Model;

#[test]
fn a_december_release_keeps_its_own_year() {
    let model = Model {
        release: Some(19_722),
        ..Model::named("year-end")
    };
    assert_eq!(model.released().as_deref(), Some("2023-12"));
}
