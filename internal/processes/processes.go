package processes

import (
	"os/exec"
	"syscall"
)

func StartProgram(path string, args ...string) error {
	cmd := exec.Command(path, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP,
	}
	return cmd.Start()
}

func KillProcess(procName string) error {
	cmd := exec.Command("taskkill", "/F", "/IM", procName)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	return cmd.Run()
}
