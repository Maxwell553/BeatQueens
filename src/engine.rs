//! LinkedIn Queens.
//!
//! One queen per row, column, and color region. Two queens may not touch,
//! including on the diagonal. Only the eight neighboring cells are forbidden:
//! a queen does not attack a whole chess diagonal.
//!
//! The search state is a bitset per row. Propagation closes singletons, unique
//! columns, region confinement, and Hall/pigeonhole subsets. Search branches
//! on the row with the fewest candidates and stops at the first solution unless
//! a full count is requested.

const MAX: usize = 20;

#[derive(Clone, Copy)]
struct Mode {
    /// 0 off, 1 pairs, 2 pairs+triples, 3 every subset while it is tiny.
    level: u8,
    /// Order branch values so a single solution is found before the dead ends.
    order: bool,
}

const ROOT: Mode = Mode {
    level: 3,
    order: false,
};
const SEARCH_ONE: Mode = Mode {
    level: 1,
    order: true,
};
const SEARCH_ALL: Mode = Mode {
    level: 0,
    order: false,
};

#[derive(Clone, Copy)]
struct RegInfo {
    rows: u64,
    cols: u64,
    count: u16,
    only_r: u8,
    only_c: u8,
}

#[derive(Clone, Copy)]
struct State {
    row: [u64; MAX],
    assign: [u8; MAX],
    open_rows: u64,
    open_cols: u64,
    open_regs: u64,
}

pub struct Board {
    pub n: usize,
    region: [[u8; MAX]; MAX],
    reg_cols: [[u64; MAX]; MAX],
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub solutions: u64,
    pub nodes: u64,
    /// Column of the queen in each row, for the first solution found.
    pub queens: Option<Vec<usize>>,
}

enum Walk {
    Found { queens: Vec<usize>, nodes: u64 },
    Empty { nodes: u64 },
    Cut { nodes: u64 },
}

impl Board {
    pub fn parse_rows(lines: &[String]) -> Result<Self, String> {
        if lines.is_empty() {
            return Err("board is empty".into());
        }
        let n = lines.len();
        if n > MAX {
            return Err(format!("board size {n} exceeds the supported maximum {MAX}"));
        }
        let mut labels: Vec<Vec<String>> = Vec::with_capacity(n);
        for (i, line) in lines.iter().enumerate() {
            let cells = split_cells(line);
            if cells.len() != n {
                return Err(format!(
                    "row {i} has {} cells, expected {n}",
                    cells.len()
                ));
            }
            labels.push(cells);
        }
        let mut ids: Vec<String> = Vec::with_capacity(n);
        for row in &labels {
            for cell in row {
                if !ids.iter().any(|id| id == cell) {
                    ids.push(cell.clone());
                }
            }
        }
        if ids.len() != n {
            return Err(format!(
                "a {n}×{n} Queens board needs {n} regions, found {}",
                ids.len()
            ));
        }
        let mut region = [[0u8; MAX]; MAX];
        let mut reg_cols = [[0u64; MAX]; MAX];
        for r in 0..n {
            for c in 0..n {
                let g = ids.iter().position(|id| id == &labels[r][c]).unwrap() as u8;
                region[r][c] = g;
                reg_cols[g as usize][r] |= 1u64 << c;
            }
        }
        Ok(Board { n, region, reg_cols })
    }

    pub fn solve(&self, limit: u64) -> Outcome {
        let mut out = Outcome {
            solutions: 0,
            nodes: 0,
            queens: None,
        };
        if limit == 0 {
            return out;
        }
        // A tight row-by-row scan finds easy unique boards in a few hundred
        // nanoseconds. Hard boards blow that scan up, so it stops at a node
        // cap and the propagator takes over.
        if limit == 1 {
            match self.naive_first(384) {
                Walk::Found { queens, nodes } => {
                    out.solutions = 1;
                    out.nodes = nodes;
                    out.queens = Some(queens);
                    return out;
                }
                Walk::Empty { nodes } => {
                    out.nodes = nodes;
                    return out;
                }
                Walk::Cut { nodes } => out.nodes = nodes,
            }
        }
        let mut state = self.initial_state();
        if !propagate(self, &mut state, ROOT) {
            return out;
        }
        let mode = if limit == 1 { SEARCH_ONE } else { SEARCH_ALL };
        search(self, &state, mode, limit, &mut out);
        out
    }

    fn naive_first(&self, cap: u32) -> Walk {
        fn rec(
            board: &Board,
            row: usize,
            cols: u64,
            regs: u64,
            prev: Option<usize>,
            assign: &mut [usize; MAX],
            nodes: &mut u32,
            cap: u32,
        ) -> Walk {
            *nodes += 1;
            if *nodes > cap {
                return Walk::Cut { nodes: u64::from(*nodes) };
            }
            if row == board.n {
                return Walk::Found {
                    queens: assign[..board.n].to_vec(),
                    nodes: u64::from(*nodes),
                };
            }
            let mut avail = ((1u64 << board.n) - 1) & !cols;
            if let Some(p) = prev {
                avail &= !diag_ban(p, board.n);
            }
            let mut used = regs;
            while used != 0 {
                let g = used.trailing_zeros() as usize;
                used &= used - 1;
                avail &= !board.reg_cols[g][row];
            }
            while avail != 0 {
                let c = avail.trailing_zeros() as usize;
                avail &= avail - 1;
                let g = board.region[row][c] as usize;
                assign[row] = c;
                match rec(
                    board,
                    row + 1,
                    cols | (1u64 << c),
                    regs | (1u64 << g),
                    Some(c),
                    assign,
                    nodes,
                    cap,
                ) {
                    Walk::Empty { .. } => {}
                    other => return other,
                }
            }
            Walk::Empty {
                nodes: u64::from(*nodes),
            }
        }
        let mut assign = [0usize; MAX];
        rec(self, 0, 0, 0, None, &mut assign, &mut 0, cap)
    }

    /// Row-by-row bitmask counter with no propagation. Used as the oracle.
    /// `deadline` aborts the walk so a baseline can be timed against hard boards.
    pub fn count_naive(&self, limit: u64) -> u64 {
        self.count_naive_until(limit, None).unwrap_or(0)
    }

    pub fn count_naive_until(&self, limit: u64, deadline: Option<std::time::Instant>) -> Option<u64> {
        fn rec(
            board: &Board,
            row: usize,
            cols: u64,
            regs: u64,
            prev: Option<usize>,
            limit: u64,
            ticks: &mut u32,
            deadline: Option<std::time::Instant>,
        ) -> Option<u64> {
            *ticks = ticks.wrapping_add(1);
            if *ticks & 255 == 0 {
                if let Some(deadline) = deadline {
                    if std::time::Instant::now() >= deadline {
                        return None;
                    }
                }
            }
            let n = board.n;
            if row == n {
                return Some(1);
            }
            let mut avail = ((1u64 << n) - 1) & !cols;
            if let Some(p) = prev {
                avail &= !diag_ban(p, n);
            }
            let mut used = regs;
            while used != 0 {
                let g = used.trailing_zeros() as usize;
                used &= used - 1;
                avail &= !board.reg_cols[g][row];
            }
            let mut total = 0u64;
            while avail != 0 {
                let c = avail.trailing_zeros() as usize;
                avail &= avail - 1;
                let g = board.region[row][c] as usize;
                total += rec(
                    board,
                    row + 1,
                    cols | (1u64 << c),
                    regs | (1u64 << g),
                    Some(c),
                    limit,
                    ticks,
                    deadline,
                )?;
                if total >= limit {
                    return Some(total);
                }
            }
            Some(total)
        }
        if limit == 0 {
            return Some(0);
        }
        rec(self, 0, 0, 0, None, limit, &mut 0, deadline)
    }

    fn initial_state(&self) -> State {
        let all = (1u64 << self.n) - 1;
        let mut state = State {
            row: [0; MAX],
            assign: [0; MAX],
            open_rows: all,
            open_cols: all,
            open_regs: all,
        };
        for r in 0..self.n {
            state.row[r] = all;
        }
        state
    }
}

pub fn is_solution(board: &Board, queens: &[usize]) -> bool {
    let n = board.n;
    if queens.len() != n {
        return false;
    }
    let mut seen_col = 0u64;
    let mut seen_reg = 0u64;
    for r in 0..n {
        let c = queens[r];
        if c >= n {
            return false;
        }
        let bit = 1u64 << c;
        if seen_col & bit != 0 {
            return false;
        }
        seen_col |= bit;
        let g = 1u64 << board.region[r][c];
        if seen_reg & g != 0 {
            return false;
        }
        seen_reg |= g;
        for o in 0..r {
            let dr = r - o;
            let dc = (c as i32 - queens[o] as i32).unsigned_abs();
            if dr.max(dc as usize) == 1 {
                return false;
            }
        }
    }
    seen_col == (1u64 << n) - 1 && seen_reg == (1u64 << n) - 1
}

fn split_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if trimmed.chars().any(|ch| ch.is_whitespace()) {
        trimmed.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        trimmed.chars().map(|ch| ch.to_string()).collect()
    }
}

#[inline]
fn diag_ban(c: usize, n: usize) -> u64 {
    let mut ban = 0u64;
    if c > 0 {
        ban |= 1u64 << (c - 1);
    }
    if c + 1 < n {
        ban |= 1u64 << (c + 1);
    }
    ban
}

#[inline]
fn apply(board: &Board, state: &mut State, r: usize, c: usize) -> bool {
    let bit = 1u64 << c;
    if state.open_rows & (1u64 << r) == 0 || state.row[r] & bit == 0 {
        return false;
    }
    let g = board.region[r][c] as usize;
    if state.open_regs & (1u64 << g) == 0 {
        return false;
    }
    state.assign[r] = c as u8;
    state.open_rows &= !(1u64 << r);
    state.open_cols &= !bit;
    state.open_regs &= !(1u64 << g);

    let ban = diag_ban(c, board.n);
    let neighbors = if r > 0 { 1u64 << (r - 1) } else { 0 }
        | if r + 1 < board.n { 1u64 << (r + 1) } else { 0 };
    let mut rows = state.open_rows;
    while rows != 0 {
        let rr = rows.trailing_zeros() as usize;
        rows &= rows - 1;
        let mut mask = state.row[rr] & !bit & !board.reg_cols[g][rr];
        if (1u64 << rr) & neighbors != 0 {
            mask &= !ban;
        }
        state.row[rr] = mask;
    }
    true
}

fn region_infos(board: &Board, state: &State, info: &mut [RegInfo; MAX]) {
    let mut regs = state.open_regs;
    while regs != 0 {
        let g = regs.trailing_zeros() as usize;
        regs &= regs - 1;
        let mut count = 0u16;
        let mut rows = 0u64;
        let mut cols = 0u64;
        let mut only_r = 0u8;
        let mut only_c = 0u8;
        let mut open = state.open_rows;
        while open != 0 {
            let r = open.trailing_zeros() as usize;
            open &= open - 1;
            let mask = state.row[r] & board.reg_cols[g][r];
            if mask == 0 {
                continue;
            }
            let bits = mask.count_ones() as u16;
            if count == 0 && bits == 1 {
                only_r = r as u8;
                only_c = mask.trailing_zeros() as u8;
            }
            count += bits;
            rows |= 1u64 << r;
            cols |= mask;
        }
        info[g] = RegInfo {
            rows,
            cols,
            count,
            only_r,
            only_c,
        };
    }
}

/// Cheap forced moves and domain wipes. Places at most one queen.
fn fast(board: &Board, state: &mut State) -> Result<bool, ()> {
    let mut rows = state.open_rows;
    let mut cover = 0u64;
    while rows != 0 {
        let r = rows.trailing_zeros() as usize;
        rows &= rows - 1;
        state.row[r] &= state.open_cols;
        if state.row[r] == 0 {
            return Err(());
        }
        cover |= state.row[r];
    }
    if cover != state.open_cols {
        return Err(());
    }

    let mut info = [RegInfo {
        rows: 0,
        cols: 0,
        count: 0,
        only_r: 0,
        only_c: 0,
    }; MAX];
    region_infos(board, state, &mut info);

    let mut regs = state.open_regs;
    while regs != 0 {
        let g = regs.trailing_zeros() as usize;
        regs &= regs - 1;
        if info[g].count == 0 {
            return Err(());
        }
    }

    regs = state.open_regs;
    while regs != 0 {
        let g = regs.trailing_zeros() as usize;
        regs &= regs - 1;
        if info[g].count == 1
            && !apply(board, state, info[g].only_r as usize, info[g].only_c as usize)
        {
            return Err(());
        }
        if info[g].count == 1 {
            return Ok(true);
        }
    }

    rows = state.open_rows;
    while rows != 0 {
        let r = rows.trailing_zeros() as usize;
        rows &= rows - 1;
        if state.row[r].count_ones() == 1 {
            let c = state.row[r].trailing_zeros() as usize;
            if !apply(board, state, r, c) {
                return Err(());
            }
            return Ok(true);
        }
    }

    let mut once = 0u64;
    let mut multi = 0u64;
    rows = state.open_rows;
    while rows != 0 {
        let r = rows.trailing_zeros() as usize;
        rows &= rows - 1;
        let mask = state.row[r];
        multi |= once & mask;
        once |= mask;
    }
    let unique = once & !multi;
    if unique != 0 {
        let c = unique.trailing_zeros() as usize;
        let bit = 1u64 << c;
        rows = state.open_rows;
        while rows != 0 {
            let r = rows.trailing_zeros() as usize;
            rows &= rows - 1;
            if state.row[r] & bit != 0 {
                if !apply(board, state, r, c) {
                    return Err(());
                }
                return Ok(true);
            }
        }
        return Err(());
    }

    let mut progress = false;
    regs = state.open_regs;
    while regs != 0 {
        let g = regs.trailing_zeros() as usize;
        regs &= regs - 1;
        if info[g].rows.count_ones() == 1 {
            let r = info[g].rows.trailing_zeros() as usize;
            let before = state.row[r];
            state.row[r] &= board.reg_cols[g][r];
            progress |= state.row[r] != before;
        }
        if info[g].cols.count_ones() == 1 {
            let c = info[g].cols.trailing_zeros() as usize;
            let bit = 1u64 << c;
            let mut open = state.open_rows;
            while open != 0 {
                let r = open.trailing_zeros() as usize;
                open &= open - 1;
                if board.reg_cols[g][r] & bit == 0 && state.row[r] & bit != 0 {
                    state.row[r] &= !bit;
                    progress = true;
                }
            }
        }
    }
    Ok(progress)
}

fn pigeon(board: &Board, state: &mut State, level: u8) -> Result<bool, ()> {
    let mut progress = false;
    let row_deep = level;
    let col_deep = level;
    let reg_deep = level;

    let mut info = [RegInfo {
        rows: 0,
        cols: 0,
        count: 0,
        only_r: 0,
        only_c: 0,
    }; MAX];
    region_infos(board, state, &mut info);

    visit_subsets(state.open_rows, row_deep, |sub| {
        let k = sub.count_ones();
        let mut cols = 0u64;
        let mut bits = sub;
        while bits != 0 {
            let r = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            cols |= state.row[r];
        }
        let ck = cols.count_ones();
        if ck < k {
            return Err(());
        }
        if ck == k {
            let mut others = state.open_rows & !sub;
            while others != 0 {
                let r = others.trailing_zeros() as usize;
                others &= others - 1;
                let before = state.row[r];
                state.row[r] &= !cols;
                progress |= state.row[r] != before;
            }
        }
        Ok(())
    })?;

    visit_subsets(state.open_cols, col_deep, |sub| {
        let k = sub.count_ones();
        let mut touching = 0u64;
        let mut rows = state.open_rows;
        while rows != 0 {
            let r = rows.trailing_zeros() as usize;
            rows &= rows - 1;
            if state.row[r] & sub != 0 {
                touching |= 1u64 << r;
            }
        }
        let tk = touching.count_ones();
        if tk < k {
            return Err(());
        }
        if tk == k {
            let mut bits = touching;
            while bits != 0 {
                let r = bits.trailing_zeros() as usize;
                bits &= bits - 1;
                let before = state.row[r];
                state.row[r] &= sub;
                progress |= state.row[r] != before;
            }
        }
        Ok(())
    })?;

    visit_subsets(state.open_regs, reg_deep, |sub| {
        let k = sub.count_ones();
        let mut rows = 0u64;
        let mut cols = 0u64;
        let mut bits = sub;
        while bits != 0 {
            let g = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            rows |= info[g].rows;
            cols |= info[g].cols;
        }
        if rows.count_ones() < k || cols.count_ones() < k {
            return Err(());
        }
        if rows.count_ones() == k {
            let mut rb = rows;
            while rb != 0 {
                let r = rb.trailing_zeros() as usize;
                rb &= rb - 1;
                let mut allow = 0u64;
                let mut gb = sub;
                while gb != 0 {
                    let g = gb.trailing_zeros() as usize;
                    gb &= gb - 1;
                    allow |= board.reg_cols[g][r];
                }
                let before = state.row[r];
                state.row[r] &= allow;
                progress |= state.row[r] != before;
            }
        }
        if cols.count_ones() == k {
            let mut open = state.open_rows;
            while open != 0 {
                let r = open.trailing_zeros() as usize;
                open &= open - 1;
                let keep = state.row[r] & !cols;
                let mut gb = sub;
                let mut inside = 0u64;
                while gb != 0 {
                    let g = gb.trailing_zeros() as usize;
                    gb &= gb - 1;
                    inside |= board.reg_cols[g][r] & cols;
                }
                let next = keep | (state.row[r] & inside);
                progress |= next != state.row[r];
                state.row[r] = next;
            }
        }
        Ok(())
    })?;

    Ok(progress)
}

fn visit_subsets(mask: u64, level: u8, mut f: impl FnMut(u64) -> Result<(), ()>) -> Result<(), ()> {
    let n = mask.count_ones();
    if n < 2 || level == 0 {
        return Ok(());
    }
    if level >= 3 && n <= 10 {
        let mut sub = mask;
        loop {
            if sub == 0 {
                break;
            }
            if sub.count_ones() >= 2 {
                f(sub)?;
            }
            sub = (sub - 1) & mask;
        }
        return Ok(());
    }

    let mut ids = [0u8; MAX];
    let mut len = 0usize;
    let mut bits = mask;
    while bits != 0 {
        ids[len] = bits.trailing_zeros() as u8;
        len += 1;
        bits &= bits - 1;
    }
    for i in 0..len {
        for j in (i + 1)..len {
            f((1u64 << ids[i]) | (1u64 << ids[j]))?;
        }
    }
    if level >= 2 && len >= 3 && len <= 12 {
        for i in 0..len {
            for j in (i + 1)..len {
                for k in (j + 1)..len {
                    f((1u64 << ids[i]) | (1u64 << ids[j]) | (1u64 << ids[k]))?;
                }
            }
        }
    }
    Ok(())
}

fn propagate(board: &Board, state: &mut State, mode: Mode) -> bool {
    // Each pass places or deletes at least one candidate, so this bounds the loop.
    let cap = board.n * board.n + 4;
    for _ in 0..cap {
        match fast(board, state) {
            Err(()) => return false,
            Ok(true) => continue,
            Ok(false) => {}
        }
        if mode.level == 0 {
            return true;
        }
        match pigeon(board, state, mode.level) {
            Err(()) => return false,
            Ok(true) => continue,
            Ok(false) => return true,
        }
    }
    true
}

fn search(board: &Board, state: &State, mode: Mode, limit: u64, out: &mut Outcome) -> bool {
    out.nodes += 1;
    if state.open_rows == 0 {
        out.solutions += 1;
        if out.queens.is_none() {
            out.queens = Some((0..board.n).map(|r| state.assign[r] as usize).collect());
        }
        return out.solutions >= limit;
    }

    let mut best_r = 0usize;
    let mut best_pc = u32::MAX;
    let mut rows = state.open_rows;
    while rows != 0 {
        let r = rows.trailing_zeros() as usize;
        rows &= rows - 1;
        let pc = state.row[r].count_ones();
        if pc < best_pc {
            best_pc = pc;
            best_r = r;
            if pc <= 1 {
                break;
            }
        }
    }

    if !mode.order {
        let mut opts = state.row[best_r];
        while opts != 0 {
            let c = opts.trailing_zeros() as usize;
            opts &= opts - 1;
            let mut child = *state;
            if apply(board, &mut child, best_r, c) && propagate(board, &mut child, mode) && search(board, &child, mode, limit, out)
            {
                return true;
            }
        }
        return false;
    }

    let mut info = [RegInfo {
        rows: 0,
        cols: 0,
        count: 0,
        only_r: 0,
        only_c: 0,
    }; MAX];
    region_infos(board, state, &mut info);

    let mut best_reg = 0usize;
    let mut best_reg_pc = u32::MAX;
    let mut regs = state.open_regs;
    while regs != 0 {
        let g = regs.trailing_zeros() as usize;
        regs &= regs - 1;
        let pc = info[g].count as u32;
        if pc < best_reg_pc {
            best_reg_pc = pc;
            best_reg = g;
        }
    }

    let mut freq = [0u8; MAX];
    let mut open = state.open_rows;
    while open != 0 {
        let r = open.trailing_zeros() as usize;
        open &= open - 1;
        let mut mask = state.row[r];
        while mask != 0 {
            let c = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            freq[c] = freq[c].saturating_add(1);
        }
    }

    // (score, row, col). Lower scores are the placements most likely to be forced.
    let mut choice = [(0u16, 0u8, 0u8); MAX];
    let mut nchoice = 0usize;
    if best_reg_pc < best_pc {
        let mut rows = info[best_reg].rows;
        while rows != 0 {
            let r = rows.trailing_zeros() as usize;
            rows &= rows - 1;
            let mut mask = state.row[r] & board.reg_cols[best_reg][r];
            while mask != 0 {
                let c = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                let score = (u16::from(freq[c]) << 8) | info[board.region[r][c] as usize].count;
                choice[nchoice] = (score, r as u8, c as u8);
                nchoice += 1;
            }
        }
    } else {
        let mut mask = state.row[best_r];
        while mask != 0 {
            let c = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            let score = (u16::from(freq[c]) << 8) | info[board.region[best_r][c] as usize].count;
            choice[nchoice] = (score, best_r as u8, c as u8);
            nchoice += 1;
        }
    }
    choice[..nchoice].sort_unstable_by_key(|item| std::cmp::Reverse(item.0));

    for i in 0..nchoice {
        let r = choice[i].1 as usize;
        let c = choice[i].2 as usize;
        let mut child = *state;
        if apply(board, &mut child, r, c)
            && propagate(board, &mut child, mode)
            && search(board, &child, mode, limit, out)
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(rows: &[&str]) -> Board {
        let lines: Vec<String> = rows.iter().map(|s| (*s).to_string()).collect();
        Board::parse_rows(&lines).unwrap()
    }

    #[test]
    fn adjacent_pair_is_impossible() {
        let b = board(&["AB", "BA"]);
        assert_eq!(b.solve(10).solutions, 0);
        assert_eq!(b.count_naive(10), 0);
    }

    #[test]
    fn column_regions_match_naive() {
        let b = board(&["ABCD", "ABCD", "ABCD", "ABCD"]);
        let smart = b.solve(10_000);
        let naive = b.count_naive(10_000);
        assert_eq!(smart.solutions, naive);
        assert!(smart.solutions > 0);
        assert!(is_solution(&b, smart.queens.as_ref().unwrap()));
    }

    #[test]
    fn community_level_one_has_fourteen_solutions() {
        let b = board(&[
            "AAABCD", "AAABCD", "BBBBCD", "BBECCD", "FEECCD", "FFEECC",
        ]);
        let out = b.solve(100);
        assert_eq!(out.solutions, 14);
        assert_eq!(b.count_naive(100), 14);
        assert!(is_solution(&b, out.queens.as_ref().unwrap()));
    }

    #[test]
    fn random_boards_match_the_naive_counter() {
        let mut seed = 0xC0FFEE_u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed
        };
        for n in 4..=7 {
            for _ in 0..12 {
                let mut rows = Vec::with_capacity(n);
                for _r in 0..n {
                    let mut line = String::new();
                    for _c in 0..n {
                        let label = (b'A' + (next() as usize % n) as u8) as char;
                        line.push(label);
                    }
                    rows.push(line);
                }
                let lines: Vec<String> = rows.iter().cloned().collect();
                let Ok(b) = Board::parse_rows(&lines) else {
                    continue;
                };
                let smart = b.solve(50_000).solutions;
                let naive = b.count_naive(50_000);
                assert_eq!(smart, naive, "mismatch on {rows:?}");
            }
        }
    }

    #[test]
    fn rejects_bad_shape() {
        let lines = vec!["AB".to_string(), "ABC".to_string()];
        assert!(Board::parse_rows(&lines).is_err());
    }
}
