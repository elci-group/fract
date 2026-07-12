//! Structural-entropy scoring: blends module size, cyclomatic complexity,
//! cohesion loss, dependency density, public surface, and duplication
//! into one [0, 1] score, each sub-score soft-clamped through a sigmoid.

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

    #[test]
    fn zero_line_module_scores_zero_surface_and_duplication() {
        let m = sample(0, 0, 0);
        let e = entropy(&m);
        // Only the always-0.5 sigmoid floors contribute: 0.3*0.5 + 0.2*0.5 +
        // 0.1*0.5 = 0.3; the zero-guard branches contribute nothing.
        assert!((e - 0.3).abs() < 1e-9, "entropy was {e}");
    }

    // --- Entropy property/invariant suites (hand-rolled, deterministic LCG) ---

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn usize_below(&mut self, n: u64) -> usize {
            usize::try_from(self.below(n)).unwrap()
        }
    }

    /// Module with fields randomized over plausible ranges.
    fn random_module(rng: &mut Rng) -> Module {
        let mut m = sample(
            1 + rng.usize_below(5000),
            rng.usize_below(101),
            rng.usize_below(301),
        );
        m.public_api_size = rng.usize_below(501);
        m.fan_out = rng.usize_below(51);
        m.fan_in = rng.usize_below(51);
        m.duplicates = rng.usize_below(201);
        m
    }

    #[test]
    fn prop_entropy_always_within_unit_interval() {
        let mut rng = Rng(0x243f_6a88_85a3_08d3);
        for _ in 0..200 {
            let m = random_module(&mut rng);
            let e = entropy(&m);
            assert!(
                (0.0..=1.0).contains(&e),
                "entropy {e} outside [0, 1] for module {m:?}"
            );
        }
    }

    #[test]
    fn prop_entropy_monotone_in_non_density_fields() {
        // Each of these fields feeds exactly one sub-score that is
        // non-decreasing in the field (all other sub-scores are independent
        // of it), so bumping one field at a time must never lower entropy.
        let mut rng = Rng(0x1310_98a2_df07_3432);
        for _ in 0..100 {
            let base = random_module(&mut rng);
            let before = entropy(&base);
            let bumps = [
                ("cyclomatic_complexity", {
                    let mut m = base.clone();
                    m.cyclomatic_complexity += 1 + rng.usize_below(50);
                    m
                }),
                ("fan_out", {
                    let mut m = base.clone();
                    m.fan_out += 1 + rng.usize_below(20);
                    m
                }),
                ("fan_in", {
                    let mut m = base.clone();
                    m.fan_in += 1 + rng.usize_below(20);
                    m
                }),
                ("duplicates", {
                    let mut m = base.clone();
                    m.duplicates += 1 + rng.usize_below(50);
                    m
                }),
                ("public_api_size", {
                    let mut m = base.clone();
                    m.public_api_size += 1 + rng.usize_below(50);
                    m
                }),
            ];
            for (field, bumped) in bumps {
                let after = entropy(&bumped);
                assert!(
                    after >= before,
                    "raising {field} lowered entropy {before} -> {after} for {bumped:?}"
                );
            }
        }
    }

    #[test]
    fn prop_entropy_lines_monotone_when_density_fields_zero() {
        // FINDING: entropy is NOT globally monotone in `lines` (see
        // `entropy_can_decrease_when_lines_dilute_densities`): public_surface
        // and duplication are density scores (count / lines), so growing
        // `lines` with a fixed public API or duplicate count dilutes them.
        // With both density fields at zero those sub-scores are constant in
        // `lines`, and the remaining size/cohesion sub-scores are
        // non-decreasing — so monotonicity holds in this restricted class.
        let mut rng = Rng(0xfeed_face_1234_5678);
        for _ in 0..100 {
            let mut m = random_module(&mut rng);
            m.public_api_size = 0;
            m.duplicates = 0;
            let before = entropy(&m);
            m.lines += 1 + rng.usize_below(1000);
            let after = entropy(&m);
            assert!(
                after >= before,
                "lines-only growth lowered entropy {before} -> {after} for {m:?}"
            );
        }
    }

    #[test]
    fn entropy_can_decrease_when_lines_dilute_densities() {
        // Concrete witness for the documented non-monotonicity in `lines`:
        // the size sub-score is nearly saturated at 2,500 lines, so doubling
        // `lines` gains almost nothing there while the public-surface and
        // duplication densities halve, dragging the weighted sum down.
        let mut m = sample(2500, 0, 0);
        m.public_api_size = 125;
        m.duplicates = 125;
        let dense = entropy(&m);
        m.lines = 5000;
        let diluted = entropy(&m);
        assert!(
            diluted < dense,
            "expected dilution to lower entropy, got {dense} -> {diluted}"
        );
    }
}
