use std::io::Write as _;

fn main() -> std::io::Result<()> {
    std::io::stdout()
        .write_all(quantick_operability::matrix::ui_behaviour_matrix_markdown().as_bytes())
}
