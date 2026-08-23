//! Advanced transformation capabilities beyond basic function extraction.
//!
//! Supports multi-function extraction, trait extraction, module reorganization,
//! and other complex refactoring scenarios.

use std::path::PathBuf;
use crate::error::Result;

/// Type of advanced transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformKind {
    /// Extract multiple related functions
    MultiFunctionExtraction,
    /// Extract trait definition and implementation
    TraitExtraction,
    /// Reorganize module structure
    ModuleReorganization,
    /// Extract interface/contract from multiple functions
    InterfaceExtraction,
    /// Consolidate duplicated logic
    DeduplicationRefactor,
}

/// Configuration for multi-function extraction.
#[derive(Debug, Clone)]
pub struct MultiFunctionConfig {
    /// Functions to extract together
    pub functions: Vec<String>,
    /// Shared dependencies between functions
    pub shared_types: Vec<String>,
    /// Whether to create common helper module
    pub create_helper_module: bool,
}

/// Configuration for trait extraction.
#[derive(Debug, Clone)]
pub struct TraitExtractionConfig {
    /// Functions implementing the trait
    pub functions: Vec<String>,
    /// Trait name to create
    pub trait_name: String,
    /// Target module for trait definition
    pub target_module: String,
}

/// Configuration for module reorganization.
#[derive(Debug, Clone)]
pub struct ModuleReorganizationConfig {
    /// Files to reorganize
    pub files: Vec<PathBuf>,
    /// New module structure
    pub new_structure: String,
    /// Whether to keep backward compatibility
    pub maintain_compatibility: bool,
}

/// Advanced transformation plan.
#[derive(Debug, Clone)]
pub struct AdvancedTransformPlan {
    pub kind: TransformKind,
    pub description: String,
    pub estimated_complexity: f64,  // 0.0-1.0, higher = more complex
    pub requires_manual_review: bool,
    pub potential_issues: Vec<String>,
}

impl AdvancedTransformPlan {
    pub fn new(kind: TransformKind, description: String) -> Self {
        Self {
            kind,
            description,
            estimated_complexity: 0.5,
            requires_manual_review: false,
            potential_issues: Vec::new(),
        }
    }

    pub fn with_complexity(mut self, complexity: f64) -> Self {
        self.estimated_complexity = complexity.max(0.0).min(1.0);
        self
    }

    pub fn with_review_required(mut self) -> Self {
        self.requires_manual_review = true;
        self
    }

    pub fn add_issue(mut self, issue: String) -> Self {
        self.potential_issues.push(issue);
        self
    }
}

/// Analyzer for advanced transformation opportunities.
pub struct AdvancedTransformAnalyzer;

impl AdvancedTransformAnalyzer {
    /// Analyze code for multi-function extraction opportunities.
    pub fn find_multifunction_extraction_opportunities(
        _file_content: &str,
    ) -> Result<Vec<MultiFunctionConfig>> {
        // In real implementation, would use AST analysis
        // For now, return empty - would need full syn parsing
        Ok(Vec::new())
    }

    /// Analyze code for trait extraction opportunities.
    pub fn find_trait_extraction_opportunities(
        _file_content: &str,
    ) -> Result<Vec<TraitExtractionConfig>> {
        // In real implementation, would use AST analysis
        Ok(Vec::new())
    }

    /// Create plan for multi-function extraction.
    pub fn plan_multifunction_extraction(
        config: &MultiFunctionConfig,
    ) -> Result<AdvancedTransformPlan> {
        let description = format!(
            "Extract {} functions together with shared types: {:?}",
            config.functions.len(),
            config.shared_types
        );

        let mut plan = AdvancedTransformPlan::new(
            TransformKind::MultiFunctionExtraction,
            description,
        ).with_complexity(0.7);

        // Multi-function extractions are more complex
        plan.potential_issues.push(
            "Ensure all interdependencies between functions are preserved".to_string(),
        );
        plan.potential_issues.push(
            "Verify shared type visibility in new module".to_string(),
        );

        if config.create_helper_module {
            plan = plan.with_review_required();
        }

        Ok(plan)
    }

    /// Create plan for trait extraction.
    pub fn plan_trait_extraction(
        config: &TraitExtractionConfig,
    ) -> Result<AdvancedTransformPlan> {
        let description = format!(
            "Extract trait '{}' from {} implementing functions",
            config.trait_name,
            config.functions.len()
        );

        let mut plan = AdvancedTransformPlan::new(
            TransformKind::TraitExtraction,
            description,
        ).with_complexity(0.8)
         .with_review_required();

        plan.potential_issues.push(
            "Trait bounds and associated types must be carefully analyzed".to_string(),
        );
        plan.potential_issues.push(
            "Existing trait impls may need adjustment".to_string(),
        );

        Ok(plan)
    }

    /// Create plan for module reorganization.
    pub fn plan_module_reorganization(
        config: &ModuleReorganizationConfig,
    ) -> Result<AdvancedTransformPlan> {
        let description = format!(
            "Reorganize {} files to structure: {}",
            config.files.len(),
            config.new_structure
        );

        let mut plan = AdvancedTransformPlan::new(
            TransformKind::ModuleReorganization,
            description,
        ).with_complexity(0.9)
         .with_review_required();

        plan.potential_issues.push("Public API changes may break downstream consumers".to_string());
        plan.potential_issues.push("Re-exports needed for backward compatibility".to_string());

        if !config.maintain_compatibility {
            plan.potential_issues.push("No backward compatibility maintained".to_string());
        }

        Ok(plan)
    }

    /// Assess complexity of advanced transformation.
    pub fn assess_complexity(plan: &AdvancedTransformPlan) -> String {
        match plan.estimated_complexity {
            c if c < 0.3 => "Low".to_string(),
            c if c < 0.6 => "Medium".to_string(),
            c if c < 0.8 => "High".to_string(),
            _ => "Very High".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multifunction_config_creation() {
        let config = MultiFunctionConfig {
            functions: vec!["foo".to_string(), "bar".to_string()],
            shared_types: vec!["SharedType".to_string()],
            create_helper_module: true,
        };

        assert_eq!(config.functions.len(), 2);
        assert!(config.create_helper_module);
    }

    #[test]
    fn advanced_transform_plan_creation() {
        let plan = AdvancedTransformPlan::new(
            TransformKind::MultiFunctionExtraction,
            "Extract two functions".to_string(),
        ).with_complexity(0.75)
         .with_review_required()
         .add_issue("Check dependencies".to_string());

        assert_eq!(plan.kind, TransformKind::MultiFunctionExtraction);
        assert!(plan.requires_manual_review);
        assert_eq!(plan.potential_issues.len(), 1);
        assert!(plan.estimated_complexity > 0.7);
    }

    #[test]
    fn multifunction_extraction_planning() {
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
    fn trait_extraction_planning() {
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
    fn complexity_assessment() {
        let low = AdvancedTransformPlan::new(
            TransformKind::MultiFunctionExtraction,
            "test".to_string(),
        ).with_complexity(0.2);

        let high = AdvancedTransformPlan::new(
            TransformKind::ModuleReorganization,
            "test".to_string(),
        ).with_complexity(0.9);

        assert_eq!(AdvancedTransformAnalyzer::assess_complexity(&low), "Low");
        assert_eq!(AdvancedTransformAnalyzer::assess_complexity(&high), "Very High");
    }
}
