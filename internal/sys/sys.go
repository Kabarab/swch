package sys

import (
	"os/exec"
	"path/filepath"
	"syscall"

	"golang.org/x/sys/windows/registry"
)

// GetSteamPath находит путь к Steam в реестре
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
	return filepath.Clean(path), nil
}

// KillSteam убивает процесс Steam
func KillSteam() {
	// Используем taskkill для надежности
	cmd := exec.Command("taskkill", "/F", "/IM", "steam.exe")
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Run()
}

// SetSteamUser меняет пользователя авто-логина в реестре
func SetSteamUser(username string) error {
	k, _, err := registry.CreateKey(registry.CURRENT_USER, `Software\Valve\Steam`, registry.SET_VALUE)
	if err != nil {
		return err
	}
	defer k.Close()

	if err := k.SetStringValue("AutoLoginUser", username); err != nil {
		return err
	}
	if err := k.SetStringValue("RememberPassword", "1"); err != nil {
		return err
	}
	return nil
}

// StartGame запускает игру через steam:// протокол
func StartGame(appID string) {
	cmd := exec.Command("cmd", "/C", "start", "steam://run/"+appID)
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true}
	cmd.Start()
}