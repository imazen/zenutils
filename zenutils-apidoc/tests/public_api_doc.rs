//! Dogfood: snapshot this workspace's own public API surfaces with the
//! library under test. Generates `docs/public-api/{zenutils-fuzz,
//! zenutils-apidoc}.txt` at the workspace root.

#[test]
fn public_api_surface_docs_are_current() {
    zenutils_apidoc::run();
}
