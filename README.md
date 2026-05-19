# AI Code Detector

Deterministic Rust CLI for scoring source code for generated-code signals. The primary target is C# source, including decompiled C# stored as `.blob` files. Other supported languages are scanned on a best-effort basis.

Scores are continuous from `0.0` to `100.0`; the `--Deem` threshold only controls when `IsAI` becomes `true`.

The tool always prints a JSON payload to stdout. When file output is enabled, it also writes a JSON report and, unless JSON-only mode is selected, a Markdown report.

## Usage

```powershell
cargo run -- <path-to-repo>
cargo run -- <path-to-repo> --Deem 75
cargo run -- <path-to-repo> --OnlyJSON true --OutputFiles false
cargo run -- <path-to-repo> --ResultOnly true
cargo run -- <path-to-repo> --OutputDir ./scan-results --OutputName repo
```

Supported arguments:

- `--Deem <0-100>` or `--deem <0-100>`
- `--OnlyJSON <true|false>`, `--only-json`, or `OnlyJSON=true`
- `--OutputFiles <true|false>`, `--output-files`, `--no-output-files`, or `OutputFiles=false`
- `--ResultOnly <true|false>`, `--result-only`, or `ResultOnly=true`
- `--OutputDir <path>` or `OutputDir=<path>`
- `--OutputName <name>` or `OutputName=<name>`

Optional private calibration can be enabled with environment variables:

- `AI_CODE_DETECTOR_AI_CORPUS`
- `AI_CODE_DETECTOR_HUMAN_CORPUS`

Calibration paths are never emitted in JSON or Markdown output. Reports only expose whether calibration was enabled and how many normalized profile lines were loaded.

## Output

Minimal payload for software consumers:

```json
{
  "application": {"name":"ai-code-detector","version":"0.1.0"},
  "data": {"Percentage":100.0,"IsAI":true}
}
```

The full JSON also includes per-file summaries, inferred purpose, detected implementation elements, human-match assessment, considered line records, excluded-line summaries, calibration status, and generic match reasons.

With `ResultOnly=true`, JSON and Markdown reports include only the `IsAI` result.

## Notes

This is a broad heuristic detector, not proof of authorship. Comments, blank lines, low-information syntax-only lines, repeated scaffold/template lines, and recognized compiler/decompiler scaffolding are excluded from the percentage.

For C#, the detector looks for signals such as generated standalone plugin structure, minimal runtime patch shape, dense Harmony/config/reflection patching, config-option prose boilerplate, inline object-array event protocols, immediate-mode UI assembly, broad runtime type/method discovery, string-based integration lookups, repeated safe-cast/sanitizer helpers, runtime reflection adapters, silent reflection/fallback guards, loader/cache scaffolding density, release test-hook surfaces, repeated protocol fallback scaffolding, explicit generated-code labels, decompiled metadata, and authored-code indicators such as richer API/library shape, detour API wrapper topology, library registration surfaces, source-generator metadata scaffolds, patcher/preloader utility workflows, release metadata, and broad dependency surfaces.
