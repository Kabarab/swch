//go:build darwin

package processes

import (
	"os/exec"
)

func StartProgram(path string, args ...string) error {
	cmd := exec.Command(path, args...)
	// На macOS нет прямого аналога CREATE_NEW_PROCESS_GROUP в SysProcAttr,
	// который необходим в данном контексте, поэтому запускаем стандартно.
	return cmd.Start()
}

func KillProcess(procName string) error {
	// Используем pkill для завершения процессов по имени (аналог taskkill /IM)
	cmd := exec.Command("pkill", "-il", procName)
	return cmd.Run()
}