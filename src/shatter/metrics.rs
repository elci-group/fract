//! Metrics collection and reporting for shatter transformations.
//!
//! Tracks success rates, performance, confidence scores, and outcomes
//! to enable data-driven refinement of transformation strategies.

use std::time::Instant;
use std::collections::HashMap;

/// A single transformation metric event.
#[derive(Debug, Clone)]
pub struct MetricEvent {
    /// Candidate function name
    pub function_name: String,
    /// Fract confidence score (0.0-1.0)
    pub confidence: f64,
    /// Whether transformation succeeded
    pub succeeded: bool,
    /// Time taken (milliseconds)
    pub duration_ms: u128,
    /// Error message if failed
    pub error: Option<String>,
}

impl MetricEvent {
    pub fn new(
        function_name: String,
        confidence: f64,
        succeeded: bool,
        duration_ms: u128,
    ) -> Self {
        Self {
            function_name,
            confidence,
            succeeded,
            duration_ms,
            error: None,
        }
    }

    pub fn with_error(mut self, error: String) -> Self {
        self.error = Some(error);
        self
    }
}

/// Aggregated metrics for a batch of transformations.
#[derive(Debug, Clone)]
pub struct BatchMetrics {
    pub events: Vec<MetricEvent>,
    pub total_duration_ms: u128,
    pub start_time: Option<Instant>,
}

impl BatchMetrics {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            total_duration_ms: 0,
            start_time: None,
        }
    }

    pub fn start(&mut self) {
        self.start_time = Some(Instant::now());
    }

    pub fn finish(&mut self) {
        if let Some(start) = self.start_time {
            self.total_duration_ms = start.elapsed().as_millis();
        }
    }

    pub fn add_event(&mut self, event: MetricEvent) {
        self.events.push(event);
    }

    pub fn success_rate(&self) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }
        let successes = self.events.iter().filter(|e| e.succeeded).count();
        successes as f64 / self.events.len() as f64
    }

    pub fn average_confidence(&self) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.events.iter().map(|e| e.confidence).sum();
        sum / self.events.len() as f64
    }

    pub fn average_duration_ms(&self) -> f64 {
        if self.events.is_empty() {
            return 0.0;
        }
        let sum: u128 = self.events.iter().map(|e| e.duration_ms).sum();
        sum as f64 / self.events.len() as f64
    }

    pub fn confidence_vs_success(&self) -> ConfidenceAnalysis {
        let mut high_confidence_success = 0;
        let mut high_confidence_total = 0;
        let mut low_confidence_success = 0;
        let mut low_confidence_total = 0;

        let threshold = 0.75;

        for event in &self.events {
            if event.confidence >= threshold {
                high_confidence_total += 1;
                if event.succeeded {
                    high_confidence_success += 1;
                }
            } else {
                low_confidence_total += 1;
                if event.succeeded {
                    low_confidence_success += 1;
                }
            }
        }

        ConfidenceAnalysis {
            high_confidence_success_rate: if high_confidence_total > 0 {
                high_confidence_success as f64 / high_confidence_total as f64
            } else {
                0.0
            },
            low_confidence_success_rate: if low_confidence_total > 0 {
                low_confidence_success as f64 / low_confidence_total as f64
            } else {
                0.0
            },
            high_confidence_count: high_confidence_total,
            low_confidence_count: low_confidence_total,
        }
    }

    pub fn failure_reasons(&self) -> HashMap<String, usize> {
        let mut reasons: HashMap<String, usize> = HashMap::new();
        for event in &self.events {
            if !event.succeeded {
                if let Some(error) = &event.error {
                    *reasons.entry(error.clone()).or_insert(0) += 1;
                } else {
                    *reasons.entry("unknown".to_string()).or_insert(0) += 1;
                }
            }
        }
        reasons
    }
}

impl Default for BatchMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Analysis of confidence vs success correlation.
#[derive(Debug, Clone)]
pub struct ConfidenceAnalysis {
    pub high_confidence_success_rate: f64,
    pub low_confidence_success_rate: f64,
    pub high_confidence_count: usize,
    pub low_confidence_count: usize,
}

impl ConfidenceAnalysis {
    pub fn is_predictive(&self) -> bool {
        // If high confidence has significantly better success rate
        (self.high_confidence_success_rate - self.low_confidence_success_rate).abs() > 0.15
    }
}

/// Detailed transformation report with metrics.
#[derive(Debug, Clone)]
pub struct TransformationReport {
    pub batch_metrics: BatchMetrics,
    pub total_candidates: usize,
    pub successful_candidates: usize,
    pub failed_candidates: usize,
    pub avg_confidence_attempted: f64,
    pub confidence_analysis: ConfidenceAnalysis,
}

impl TransformationReport {
    pub fn new(metrics: BatchMetrics) -> Self {
        let successful = metrics.events.iter().filter(|e| e.succeeded).count();
        let failed = metrics.events.len() - successful;
        let avg_confidence = metrics.average_confidence();
        let confidence_analysis = metrics.confidence_vs_success();

        Self {
            batch_metrics: metrics,
            total_candidates: successful + failed,
            successful_candidates: successful,
            failed_candidates: failed,
            avg_confidence_attempted: avg_confidence,
            confidence_analysis,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "Transformations: {}/{} succeeded | Avg confidence: {:.2} | Duration: {}ms",
            self.successful_candidates,
            self.total_candidates,
            self.avg_confidence_attempted,
            self.batch_metrics.total_duration_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_event_creation() {
        let event = MetricEvent::new("foo".to_string(), 0.95, true, 100);
        assert_eq!(event.function_name, "foo");
        assert_eq!(event.confidence, 0.95);
        assert!(event.succeeded);
        assert_eq!(event.duration_ms, 100);
    }

    #[test]
    fn batch_metrics_success_rate() {
        let mut metrics = BatchMetrics::new();
        metrics.add_event(MetricEvent::new("foo".to_string(), 0.9, true, 100));
        metrics.add_event(MetricEvent::new("bar".to_string(), 0.8, false, 150));
        metrics.add_event(MetricEvent::new("baz".to_string(), 0.85, true, 120));

        assert_eq!(metrics.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn batch_metrics_average_confidence() {
        let mut metrics = BatchMetrics::new();
        metrics.add_event(MetricEvent::new("foo".to_string(), 0.9, true, 100));
        metrics.add_event(MetricEvent::new("bar".to_string(), 0.8, true, 100));
        metrics.add_event(MetricEvent::new("baz".to_string(), 0.7, true, 100));

        assert!((metrics.average_confidence() - 0.8).abs() < 0.01);
    }

    #[test]
    fn confidence_analysis_predictive() {
        let mut metrics = BatchMetrics::new();
        // High confidence: all succeed
        for i in 0..5 {
            metrics.add_event(MetricEvent::new(
                format!("high_{}", i),
                0.95,
                true,
                100,
            ));
        }
        // Low confidence: all fail
        for i in 0..5 {
            metrics.add_event(MetricEvent::new(
                format!("low_{}", i),
                0.2,
                false,
                100,
            ));
        }

        let analysis = metrics.confidence_vs_success();
        assert!(analysis.is_predictive());
    }
}
