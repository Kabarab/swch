package processes

import (
	"fmt"
	"os/exec"
	"syscall"
)

// StartProgram запускает приложение.
// args - строка аргументов.
func StartProgram(path string, args ...string) error {
	cmd := exec.Command(path, args...)
	// Detach process (чтобы он не закрывался вместе с свитчером)
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP,
	}
	return cmd.Start()
}

// KillProcess убивает процесс по имени (аналог taskkill).
func KillProcess(procName string) error {
	// В Windows проще всего вызвать taskkill, чем перебирать PID через syscall
	cmd := exec.Command("taskkill", "/F", "/IM", procName)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}