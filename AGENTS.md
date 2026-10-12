# Graphify for SWCH

Graphify is installed locally in `.graphify-tools/venv`; its project skill is
`.agents/skills/graphify/SKILL.md`. On Windows, run commands from the project root
through `./scripts/graphify.ps1` (for example,
`./scripts/graphify.ps1 query "SteamSessionManager" --budget 1500`).
Use this launcher in place of bare `graphify` commands in the upstream skill.
The Python interpreter is `.graphify-tools/venv/Scripts/python.exe`; use
PowerShell equivalents of the skill's Bash snippets.

For architecture and cross-file relationship questions, query the existing
`graphify-out/graph.json` first, then verify relevant source code. The graph is
an index, not a substitute for the source. For a broad overview, read
`graphify-out/GRAPH_REPORT.md`.

After relevant code changes, or if the graph is missing, rebuild with:

```powershell
./scripts/graphify.ps1 extract . --code-only --cargo --max-workers 4
./scripts/graphify.ps1 cluster-only . --no-label
```

The default setup uses local AST extraction. Respect `.graphifyignore`; build
artifacts, bundled third-party WebView2 headers, credentials and agent metadata
are excluded. See `docs/graphify.md` for setup and known limitations.
