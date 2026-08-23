use std::collections::HashSet;

use crate::durable::fact::FactKind;

#[test]
fn recovery_vocabulary_has_exactly_38_unique_facts() {
    assert_eq!(FactKind::ALL.len(), 38);
    assert_eq!(FactKind::ALL.into_iter().collect::<HashSet<_>>().len(), 38);
}
