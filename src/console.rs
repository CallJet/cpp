//! CLI console presentation and actionable startup diagnostics.

use std::path::Path;

/// ASCII wordmark for the CallJet CLI.
pub const CALLJET_ASCII_ART: &str = r#"
  _____      _ _      _      _
 / ____|    | | |    | |    | |
| |     __ _| | |    | | ___| |_
| |    / _` | | | _  | |/ _ \ __|
| |___| (_| | | || |_| |  __/ |_
 \_____\__,_|_|_| \___/ \___|\__|

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
