//! Public shared-domain regression tests.

use tournamint::Color;

#[test]
fn opposite_color_is_public_exact_and_involutive() {
    for (color, expected) in [(Color::White, Color::Black), (Color::Black, Color::White)] {
        assert_eq!(color.opposite(), expected);
        assert_eq!(color.opposite().opposite(), color);
    }
}
