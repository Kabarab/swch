package sys

import (
	"fmt"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/sys/windows/registry"
)

func GetSteamPath() (string, error) {
	k, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.QUERY_VALUE)
	if err != nil {
		return "", err
	}
	defer k.Close()

	path, _, err := k.GetStringValue("SteamPath")
	if err != nil {
		return "", err
	}
	return filepath.Clean(strings.ReplaceAll(path, "/", "\\")), nil
}

func KillSteam() {
	processes := []string{"steam.exe", "steamwebhelper.exe", "GameOverlayUI.exe"}

	for _, p := range processes {
		cmd := exec.Command("taskkill", "/F", "/IM", p)
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		_ = cmd.Run()
	}

	for i := 0; i < 20; i++ {
		stillAlive := false
		cmd := exec.Command("tasklist")
		cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
		output, _ := cmd.Output()
		outStr := string(output)

		if strings.Contains(outStr, "steam.exe") {
			stillAlive = true
		}

		if !stillAlive {
			time.Sleep(500 * time.Millisecond)
			return
		}
		time.Sleep(500 * time.Millisecond)
	}
}

func SetSteamUser(username string) error {
	if username == "" {
		return fmt.Errorf("username is empty")
	}

	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}
	if err := k.SetDWordValue("RememberPassword", 1); err != nil {
		return err
	}

	activeKey, err := registry.OpenKey(registry.CURRENT_USER, `Software\Valve\Steam\ActiveProcess`, registry.SET_VALUE)
	if err == nil {
		defer activeKey.Close()
		_ = activeKey.SetDWordValue("ActiveUser", 0)
		_ = activeKey.SetDWordValue("pid", 0)
	}

	return nil
}

func StartGame(pathOrUrl string) {
	cmd := exec.Command("cmd", "/C", "start", "", pathOrUrl)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}

func StartGameWithArgs(exePath string, args ...string) {
	cmd := exec.Command(exePath, args...)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}