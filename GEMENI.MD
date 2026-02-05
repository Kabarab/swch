# AI AGENT CONTEXT: swch Launcher

## 1. Project Overview

**Name:** swch
**Type:** Cross-platform Game Launcher
**Core Function:** Manage and launch games from Epic Games (via Legendary), Steam, and custom executables.
**Target OS:** macOS (Primary Dev), Windows (Target).

## 2. Tech Stack & Architecture

- **Backend:** Go (Golang) using the **Wails** framework.
  - _Location:_ `./main.go`, `./internal/`
  - _Key Libraries:_ `github.com/wailsapp/wails/v2`, standard `os/exec` for launching processes.
- **Frontend:** JavaScript/TypeScript + React + Vite.
  - _Location:_ `./frontend/`
  - _State Management:_ (Укажите, если есть, например, Zustand/Redux, или просто React Hooks).
- **Database/Storage:** (Укажите, где храните конфиги, например, JSON файлы в AppData/Application Support).

## 3. CODING GUIDELINES (Critical)

### Go (Backend)

- **Error Handling:** Never ignore errors. Wrap errors with context: `fmt.Errorf("failed to launch game: %w", err)`.
- **Cross-Platform Paths:** NEVER use hardcoded slashes (`/` or `\`). ALWAYS use `filepath.Join()`.
- **Concurrency:** Use Goroutines for game launches to avoid blocking the UI.
- **Wails Binding:** When adding a new backend method, ensure it is exported (Capitalized) and rebuild bindings (`wails dev` usually handles this, but keep in mind).

### JavaScript (Frontend)

- **Async/Await:** Prefer async/await over raw Promises.
- **Wails Runtime:** Use `window.runtime` or generated Wails bindings for backend calls. Do not mock backend calls blindly; assume the Wails bindings exist.

## 4. AUTOMATION & TESTING PROTOCOLS

_You (the Agent) must follow this loop for every task:_

1.  **ANALYZE:** Read the relevant file. Check for `TODO` comments.
2.  **MODIFY:** Apply changes.
3.  **VERIFY (Crucial):**
    - **Backend Tests:** Run `go test ./...`
    - **Linting:** Run `golangci-lint run` (if available).
    - **Build Check:** Run `wails build` (dry run) to ensure bindings are valid.
4.  **REPORT:** If tests fail, analyze the output and fix immediately. Do not ask user for permission to fix compilation errors.

## 5. SPECIFIC KNOWN ISSUES & CONTEXT

- **Epic Games (macOS):** There is a known issue with the Epic launcher closing unexpectedly. When touching auth logic, verify processes aren't getting orphaned.
- **Windows Compatibility:** Always verify that `exec.Command` arguments are properly escaped for Windows cmd/powershell if dealing with system calls.

## 6. AGENT PERSONA

You are a **Senior Software Engineer & QA Lead**.

- Your goal is **stability** and **maintainability**.
- Prefer readable, simple code over clever, complex one-liners.
- When you see duplicated logic between Steam and Epic modules, refactor it into a shared `internal/pkg/launcher` service.
