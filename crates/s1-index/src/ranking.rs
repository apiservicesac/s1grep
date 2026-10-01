/// Nearest units to a query vector by cosine similarity (vectors are unit length, so a dot product).
pub struct VectorRanking;

impl VectorRanking {
    /// Indexes of the `limit` most similar vectors, best first.
    pub fn top(query: &[f32], vectors: &[&[f32]], limit: usize) -> Vec<(usize, f32)> {
        let mut scored: Vec<(usize, f32)> = vectors
            .iter()
            .enumerate()
            .map(|(index, vector)| (index, vector.iter().zip(query).map(|(left, right)| left * right).sum()))
            .collect();
        scored.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
        scored.truncate(limit);
        scored
    }
}

/// Reciprocal-rank fusion of the retriever's order and the judge's order, with the weights chosen on the dev exam.
#[derive(Debug, Clone, Copy)]
pub struct FusionWeights {
    /// Share of the judge's rank; the retriever gets the rest.
    pub judge: f64,
    pub smoothing: f64,
}

impl FusionWeights {
    /// Chosen on the dev split of the exam for s1-code v3 (docs/decisions.md), per retriever and judged count.
    pub fn tuned(retriever: &str, judged: usize) -> Self {
        let (judge, smoothing) = match (retriever, judged) {
            ("qwen3", ..=5) => (0.45, 5.0),
            ("qwen3", _) => (0.35, 30.0),
            (_, ..=5) => (0.55, 5.0),
            (_, ..=10) => (0.65, 1.0),
            _ => (0.75, 10.0),
        };
        Self { judge, smoothing }
    }
}

pub struct RankFusion;

impl RankFusion {
    /// New order of the retriever's candidates. `judge_scores[i]` scores candidate `i` (retriever order); only the
    /// first `judge_scores.len()` candidates were judged, the rest keep their retriever position.
    pub fn order(candidate_count: usize, judge_scores: &[f64], weights: FusionWeights) -> Vec<usize> {
        let mut judge_order: Vec<usize> = (0..judge_scores.len()).collect();
        judge_order.sort_by(|&left, &right| {
            judge_scores[right]
                .total_cmp(&judge_scores[left])
                .then(left.cmp(&right))
        });
        let mut judge_rank = vec![None; candidate_count];
        for (rank, &index) in judge_order.iter().enumerate() {
            judge_rank[index] = Some(rank + 1);
        }
        let fused: Vec<f64> = (0..candidate_count)
            .map(|index| {
                let judge = judge_rank[index].map_or(0.0, |rank| weights.judge / (weights.smoothing + rank as f64));
                judge + (1.0 - weights.judge) / (weights.smoothing + index as f64 + 1.0)
            })
            .collect();
        let mut order: Vec<usize> = (0..candidate_count).collect();
        order.sort_by(|&left, &right| fused[right].total_cmp(&fused[left]).then(left.cmp(&right)));
        order
    }
}

#[cfg(test)]
mod tests {
    use super::{FusionWeights, RankFusion};

    #[test]
    fn a_confident_judge_moves_its_choice_to_the_top() {
        let order = RankFusion::order(
            4,
            &[0.1, 0.2, 0.9],
            FusionWeights {
                judge: 0.6,
                smoothing: 1.0,
            },
        );
        assert_eq!(order[0], 2);
        assert_eq!(order[3], 3);
    }
}
