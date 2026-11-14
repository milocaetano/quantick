use super::*;
#[test]
fn silhouettes_never_expand_or_fill_the_earned_disc_even_at_subpixel_radii() {
    for radius in [0.01, 0.2, 1.0, 3.0, 12.0] {
        let disc = FlowDisc {
            center: [0.0, 0.0],
            radius,
        };
        let mut mesh = egui::Mesh::default();
        add_perimeter(
            &mut mesh,
            disc,
            0.0,
            std::f64::consts::TAU,
            egui::Color32::RED,
            false,
        );
        assert!(!mesh.is_empty());
        for vertex in &mesh.vertices {
            let distance = vertex.pos.distance(disc.center.into());
            assert!(distance <= radius + 0.00001);
            assert!(distance >= radius * 0.75 - 0.00001);
        }
        let mut exception = egui::Mesh::default();
        add_perimeter(
            &mut exception,
            disc,
            0.0,
            std::f64::consts::TAU,
            egui::Color32::RED,
            true,
        );
        assert!(!exception.is_empty());
        assert!(
            exception
                .vertices
                .iter()
                .all(|vertex| vertex.pos.distance(disc.center.into()) >= radius * 0.75 - 0.00001)
        );
        assert_ne!(exception.vertices, mesh.vertices);
    }
}
