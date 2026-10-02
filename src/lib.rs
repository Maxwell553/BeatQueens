mod engine;

pub use engine::{is_solution, Board, Outcome};

/// Solve a Queens board. Each string is one row of region labels, either
/// glued together (`AABB`) or separated by whitespace (`1 1 2 2`).
///
/// Returns queen columns, one per row, for the first solution. `Ok(None)`
/// means the board has no solution.
pub fn beat(rows: &[String], limit: u64) -> Result<Outcome, String> {
    let board = Board::parse_rows(rows)?;
    Ok(board.solve(limit))
}
