//go:build !windows

package legendary

import "os/exec"

// setSysProcAttr ничего не делает на macOS и Linux
func setSysProcAttr(cmd *exec.Cmd) {
	// На Unix-системах дополнительных действий не требуется
}