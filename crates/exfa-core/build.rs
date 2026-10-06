// The compiled SDE lives in this crate: static tables plus the generated effect code
// (see crates/exfa-codegen).
fn main() {
    exfa_codegen::run_tables();
    exfa_codegen::run_effects();
}
