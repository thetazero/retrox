use retrox_gen::{GalaxyConfig, generate_physical, rocks};
use retrox_sim::BodyKind;

#[test]
fn every_belt_has_fields_with_rocks() {
    let state = generate_physical(2, &GalaxyConfig::default());
    let belts: Vec<_> = state
        .bodies
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Belt)
        .map(|(id, _)| id)
        .collect();
    assert!(!belts.is_empty());
    for belt in belts {
        let fields: Vec<_> = state
            .fields
            .as_slice()
            .iter()
            .filter(|f| f.belt == belt)
            .collect();
        assert!(
            (2..=5).contains(&fields.len()),
            "{}",
            state.bodies[belt].name
        );
        for f in fields {
            assert_eq!(f.orbit.parent, state.bodies[belt].orbit.parent);
            let rocks = rocks(f);
            assert!(rocks.len() >= 150);
            assert_eq!(rocks, retrox_gen::rocks(f), "rocks must be deterministic");
        }
    }
}
