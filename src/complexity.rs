use crate::Module;

/// Compute structural entropy for a module.
///
/// Entropy =
///   0.3 * `module_size_score`
/// + 0.2 * `cyclomatic_score`
/// + 0.2 * `cohesion_loss_score`
/// + 0.1 * `dependency_density_score`
/// + 0.1 * `public_surface_score`
/// + 0.1 * `duplication_score`
///
/// Each sub-score is normalised to [0, 1] with soft clamping.
#[must_use]
pub fn entropy(module: &Module) -> f64 {
    let module_size = score_module_size(module.lines);
    let cyclomatic = score_cyclomatic(module.cyclomatic_complexity);
    let cohesion_loss = score_cohesion_loss(module.lines, module.functions);
    let dependency_density = score_dependency_density(module.fan_out, module.fan_in);
    let public_surface = score_public_surface(module.public_api_size, module.lines);
    let duplication = score_duplication(module.duplicates, module.lines);

    0.30 * module_size
        + 0.20 * cyclomatic
        + 0.20 * cohesion_loss
        + 0.10 * dependency_density
        + 0.10 * public_surface
        + 0.10 * duplication
}

fn score_module_size(lines: usize) -> f64 {
    // 3,000 lines → ~1.0
    sigmoid(lines as f64 / 500.0)
}

fn score_cyclomatic(cc: usize) -> f64 {
    // 100 branches → ~1.0
    sigmoid(cc as f64 / 17.0)
}

fn score_cohesion_loss(lines: usize, functions: usize) -> f64 {
    if functions == 0 {
        return 0.0;
    }
    let avg = lines as f64 / functions as f64;
    // Average function size > 80 suggests poor cohesion.
    sigmoid((avg - 30.0) / 20.0)
}

fn score_dependency_density(fan_out: usize, fan_in: usize) -> f64 {
    // High fan-out plus some fan-in indicates dense coupling.
    sigmoid((fan_out as f64 + fan_in as f64 * 0.5) / 10.0)
}

fn score_public_surface(public: usize, lines: usize) -> f64 {
    if lines == 0 {
        return 0.0;
    }
    let density = public as f64 / lines as f64;
    // 20% public is already a lot.
    sigmoid((density - 0.05) * 40.0)
}

fn score_duplication(duplicates: usize, lines: usize) -> f64 {
    if lines == 0 {
        return 0.0;
    }
    let density = duplicates as f64 / lines as f64;
    sigmoid(density * 20.0)
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now;
    use crate::{Health, Language};
    use std::path::PathBuf;

    fn sample(lines: usize, functions: usize, cc: usize) -> Module {
        Module {
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            lines,
            functions,
            cyclomatic_complexity: cc,
            public_api_size: 0,
            fan_out: 0,
            fan_in: 0,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: None,
            churn: 0,
            test_coverage: 0.0,
            entropy: 0.0,
            health: Health::Healthy,
            last_modified: now(),
        }
    }

    #[test]
    fn small_module_is_healthy() {
        let m = sample(50, 8, 4);
        assert!(entropy(&m) < 0.45, "entropy was {}", entropy(&m));
    }

    #[test]
    fn god_object_is_critical() {
        let mut m = sample(3000, 65, 250);
        m.public_api_size = 600;
        m.duplicates = 150;
        m.fan_out = 30;
        m.fan_in = 20;
        assert!(entropy(&m) > 0.80, "entropy was {}", entropy(&m));
    }
}
