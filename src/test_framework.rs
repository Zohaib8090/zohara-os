// src/test_framework.rs

//! Kernel test framework — structured test execution and reporting.
//!
//! Tests are registered as named functions. The framework runs them
//! sequentially and reports PASS/FAIL for each.
//!
//! Future: cargo test-kernel, run.sh --test <suite>

/// A single test case.
#[derive(Copy, Clone)]
pub struct TestCase {
    pub name: &'static str,
    pub func: fn(),
}

/// Maximum number of registered tests.
const MAX_TESTS: usize = 64;

static mut TESTS: [Option<TestCase>; MAX_TESTS] = [None; MAX_TESTS];
static mut TEST_COUNT: usize = 0;

/// Register a test case.
pub fn register_test(name: &'static str, func: fn()) {
    unsafe {
        if TEST_COUNT < MAX_TESTS {
            TESTS[TEST_COUNT] = Some(TestCase { name, func });
            TEST_COUNT += 1;
        }
    }
}

/// Run all registered tests and report results.
pub fn run_all() {
    unsafe {
        let count = TEST_COUNT;
        crate::println!("=== Kernel Test Suite ({} tests) ===", count);
        let mut passed = 0;
        let mut failed = 0;
        for i in 0..count {
            if let Some(ref test) = TESTS[i] {
                crate::print!("  [{}] ... ", test.name);
                // Run the test — if it panics, the panic handler catches it.
                // For a proper framework, we'd set up a recovery mechanism,
                // but for now we just run and trust the test.
                (test.func)();
                crate::println!("PASS");
                passed += 1;
            }
        }
        crate::println!("=== Results: {} passed, {} failed ===", passed, failed);
    }
}

/// Run a specific test by name.
pub fn run_by_name(name: &str) -> bool {
    unsafe {
        for i in 0..TEST_COUNT {
            if let Some(ref test) = TESTS[i] {
                if test.name == name {
                    crate::print!("  [{}] ... ", test.name);
                    (test.func)();
                    crate::println!("PASS");
                    return true;
                }
            }
        }
    }
    crate::println!("  Test '{}' not found", name);
    false
}
