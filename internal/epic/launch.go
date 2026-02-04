package epic

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"runtime"
	"swch/internal/sys"
)

// GetLaunchCommand prepares the command to launch an Epic game
// It generates a fresh exchange code for DRM bypass
func (c *EpicClient) GetLaunchCommand(appName string, installPath string) (*exec.Cmd, error) {
	// 1. Get exchange code needed for DRM
	exchangeCode, err := c.GetExchangeCode()
	if err != nil {
		return nil, fmt.Errorf("failed to get exchange code: %w", err)
	}

	// 2. Determine Executable Path
	// Ideally we should parse the manifest to find the MainGameAppName executable
	// or scan the directory. For now, we rely on what was passed or passed in via library.

	// If installPath is the folder, we need to find the executable.
	// This is hard without the manifest details on explicit executable.
	// But ScanInstalledGames fills 'AppName' as the ID.

	// WARNING: Just launching the executable might not work if it expects specific args.
	// However, the standard Epic simplified launch is:
	// Binary.exe -AUTH_LOGIN=unused -AUTH_PASSWORD=<ExchangeCode> -AUTH_TYPE=exchangecode -epicapp=<AppID> -epicenv=Prod -EpicPortal

	// We need the executable path.
	// If installPath is just the directory, we might need to search for .app (macOS) or .exe (Windows)

	targetExe := installPath // Default assumption

	if runtime.GOOS == "darwin" {
		// On macOS, installPath typically points to the .app bundle or the folder containing it.
		// If it points to the folder, we need to find the .app
		if filepath.Ext(targetExe) != ".app" {
			// Search for .app in the folder
			files, _ := filepath.Glob(filepath.Join(installPath, "*.app"))
			if len(files) > 0 {
				targetExe = files[0]
			}
		}

		// If it is a .app, we use 'open' command
		if filepath.Ext(targetExe) == ".app" {
			args := []string{
				"-n", targetExe, // -n opens a new instance
				"--args",
				"-AUTH_LOGIN=unused",
				"-AUTH_PASSWORD=" + exchangeCode,
				"-AUTH_TYPE=exchangecode",
				"-epicapp=" + appName,
				"-epicenv=Prod",
				"-EpicPortal",
			}
			return exec.Command("open", args...), nil
		}
	} else {
		// Windows logic (placeholder if needed, but we focus on macOS)
		// Usually we need the actual .exe
	}

	// Fallback for direct binary launch
	args := []string{
		"-AUTH_LOGIN=unused",
		"-AUTH_PASSWORD=" + exchangeCode,
		"-AUTH_TYPE=exchangecode",
		"-epicapp=" + appName,
		"-epicenv=Prod",
		"-EpicPortal",
	}

	cmd := exec.Command(targetExe, args...)
	cmd.Dir = filepath.Dir(targetExe)
	return cmd, nil
}

func (c *EpicClient) RunExecutable(cmd *exec.Cmd) error {
	sys.ConfigureCommand(cmd)
	return cmd.Start()
}
