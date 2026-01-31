//go:build windows

package legendary

import (
	"os/exec"
	"syscall"
)

// setSysProcAttr скрывает окно консоли (cmd.exe) при запуске процесса
func setSysProcAttr(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{
		HideWindow:    true,
		CreationFlags: 0x08000000, // CREATE_NO_WINDOW
	}
}