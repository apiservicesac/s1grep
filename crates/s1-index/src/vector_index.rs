/// What the vector index keeps about each unit besides its vector: enough to filter by folder and fetch the unit.
#[derive(Debug, Clone)]
pub struct VectorRow {
    pub unit_id: i64,
    pub path: String,
    /// Whether the vector comes from the whole source (otherwise from the outline).
    pub whole: bool,
}

/// The vectors of one project in one contiguous matrix, searched by brute force. At tens of thousands of functions
/// a full pass takes milliseconds (ADR-0003); only the rows and similarities of the best matches leave it.
pub struct VectorIndex {
    dimension: usize,
    rows: Vec<VectorRow>,
    values: Vec<f32>,
}

impl VectorIndex {
    pub fn new(entries: Vec<(VectorRow, Vec<f32>)>) -> Self {
        let dimension = entries.first().map_or(0, |(_, vector)| vector.len());
        let mut rows = Vec::with_capacity(entries.len());
        let mut values = Vec::with_capacity(entries.len() * dimension);
        for (row, vector) in entries {
            if vector.len() == dimension {
                rows.push(row);
                values.extend(vector);
            }
        }
        Self {
            dimension,
            rows,
            values,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, position: usize) -> &VectorRow {
        &self.rows[position]
    }

    pub fn rows(&self) -> &[VectorRow] {
        &self.rows
    }

    /// Positions of the `limit` rows most similar to `query` among those `keep` accepts, best first. Vectors are unit
    /// length, so similarity is a dot product.
    pub fn nearest(&self, query: &[f32], limit: usize, keep: impl Fn(&VectorRow) -> bool) -> Vec<(usize, f32)> {
        if query.len() != self.dimension || limit == 0 {
            return Vec::new();
        }
        let mut scored: Vec<(usize, f32)> = self
            .values
            .chunks_exact(self.dimension)
            .enumerate()
            .filter(|(position, _)| keep(&self.rows[*position]))
            .map(|(position, vector)| {
                (
                    position,
                    vector.iter().zip(query).map(|(left, right)| left * right).sum(),
                )
            })
            .collect();
        let best_first =
            |left: &(usize, f32), right: &(usize, f32)| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0));
        if scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, best_first);
            scored.truncate(limit);
        }
        scored.sort_by(best_first);
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::{VectorIndex, VectorRow};

    fn row(unit_id: i64, path: &str) -> VectorRow {
        VectorRow {
            unit_id,
            path: path.to_string(),
            whole: true,
        }
    }

    #[test]
    fn returns_the_nearest_rows_that_pass_the_filter_best_first() {
        let index = VectorIndex::new(vec![
            (row(1, "a.py"), vec![1.0, 0.0]),
            (row(2, "b.py"), vec![0.6, 0.8]),
            (row(3, "c.py"), vec![0.0, 1.0]),
            (row(4, "skip.py"), vec![1.0, 0.0]),
        ]);
        let nearest = index.nearest(&[1.0, 0.0], 2, |row| row.path != "skip.py");
        let ids: Vec<i64> = nearest
            .iter()
            .map(|(position, _)| index.row(*position).unit_id)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }
}
