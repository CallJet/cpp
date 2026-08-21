//! CLI console presentation and actionable startup diagnostics.

use std::path::Path;

/// ASCII rendering of the CallJet aircraft and branching call paths.
pub const CALLJET_ASCII_ART: &str = r#"
                         o
                        /
             __________/
==\    _____/___
===\__/_________\____________>---o
===/  \         /
==/    \_______/
                         \
                          \_____o

                 CallJet C++
       FIND THE PATH. SKIP THE WHOLE GRAPH.
"#;

/// Actionable help displayed when the compilation database is absent.
pub fn missing_compilation_database_help(path: &Path) -> String {
    format!(
        "[CallJet] compilation database not found: {}\n\
[CallJet] analysis stopped before source parsing; no compiler command was run.\n\
\n\
Create a compilation database with CMake:\n\
  cmake -S . -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON\n\
\n\
Then pass the generated file to CallJet:\n\
  calljet <COMMAND> --root . --compile-commands build/compile_commands.json",
        path.display()
    )
}
