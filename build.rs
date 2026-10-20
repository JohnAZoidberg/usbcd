// Compiles the window's bundled icons into a GResource that gui.rs registers at
// startup. Only the `gui` feature needs it; the CLI builds without glib tooling.
fn main() {
    #[cfg(feature = "gui")]
    {
        glib_build_tools::compile_resources(
            &["resources"],
            "resources/usbc.gresource.xml",
            "usbc.gresource",
        );
    }
}
