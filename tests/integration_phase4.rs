//! Phase 4 integration tests for advanced transformations and AI-guided optimization.

use std::path::PathBuf;

#[test]
fn test_metrics_event_creation() {
    use fract::shatter::metrics::MetricEvent;

    let event = MetricEvent::new("foo".to_string(), 0.95, true, 100);
    assert_eq!(event.function_name, "foo");
    assert_eq!(event.confidence, 0.95);
    assert!(event.succeeded);
    assert_eq!(event.duration_ms, 100);
}

#[test]
fn test_metrics_batch_success_rate() {
    use fract::shatter::metrics::{MetricEvent, BatchMetrics};

    let mut metrics = BatchMetrics::new();
    metrics.add_event(MetricEvent::new("foo".to_string(), 0.9, true, 100));
    metrics.add_event(MetricEvent::new("bar".to_string(), 0.8, false, 150));
    metrics.add_event(MetricEvent::new("baz".to_string(), 0.85, true, 120));

    assert_eq!(metrics.success_rate(), 2.0 / 3.0);
    assert!(metrics.average_duration_ms() > 0.0);
}

#[test]
fn test_metrics_transformation_report() {
    use fract::shatter::metrics::{MetricEvent, BatchMetrics, TransformationReport};

    let mut metrics = BatchMetrics::new();
    metrics.add_event(MetricEvent::new("foo".to_string(), 0.95, true, 100));
    metrics.add_event(MetricEvent::new("bar".to_string(), 0.60, false, 150));

    let report = TransformationReport::new(metrics);
    assert_eq!(report.successful_candidates, 1);
    assert_eq!(report.failed_candidates, 1);
    let summary = report.summary();
    assert!(summary.contains("1/2"));
}

#[test]
fn test_metrics_confidence_analysis() {
    use fract::shatter::metrics::{MetricEvent, BatchMetrics};

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
    assert!(analysis.high_confidence_success_rate > 0.9);
    assert!(analysis.low_confidence_success_rate < 0.1);
}

#[test]
fn test_confidence_optimizer_sorts_by_strategy() {
    use fract::shatter::confidence_optimizer::{ConfidenceOptimizer, OrderingStrategy};
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.5,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "bar".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "baz".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.75,
        ),
    ];

    let mut optimizer = ConfidenceOptimizer::new(candidates, OrderingStrategy::HighestConfidenceFirst);
    let ordered = optimizer.optimize().unwrap();

    assert_eq!(ordered[0].confidence, 0.95);
    assert_eq!(ordered[1].confidence, 0.75);
    assert_eq!(ordered[2].confidence, 0.5);
}

#[test]
fn test_confidence_optimizer_batches_by_level() {
    use fract::shatter::confidence_optimizer::ConfidenceOptimizer;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "high".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "medium".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.60,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "low".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.30,
        ),
    ];

    let optimizer = ConfidenceOptimizer::new(candidates, fract::shatter::confidence_optimizer::OrderingStrategy::Clustered);
    let batches = optimizer.batch_by_confidence();

    assert_eq!(batches.high_count(), 1);
    assert_eq!(batches.medium_count(), 1);
    assert_eq!(batches.low_count(), 1);
}

#[test]
fn test_confidence_optimizer_filters_by_threshold() {
    use fract::shatter::confidence_optimizer::ConfidenceOptimizer;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "high".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "low".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.60,
        ),
    ];

    let optimizer = ConfidenceOptimizer::new(
        candidates,
        fract::shatter::confidence_optimizer::OrderingStrategy::HighestConfidenceFirst,
    );
    let filtered = optimizer.filter_by_confidence(0.75);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].confidence, 0.95);
}

#[test]
fn test_batch_processor_detects_conflicts() {
    use fract::shatter::batch_processor::BatchProcessor;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::other".to_string(),
            PathBuf::from("src/other.rs"),
            0.90,
        ),
    ];

    let processor = BatchProcessor::new(candidates);
    let batch = processor.analyze_conflicts().unwrap();

    assert_eq!(batch.conflict_count(), 1);
}

#[test]
fn test_batch_processor_accepts_compatible_candidates() {
    use fract::shatter::batch_processor::BatchProcessor;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "bar".to_string(),
            "crate::other".to_string(),
            PathBuf::from("src/other.rs"),
            0.90,
        ),
    ];

    let processor = BatchProcessor::new(candidates);
    let batch = processor.analyze_conflicts().unwrap();

    assert_eq!(batch.conflict_count(), 0);
    assert!(processor.can_process_safely(&batch));
}

#[test]
fn test_batch_processor_splits_conflicting_batches() {
    use fract::shatter::batch_processor::BatchProcessor;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::other".to_string(),
            PathBuf::from("src/other.rs"),
            0.90,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "bar".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.85,
        ),
    ];

    let processor = BatchProcessor::new(candidates);
    let batches = processor.split_into_safe_batches().unwrap();

    assert!(batches.len() >= 1);
    assert!(batches.iter().any(|b| processor.can_process_safely(b)));
}

#[test]
fn test_advanced_multifunction_extraction_plan() {
    use fract::shatter::advanced_transforms::{
        AdvancedTransformAnalyzer, MultiFunctionConfig, TransformKind,
    };

    let config = MultiFunctionConfig {
        functions: vec!["foo".to_string(), "bar".to_string()],
        shared_types: vec!["SharedType".to_string()],
        create_helper_module: false,
    };

    let plan = AdvancedTransformAnalyzer::plan_multifunction_extraction(&config).unwrap();

    assert_eq!(plan.kind, TransformKind::MultiFunctionExtraction);
    assert!(!plan.potential_issues.is_empty());
}

#[test]
fn test_advanced_trait_extraction_plan() {
    use fract::shatter::advanced_transforms::{
        AdvancedTransformAnalyzer, TraitExtractionConfig, TransformKind,
    };

    let config = TraitExtractionConfig {
        functions: vec!["foo".to_string(), "bar".to_string()],
        trait_name: "MyTrait".to_string(),
        target_module: "crate::traits".to_string(),
    };

    let plan = AdvancedTransformAnalyzer::plan_trait_extraction(&config).unwrap();

    assert_eq!(plan.kind, TransformKind::TraitExtraction);
    assert!(plan.requires_manual_review);
}

#[test]
fn test_advanced_module_reorganization_plan() {
    use fract::shatter::advanced_transforms::{
        AdvancedTransformAnalyzer, ModuleReorganizationConfig,
    };

    let config = ModuleReorganizationConfig {
        files: vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/utils.rs")],
        new_structure: "modular".to_string(),
        maintain_compatibility: true,
    };

    let plan = AdvancedTransformAnalyzer::plan_module_reorganization(&config).unwrap();

    assert!(plan.requires_manual_review);
    assert!(!plan.potential_issues.is_empty());
}

#[test]
fn test_advanced_complexity_assessment() {
    use fract::shatter::advanced_transforms::{AdvancedTransformAnalyzer, AdvancedTransformPlan, TransformKind};

    let low = AdvancedTransformPlan::new(TransformKind::MultiFunctionExtraction, "test".to_string())
        .with_complexity(0.2);
    let high =
        AdvancedTransformPlan::new(TransformKind::ModuleReorganization, "test".to_string())
            .with_complexity(0.9);

    assert_eq!(AdvancedTransformAnalyzer::assess_complexity(&low), "Low");
    assert_eq!(AdvancedTransformAnalyzer::assess_complexity(&high), "Very High");
}

#[test]
fn test_batch_conflict_types() {
    use fract::shatter::batch_processor::{Conflict, ConflictKind};

    let conflict = Conflict {
        candidate_a: "foo".to_string(),
        candidate_b: "bar".to_string(),
        kind: ConflictKind::SameSourceFunction,
        message: "test".to_string(),
    };

    assert_eq!(conflict.kind, ConflictKind::SameSourceFunction);
}

#[test]
fn test_ranked_candidate_confidence_bands() {
    use fract::shatter::confidence_optimizer::RankedCandidate;
    use fract::shatter::CandidateFunction;

    let high = RankedCandidate::new(
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        0.95,
    );
    assert!(high.is_high_confidence());

    let medium = RankedCandidate::new(
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "bar".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.60,
        ),
        0.60,
    );
    assert!(medium.is_medium_confidence());

    let low = RankedCandidate::new(
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "baz".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.30,
        ),
        0.30,
    );
    assert!(low.is_low_confidence());
}

#[test]
fn test_metrics_failure_reasons() {
    use fract::shatter::metrics::{MetricEvent, BatchMetrics};

    let mut metrics = BatchMetrics::new();
    let mut e1 = MetricEvent::new("foo".to_string(), 0.5, false, 100);
    e1.error = Some("import error".to_string());
    metrics.add_event(e1);

    let mut e2 = MetricEvent::new("bar".to_string(), 0.3, false, 150);
    e2.error = Some("import error".to_string());
    metrics.add_event(e2);

    let reasons = metrics.failure_reasons();
    assert_eq!(reasons.get("import error"), Some(&2));
}

#[test]
fn test_batch_processor_dependency_order() {
    use fract::shatter::batch_processor::BatchProcessor;
    use fract::shatter::CandidateFunction;

    let candidates = vec![
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "foo".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.95,
        ),
        CandidateFunction::new(
            PathBuf::from("src/lib.rs"),
            "bar".to_string(),
            "crate::utils".to_string(),
            PathBuf::from("src/utils.rs"),
            0.90,
        ),
    ];

    let processor = BatchProcessor::new(candidates);
    let order = processor.dependency_order().unwrap();

    assert_eq!(order.len(), 2);
}
