package epic

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"swch/internal/sys"
)

func (c *EpicClient) LaunchGame(appName string, installLocation string, namespace string) error {
	// 1. Get exchange code for DRM
	exchangeCode, err := c.GetExchangeCode()
	if err != nil {
		fmt.Printf("Warning: failed to get exchange code for DRM: %v\n", err)
		// We can still try to launch, some games don't need it
	}

	// 2. Find executable (this usually requires manifest info)
	// If we don't have the manifest, we might need to search the folder.
	// But let assume for now it's handled at a higher level or by the official launcher scheme.

	// If we are launching directly (native client bypass/replacement):
	// The arguments for Epic games are usually:
	// -AUTH_LOGIN=unused -AUTH_PASSWORD={exchange_code} -AUTH_TYPE=exchangecode -epicapp={appName} -epicenv=Prod -EpicPortal

	// Note: Finding the .exe reliably without the manifest is hard.
	// We'll rely on the caller providing the path or use the manifest scanner.

	return nil
}

// GetLaunchArgs returns the standard arguments for an Epic game
func (c *EpicClient) GetLaunchArgs(appName string, exchangeCode string) []string {
	return []string{
		"-AUTH_LOGIN=unused",
		"-AUTH_PASSWORD=" + exchangeCode,
		"-AUTH_TYPE=exchangecode",
		"-epicapp=" + appName,
		"-epicenv=Prod",
		"-EpicPortal",
	}
}

func (c *EpicClient) RunExecutable(exePath string, args []string) error {
	cmd := exec.Command(exePath, args...)
	cmd.Dir = filepath.Dir(exePath)
	sys.ConfigureCommand(cmd)
	return cmd.Start()
}
