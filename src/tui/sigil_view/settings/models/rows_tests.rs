use std::collections::BTreeMap;

use super::{ModelRow, list_rows};
use crate::recent::Seat;

#[test]
fn the_current_model_heading_names_its_provider() {
    let choices = [
        Seat::new("zai", "glm-5.3-flash"),
        Seat::new("glm-intl", "glm-5.3"),
    ];
    let rows = list_rows(&choices, [1, 0, 0], &BTreeMap::new(), 0);
    let headers: Vec<&str> = rows
        .iter()
        .filter_map(|row| match row {
            ModelRow::Header(title) => Some(title.as_str()),
            ModelRow::Provider(..) | ModelRow::Seat(_) | ModelRow::Fix(_) | ModelRow::Older(_) => {
                None
            }
        })
        .collect();
    assert_eq!(headers, ["current · zai"]);
}
