use crate::models::LatencyBin;

const LATENCY_SERIES_LIMIT: usize = 600;

pub fn latency_series(values: &[f64]) -> Vec<f64> {
    let start = values.len().saturating_sub(LATENCY_SERIES_LIMIT);
    values[start..].to_vec()
}

pub fn latency_bins(values: &[f64]) -> Vec<LatencyBin> {
    if values.is_empty() {
        return Vec::new();
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p5 = percentile_sorted(&sorted, 5.0);
    let p95 = percentile_sorted(&sorted, 95.0);
    let mut bin_width = (p95 - p5) / 6.0;
    if !bin_width.is_finite() || bin_width < 0.001 {
        bin_width = 0.001;
    }

    let mut ranges = Vec::<(String, f64, Option<f64>)>::new();
    ranges.push((format!("<{p5:.3}"), 0.0, Some(p5)));
    for index in 0..6 {
        let min_ms = p5 + index as f64 * bin_width;
        let max_ms = p5 + (index + 1) as f64 * bin_width;
        ranges.push((format!("{min_ms:.3}-{max_ms:.3}"), min_ms, Some(max_ms)));
    }
    ranges.push((format!(">{p95:.3}"), p95, None));

    ranges
        .into_iter()
        .map(|(label, min_ms, max_ms)| {
            let count = values
                .iter()
                .filter(|value| {
                    **value >= min_ms && max_ms.map(|max| **value < max).unwrap_or(true)
                })
                .count();
            LatencyBin {
                label,
                min_ms,
                max_ms,
                count,
                percent: 100.0 * count as f64 / values.len() as f64,
            }
        })
        .collect()
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::{latency_bins, latency_series};

    #[test]
    fn latency_series_keeps_recent_values() {
        let values = (0..650).map(f64::from).collect::<Vec<_>>();

        let series = latency_series(&values);

        assert_eq!(series.len(), 600);
        assert_eq!(series.first().copied(), Some(50.0));
        assert_eq!(series.last().copied(), Some(649.0));
    }

    #[test]
    fn latency_bins_keep_existing_range_count() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];

        let bins = latency_bins(&values);

        assert_eq!(bins.len(), 8);
        assert_eq!(bins.first().map(|bin| bin.label.as_str()), Some("<1.000"));
        assert_eq!(bins.last().map(|bin| bin.label.as_str()), Some(">5.000"));
    }
}
