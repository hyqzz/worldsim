#[cfg(test)]
mod tests {
    use crate::heat::HeatField;
    use ws_core::math::{Aabb, Vec3};

    #[test]
    fn finite_source_lifetime_is_one_macro_tick_not_one_substep() {
        let bounds = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let mut field = HeatField::new(bounds, [4, 4, 4], 1e-7, 4.186e6);
        field.add_source(Vec3::zero(), 0.8, 1000.0, 2);
        field.step(1.0, None::<fn(Vec3) -> Vec3>);
        assert_eq!(field.source_count(), 1);
        assert_eq!(field.sources()[0].ttl, 2);
        field.advance_source_lifetimes();
        assert_eq!(field.sources()[0].ttl, 1);
        field.step(1.0, None::<fn(Vec3) -> Vec3>);
        assert_eq!(field.source_count(), 1);
        field.advance_source_lifetimes();
        assert_eq!(field.source_count(), 0);
    }

    #[test]
    fn restoring_temperature_resets_both_buffers() {
        let bounds = Aabb::new(Vec3::zero(), Vec3::splat(1.0));
        let mut field = HeatField::new(bounds, [4, 4, 4], 1e-7, 4.186e6);
        let values = vec![350.0; 64];
        field.restore_temperature_grid(values.clone()).unwrap();
        field.step(0.1, None::<fn(Vec3) -> Vec3>);
        assert!(field.average_temperature() >= 293.15);
        field.restore_temperature_grid(values.clone()).unwrap();
        assert_eq!(field.average_temperature(), 350.0);
    }
}
