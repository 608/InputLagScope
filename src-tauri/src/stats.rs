use crate::models::Summary;

pub fn summarize(values: &[f64]) -> Summary {
    if values.is_empty() {
        return Summary::default();
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);

    let count = sorted.len();
    let raw_average = average(&sorted);
    let jitter = if count >= 2 {
        let variance = sorted
            .iter()
            .map(|value| {
                let delta = value - raw_average;
                delta * delta
            })
            .sum::<f64>()
            / (count as f64 - 1.0);
        variance.sqrt()
    } else {
        0.0
    };
    let p05 = percentile_sorted(&sorted, 0.05);
    let p95 = percentile_sorted(&sorted, 0.95);
    let trimmed = percentile_range_values(&sorted, p05, p95);
    let summary_values = if trimmed.is_empty() {
        sorted.as_slice()
    } else {
        trimmed.as_slice()
    };

    Summary {
        count,
        average_ms: Some(average(summary_values)),
        jitter_ms: Some(jitter),
        min_ms: sorted.first().copied(),
        p05_ms: Some(p05),
        median_ms: Some(percentile_sorted(summary_values, 0.5)),
        p95_ms: Some(p95),
        max_ms: sorted.last().copied(),
    }
}

fn average(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile_range_values(sorted: &[f64], low: f64, high: f64) -> Vec<f64> {
    sorted
        .iter()
        .copied()
        .filter(|value| *value >= low && *value <= high)
        .collect()
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }

    let index = (sorted.len() as f64 - 1.0) * p;
    let lo = index.floor() as usize;
    let hi = index.ceil() as usize;
    if lo == hi {
        return sorted[lo];
    }

    let ratio = index - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * ratio
}

#[cfg(test)]
mod tests {
    use super::summarize;

    #[test]
    fn average_and_median_use_p05_to_p95_values() {
        let mut values: Vec<f64> = (1..=100).map(f64::from).collect();
        values.push(1000.0);

        let summary = summarize(&values);

        assert_eq!(summary.min_ms, Some(1.0));
        assert_eq!(summary.p05_ms, Some(6.0));
        assert_eq!(summary.average_ms, Some(51.0));
        assert_eq!(summary.median_ms, Some(51.0));
        assert_eq!(summary.p95_ms, Some(96.0));
        assert_eq!(summary.max_ms, Some(1000.0));
    }
}
